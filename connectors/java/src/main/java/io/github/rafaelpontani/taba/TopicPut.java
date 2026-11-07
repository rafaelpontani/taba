package io.github.rafaelpontani.taba;

/** The answer of {@link TabaClient#createOrUpdateTopic(TopicRequest, Recreate)}: the topic as it is now and what was done. */
public final class TopicPut {
    private final Topic topic;
    private final TopicOutcome outcome;

    public TopicPut(Topic topic, TopicOutcome outcome) {
        this.topic = topic;
        this.outcome = outcome;
    }

    public Topic getTopic() {
        return topic;
    }

    public TopicOutcome getOutcome() {
        return outcome;
    }
}
