using System.Collections.Generic;

namespace Taba
{
    /// <summary>
    /// As opções de <c>consume</c>. O conector manda só o que foi informado: o que faltar vale o padrão
    /// do servidor.
    /// </summary>
    public sealed class ConsumeOptions
    {
        public int? MaxMessages { get; set; }

        /// <summary>Segundos de invisibilidade das mensagens entregues.</summary>
        public int? VisibilityTimeout { get; set; }

        /// <summary>Segundos de long polling.</summary>
        public int? WaitTime { get; set; }

        public bool AutoCreate { get; set; }

        /// <summary>Os quatro seguintes só valem com <see cref="AutoCreate"/>.</summary>
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
