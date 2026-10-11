namespace Taba
{
    /// <summary>As opções de <c>publish</c>.</summary>
    public sealed class PublishOptions
    {
        /// <summary>Padrão <c>application/json</c>.</summary>
        public string ContentType { get; set; }

        public string GroupId { get; set; }

        /// <summary>Só na fila FIFO.</summary>
        public string DeduplicationId { get; set; }

        public string RequestId { get; set; }

        /// <summary>Cabeçalho <c>traceparent</c> (W3C Trace Context), se a aplicação usa.</summary>
        public string Traceparent { get; set; }

        /// <summary>Marque quando o tópico é uma fila FIFO: só nela o <c>502</c>/<c>504</c> pode ser repetido.</summary>
        public bool Fifo { get; set; }
    }
}
