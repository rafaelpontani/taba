namespace Taba
{
    /// <summary>A cópia de uma publicação feita por um distribuidor para uma fila.</summary>
    public sealed class DistributorDelivery
    {
        internal DistributorDelivery(string topic, string id, string errorCode, string errorMessage)
        {
            Topic = topic;
            Id = id;
            ErrorCode = errorCode;
            ErrorMessage = errorMessage;
        }

        /// <summary>A fila de destino.</summary>
        public string Topic { get; private set; }

        /// <summary>O <c>id</c> da cópia, quando foi criada.</summary>
        public string Id { get; private set; }

        /// <summary>O <c>code</c> do erro, quando a cópia falhou (falha parcial não é erro do distribuidor).</summary>
        public string ErrorCode { get; private set; }

        public string ErrorMessage { get; private set; }

        public bool Succeeded
        {
            get { return ErrorCode == null; }
        }
    }
}
