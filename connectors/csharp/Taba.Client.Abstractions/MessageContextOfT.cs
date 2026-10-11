using System;
using System.Threading;
using System.Threading.Tasks;

namespace Taba
{
    /// <summary>
    /// O <see cref="MessageContext"/> de um <c>Consume&lt;T&gt;</c>: além da mensagem, traz o corpo já
    /// deserializado em <see cref="Body"/>.
    /// </summary>
    public sealed class MessageContext<T> : MessageContext
    {
        internal MessageContext(Message message, T body, int leaseSeconds, Func<int, CancellationToken, Task> change, CancellationToken cancellationToken)
            : base(message, leaseSeconds, change, cancellationToken)
        {
            Body = body;
        }

        /// <summary>O corpo da mensagem deserializado como <typeparamref name="T"/> (pelo <see cref="ITabaJsonSerializer"/>).</summary>
        public T Body { get; private set; }
    }
}
