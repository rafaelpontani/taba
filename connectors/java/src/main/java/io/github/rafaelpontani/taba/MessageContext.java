package io.github.rafaelpontani.taba;

/**
 * What the {@code consume} handler receives: the {@link #getMessage() message}, the {@link #setDelete(boolean) delete}
 * flag (the message is deleted at the end if the handler finishes without throwing; the default is yes) and
 * {@link #changeVisibility(int)}. While the handler runs, the connector renews the invisibility by itself, so no
 * other consumer takes the message.
 */
public class MessageContext {
    interface Change {
        void apply(int seconds);
    }

    private final Message message;
    private final Change change;
    private volatile boolean delete = true;
    private volatile int leaseSeconds;
    private volatile boolean leaseLost;

    MessageContext(Message message, int leaseSeconds, Change change) {
        this.message = message;
        this.leaseSeconds = leaseSeconds;
        this.change = change;
    }

    public Message getMessage() {
        return message;
    }

    public boolean isDelete() {
        return delete;
    }

    /** {@code false} leaves the message in the queue: it becomes visible again at the end of the invisibility. */
    public void setDelete(boolean delete) {
        this.delete = delete;
    }

    /** The seconds of invisibility the automatic renewal requests on each round. */
    public int getLeaseSeconds() {
        return leaseSeconds;
    }

    /** True when the server refused the renewal (the message already went back to the queue or was deleted). */
    public boolean isLeaseLost() {
        return leaseLost;
    }

    /** Changes the invisibility to {@code seconds} from now on. The value becomes the one of the automatic renewal. */
    public void changeVisibility(int seconds) {
        change.apply(seconds);
        leaseSeconds = seconds;
    }

    void markLost() {
        leaseLost = true;
    }
}
