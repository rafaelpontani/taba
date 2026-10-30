using System.Collections.Generic;
using System.Threading;
using System.Threading.Tasks;

namespace Taba
{
    /// <summary>
    /// The operations in batch (doc 21): publish, acknowledge and change the invisibility of many messages in one
    /// request. Each item is answered as its single call and one that fails does not undo the others. A separate
    /// interface, so whoever implements <see cref="ITabaClient"/> (a mock in tests) is not broken by it.
    /// </summary>
    public interface ITabaBatchClient
    {
        /// <summary>
        /// <c>POST /topics/{topic}/messages/batch</c>: 1 to the server's <c>api.batch.max-messages</c> (1000 by
        /// default). On a FIFO queue the items of a group are published in the order of the list. A failure of the
        /// network is not retried (it could duplicate).
        /// </summary>
        Task<IList<BatchResult>> PublishBatchAsync(string topic, IList<BatchMessage> messages, CancellationToken cancellationToken = default(CancellationToken));

        /// <summary><c>POST /topics/{topic}/messages/ack</c>: acknowledges the messages received from <c>ConsumeAsync</c>.</summary>
        Task<IList<BatchResult>> AckBatchAsync(string topic, IList<Message> messages, CancellationToken cancellationToken = default(CancellationToken));

        /// <summary><c>POST /topics/{topic}/messages/visibility</c>: the invisibility of each message, <paramref name="seconds"/> from now.</summary>
        Task<IList<BatchResult>> ChangeVisibilityBatchAsync(string topic, IList<Message> messages, int seconds, CancellationToken cancellationToken = default(CancellationToken));

        IList<BatchResult> PublishBatch(string topic, IList<BatchMessage> messages);

        IList<BatchResult> AckBatch(string topic, IList<Message> messages);

        IList<BatchResult> ChangeVisibilityBatch(string topic, IList<Message> messages, int seconds);
    }
}
