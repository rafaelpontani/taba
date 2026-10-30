using System;

namespace Taba
{
    /// <summary>
    /// One message of <see cref="ITabaBatchClient.PublishBatchAsync"/>: the same fields as
    /// <see cref="ITabaClient.PublishAsync"/>. The bytes travel in base64, so any type fits.
    /// </summary>
    public sealed class BatchMessage
    {
        public BatchMessage(byte[] body)
        {
            if (body == null)
            {
                throw new ArgumentNullException("body");
            }
            Body = body;
        }

        /// <summary>A text (<c>text/plain; charset=utf-8</c>).</summary>
        public static BatchMessage Text(string text)
        {
            return new BatchMessage(Utf8.Encoding.GetBytes(text)) { ContentType = "text/plain; charset=utf-8" };
        }

        public byte[] Body { get; private set; }

        /// <summary>What the bytes are (default <c>application/json</c>).</summary>
        public string ContentType { get; set; }

        /// <summary>Required on a FIFO queue; elsewhere it guides the fair delivery.</summary>
        public string GroupId { get; set; }

        /// <summary>FIFO queues only: the deduplication key inside the group.</summary>
        public string DeduplicationId { get; set; }
    }
}
