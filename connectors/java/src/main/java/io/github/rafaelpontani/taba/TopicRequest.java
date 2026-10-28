package io.github.rafaelpontani.taba;

import java.util.List;
import java.util.Map;

/**
 * {@code POST /topics}. The connector sends only what was filled in; whatever does not apply to the type, the
 * server refuses.
 */
public final class TopicRequest {
    private String name;
    private String type;
    private Map<String, String> tags;
    private List<String> prefixes;
    private Boolean volatileTopic;
    private Long idleTimeout;
    private Boolean fifo;
    private Long maxMessageBytes;
    private Long ttl;
    private Long maxDeliveries;

    public TopicRequest() {
    }

    public static TopicRequest queue(String name) {
        return new TopicRequest().name(name).type("queue");
    }

    public static TopicRequest distributor(String name) {
        return new TopicRequest().name(name).type("distributor");
    }

    public TopicRequest name(String name) {
        this.name = name;
        return this;
    }

    /** {@code queue} or {@code distributor}. */
    public TopicRequest type(String type) {
        this.type = type;
        return this;
    }

    public TopicRequest tags(Map<String, String> tags) {
        this.tags = tags;
        return this;
    }

    /** Distributor only: the name prefixes of the queues each publication is copied to. */
    public TopicRequest prefixes(List<String> prefixes) {
        this.prefixes = prefixes;
        return this;
    }

    public TopicRequest volatileTopic(Boolean volatileTopic) {
        this.volatileTopic = volatileTopic;
        return this;
    }

    public TopicRequest idleTimeout(Long idleTimeout) {
        this.idleTimeout = idleTimeout;
        return this;
    }

    public TopicRequest fifo(Boolean fifo) {
        this.fifo = fifo;
        return this;
    }

    public TopicRequest maxMessageBytes(Long maxMessageBytes) {
        this.maxMessageBytes = maxMessageBytes;
        return this;
    }

    public TopicRequest ttl(Long ttl) {
        this.ttl = ttl;
        return this;
    }

    public TopicRequest maxDeliveries(Long maxDeliveries) {
        this.maxDeliveries = maxDeliveries;
        return this;
    }

    public String getName() {
        return name;
    }

    String toJson() {
        StringBuilder out = new StringBuilder("{");
        out.append("\"name\":").append(Json.quote(name == null ? "" : name));
        out.append(",\"type\":").append(Json.quote(type == null ? "" : type));
        if (tags != null && !tags.isEmpty()) {
            out.append(",\"tags\":").append(map(tags));
        }
        if (prefixes != null && !prefixes.isEmpty()) {
            out.append(",\"prefixes\":").append(list(prefixes));
        }
        field(out, "volatile", volatileTopic);
        field(out, "idleTimeout", idleTimeout);
        field(out, "fifo", fifo);
        field(out, "maxMessageBytes", maxMessageBytes);
        field(out, "ttl", ttl);
        field(out, "maxDeliveries", maxDeliveries);
        return out.append('}').toString();
    }

    private static void field(StringBuilder out, String key, Object value) {
        if (value != null) {
            out.append(",\"").append(key).append("\":").append(value);
        }
    }

    static String map(Map<String, String> tags) {
        StringBuilder out = new StringBuilder("{");
        boolean first = true;
        for (Map.Entry<String, String> entry : tags.entrySet()) {
            out.append(first ? "" : ",").append(Json.quote(entry.getKey())).append(':').append(Json.quote(entry.getValue()));
            first = false;
        }
        return out.append('}').toString();
    }

    static String list(List<String> items) {
        StringBuilder out = new StringBuilder("[");
        boolean first = true;
        for (String item : items) {
            out.append(first ? "" : ",").append(Json.quote(item));
            first = false;
        }
        return out.append(']').toString();
    }
}
