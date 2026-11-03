package io.github.rafaelpontani.taba;

import java.util.Collections;
import java.util.List;

/**
 * The result of one item of a batch ({@code publishBatch}, {@code ackBatch}, {@code changeVisibilityBatch}), in
 * the order of the request: the {@code status} the single call would have answered and what it would have returned.
 * An item that failed does not throw: {@link #isOk()} is false and {@link #getErrorCode()} says why.
 */
public final class BatchResult {
    private final int status;
    private final String id;
    private final Boolean duplicate;
    private final Long matched;
    private final List<DistributorDelivery> deliveries;
    private final String invisibleUntil;
    private final String errorCode;
    private final String errorMessage;
    private final String raw;

    BatchResult(int status, String id, Boolean duplicate, Long matched, List<DistributorDelivery> deliveries,
            String invisibleUntil, String errorCode, String errorMessage, String raw) {
        this.status = status;
        this.id = id;
        this.duplicate = duplicate;
        this.matched = matched;
        this.deliveries = Collections.unmodifiableList(deliveries);
        this.invisibleUntil = invisibleUntil;
        this.errorCode = errorCode;
        this.errorMessage = errorMessage;
        this.raw = raw;
    }

    /** {@code 201} published, {@code 200} a FIFO duplicate, a distributor or a new invisibility, {@code 204} acked, or the error's status. */
    public int getStatus() {
        return status;
    }

    /** The item was applied (no error). */
    public boolean isOk() {
        return errorCode == null && status < 300;
    }

    /** The message {@code id}; null on an error and on a distributor's publication. */
    public String getId() {
        return id;
    }

    /** FIFO queue only: the publication repeated one inside the deduplication window. */
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

    /** {@code changeVisibilityBatch} only: the new deadline. */
    public String getInvisibleUntil() {
        return invisibleUntil;
    }

    /** The error code of this item ({@code message.stale-receipt}...), null when it was applied. */
    public String getErrorCode() {
        return errorCode;
    }

    public String getErrorMessage() {
        return errorMessage;
    }

    public String getRaw() {
        return raw;
    }
}
