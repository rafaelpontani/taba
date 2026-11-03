using System;
using System.Collections.Generic;
using System.Globalization;
using System.Net.Http;
using System.Text;
using System.Threading;
using System.Threading.Tasks;

namespace Taba
{
    public sealed partial class TabaClient
    {
        /// <summary>
        /// <c>POST /topics/{topic}/messages/batch</c>: publishes many messages in one request. Each item is answered
        /// as its single publication and one that fails does not undo the others (see <see cref="BatchResult.IsOk"/>).
        /// </summary>
        public async Task<IList<BatchResult>> PublishBatchAsync(
            string topic,
            IList<BatchMessage> messages,
            CancellationToken cancellationToken = default(CancellationToken))
        {
            if (messages == null)
            {
                throw new ArgumentNullException("messages");
            }
            var json = new StringBuilder("{\"messages\":[");
            for (int i = 0; i < messages.Count; i++)
            {
                BatchMessage message = messages[i];
                if (i > 0)
                {
                    json.Append(',');
                }
                json.Append("{\"body\":\"").Append(Convert.ToBase64String(message.Body)).Append("\",\"bodyEncoding\":\"base64\"");
                AppendField(json, "contentType", message.ContentType);
                AppendField(json, "groupId", message.GroupId);
                AppendField(json, "deduplicationId", message.DeduplicationId);
                json.Append('}');
            }
            var call = new ApiCall(HttpMethod.Post, "/topics/" + Segment(topic) + "/messages/batch")
                .Json(json.Append("]}").ToString());
            ApiReply reply = await SendAsync(call, cancellationToken).ConfigureAwait(false);
            return ModelParser.ParseBatchResults(reply.Text);
        }

        /// <summary><c>POST /topics/{topic}/messages/ack</c>: acknowledges many messages received from <see cref="ConsumeAsync"/>.</summary>
        public async Task<IList<BatchResult>> AckBatchAsync(
            string topic,
            IList<Message> messages,
            CancellationToken cancellationToken = default(CancellationToken))
        {
            var call = new ApiCall(HttpMethod.Post, "/topics/" + Segment(topic) + "/messages/ack")
                .Json(Receipts(messages, string.Empty));
            ApiReply reply = await SendAsync(call, cancellationToken).ConfigureAwait(false);
            return ModelParser.ParseBatchResults(reply.Text);
        }

        /// <summary><c>POST /topics/{topic}/messages/visibility</c>: the invisibility of each message, <paramref name="seconds"/> from now.</summary>
        public async Task<IList<BatchResult>> ChangeVisibilityBatchAsync(
            string topic,
            IList<Message> messages,
            int seconds,
            CancellationToken cancellationToken = default(CancellationToken))
        {
            var call = new ApiCall(HttpMethod.Post, "/topics/" + Segment(topic) + "/messages/visibility")
                .Json(Receipts(messages, ",\"visibilityTimeout\":" + seconds.ToString(CultureInfo.InvariantCulture)));
            ApiReply reply = await SendAsync(call, cancellationToken).ConfigureAwait(false);
            return ModelParser.ParseBatchResults(reply.Text);
        }

        public IList<BatchResult> PublishBatch(string topic, IList<BatchMessage> messages)
        {
            return Wait(PublishBatchAsync(topic, messages));
        }

        public IList<BatchResult> AckBatch(string topic, IList<Message> messages)
        {
            return Wait(AckBatchAsync(topic, messages));
        }

        public IList<BatchResult> ChangeVisibilityBatch(string topic, IList<Message> messages, int seconds)
        {
            return Wait(ChangeVisibilityBatchAsync(topic, messages, seconds));
        }

        private static void AppendField(StringBuilder json, string name, string value)
        {
            if (value != null)
            {
                json.Append(",\"").Append(name).Append("\":").Append(Json.Quote(value));
            }
        }

        /// <summary>The items of an ack or a change of invisibility in batch: <c>id</c> and <c>receiptToken</c>, plus <paramref name="extra"/>.</summary>
        private static string Receipts(IList<Message> messages, string extra)
        {
            if (messages == null)
            {
                throw new ArgumentNullException("messages");
            }
            var json = new StringBuilder("{\"messages\":[");
            for (int i = 0; i < messages.Count; i++)
            {
                if (i > 0)
                {
                    json.Append(',');
                }
                json.Append("{\"id\":").Append(Json.Quote(messages[i].Id))
                    .Append(",\"receiptToken\":").Append(Json.Quote(messages[i].ReceiptToken))
                    .Append(extra).Append('}');
            }
            return json.Append("]}").ToString();
        }
    }
}
