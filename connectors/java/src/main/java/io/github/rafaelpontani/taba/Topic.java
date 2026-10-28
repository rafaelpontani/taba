package io.github.rafaelpontani.taba;

/** A topic (queue or distributor). */
public final class Topic {
    private final String id;
    private final String name;
    private final String type;
    private final String createdAt;
    private final Boolean fifo;
    private final Boolean volatileTopic;
    private final String raw;

    Topic(String id, String name, String type, String createdAt, Boolean fifo, Boolean volatileTopic, String raw) {
        this.id = id;
        this.name = name;
        this.type = type;
        this.createdAt = createdAt;
        this.fifo = fifo;
        this.volatileTopic = volatileTopic;
        this.raw = raw;
    }

    public String getId() {
        return id;
    }

    public String getName() {
        return name;
    }

    /** {@code queue} or {@code distributor}. */
    public String getType() {
        return type;
    }

    /** Creation date, as RFC 3339 text. */
    public String getCreatedAt() {
        return createdAt;
    }

    /** Null when the server did not say. */
    public Boolean getFifo() {
        return fifo;
    }

    /** Null when the server did not say. */
    public Boolean isVolatile() {
        return volatileTopic;
    }

    /** The topic JSON, as the server returned it. */
    public String getRaw() {
        return raw;
    }

    @Override
    public String toString() {
        return "Topic{" + name + ", " + type + "}";
    }
}
