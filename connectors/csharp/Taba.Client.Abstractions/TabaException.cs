using System;

namespace Taba
{
    /// <summary>
    /// The only kind of error of the connector (doc 19, section 18.4): the API <c>ErrorResponse</c>, or a
    /// failure of the client itself, with <see cref="Status"/> zero and <see cref="Code"/>
    /// <c>client.network</c>, <c>client.timeout</c>, <c>client.invalid-config</c> or
    /// <c>client.invalid-response</c>. The application program tests the <see cref="Code"/>, never the
    /// <see cref="Exception.Message"/>.
    /// </summary>
    public sealed class TabaException : Exception
    {
        public TabaException(int status, string code, string message)
            : base(message)
        {
            Status = status;
            Code = code ?? string.Empty;
            Body = new byte[0];
        }

        /// <summary>The HTTP status, or zero on a failure of the client itself.</summary>
        public int Status { get; }

        /// <summary>The error's <c>group.reason</c> code.</summary>
        public string Code { get; }

        /// <summary>The <c>requestId</c> of the <c>ErrorResponse</c>, or the <c>X-Request-Id</c> header.</summary>
        public string RequestId { get; internal set; }

        /// <summary>The seconds of the <c>Retry-After</c>, if the response carried it.</summary>
        public int? RetryAfter { get; internal set; }

        /// <summary>The <c>details</c> list of the <c>ErrorResponse</c>, as JSON text.</summary>
        public string Details { get; internal set; }

        /// <summary>The response body, as it came (empty on a client failure).</summary>
        public byte[] Body { get; internal set; }

        /// <summary>A failure of the client itself, not a server response.</summary>
        public bool IsClientSide
        {
            get { return Status == 0; }
        }

        public override string ToString()
        {
            string id = RequestId == null ? string.Empty : " (requestId " + RequestId + ")";
            return Code + ": " + Message + id;
        }

        internal static TabaException Client(string code, string message)
        {
            return new TabaException(0, code, message);
        }

        internal static TabaException Client(string code, string message, Exception inner)
        {
            return Client(code, message + (inner == null ? string.Empty : ": " + inner.Message));
        }
    }
}
