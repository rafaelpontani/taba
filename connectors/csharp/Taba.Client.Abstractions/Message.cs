namespace Taba
{
    /// <summary>Uma mensagem entregue. O <see cref="Body"/> sai sempre como bytes (doc 19, seção 18.4).</summary>
    public sealed class Message
    {
        internal Message()
        {
        }

        public string Id { get; internal set; }

        /// <summary>
        /// JSON: o texto JSON tal qual veio. <c>text/*</c> em UTF-8: o texto em UTF-8. Base64: os bytes
        /// já decodificados.
        /// </summary>
        public byte[] Body { get; internal set; }

        public string ContentType { get; internal set; }

        public string ReceiptToken { get; internal set; }

        public long DeliveryCount { get; internal set; }

        /// <summary>Data de publicação, como texto RFC 3339.</summary>
        public string PublishedAt { get; internal set; }

        /// <summary>Até quando a mensagem fica invisível, como texto RFC 3339.</summary>
        public string InvisibleUntil { get; internal set; }

        public string GroupId { get; internal set; }

        public string DeduplicationId { get; internal set; }

        public string DeadLetteredAt { get; internal set; }

        public string DeadLetterReason { get; internal set; }

        /// <summary>O JSON da mensagem, como o servidor devolveu.</summary>
        public string Raw { get; internal set; }

        /// <summary>O <see cref="Body"/> como texto UTF-8.</summary>
        public string BodyText
        {
            get { return Utf8.Encoding.GetString(Body); }
        }
    }
}
