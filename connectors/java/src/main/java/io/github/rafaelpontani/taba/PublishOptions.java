package io.github.rafaelpontani.taba;

/** The {@code publish} options. */
public final class PublishOptions {
    private String contentType;
    private String groupId;
    private String deduplicationId;
    private String requestId;
    private String traceparent;
    private boolean fifo;

    /** Default {@code application/json}. */
    public PublishOptions contentType(String contentType) {
        this.contentType = contentType;
        return this;
    }

    public PublishOptions groupId(String groupId) {
        this.groupId = groupId;
        return this;
    }

    /** FIFO queue only. */
    public PublishOptions deduplicationId(String deduplicationId) {
        this.deduplicationId = deduplicationId;
        return this;
    }

    public PublishOptions requestId(String requestId) {
        this.requestId = requestId;
        return this;
    }

    /** {@code traceparent} header (W3C Trace Context), if the application uses it. */
    public PublishOptions traceparent(String traceparent) {
        this.traceparent = traceparent;
        return this;
    }

    /** Set it when the topic is a FIFO queue: only there can a {@code 502}/{@code 504} be retried. */
    public PublishOptions fifo(boolean fifo) {
        this.fifo = fifo;
        return this;
    }

    String getContentType() {
        return contentType;
    }

    String getGroupId() {
        return groupId;
    }

    String getDeduplicationId() {
        return deduplicationId;
    }

    String getRequestId() {
        return requestId;
    }

    String getTraceparent() {
        return traceparent;
    }

    boolean isFifo() {
        return fifo;
    }

    PublishOptions copy() {
        return new PublishOptions()
                .contentType(contentType)
                .groupId(groupId)
                .deduplicationId(deduplicationId)
                .requestId(requestId)
                .traceparent(traceparent)
                .fifo(fifo);
    }
}
