namespace Taba
{
    /// <summary>A topic (queue or distributor).</summary>
    public sealed class Topic
    {
        internal Topic()
        {
        }

        public string Id { get; internal set; }

        public string Name { get; internal set; }

        /// <summary><c>queue</c> or <c>distributor</c>.</summary>
        public string Type { get; internal set; }

        /// <summary>Creation date, as RFC 3339 text.</summary>
        public string CreatedAt { get; internal set; }

        public bool? Fifo { get; internal set; }

        public bool? Volatile { get; internal set; }

        /// <summary>The topic JSON, as the server returned it.</summary>
        public string Raw { get; internal set; }
    }
}
