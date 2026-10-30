package io.github.rafaelpontani.taba.omb;

import com.fasterxml.jackson.annotation.JsonIgnoreProperties;

/** The YAML of the driver (connectors/omb/driver-taba.yaml). */
@JsonIgnoreProperties(ignoreUnknown = true)
public class TabaDriverConfig {
    public String name = "Taba";

    public String driverClass;

    /** Address of the client API (the load balancer in front of the cluster). */
    public String baseUrl = "http://localhost:8080";

    /** Bearer token of the client API. Empty: no header. */
    public String token = "";

    /** {@code none}: ordinary queue. {@code per-key}: FIFO queue, the key of the message is the group. */
    public String ordering = "none";

    public Producer producer = new Producer();
    public Consumer consumer = new Consumer();

    @JsonIgnoreProperties(ignoreUnknown = true)
    public static class Producer {
        /** Requests in flight per producer; above it {@code sendAsync} waits for a slot. */
        public int inFlight = 1000;
        /** Messages per request ({@code POST .../messages/batch}); 1 publishes one request per message. */
        public int batchSize = 100;
        /** How long a message waits in the buffer for the batch to fill, in milliseconds. */
        public long lingerMs = 1;
    }

    @JsonIgnoreProperties(ignoreUnknown = true)
    public static class Consumer {
        /** Long polling, in seconds (the server accepts 0 to 20). */
        public int waitTime = 20;
        public int maxMessages = 100;
        /** Seconds a delivered message stays invisible; it has to outlast the slowest callback. */
        public int visibilityTimeout = 60;
        /** Acks what one {@code GET} delivered in one request ({@code POST .../messages/ack}); false acks one by one. */
        public boolean ackBatch = true;
    }

    public boolean isPerKey() {
        return "per-key".equals(ordering);
    }

    public void validate() {
        if (!"none".equals(ordering) && !"per-key".equals(ordering)) {
            throw new IllegalArgumentException("ordering must be 'none' or 'per-key', not '" + ordering + "'");
        }
        if (producer.inFlight < 1) {
            throw new IllegalArgumentException("producer.inFlight must be at least 1");
        }
        if (producer.batchSize < 1) {
            throw new IllegalArgumentException("producer.batchSize must be at least 1");
        }
        if (producer.lingerMs < 0) {
            throw new IllegalArgumentException("producer.lingerMs cannot be negative");
        }
        if (consumer.waitTime < 0 || consumer.waitTime > 20) {
            throw new IllegalArgumentException("consumer.waitTime must be between 0 and 20");
        }
        if (consumer.maxMessages < 1) {
            throw new IllegalArgumentException("consumer.maxMessages must be at least 1");
        }
        if (consumer.visibilityTimeout < 1) {
            throw new IllegalArgumentException("consumer.visibilityTimeout must be at least 1");
        }
    }
}
