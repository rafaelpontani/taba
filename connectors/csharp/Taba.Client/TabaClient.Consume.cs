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
        internal static ApiCall ConsumeCall(string topic, ConsumeOptions options)
        {
            options = options ?? new ConsumeOptions();
            var call = new ApiCall(HttpMethod.Get, "/topics/" + Segment(topic) + "/messages");
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
        /// <c>GET /topics/{topic}/messages</c>. The read time is <c>waitTime + 10 s</c>, not the
        /// <c>RequestTimeout</c>. A timeout is not retried (the node may have captured messages, which
        /// come back by themselves at the end of the <c>visibilityTimeout</c>). Returns an empty list if there are no messages.
        /// </summary>
        public async Task<IList<Message>> ConsumeAsync(string topic, ConsumeOptions options = null, CancellationToken cancellationToken = default(CancellationToken))
        {
            ApiReply reply = await SendAsync(ConsumeCall(topic, options), cancellationToken).ConfigureAwait(false);
            JsonFields fields = ModelParser.Fields(reply.Text, "consumo");
            var messages = new List<Message>();
            try
            {
                foreach (string item in Json.ArrayItems(fields.Raw("messages") ?? "[]"))
                {
                    messages.Add(ModelParser.ParseMessage(item));
                }
            }
            catch (JsonException e)
            {
                throw TabaException.Client("client.invalid-response", "messages: " + e.Message);
            }
            return messages;
        }

        /// <summary><c>DELETE /topics/{topic}/messages/{id}?receiptToken=</c>.</summary>
        public async Task AckAsync(string topic, string messageId, string receiptToken, CancellationToken cancellationToken = default(CancellationToken))
        {
            var call = new ApiCall(HttpMethod.Delete, "/topics/" + Segment(topic) + "/messages/" + Segment(messageId))
                .With("receiptToken", receiptToken);
            await SendAsync(call, cancellationToken).ConfigureAwait(false);
        }

        /// <summary>Acknowledges the message received from <see cref="ConsumeAsync"/>.</summary>
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
            var call = new ApiCall(HttpMethod.Put, "/topics/" + Segment(topic) + "/messages/" + Segment(messageId) + "/visibility")
                .With("receiptToken", receiptToken)
                .Json("{\"visibilityTimeout\":" + seconds.ToString(CultureInfo.InvariantCulture) + "}");
            ApiReply reply = await SendAsync(call, cancellationToken).ConfigureAwait(false);
            return ModelParser.ParseVisibilityResponse(reply.Text);
        }

        /// <summary>Changes the invisibility of the message received from <see cref="ConsumeAsync"/>.</summary>
        public Task<VisibilityResponse> ChangeVisibilityAsync(string topic, Message message, int seconds, CancellationToken cancellationToken = default(CancellationToken))
        {
            return ChangeVisibilityAsync(topic, message.Id, message.ReceiptToken, seconds, cancellationToken);
        }

    }
}
