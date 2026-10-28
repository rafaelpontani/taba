package io.github.rafaelpontani.taba;

/** A handler that may throw: {@link java.util.function.Consumer} cannot throw a checked exception. */
@FunctionalInterface
public interface ThrowingConsumer<T> {
    void accept(T value) throws Exception;
}
