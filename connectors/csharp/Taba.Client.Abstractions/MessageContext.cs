using System;
using System.Threading;
using System.Threading.Tasks;

namespace Taba
{
    /// <summary>
    /// What the <c>Consume</c> <c>handler</c> receives: the <see cref="Message"/> with all its properties,
    /// the <see cref="Delete"/> flag (the message is deleted at the end if the handler finishes without an exception; the default is
    /// yes) and <see cref="ChangeVisibilityAsync"/>. While the handler runs, the connector renews the
    /// invisibility by itself, so no other consumer takes the message.
    /// </summary>
    public class MessageContext
    {
        private readonly Func<int, CancellationToken, Task> _change;
        private int _leaseSeconds;
        private int _lost;

        internal MessageContext(Message message, int leaseSeconds, Func<int, CancellationToken, Task> change, CancellationToken cancellationToken)
        {
            Message = message;
            _leaseSeconds = leaseSeconds;
            _change = change;
            CancellationToken = cancellationToken;
        }

        public Message Message { get; private set; }

        /// <summary>
        /// <c>true</c> (the default): the message is deleted (ack) when the handler finishes without an exception. Set
        /// <c>false</c> to leave it in the queue: it becomes visible again at the end of the invisibility.
        /// </summary>
        public bool Delete { get; set; } = true;

        /// <summary>The <c>Consume</c> token, for the handler to pass on to its own calls.</summary>
        public CancellationToken CancellationToken { get; private set; }

        /// <summary>The seconds of invisibility the automatic renewal requests on each round.</summary>
        public int LeaseSeconds
        {
            get { return Volatile.Read(ref _leaseSeconds); }
        }

        /// <summary><c>true</c> when the server refused the renewal (the message already went back to the queue or was deleted).</summary>
        public bool LeaseLost
        {
            get { return Volatile.Read(ref _lost) != 0; }
        }

        /// <summary>
        /// Changes the invisibility to <paramref name="seconds"/> from now on. The value becomes the one
        /// of the automatic renewal.
        /// </summary>
        public async Task ChangeVisibilityAsync(int seconds)
        {
            await _change(seconds, CancellationToken).ConfigureAwait(false);
            Volatile.Write(ref _leaseSeconds, seconds);
        }

        /// <summary>The synchronous form of <see cref="ChangeVisibilityAsync"/>.</summary>
        public void ChangeVisibility(int seconds)
        {
            ChangeVisibilityAsync(seconds).GetAwaiter().GetResult();
        }

        internal void MarkLost()
        {
            Volatile.Write(ref _lost, 1);
        }
    }
}
