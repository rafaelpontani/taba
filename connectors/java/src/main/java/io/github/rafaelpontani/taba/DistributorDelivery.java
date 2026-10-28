package io.github.rafaelpontani.taba;

/** The copy of a publication made by a distributor to a queue. */
public final class DistributorDelivery {
    private final String topic;
    private final String id;
    private final String errorCode;
    private final String errorMessage;

    DistributorDelivery(String topic, String id, String errorCode, String errorMessage) {
        this.topic = topic;
        this.id = id;
        this.errorCode = errorCode;
        this.errorMessage = errorMessage;
    }

    /** The destination queue. */
    public String getTopic() {
        return topic;
    }

    /** The {@code id} of the copy, when it was created. May be null. */
    public String getId() {
        return id;
    }

    /** The error {@code code}, when the copy failed (a partial failure is not a distributor error). May be null. */
    public String getErrorCode() {
        return errorCode;
    }

    public String getErrorMessage() {
        return errorMessage;
    }

    public boolean isSucceeded() {
        return errorCode == null;
    }
}
