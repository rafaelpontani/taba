using System.Collections.Generic;

namespace Taba
{
    /// <summary>A resposta de uma publicação: <c>PublishResponse</c> (fila) ou <c>DistributorPublishResponse</c>.</summary>
    public sealed class PublishResponse
    {
        internal PublishResponse()
        {
        }

        /// <summary>O <c>id</c> da mensagem (fila). Ausente na resposta de um distribuidor.</summary>
        public string Id { get; internal set; }

        /// <summary>Só na fila FIFO: <c>true</c> quando repetiu uma publicação da janela de deduplicação.</summary>
        public bool? Duplicate { get; internal set; }

        /// <summary>Só no distribuidor: quantas filas casaram.</summary>
        public long? Matched { get; internal set; }

        /// <summary>Só no distribuidor: uma entrada por fila que casou.</summary>
        public IList<DistributorDelivery> Deliveries { get; internal set; }

        public string Raw { get; internal set; }
    }
}
