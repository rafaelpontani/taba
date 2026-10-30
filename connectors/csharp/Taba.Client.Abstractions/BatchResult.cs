namespace Taba
{
    /// <summary>
    /// The result of one item of a batch, in the order of the request: the <see cref="Status"/> the single call would
    /// have answered and what it would have returned. An item that failed does not throw: <see cref="IsOk"/> is
    /// <c>false</c> and <see cref="ErrorCode"/> says why.
    /// </summary>
    public sealed class BatchResult
    {
        internal BatchResult()
        {
        }

        /// <summary><c>201</c> published, <c>200</c> a FIFO duplicate, a distributor or a new invisibility, <c>204</c> acked, or the error's status.</summary>
        public int Status { get; internal set; }

        /// <summary>The item was applied (no error).</summary>
        public bool IsOk
        {
            get { return ErrorCode == null && Status < 300; }
        }

        /// <summary>The message <c>id</c>; <c>null</c> on an error and on a distributor's publication.</summary>
        public string Id { get; internal set; }

        /// <summary>FIFO queue only: the publication repeated one inside the deduplication window.</summary>
        public bool? Duplicate { get; internal set; }

        /// <summary>Distributor only: how many queues matched.</summary>
        public long? Matched { get; internal set; }

        /// <summary><c>ChangeVisibilityBatch</c> only: the new deadline.</summary>
        public string InvisibleUntil { get; internal set; }

        /// <summary>The error code of this item (<c>message.stale-receipt</c>...), <c>null</c> when it was applied.</summary>
        public string ErrorCode { get; internal set; }

        public string ErrorMessage { get; internal set; }

        public string Raw { get; internal set; }
    }
}
