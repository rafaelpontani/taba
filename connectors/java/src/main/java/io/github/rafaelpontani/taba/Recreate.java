package io.github.rafaelpontani.taba;

/**
 * What {@link TabaClient#createOrUpdateTopic(TopicRequest, Recreate)} does when the topic exists with another
 * {@code type}, {@code fifo} or {@code volatile} (fields that do not change in place).
 */
public enum Recreate {
    /** Refuses with {@code 409 topic.immutable-mismatch}; nothing changes. */
    NEVER(null),
    /**
     * Removes and creates it again only if the queue and its dead-letter hold no message on any node
     * ({@code 409 topic.not-empty} otherwise).
     */
    IF_EMPTY("if-empty"),
    /** Removes it with its messages and dead-letter and creates it again (new {@code id}). */
    ALWAYS("always");

    private final String queryValue;

    Recreate(String queryValue) {
        this.queryValue = queryValue;
    }

    String queryValue() {
        return queryValue;
    }
}
