package io.github.rafaelpontani.taba;

import java.util.List;
import java.util.Map;

/**
 * {@code PATCH /topics/{topic}}: only what was filled in is sent. For {@code maxMessageBytes}, {@code ttl} and
 * {@code maxDeliveries}, the {@code ...Default()} methods send {@code null}: the topic goes back to the server default.
 */
public final class TopicUpdate {
    private Map<String, String> tags;
    private List<String> prefixes;
    private Long idleTimeout;
    private String maxMessageBytes;
    private String ttl;
    private String maxDeliveries;

    /** Replaces the whole map (an empty map clears the tags). */
    public TopicUpdate tags(Map<String, String> tags) {
        this.tags = tags;
        return this;
    }

    public TopicUpdate prefixes(List<String> prefixes) {
        this.prefixes = prefixes;
        return this;
    }

    public TopicUpdate idleTimeout(long idleTimeout) {
        this.idleTimeout = idleTimeout;
        return this;
    }

    public TopicUpdate maxMessageBytes(long value) {
        this.maxMessageBytes = Long.toString(value);
        return this;
    }

    public TopicUpdate maxMessageBytesDefault() {
        this.maxMessageBytes = "null";
        return this;
    }

    public TopicUpdate ttl(long value) {
        this.ttl = Long.toString(value);
        return this;
    }

    public TopicUpdate ttlDefault() {
        this.ttl = "null";
        return this;
    }

    public TopicUpdate maxDeliveries(long value) {
        this.maxDeliveries = Long.toString(value);
        return this;
    }

    public TopicUpdate maxDeliveriesDefault() {
        this.maxDeliveries = "null";
        return this;
    }

    String toJson() {
        StringBuilder out = new StringBuilder("{");
        boolean first = true;
        if (tags != null) {
            out.append("\"tags\":").append(TopicRequest.map(tags));
            first = false;
        }
        if (prefixes != null) {
            out.append(first ? "" : ",").append("\"prefixes\":").append(TopicRequest.list(prefixes));
            first = false;
        }
        String[][] numbers = {
            {"idleTimeout", idleTimeout == null ? null : idleTimeout.toString()},
            {"maxMessageBytes", maxMessageBytes},
            {"ttl", ttl},
            {"maxDeliveries", maxDeliveries},
        };
        for (String[] number : numbers) {
            if (number[1] != null) {
                out.append(first ? "" : ",").append('"').append(number[0]).append("\":").append(number[1]);
                first = false;
            }
        }
        return out.append('}').toString();
    }
}
