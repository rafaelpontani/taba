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
    /// O cliente da API de cliente do Taba (doc 19, seção 18.3): as nove operações, todas com a forma
    /// assíncrona (<c>...Async</c>, que é a implementação) e a síncrona (que espera pela assíncrona), e o
    /// <c>subscribe</c>. É seguro para uso concorrente: um só objeto serve a várias tarefas e threads.
    /// </summary>
    /// <remarks>
    /// TLS: o conector usa o da plataforma e não o altera. No .NET Framework 4.6.x o protocolo é o do
    /// <c>ServicePointManager.SecurityProtocol</c> (confira que inclui o TLS 1.2); do 4.7 em diante é o
    /// do sistema. A verificação do certificado nunca é desligada.
    /// </remarks>
    public sealed class TabaClient : IDisposable
    {
        /// <summary>A versão deste conector.</summary>
        public const string Version = "0.1.0";

        /// <summary>A versão do contrato da API que este conector implementa.</summary>
        public const string ContractVersion = "1";

        /// <summary>O <c>consume</c> espera <c>waitTime</c> mais esta folga, e não o <c>RequestTimeout</c>.</summary>
        internal static readonly TimeSpan LongPollSlack = TimeSpan.FromSeconds(10);

        private static readonly HttpMethod Patch = new HttpMethod("PATCH");

        private readonly TabaConfig _config;
        private readonly string _baseUrl;
        private readonly HttpClient _http;
        private readonly Func<TimeSpan, CancellationToken, Task> _delay;

        public TabaClient(TabaConfig config)
            : this(config, null, null)
        {
        }

        /// <summary>
        /// Para trocar a pilha de rede: um <see cref="HttpMessageHandler"/> próprio (um proxy, um
        /// transporte de mentira nos testes de quem usa o conector). O cliente não o descarta.
        /// </summary>
        public TabaClient(TabaConfig config, HttpMessageHandler handler)
            : this(config, handler, null)
        {
        }

        internal TabaClient(TabaConfig config, HttpMessageHandler handler, Func<TimeSpan, CancellationToken, Task> delay)
        {
            if (config == null)
            {
                throw new ArgumentNullException("config");
            }
            _config = config.Copy();
            _baseUrl = ValidateBaseUrl(_config.BaseUrl);
            bool owned = handler == null;
            if (owned)
            {
                handler = CreateHandler(_config);
            }
            _http = new HttpClient(handler, owned) { Timeout = Timeout.InfiniteTimeSpan };
            _delay = delay ?? Task.Delay;
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

        /// <summary>Uma cópia da configuração em uso.</summary>
        public TabaConfig Config
        {
            get { return _config.Copy(); }
        }

        public void Dispose()
        {
            _http.Dispose();
        }

        // -----------------------------------------------------------------------------------------
        // A chamada, com a repetição do contrato comum (seção 18.4)
        // -----------------------------------------------------------------------------------------

        internal sealed class Call
        {
            public Call(HttpMethod method, string path)
            {
                Method = method;
                Path = path;
            }

            public HttpMethod Method { get; private set; }

            /// <summary>Caminho já com os segmentos codificados (<c>/topics/pedidos</c>).</summary>
            public string Path { get; private set; }

            public List<KeyValuePair<string, string>> Query { get; } = new List<KeyValuePair<string, string>>();

            public byte[] Body { get; set; }

            public string ContentType { get; set; }

            public string RequestId { get; set; }

            public string Traceparent { get; set; }

            /// <summary>Troca o tempo total da chamada (o <c>consume</c> usa <c>waitTime + 10 s</c>).</summary>
            public TimeSpan? Timeout { get; set; }

            /// <summary>Repete <c>502</c> e <c>504</c>: só a publicação em fila FIFO, em que o primário deduplica.</summary>
            public bool RetryGateway { get; set; }

            /// <summary>Depois de um <c>503</c>, estas respostas contam como sucesso (a operação já tinha sido aplicada).</summary>
            public List<KeyValuePair<int, string>> Recover { get; } = new List<KeyValuePair<int, string>>();

            public Call With(string key, object value)
            {
                Query.Add(new KeyValuePair<string, string>(key, Format(value)));
                return this;
            }

            public Call Json(string json)
            {
                Body = Utf8.Encoding.GetBytes(json);
                return this;
            }
        }

        internal sealed class Reply
        {
            public int Status { get; set; }

            public byte[] Body { get; set; }

            public bool Recovered { get; set; }

            public string Text
            {
                get { return Utf8.Encoding.GetString(Body); }
            }
        }

        internal static string Format(object value)
        {
            if (value is bool)
            {
                return (bool)value ? "true" : "false";
            }
            return Convert.ToString(value, CultureInfo.InvariantCulture);
        }

        private static string Segment(string value)
        {
            return Uri.EscapeDataString(value);
        }

        private string Url(Call call)
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

        private HttpRequestMessage Wire(Call call)
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
        internal async Task<Reply> SendAsync(Call call, CancellationToken cancellationToken)
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
                                return new Reply { Status = status, Body = body };
                            }
                            if (afterRetryable && Recovers(call, status, body))
                            {
                                return new Reply { Status = status, Body = body, Recovered = true };
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

        private static bool Recovers(Call call, int status, byte[] body)
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

        // -----------------------------------------------------------------------------------------
        // As nove operações
        // -----------------------------------------------------------------------------------------

        /// <summary><c>POST /topics</c>.</summary>
        public async Task<Topic> CreateTopicAsync(TopicRequest request, CancellationToken cancellationToken = default(CancellationToken))
        {
            if (request == null)
            {
                throw new ArgumentNullException("request");
            }
            var call = new Call(HttpMethod.Post, "/topics").Json(request.ToJson());
            call.Recover.Add(new KeyValuePair<int, string>(409, "topic.already-exists"));
            Reply reply = await SendAsync(call, cancellationToken).ConfigureAwait(false);
            if (reply.Recovered)
            {
                return await GetTopicAsync(request.Name, cancellationToken).ConfigureAwait(false);
            }
            return Topic.Parse(reply.Text);
        }

        /// <summary><c>GET /topics/{topic}</c>.</summary>
        public async Task<Topic> GetTopicAsync(string name, CancellationToken cancellationToken = default(CancellationToken))
        {
            Reply reply = await SendAsync(new Call(HttpMethod.Get, "/topics/" + Segment(name)), cancellationToken)
                .ConfigureAwait(false);
            return Topic.Parse(reply.Text);
        }

        /// <summary><c>GET /topics</c>. Os parâmetros <c>null</c> não vão na query.</summary>
        public async Task<Page<Topic>> ListTopicsAsync(
            int? limit = null,
            string cursor = null,
            string name = null,
            CancellationToken cancellationToken = default(CancellationToken))
        {
            var call = new Call(HttpMethod.Get, "/topics");
            if (limit.HasValue)
            {
                call.With("limit", limit.Value);
            }
            if (cursor != null)
            {
                call.With("cursor", cursor);
            }
            if (name != null)
            {
                call.With("name", name);
            }
            Reply reply = await SendAsync(call, cancellationToken).ConfigureAwait(false);
            JsonFields fields = Topic.Fields(reply.Text, "página");
            var items = new List<Topic>();
            try
            {
                foreach (string item in Json.ArrayItems(fields.Raw("items") ?? "[]"))
                {
                    items.Add(Topic.Parse(item));
                }
            }
            catch (JsonException e)
            {
                throw TabaException.Client("client.invalid-response", "itens da página: " + e.Message);
            }
            return new Page<Topic>(items, fields.String("nextCursor"), reply.Text);
        }

        /// <summary>Todos os tópicos, página a página.</summary>
        public async Task<IList<Topic>> ListAllTopicsAsync(string name = null, CancellationToken cancellationToken = default(CancellationToken))
        {
            var all = new List<Topic>();
            string cursor = null;
            do
            {
                Page<Topic> page = await ListTopicsAsync(null, cursor, name, cancellationToken).ConfigureAwait(false);
                all.AddRange(page.Items);
                cursor = page.NextCursor;
            }
            while (cursor != null);
            return all;
        }

        /// <summary><c>PATCH /topics/{topic}</c>. É um <c>PATCH</c> de verdade, sem cabeçalho de troca de verbo.</summary>
        public async Task<Topic> UpdateTopicAsync(string name, TopicUpdate update, CancellationToken cancellationToken = default(CancellationToken))
        {
            if (update == null)
            {
                throw new ArgumentNullException("update");
            }
            var call = new Call(Patch, "/topics/" + Segment(name)).Json(update.ToJson());
            Reply reply = await SendAsync(call, cancellationToken).ConfigureAwait(false);
            return Topic.Parse(reply.Text);
        }

        /// <summary><c>DELETE /topics/{topic}</c>.</summary>
        public async Task DeleteTopicAsync(string name, CancellationToken cancellationToken = default(CancellationToken))
        {
            var call = new Call(HttpMethod.Delete, "/topics/" + Segment(name));
            call.Recover.Add(new KeyValuePair<int, string>(404, "topic.not-found"));
            await SendAsync(call, cancellationToken).ConfigureAwait(false);
        }

        /// <summary><c>POST /topics/{topic}/messages</c>. O corpo vai como bytes, sem validação.</summary>
        public async Task<PublishResponse> PublishAsync(
            string topic,
            byte[] body,
            PublishOptions options = null,
            CancellationToken cancellationToken = default(CancellationToken))
        {
            options = options ?? new PublishOptions();
            var call = new Call(HttpMethod.Post, "/topics/" + Segment(topic) + "/messages")
            {
                Body = body ?? new byte[0],
                ContentType = options.ContentType ?? "application/json",
                RequestId = options.RequestId,
                Traceparent = options.Traceparent,
                RetryGateway = options.Fifo,
            };
            if (options.GroupId != null)
            {
                call.With("groupId", options.GroupId);
            }
            if (options.DeduplicationId != null)
            {
                call.With("deduplicationId", options.DeduplicationId);
            }
            Reply reply = await SendAsync(call, cancellationToken).ConfigureAwait(false);
            return PublishResponse.Parse(reply.Text);
        }

        /// <summary>Publica um texto JSON (<c>Content-Type: application/json</c>).</summary>
        public Task<PublishResponse> PublishJsonAsync(string topic, string json, CancellationToken cancellationToken = default(CancellationToken))
        {
            return PublishAsync(topic, Utf8.Encoding.GetBytes(json), null, cancellationToken);
        }

        /// <summary>Publica um texto (<c>Content-Type: text/plain; charset=utf-8</c>).</summary>
        public Task<PublishResponse> PublishTextAsync(string topic, string text, CancellationToken cancellationToken = default(CancellationToken))
        {
            var options = new PublishOptions { ContentType = "text/plain; charset=utf-8" };
            return PublishAsync(topic, Utf8.Encoding.GetBytes(text), options, cancellationToken);
        }

        internal static Call ConsumeCall(string topic, ConsumeOptions options)
        {
            options = options ?? new ConsumeOptions();
            var call = new Call(HttpMethod.Get, "/topics/" + Segment(topic) + "/messages");
            if (options.MaxMessages.HasValue)
            {
                call.With("maxMessages", options.MaxMessages.Value);
            }
            if (options.VisibilityTimeout.HasValue)
            {
                call.With("visibilityTimeout", options.VisibilityTimeout.Value);
            }
            if (options.WaitTime.HasValue)
            {
                call.With("waitTime", options.WaitTime.Value);
            }
            if (options.AutoCreate)
            {
                call.With("autoCreate", true);
                if (options.Volatile.HasValue)
                {
                    call.With("volatile", options.Volatile.Value);
                }
                if (options.IdleTimeout.HasValue)
                {
                    call.With("idleTimeout", options.IdleTimeout.Value);
                }
                if (options.Fifo.HasValue)
                {
                    call.With("fifo", options.Fifo.Value);
                }
                if (options.Tags != null)
                {
                    foreach (var tag in options.Tags)
                    {
                        call.With("tag." + tag.Key, tag.Value);
                    }
                }
            }
            call.RequestId = options.RequestId;
            call.Traceparent = options.Traceparent;
            call.Timeout = TimeSpan.FromSeconds(options.WaitTime ?? 0) + LongPollSlack;
            return call;
        }

        /// <summary>
        /// <c>GET /topics/{topic}/messages</c>. O tempo de leitura é <c>waitTime + 10 s</c>, e não o
        /// <c>RequestTimeout</c>. Estourar o tempo não é repetido (o nó pode ter capturado mensagens, que
        /// voltam sozinhas ao fim do <c>visibilityTimeout</c>). Devolve uma lista vazia se não há mensagens.
        /// </summary>
        public async Task<IList<Message>> ConsumeAsync(string topic, ConsumeOptions options = null, CancellationToken cancellationToken = default(CancellationToken))
        {
            Reply reply = await SendAsync(ConsumeCall(topic, options), cancellationToken).ConfigureAwait(false);
            JsonFields fields = Topic.Fields(reply.Text, "consumo");
            var messages = new List<Message>();
            try
            {
                foreach (string item in Json.ArrayItems(fields.Raw("messages") ?? "[]"))
                {
                    messages.Add(Message.Parse(item));
                }
            }
            catch (JsonException e)
            {
                throw TabaException.Client("client.invalid-response", "mensagens: " + e.Message);
            }
            return messages;
        }

        /// <summary><c>DELETE /topics/{topic}/messages/{id}?receiptToken=</c>.</summary>
        public async Task AckAsync(string topic, string messageId, string receiptToken, CancellationToken cancellationToken = default(CancellationToken))
        {
            var call = new Call(HttpMethod.Delete, "/topics/" + Segment(topic) + "/messages/" + Segment(messageId))
                .With("receiptToken", receiptToken);
            await SendAsync(call, cancellationToken).ConfigureAwait(false);
        }

        /// <summary>Confirma a mensagem recebida do <see cref="ConsumeAsync"/>.</summary>
        public Task AckAsync(string topic, Message message, CancellationToken cancellationToken = default(CancellationToken))
        {
            return AckAsync(topic, message.Id, message.ReceiptToken, cancellationToken);
        }

        /// <summary><c>PUT /topics/{topic}/messages/{id}/visibility?receiptToken=</c>.</summary>
        public async Task<VisibilityResponse> ChangeVisibilityAsync(
            string topic,
            string messageId,
            string receiptToken,
            int seconds,
            CancellationToken cancellationToken = default(CancellationToken))
        {
            var call = new Call(HttpMethod.Put, "/topics/" + Segment(topic) + "/messages/" + Segment(messageId) + "/visibility")
                .With("receiptToken", receiptToken)
                .Json("{\"visibilityTimeout\":" + seconds.ToString(CultureInfo.InvariantCulture) + "}");
            Reply reply = await SendAsync(call, cancellationToken).ConfigureAwait(false);
            return VisibilityResponse.Parse(reply.Text);
        }

        /// <summary>Muda a invisibilidade da mensagem recebida do <see cref="ConsumeAsync"/>.</summary>
        public Task<VisibilityResponse> ChangeVisibilityAsync(string topic, Message message, int seconds, CancellationToken cancellationToken = default(CancellationToken))
        {
            return ChangeVisibilityAsync(topic, message.Id, message.ReceiptToken, seconds, cancellationToken);
        }

        // -----------------------------------------------------------------------------------------
        // subscribe
        // -----------------------------------------------------------------------------------------

        /// <summary>
        /// O laço de consumo (D71.1): long polling (<c>waitTime</c> 20 e uma mensagem por vez, se não
        /// informados), chama o <paramref name="handler"/> e confirma sozinho **só se ele terminar sem
        /// exceção** (a mensagem que falha volta à fila ao fim do <c>visibilityTimeout</c>, e o erro vai
        /// para <paramref name="onError"/>). Termina, sem exceção, quando o <paramref name="cancellationToken"/>
        /// é cancelado. Um erro do servidor ao consumir sobe.
        /// </summary>
        public async Task SubscribeAsync(
            string topic,
            ConsumeOptions options,
            Func<Message, Task> handler,
            CancellationToken cancellationToken,
            Action<Message, Exception> onError = null)
        {
            if (handler == null)
            {
                throw new ArgumentNullException("handler");
            }
            ConsumeOptions poll = options == null ? new ConsumeOptions() : options.Clone();
            if (!poll.WaitTime.HasValue)
            {
                poll.WaitTime = 20;
            }
            if (!poll.MaxMessages.HasValue)
            {
                poll.MaxMessages = 1;
            }
            while (!cancellationToken.IsCancellationRequested)
            {
                IList<Message> messages;
                try
                {
                    messages = await ConsumeAsync(topic, poll, cancellationToken).ConfigureAwait(false);
                }
                catch (OperationCanceledException) when (cancellationToken.IsCancellationRequested)
                {
                    return;
                }
                foreach (Message message in messages)
                {
                    bool handled;
                    try
                    {
                        await handler(message).ConfigureAwait(false);
                        handled = true;
                    }
                    catch (Exception e)
                    {
                        handled = false;
                        if (onError != null)
                        {
                            onError(message, e);
                        }
                    }
                    if (handled)
                    {
                        try
                        {
                            // Sem o token: a mensagem já foi tratada, e confirmá-la não deve ser cortado pelo cancelamento.
                            await AckAsync(topic, message, CancellationToken.None).ConfigureAwait(false);
                        }
                        catch (TabaException e) when (e.Status == 409 || e.Status == 404)
                        {
                            // A mensagem já voltou à fila (prazo vencido) ou já foi embora: não há o que confirmar.
                        }
                    }
                    if (cancellationToken.IsCancellationRequested)
                    {
                        return;
                    }
                }
            }
        }

        // -----------------------------------------------------------------------------------------
        // A forma síncrona: espera pela assíncrona
        // -----------------------------------------------------------------------------------------

        public Topic CreateTopic(TopicRequest request)
        {
            return Wait(CreateTopicAsync(request));
        }

        public Topic GetTopic(string name)
        {
            return Wait(GetTopicAsync(name));
        }

        public Page<Topic> ListTopics(int? limit = null, string cursor = null, string name = null)
        {
            return Wait(ListTopicsAsync(limit, cursor, name));
        }

        public IList<Topic> ListAllTopics(string name = null)
        {
            return Wait(ListAllTopicsAsync(name));
        }

        public Topic UpdateTopic(string name, TopicUpdate update)
        {
            return Wait(UpdateTopicAsync(name, update));
        }

        public void DeleteTopic(string name)
        {
            Wait(DeleteTopicAsync(name));
        }

        public PublishResponse Publish(string topic, byte[] body, PublishOptions options = null)
        {
            return Wait(PublishAsync(topic, body, options));
        }

        public PublishResponse PublishJson(string topic, string json)
        {
            return Wait(PublishJsonAsync(topic, json));
        }

        public PublishResponse PublishText(string topic, string text)
        {
            return Wait(PublishTextAsync(topic, text));
        }

        public IList<Message> Consume(string topic, ConsumeOptions options = null)
        {
            return Wait(ConsumeAsync(topic, options));
        }

        public void Ack(string topic, string messageId, string receiptToken)
        {
            Wait(AckAsync(topic, messageId, receiptToken));
        }

        public void Ack(string topic, Message message)
        {
            Wait(AckAsync(topic, message));
        }

        public VisibilityResponse ChangeVisibility(string topic, string messageId, string receiptToken, int seconds)
        {
            return Wait(ChangeVisibilityAsync(topic, messageId, receiptToken, seconds));
        }

        public VisibilityResponse ChangeVisibility(string topic, Message message, int seconds)
        {
            return Wait(ChangeVisibilityAsync(topic, message, seconds));
        }

        /// <summary>A forma síncrona do <see cref="SubscribeAsync"/>: bloqueia a thread até o cancelamento.</summary>
        public void Subscribe(string topic, ConsumeOptions options, Action<Message> handler, CancellationToken cancellationToken, Action<Message, Exception> onError = null)
        {
            if (handler == null)
            {
                throw new ArgumentNullException("handler");
            }
            Wait(SubscribeAsync(
                topic,
                options,
                message =>
                {
                    handler(message);
                    return Task.FromResult(0);
                },
                cancellationToken,
                onError));
        }

        private static void Wait(Task task)
        {
            try
            {
                task.GetAwaiter().GetResult();
            }
            catch (AggregateException e) when (e.InnerExceptions.Count == 1)
            {
                throw e.InnerExceptions[0];
            }
        }

        private static T Wait<T>(Task<T> task)
        {
            Wait((Task)task);
            return task.GetAwaiter().GetResult();
        }
    }
}
