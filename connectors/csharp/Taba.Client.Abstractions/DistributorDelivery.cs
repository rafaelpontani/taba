namespace Taba
{
    /// <summary>The copy of a publication made by a distributor to a queue.</summary>
    public sealed class DistributorDelivery
    {
        internal DistributorDelivery(string topic, string id, string errorCode, string errorMessage)
        {
            Topic = topic;
            Id = id;
            ErrorCode = errorCode;
            ErrorMessage = errorMessage;
        }

        /// <summary>The destination queue.</summary>
        public string Topic { get; private set; }

        /// <summary>The <c>id</c> of the copy, when it was created.</summary>
        public string Id { get; private set; }

        /// <summary>The error <c>code</c>, when the copy failed (a partial failure is not a distributor error).</summary>
        public string ErrorCode { get; private set; }

        public string ErrorMessage { get; private set; }

        public bool Succeeded
        {
            get { return ErrorCode == null; }
        }
    }
}
