using System.Collections.Generic;

namespace Taba
{
    /// <summary>
    /// <c>POST /topics</c>. O conector manda só o que foi preenchido; o que não vale para o tipo, o
    /// servidor recusa.
    /// </summary>
    public sealed class TopicRequest
    {
        public string Name { get; set; }

        /// <summary><c>queue</c> ou <c>distributor</c>.</summary>
        public string Type { get; set; }

        public IDictionary<string, string> Tags { get; set; }

        /// <summary>Só no distribuidor: os prefixos de nome das filas para onde cada publicação é copiada.</summary>
        public IList<string> Prefixes { get; set; }

        public bool? Volatile { get; set; }

        public long? IdleTimeout { get; set; }

        public bool? Fifo { get; set; }

        public long? MaxMessageBytes { get; set; }

        public long? Ttl { get; set; }

        public long? MaxDeliveries { get; set; }
    }
}
