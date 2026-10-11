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
    public sealed partial class TabaClient
    {
        /// <summary><c>POST /topics/{topic}/messages</c>. O corpo vai como bytes, sem validação.</summary>
        public async Task<PublishResponse> PublishAsync(
            string topic,
            byte[] body,
            PublishOptions options = null,
            CancellationToken cancellationToken = default(CancellationToken))
        {
            options = options ?? new PublishOptions();
            var call = new ApiCall(HttpMethod.Post, "/topics/" + Segment(topic) + "/messages")
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
            ApiReply reply = await SendAsync(call, cancellationToken).ConfigureAwait(false);
            return ModelParser.ParsePublishResponse(reply.Text);
        }

        /// <summary>
        /// Serializa <paramref name="value"/> em JSON (com o <see cref="TabaConfig.JsonSerializer"/>, ou o
        /// padrão do conector) e publica com <c>Content-Type: application/json</c>. Para controlar a
        /// serialização por conta própria, use o <see cref="PublishAsync"/> com os bytes.
        /// </summary>
        public Task<PublishResponse> PublishJsonAsync<T>(string topic, T value, CancellationToken cancellationToken = default(CancellationToken))
        {
            if (value == null)
            {
                throw new ArgumentNullException("value");
            }
            string json = (_config.JsonSerializer ?? DefaultSerializer).Serialize(value);
            return PublishAsync(topic, Utf8.Encoding.GetBytes(json), null, cancellationToken);
        }

        /// <summary>Publica um texto JSON que a aplicação já montou (<c>Content-Type: application/json</c>), sem serializar.</summary>
        public Task<PublishResponse> PublishRawJsonAsync(string topic, string json, CancellationToken cancellationToken = default(CancellationToken))
        {
            return PublishAsync(topic, Utf8.Encoding.GetBytes(json), null, cancellationToken);
        }

        /// <summary>Publica um texto (<c>Content-Type: text/plain; charset=utf-8</c>).</summary>
        public Task<PublishResponse> PublishTextAsync(string topic, string text, CancellationToken cancellationToken = default(CancellationToken))
        {
            var options = new PublishOptions { ContentType = "text/plain; charset=utf-8" };
            return PublishAsync(topic, Utf8.Encoding.GetBytes(text), options, cancellationToken);
        }

    }
}
