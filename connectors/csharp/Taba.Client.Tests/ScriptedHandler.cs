using System;
using System.Collections.Concurrent;
using System.Collections.Generic;
using System.Linq;
using System.Net.Http;
using System.Text;
using System.Threading;
using System.Threading.Tasks;

namespace Taba.Tests
{
    /// <summary>A request as the connector built it, copied at send time.</summary>
    public sealed class Seen
    {
        public string Method { get; set; }

        public string PathAndQuery { get; set; }

        public string Path { get; set; }

        public Dictionary<string, string> Headers { get; set; }

        public byte[] Body { get; set; }

        public string BodyText
        {
            get { return Encoding.UTF8.GetString(Body ?? new byte[0]); }
        }

        public string Header(string name)
        {
            string value;
            return Headers.TryGetValue(name.ToLowerInvariant(), out value) ? value : null;
        }
    }

    /// <summary>
    /// The tests' fake transport: returns, one per call, the agreed responses (or failures)
    /// and records what the connector sent.
    /// </summary>
    public sealed class ScriptedHandler : HttpMessageHandler
    {
        private readonly ConcurrentQueue<Func<CancellationToken, Task<HttpResponseMessage>>> _script =
            new ConcurrentQueue<Func<CancellationToken, Task<HttpResponseMessage>>>();

        private readonly ConcurrentQueue<Seen> _seen = new ConcurrentQueue<Seen>();

        public IReadOnlyList<Seen> Requests
        {
            get { return _seen.ToArray(); }
        }

        public Seen Last
        {
            get { return _seen.Last(); }
        }

        public ScriptedHandler Reply(int status, string body = "", string contentType = "application/json", params string[] headers)
        {
            _script.Enqueue(token => Task.FromResult(Response(status, body, contentType, headers)));
            return this;
        }

        public ScriptedHandler Fail(Exception error)
        {
            _script.Enqueue(token => { throw error; });
            return this;
        }

        /// <summary>Never responds: only ends when the connector gives up (time or cancellation).</summary>
        public ScriptedHandler Hang()
        {
            _script.Enqueue(async token =>
            {
                await Task.Delay(Timeout.Infinite, token);
                return null;
            });
            return this;
        }

        public static HttpResponseMessage Response(int status, string body, string contentType, params string[] headers)
        {
            var response = new HttpResponseMessage((System.Net.HttpStatusCode)status)
            {
                Content = new ByteArrayContent(Encoding.UTF8.GetBytes(body)),
            };
            response.Content.Headers.TryAddWithoutValidation("Content-Type", contentType);
            for (int i = 0; i + 1 < headers.Length; i += 2)
            {
                response.Headers.TryAddWithoutValidation(headers[i], headers[i + 1]);
            }
            return response;
        }

        protected override async Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken cancellationToken)
        {
            var headers = new Dictionary<string, string>();
            foreach (var header in request.Headers)
            {
                headers[header.Key.ToLowerInvariant()] = string.Join(",", header.Value);
            }
            byte[] body = null;
            if (request.Content != null)
            {
                body = await request.Content.ReadAsByteArrayAsync();
                foreach (var header in request.Content.Headers)
                {
                    headers[header.Key.ToLowerInvariant()] = string.Join(",", header.Value);
                }
            }
            _seen.Enqueue(new Seen
            {
                Method = request.Method.Method,
                PathAndQuery = request.RequestUri.PathAndQuery,
                Path = request.RequestUri.AbsolutePath,
                Headers = headers,
                Body = body,
            });
            Func<CancellationToken, Task<HttpResponseMessage>> next;
            if (!_script.TryDequeue(out next))
            {
                throw new InvalidOperationException("the connector made more calls than the script foresaw: " + request.Method + " " + request.RequestUri);
            }
            return await next(cancellationToken);
        }
    }
}
