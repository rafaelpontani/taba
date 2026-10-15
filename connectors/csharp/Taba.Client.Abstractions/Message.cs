namespace Taba
{
    /// <summary>A delivered message. The <see cref="Body"/> always comes out as bytes (doc 19, section 18.4).</summary>
    public sealed class Message
    {
        internal Message()
        {
        }

        public string Id { get; internal set; }

        /// <summary>
        /// JSON: the JSON text as it came. <c>text/*</c> in UTF-8: the UTF-8 text. Base64: the bytes
        /// already decoded.
        /// </summary>
        public byte[] Body { get; internal set; }

        public string ContentType { get; internal set; }

        public string ReceiptToken { get; internal set; }

        public long DeliveryCount { get; internal set; }

        /// <summary>Publication date, as RFC 3339 text.</summary>
        public string PublishedAt { get; internal set; }

        /// <summary>Until when the message stays invisible, as RFC 3339 text.</summary>
        public string InvisibleUntil { get; internal set; }

        public string GroupId { get; internal set; }

        public string DeduplicationId { get; internal set; }

        public string DeadLetteredAt { get; internal set; }

        public string DeadLetterReason { get; internal set; }

        /// <summary>The message JSON, as the server returned it.</summary>
        public string Raw { get; internal set; }

        /// <summary>The <see cref="Body"/> as UTF-8 text.</summary>
        public string BodyText
        {
            get { return Utf8.Encoding.GetString(Body); }
        }
    }
}
