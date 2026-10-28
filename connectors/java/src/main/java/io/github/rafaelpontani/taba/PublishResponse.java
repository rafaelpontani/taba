package io.github.rafaelpontani.taba;

import java.util.Collections;
import java.util.List;

/** The response of a publication: {@code PublishResponse} (queue) or {@code DistributorPublishResponse}. */
public final class PublishResponse {
    private final String id;
    private final Boolean duplicate;
    private final Long matched;
    private final List<DistributorDelivery> deliveries;
    private final String raw;

    PublishResponse(String id, Boolean duplicate, Long matched, List<DistributorDelivery> deliveries, String raw) {
        this.id = id;
        this.duplicate = duplicate;
        this.matched = matched;
        this.deliveries = Collections.unmodifiableList(deliveries);
        this.raw = raw;
    }

    /** The message {@code id} (queue). Null in a distributor's response. */
    public String getId() {
        return id;
    }

    /** FIFO queue only: true when it repeated a publication within the deduplication window. */
    public Boolean getDuplicate() {
        return duplicate;
    }

    /** Distributor only: how many queues matched. */
    public Long getMatched() {
        return matched;
    }

    /** Distributor only: one entry per queue that matched. */
    public List<DistributorDelivery> getDeliveries() {
        return deliveries;
    }

    public String getRaw() {
        return raw;
    }
}
