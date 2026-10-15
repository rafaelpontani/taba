namespace Taba
{
    /// <summary>The <c>publish</c> options.</summary>
    public sealed class PublishOptions
    {
        /// <summary>Default <c>application/json</c>.</summary>
        public string ContentType { get; set; }

        public string GroupId { get; set; }

        /// <summary>FIFO queue only.</summary>
        public string DeduplicationId { get; set; }

        public string RequestId { get; set; }

        /// <summary><c>traceparent</c> header (W3C Trace Context), if the application uses it.</summary>
        public string Traceparent { get; set; }

        /// <summary>Set it when the topic is a FIFO queue: only there can a <c>502</c>/<c>504</c> be retried.</summary>
        public bool Fifo { get; set; }
    }
}
