package io.github.rafaelpontani.taba;

import java.util.concurrent.CompletableFuture;

/**
 * A running {@link TabaClient#subscribe subscribe} loop. {@link #close()} stops it (interrupting a long polling in
 * flight, never the handler) and waits for it to end; {@link #completion()} ends normally when stopped and
 * exceptionally with the {@link TabaException} if the server failed while consuming.
 */
public final class Subscription implements AutoCloseable {
    private final CompletableFuture<Void> completion = new CompletableFuture<>();
    private volatile boolean stopped;
    private boolean polling;
    private volatile Thread thread;

    Subscription() {
    }

    void attach(Thread thread) {
        this.thread = thread;
    }

    boolean isStopped() {
        return stopped;
    }

    /** Marks the loop as inside the long polling (where an interruption is wanted) or out of it. */
    void polling(boolean polling) {
        synchronized (this) {
            this.polling = polling;
        }
        if (!polling && stopped) {
            Thread.interrupted(); // an interruption that arrived as the poll ended must not reach the handler
        }
    }

    void finish(Throwable failure) {
        if (failure == null) {
            completion.complete(null);
        } else {
            completion.completeExceptionally(failure);
        }
    }

    /** Ends normally when {@link #close()} was called; exceptionally if the server failed while consuming. */
    public CompletableFuture<Void> completion() {
        return completion;
    }

    /** Blocks until the loop ends. Throws the {@link TabaException} if the server failed while consuming. */
    public void join() {
        try {
            completion.join();
        } catch (java.util.concurrent.CompletionException e) {
            if (e.getCause() instanceof RuntimeException) {
                throw (RuntimeException) e.getCause();
            }
            throw e;
        }
    }

    /** Stops the loop and waits for it to end. */
    @Override
    public void close() {
        Thread t = thread;
        synchronized (this) {
            stopped = true;
            if (polling && t != null) {
                t.interrupt();
            }
        }
        if (t != Thread.currentThread()) {
            completion.handle((value, failure) -> null).join();
        }
    }
}
