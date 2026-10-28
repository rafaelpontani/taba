package io.github.rafaelpontani.taba;

import java.nio.charset.StandardCharsets;

/** A delivered message. The {@link #getBody() body} always comes out as bytes (doc 21, "The delivered message"). */
public final class Message {
    private final String id;
    private final byte[] body;
    private final String contentType;
    private final String receiptToken;
    private final long deliveryCount;
    private final String publishedAt;
    private final String invisibleUntil;
    private final String groupId;
    private final String deduplicationId;
    private final String deadLetteredAt;
    private final String deadLetterReason;
    private final String raw;

    Message(
            String id,
            byte[] body,
            String contentType,
            String receiptToken,
            long deliveryCount,
            String publishedAt,
            String invisibleUntil,
            String groupId,
            String deduplicationId,
            String deadLetteredAt,
            String deadLetterReason,
            String raw) {
        this.id = id;
        this.body = body;
        this.contentType = contentType;
        this.receiptToken = receiptToken;
        this.deliveryCount = deliveryCount;
        this.publishedAt = publishedAt;
        this.invisibleUntil = invisibleUntil;
        this.groupId = groupId;
        this.deduplicationId = deduplicationId;
        this.deadLetteredAt = deadLetteredAt;
        this.deadLetterReason = deadLetterReason;
        this.raw = raw;
    }

    public String getId() {
        return id;
    }

    /**
     * JSON: the JSON text as it came. {@code text/*} in UTF-8: the UTF-8 text. Base64: the bytes already decoded.
     * A copy: changing it does not change the message.
     */
    public byte[] getBody() {
        return body.clone();
    }

    /** The {@link #getBody() body} as UTF-8 text. */
    public String getBodyText() {
        return new String(body, StandardCharsets.UTF_8);
    }

    public String getContentType() {
        return contentType;
    }

    public String getReceiptToken() {
        return receiptToken;
    }

    public long getDeliveryCount() {
        return deliveryCount;
    }

    /** Publication date, as RFC 3339 text. */
    public String getPublishedAt() {
        return publishedAt;
    }

    /** Until when the message stays invisible, as RFC 3339 text. */
    public String getInvisibleUntil() {
        return invisibleUntil;
    }

    public String getGroupId() {
        return groupId;
    }

    public String getDeduplicationId() {
        return deduplicationId;
    }

    public String getDeadLetteredAt() {
        return deadLetteredAt;
    }

    public String getDeadLetterReason() {
        return deadLetterReason;
    }

    /** The message JSON, as the server returned it. */
    public String getRaw() {
        return raw;
    }

    @Override
    public String toString() {
        return "Message{" + id + "}";
    }
}
