using System.Collections.Generic;

namespace Taba
{
    /// <summary><c>PATCH /topics/{topic}</c>: only what was filled in is sent.</summary>
    public sealed class TopicUpdate
    {
        /// <summary>Replaces the whole map (an empty map clears the tags).</summary>
        public IDictionary<string, string> Tags { get; set; }

        public IList<string> Prefixes { get; set; }

        public long? IdleTimeout { get; set; }

        public Field<long> MaxMessageBytes { get; set; }

        public Field<long> Ttl { get; set; }

        public Field<long> MaxDeliveries { get; set; }
    }
}
