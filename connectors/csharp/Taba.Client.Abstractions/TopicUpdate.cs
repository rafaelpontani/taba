using System.Collections.Generic;

namespace Taba
{
    /// <summary><c>PATCH /topics/{topic}</c>: só o que foi preenchido é enviado.</summary>
    public sealed class TopicUpdate
    {
        /// <summary>Substitui o mapa inteiro (um mapa vazio limpa as tags).</summary>
        public IDictionary<string, string> Tags { get; set; }

        public IList<string> Prefixes { get; set; }

        public long? IdleTimeout { get; set; }

        public Field<long> MaxMessageBytes { get; set; }

        public Field<long> Ttl { get; set; }

        public Field<long> MaxDeliveries { get; set; }
    }
}
