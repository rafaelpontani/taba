using System.Collections.Generic;

namespace Taba
{
    /// <summary>The response of a publication: <c>PublishResponse</c> (queue) or <c>DistributorPublishResponse</c>.</summary>
    public sealed class PublishResponse
    {
        internal PublishResponse()
        {
        }

        /// <summary>The message <c>id</c> (queue). Absent from a distributor's response.</summary>
        public string Id { get; internal set; }

        /// <summary>FIFO queue only: <c>true</c> when it repeated a publication within the deduplication window.</summary>
        public bool? Duplicate { get; internal set; }

        /// <summary>Distributor only: how many queues matched.</summary>
        public long? Matched { get; internal set; }

        /// <summary>Distributor only: one entry per queue that matched.</summary>
        public IList<DistributorDelivery> Deliveries { get; internal set; }

        public string Raw { get; internal set; }
    }
}
