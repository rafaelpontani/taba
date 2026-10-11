using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Net;
using System.Net.Http;
using System.Net.Sockets;
using System.Threading;
using System.Threading.Tasks;

namespace Taba
{
    /// <summary>
    /// O transporte HTTP do cliente: monta a requisição, manda, lê o corpo, transforma o erro da API em
    /// <see cref="TabaException"/> e faz a repetição de chamadas do contrato comum (doc 19, seção 18.4).
    /// </summary>
    internal sealed class HttpTransport : IDisposable
    {
        private readonly TabaConfig _config;
        private readonly string _baseUrl;
        private readonly HttpClient _http;
        private readonly Func<TimeSpan, CancellationToken, Task> _delay;

        /// <param name="config">Já copiada pelo cliente.</param>
        /// <param name="handler">Um <see cref="HttpMessageHandler"/> próprio, ou <c>null</c> para o padrão (que o transporte descarta).</param>
        /// <param name="delay">A espera entre tentativas (os testes a trocam).</param>
        public HttpTransport(TabaConfig config, HttpMessageHandler handler, Func<TimeSpan, CancellationToken, Task> delay)
        {
            _config = config;
            _baseUrl = ValidateBaseUrl(config.BaseUrl);
            bool owned = handler == null;
            if (owned)
            {
                handler = CreateHandler(config);
            }
            _http = new HttpClient(handler, owned) { Timeout = Timeout.InfiniteTimeSpan };
            _delay = delay ?? Task.Delay;
        }

        /// <summary>A espera entre tentativas, também usada pela renovação da invisibilidade.</summary>
        public Func<TimeSpan, CancellationToken, Task> Delay
        {
            get { return _delay; }
        }

        private static HttpMessageHandler CreateHandler(TabaConfig config)
        {
#if NET5_0_OR_GREATER
            return new SocketsHttpHandler { AllowAutoRedirect = false, ConnectTimeout = config.ConnectTimeout };
#else
            return new HttpClientHandler { AllowAutoRedirect = false };
#endif
        }

        private static string ValidateBaseUrl(string baseUrl)
        {
            Uri uri;
            if (string.IsNullOrWhiteSpace(baseUrl)
                || !Uri.TryCreate(baseUrl.Trim(), UriKind.Absolute, out uri)
                || (uri.Scheme != Uri.UriSchemeHttp && uri.Scheme != Uri.UriSchemeHttps))
            {
                throw TabaException.Client(
                    "client.invalid-config",
                    "baseUrl precisa ser um endereço http:// ou https:// (recebido: "
                        + (string.IsNullOrWhiteSpace(baseUrl) ? "vazio" : baseUrl) + ")");
            }
            return baseUrl.Trim().TrimEnd('/');
        }

        private string Url(ApiCall call)
        {
            string url = _baseUrl + call.Path;
            if (call.Query.Count > 0)
            {
                var parts = new List<string>();
                foreach (var pair in call.Query)
                {
                    parts.Add(Uri.EscapeDataString(pair.Key) + "=" + Uri.EscapeDataString(pair.Value));
                }
                url += "?" + string.Join("&", parts.ToArray());
            }
            return url;
        }

        private HttpRequestMessage Wire(ApiCall call)
        {
            var request = new HttpRequestMessage(call.Method, Url(call));
            if (!string.IsNullOrEmpty(_config.Token))
            {
                request.Headers.TryAddWithoutValidation("Authorization", "Bearer " + _config.Token);
            }
            request.Headers.TryAddWithoutValidation("Accept", "application/json");
            request.Headers.TryAddWithoutValidation("User-Agent", _config.UserAgent);
            if (call.RequestId != null)
            {
                request.Headers.TryAddWithoutValidation("X-Request-Id", call.RequestId);
            }
            if (call.Traceparent != null)
            {
                request.Headers.TryAddWithoutValidation("traceparent", call.Traceparent);
            }
            if (call.Body != null)
            {
                request.Content = new ByteArrayContent(call.Body);
                request.Content.Headers.TryAddWithoutValidation("Content-Type", call.ContentType ?? "application/json");
            }
            return request;
        }

        private static string Header(HttpResponseMessage response, string name)
        {
            IEnumerable<string> values;
            if (response.Headers.TryGetValues(name, out values))
            {
                foreach (string value in values)
                {
                    return value;
                }
            }
            return null;
        }

        private static bool IsConnectFailure(Exception e)
        {
            for (Exception inner = e; inner != null; inner = inner.InnerException)
            {
                var socket = inner as SocketException;
                if (socket != null)
                {
                    return socket.SocketErrorCode == SocketError.ConnectionRefused
                        || socket.SocketErrorCode == SocketError.HostNotFound
                        || socket.SocketErrorCode == SocketError.TryAgain
                        || socket.SocketErrorCode == SocketError.NoData
                        || socket.SocketErrorCode == SocketError.NetworkUnreachable
                        || socket.SocketErrorCode == SocketError.HostUnreachable;
                }
                var web = inner as WebException;
                if (web != null)
                {
                    return web.Status == WebExceptionStatus.ConnectFailure
                        || web.Status == WebExceptionStatus.NameResolutionFailure
                        || web.Status == WebExceptionStatus.ProxyNameResolutionFailure;
                }
            }
            return false;
        }

        private static string ErrorCodeOf(byte[] body)
        {
            try
            {
                return JsonFields.Parse(Utf8.Encoding.GetString(body)).String("code");
            }
            catch (JsonException)
            {
                return null;
            }
        }

        /// <summary>O <c>ErrorResponse</c> da resposta; se o corpo não for um, o erro leva o código <c>http.&lt;status&gt;</c>.</summary>
        private static TabaException ErrorFrom(HttpResponseMessage response, byte[] body)
        {
            string text = Utf8.Encoding.GetString(body);
            string headerId = Header(response, "X-Request-Id");
            string code = null;
            string message = null;
            string requestId = null;
            string details = null;
            try
            {
                JsonFields fields = JsonFields.Parse(text);
                code = fields.String("code");
                if (code != null)
                {
                    message = fields.String("message") ?? string.Empty;
                    requestId = fields.String("requestId");
                    details = fields.Raw("details");
                }
            }
            catch (JsonException)
            {
            }
            if (code == null)
            {
                int status = (int)response.StatusCode;
                code = "http." + status.ToString(CultureInfo.InvariantCulture);
                string trimmed = text.Trim();
                message = trimmed.Length == 0
                    ? "resposta " + status.ToString(CultureInfo.InvariantCulture) + " sem corpo"
                    : trimmed.Substring(0, Math.Min(300, trimmed.Length));
            }
            int retryAfter;
            string retryHeader = Header(response, "Retry-After");
            return new TabaException((int)response.StatusCode, code, message)
            {
                RequestId = requestId ?? headerId,
                RetryAfter = retryHeader != null
                    && int.TryParse(retryHeader.Trim(), NumberStyles.None, CultureInfo.InvariantCulture, out retryAfter)
                    ? (int?)retryAfter
                    : null,
                Details = details,
                Body = body,
            };
        }

        /// <summary>
        /// Faz a chamada, repetindo só o que o contrato manda repetir: <c>429</c> e <c>503</c> (esperando
        /// o <c>Retry-After</c>, ou 1 s, 2 s, 4 s), <c>502</c> e <c>504</c> só com <c>RetryGateway</c>, e
        /// falha ao conectar (recusa, DNS). Nunca um <c>4xx</c> (fora o <c>429</c>), nem uma falha
        /// depois de enviar o corpo, nem um estouro de tempo.
        /// </summary>
        public async Task<ApiReply> SendAsync(ApiCall call, CancellationToken cancellationToken)
        {
            TimeSpan timeout = call.Timeout ?? _config.RequestTimeout;
            int max = Math.Max(1, _config.MaxAttempts);
            bool afterRetryable = false;
            for (int attempt = 1; ; attempt++)
            {
                TimeSpan backoff = TimeSpan.FromSeconds(1 << Math.Min(attempt - 1, 6));
                TimeSpan wait;
                using (var limit = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken))
                {
                    limit.CancelAfter(timeout);
                    try
                    {
                        using (HttpRequestMessage request = Wire(call))
                        using (HttpResponseMessage response = await _http
                            .SendAsync(request, HttpCompletionOption.ResponseHeadersRead, limit.Token)
                            .ConfigureAwait(false))
                        {
                            byte[] body = await ReadBodyAsync(response, limit.Token).ConfigureAwait(false);
                            int status = (int)response.StatusCode;
                            if (status < 400)
                            {
                                return new ApiReply { Status = status, Body = body };
                            }
                            if (afterRetryable && Recovers(call, status, body))
                            {
                                return new ApiReply { Status = status, Body = body, Recovered = true };
                            }
                            TabaException error = ErrorFrom(response, body);
                            bool retry = status == 429 || status == 503
                                || ((status == 502 || status == 504) && call.RetryGateway);
                            if (!retry || attempt >= max)
                            {
                                throw error;
                            }
                            afterRetryable |= status == 503;
                            wait = error.RetryAfter.HasValue ? TimeSpan.FromSeconds(error.RetryAfter.Value) : backoff;
                        }
                    }
                    catch (OperationCanceledException)
                    {
                        if (cancellationToken.IsCancellationRequested)
                        {
                            throw;
                        }
                        // O tempo estourou. Depois de enviado o corpo não se sabe se foi aplicado.
                        throw TabaException.Client("client.timeout", "estourou o tempo esperando a resposta");
                    }
                    catch (HttpRequestException e)
                    {
                        if (!IsConnectFailure(e))
                        {
                            throw TabaException.Client("client.network", Describe(e));
                        }
                        if (attempt >= max)
                        {
                            throw TabaException.Client("client.network", Describe(e));
                        }
                        afterRetryable = true;
                        wait = backoff;
                    }
                    catch (IOException e)
                    {
                        throw TabaException.Client("client.network", Describe(e));
                    }
                }
                await _delay(wait, cancellationToken).ConfigureAwait(false);
            }
        }

        private static async Task<byte[]> ReadBodyAsync(HttpResponseMessage response, CancellationToken token)
        {
            using (Stream stream = await response.Content.ReadAsStreamAsync().ConfigureAwait(false))
            using (var buffer = new MemoryStream())
            {
                await stream.CopyToAsync(buffer, 81920, token).ConfigureAwait(false);
                return buffer.ToArray();
            }
        }

        private static bool Recovers(ApiCall call, int status, byte[] body)
        {
            foreach (var accepted in call.Recover)
            {
                if (accepted.Key == status && ErrorCodeOf(body) == accepted.Value)
                {
                    return true;
                }
            }
            return false;
        }

        private static string Describe(Exception e)
        {
            string text = e.Message;
            if (e.InnerException != null && e.InnerException.Message != text)
            {
                text += ": " + e.InnerException.Message;
            }
            return text;
        }

        public void Dispose()
        {
            _http.Dispose();
        }
    }
}
