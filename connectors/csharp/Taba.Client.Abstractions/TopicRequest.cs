using System.Collections.Generic;

namespace Taba
{
    /// <summary>
    /// <c>POST /topics</c>. The connector sends only what was filled in; whatever does not apply to the type, the
    /// server refuses.
    /// </summary>
    public sealed class TopicRequest
    {
        public string Name { get; set; }

        /// <summary><c>queue</c> or <c>distributor</c>.</summary>
        public string Type { get; set; }

        public IDictionary<string, string> Tags { get; set; }

        /// <summary>Distributor only: the name prefixes of the queues each publication is copied to.</summary>
        public IList<string> Prefixes { get; set; }

        public bool? Volatile { get; set; }

        public long? IdleTimeout { get; set; }

        public bool? Fifo { get; set; }

        public long? MaxMessageBytes { get; set; }

        public long? Ttl { get; set; }

        public long? MaxDeliveries { get; set; }
    }
}
