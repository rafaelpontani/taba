package io.github.rafaelpontani.taba;

import java.nio.charset.StandardCharsets;

/**
 * One message of {@link TabaClient#publishBatch(String, java.util.List)}: the same fields as
 * {@link TabaClient#publish(String, byte[], PublishOptions)}. The bytes travel in base64, so any type fits.
 */
public final class BatchMessage {
    private final byte[] body;
    private String contentType = "application/json";
    private String groupId;
    private String deduplicationId;

    public BatchMessage(byte[] body) {
        if (body == null) {
            throw new NullPointerException("body");
        }
        this.body = body;
    }

    /** A text ({@code text/plain; charset=utf-8}). */
    public static BatchMessage text(String text) {
        return new BatchMessage(text.getBytes(StandardCharsets.UTF_8)).contentType("text/plain; charset=utf-8");
    }

    /** A JSON text the application already built ({@code application/json}). */
    public static BatchMessage json(String json) {
        return new BatchMessage(json.getBytes(StandardCharsets.UTF_8)).contentType("application/json");
    }

    /** What the bytes are (default {@code application/json}). */
    public BatchMessage contentType(String contentType) {
        this.contentType = contentType;
        return this;
    }

    /** Required on a FIFO queue; elsewhere it guides the fair delivery. */
    public BatchMessage groupId(String groupId) {
        this.groupId = groupId;
        return this;
    }

    /** FIFO queues only: the deduplication key inside the group. */
    public BatchMessage deduplicationId(String deduplicationId) {
        this.deduplicationId = deduplicationId;
        return this;
    }

    public byte[] getBody() {
        return body;
    }

    public String getContentType() {
        return contentType;
    }

    public String getGroupId() {
        return groupId;
    }

    public String getDeduplicationId() {
        return deduplicationId;
    }

    /** The item of the request: the body in base64, the other fields only when given. */
    String toJson() {
        StringBuilder out = new StringBuilder(body.length * 4 / 3 + 96);
        out.append("{\"body\":\"").append(java.util.Base64.getEncoder().encodeToString(body))
                .append("\",\"bodyEncoding\":\"base64\"");
        if (contentType != null) {
            out.append(",\"contentType\":").append(Json.quote(contentType));
        }
        if (groupId != null) {
            out.append(",\"groupId\":").append(Json.quote(groupId));
        }
        if (deduplicationId != null) {
            out.append(",\"deduplicationId\":").append(Json.quote(deduplicationId));
        }
        return out.append('}').toString();
    }
}
