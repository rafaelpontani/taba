using System.Threading;
using System.Threading.Tasks;

namespace Taba
{
    /// <summary>
    /// Declaring a topic (doc 21): <c>PUT /topics/{topic}</c> and the <c>DELETE</c> that only removes an empty queue.
    /// A separate interface, so whoever implements <see cref="ITabaClient"/> (a mock in tests) is not broken by it.
    /// </summary>
    public interface ITabaTopicPutClient
    {
        /// <summary>
        /// <c>PUT /topics/{topic}</c>: brings the topic to the declared state, creating it if it does not exist. What
        /// is left out goes back to the server default (a full replacement, unlike <c>UpdateTopicAsync</c>);
        /// <c>request.Name</c> is the topic of the path. Another <c>Type</c>, <c>Fifo</c> or <c>Volatile</c> is
        /// <c>409 topic.immutable-mismatch</c> unless <paramref name="recreate"/> says otherwise. Repeated after a
        /// <c>503</c> like the other calls: the route is idempotent.
        /// </summary>
        Task<TopicPut> CreateOrUpdateTopicAsync(TopicRequest request, Recreate recreate = Recreate.Never, CancellationToken cancellationToken = default(CancellationToken));

        /// <summary>
        /// <c>DELETE /topics/{topic}?ifEmpty=true</c>: removes the queue only if it and its dead-letter hold no message
        /// on any node (<c>409 topic.not-empty</c> otherwise; a distributor holds none).
        /// </summary>
        Task DeleteTopicIfEmptyAsync(string name, CancellationToken cancellationToken = default(CancellationToken));

        TopicPut CreateOrUpdateTopic(TopicRequest request, Recreate recreate = Recreate.Never);

        void DeleteTopicIfEmpty(string name);
    }
}
