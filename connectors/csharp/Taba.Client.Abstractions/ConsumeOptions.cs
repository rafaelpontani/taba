using System.Collections.Generic;

namespace Taba
{
    /// <summary>
    /// The <c>consume</c> options. The connector sends only what was set: whatever is missing takes the server
    /// default.
    /// </summary>
    public sealed class ConsumeOptions
    {
        public int? MaxMessages { get; set; }

        /// <summary>Seconds of invisibility for the delivered messages.</summary>
        public int? VisibilityTimeout { get; set; }

        /// <summary>Seconds of long polling.</summary>
        public int? WaitTime { get; set; }

        public bool AutoCreate { get; set; }

        /// <summary>The next four only apply with <see cref="AutoCreate"/>.</summary>
        public bool? Volatile { get; set; }

        public int? IdleTimeout { get; set; }

        public bool? Fifo { get; set; }

        public IDictionary<string, string> Tags { get; set; }

        public string RequestId { get; set; }

        public string Traceparent { get; set; }

        internal ConsumeOptions Clone()
        {
            return (ConsumeOptions)MemberwiseClone();
        }
    }
}
