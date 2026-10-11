namespace Taba
{
    /// <summary>Um tópico (fila ou distribuidor).</summary>
    public sealed class Topic
    {
        internal Topic()
        {
        }

        public string Id { get; internal set; }

        public string Name { get; internal set; }

        /// <summary><c>queue</c> ou <c>distributor</c>.</summary>
        public string Type { get; internal set; }

        /// <summary>Data de criação, como texto RFC 3339.</summary>
        public string CreatedAt { get; internal set; }

        public bool? Fifo { get; internal set; }

        public bool? Volatile { get; internal set; }

        /// <summary>O JSON do tópico, como o servidor devolveu.</summary>
        public string Raw { get; internal set; }
    }
}
