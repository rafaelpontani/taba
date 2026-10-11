using System;

namespace Taba
{
    /// <summary>
    /// A única forma de erro do conector (doc 19, seção 18.4): o <c>ErrorResponse</c> da API, ou uma
    /// falha do próprio cliente, com <see cref="Status"/> zero e <see cref="Code"/>
    /// <c>client.network</c>, <c>client.timeout</c>, <c>client.invalid-config</c> ou
    /// <c>client.invalid-response</c>. O programa da aplicação testa o <see cref="Code"/>, nunca o
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

        /// <summary>O status HTTP, ou zero numa falha do próprio cliente.</summary>
        public int Status { get; }

        /// <summary>O código <c>grupo.motivo</c> do erro.</summary>
        public string Code { get; }

        /// <summary>O <c>requestId</c> do <c>ErrorResponse</c>, ou o cabeçalho <c>X-Request-Id</c>.</summary>
        public string RequestId { get; internal set; }

        /// <summary>Os segundos do <c>Retry-After</c>, se a resposta trouxe.</summary>
        public int? RetryAfter { get; internal set; }

        /// <summary>A lista <c>details</c> do <c>ErrorResponse</c>, como texto JSON.</summary>
        public string Details { get; internal set; }

        /// <summary>O corpo da resposta, como veio (vazio numa falha do cliente).</summary>
        public byte[] Body { get; internal set; }

        /// <summary>Falha do próprio cliente, e não uma resposta do servidor.</summary>
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
