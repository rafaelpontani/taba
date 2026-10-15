using System;
using System.Threading;
using System.Threading.Tasks;

namespace Taba
{
    /// <summary>
    /// The <see cref="MessageContext"/> of a <c>Consume&lt;T&gt;</c>: besides the message, it carries the body already
    /// deserialized in <see cref="Body"/>.
    /// </summary>
    public sealed class MessageContext<T> : MessageContext
    {
        internal MessageContext(Message message, T body, int leaseSeconds, Func<int, CancellationToken, Task> change, CancellationToken cancellationToken)
            : base(message, leaseSeconds, change, cancellationToken)
        {
            Body = body;
        }

        /// <summary>The message body deserialized as <typeparamref name="T"/> (by the <see cref="ITabaJsonSerializer"/>).</summary>
        public T Body { get; private set; }
    }
}
