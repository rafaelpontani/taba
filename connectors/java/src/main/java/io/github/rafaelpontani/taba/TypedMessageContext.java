package io.github.rafaelpontani.taba;

/** The {@link MessageContext} of the typed {@code consume}: the body already deserialized as {@code T}. */
public final class TypedMessageContext<T> extends MessageContext {
    private final T body;

    TypedMessageContext(Message message, T body, int leaseSeconds, Change change) {
        super(message, leaseSeconds, change);
        this.body = body;
    }

    public T getBody() {
        return body;
    }
}
