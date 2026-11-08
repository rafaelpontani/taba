namespace Taba
{
    /// <summary>
    /// What <c>CreateOrUpdateTopicAsync</c> does when the topic exists with another <c>type</c>, <c>fifo</c> or
    /// <c>volatile</c> (fields that do not change in place).
    /// </summary>
    public enum Recreate
    {
        /// <summary>Refuses with <c>409 topic.immutable-mismatch</c>; nothing changes.</summary>
        Never = 0,

        /// <summary>
        /// Removes and creates it again only if the queue and its dead-letter hold no message on any node
        /// (<c>409 topic.not-empty</c> otherwise).
        /// </summary>
        IfEmpty = 1,

        /// <summary>Removes it with its messages and dead-letter and creates it again (new <c>Id</c>).</summary>
        Always = 2,
    }

    /// <summary>What <c>PUT /topics/{topic}</c> did.</summary>
    public enum TopicOutcome
    {
        /// <summary>It did not exist (<c>201</c>).</summary>
        Created = 0,

        /// <summary>It existed: the mutable fields are now the declared ones, or already were (<c>200</c>).</summary>
        Updated = 1,

        /// <summary>It was removed and created again with a new <c>Id</c> (<c>201</c> with <c>Taba-Topic-Recreated: true</c>).</summary>
        Recreated = 2,
    }

    /// <summary>The answer of <c>CreateOrUpdateTopicAsync</c>: the topic as it is now and what was done.</summary>
    public sealed class TopicPut
    {
        public TopicPut(Topic topic, TopicOutcome outcome)
        {
            Topic = topic;
            Outcome = outcome;
        }

        public Topic Topic { get; private set; }

        public TopicOutcome Outcome { get; private set; }
    }
}
