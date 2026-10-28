package io.github.rafaelpontani.taba;

import java.util.Map;

/**
 * The {@code consume} options. The connector sends only what was set: whatever is missing takes the server
 * default.
 */
public final class ConsumeOptions {
    private Integer maxMessages;
    private Integer visibilityTimeout;
    private Integer waitTime;
    private boolean autoCreate;
    private Boolean volatileTopic;
    private Integer idleTimeout;
    private Boolean fifo;
    private Map<String, String> tags;
    private String requestId;
    private String traceparent;

    public ConsumeOptions maxMessages(Integer maxMessages) {
        this.maxMessages = maxMessages;
        return this;
    }

    /** Seconds of invisibility for the delivered messages. */
    public ConsumeOptions visibilityTimeout(Integer visibilityTimeout) {
        this.visibilityTimeout = visibilityTimeout;
        return this;
    }

    /** Seconds of long polling. */
    public ConsumeOptions waitTime(Integer waitTime) {
        this.waitTime = waitTime;
        return this;
    }

    public ConsumeOptions autoCreate(boolean autoCreate) {
        this.autoCreate = autoCreate;
        return this;
    }

    /** The next four only apply with {@link #autoCreate(boolean)}. */
    public ConsumeOptions volatileTopic(Boolean volatileTopic) {
        this.volatileTopic = volatileTopic;
        return this;
    }

    public ConsumeOptions idleTimeout(Integer idleTimeout) {
        this.idleTimeout = idleTimeout;
        return this;
    }

    public ConsumeOptions fifo(Boolean fifo) {
        this.fifo = fifo;
        return this;
    }

    public ConsumeOptions tags(Map<String, String> tags) {
        this.tags = tags;
        return this;
    }

    public ConsumeOptions requestId(String requestId) {
        this.requestId = requestId;
        return this;
    }

    public ConsumeOptions traceparent(String traceparent) {
        this.traceparent = traceparent;
        return this;
    }

    public Integer getMaxMessages() {
        return maxMessages;
    }

    public Integer getVisibilityTimeout() {
        return visibilityTimeout;
    }

    public Integer getWaitTime() {
        return waitTime;
    }

    boolean isAutoCreate() {
        return autoCreate;
    }

    Boolean getVolatileTopic() {
        return volatileTopic;
    }

    Integer getIdleTimeout() {
        return idleTimeout;
    }

    Boolean getFifo() {
        return fifo;
    }

    Map<String, String> getTags() {
        return tags;
    }

    String getRequestId() {
        return requestId;
    }

    String getTraceparent() {
        return traceparent;
    }

    ConsumeOptions copy() {
        return new ConsumeOptions()
                .maxMessages(maxMessages)
                .visibilityTimeout(visibilityTimeout)
                .waitTime(waitTime)
                .autoCreate(autoCreate)
                .volatileTopic(volatileTopic)
                .idleTimeout(idleTimeout)
                .fifo(fifo)
                .tags(tags)
                .requestId(requestId)
                .traceparent(traceparent);
    }
}
