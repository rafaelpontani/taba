package io.github.rafaelpontani.taba.omb;

import io.github.rafaelpontani.taba.PublishOptions;
import io.github.rafaelpontani.taba.TabaClient;
import io.openmessaging.benchmark.driver.BenchmarkProducer;
import java.util.Optional;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.Semaphore;
import java.util.concurrent.atomic.AtomicLong;

/** Publishes to the distributor of the topic; the key is the group (see {@link TabaDriverConfig#ordering}). */
final class TabaBenchmarkProducer implements BenchmarkProducer {
    private final TabaClient client;
    private final String topic;
    private final boolean perKey;
    private final int groups;
    private final Semaphore slots;
    private final AtomicLong next = new AtomicLong();

    TabaBenchmarkProducer(TabaClient client, String topic, boolean perKey, int groups, int inFlight) {
        this.client = client;
        this.topic = topic;
        this.perKey = perKey;
        this.groups = Math.max(1, groups);
        this.slots = new Semaphore(inFlight);
    }

    /** The group of a message: the key when there is one; in {@code per-key} without a key, {@code p<i>} in rotation. */
    String groupOf(Optional<String> key) {
        if (key.isPresent()) {
            return key.get();
        }
        return perKey ? "p" + Math.floorMod(next.getAndIncrement(), (long) groups) : null;
    }

    @Override
    public CompletableFuture<Void> sendAsync(Optional<String> key, byte[] payload) {
        PublishOptions options = new PublishOptions().contentType("application/octet-stream");
        String group = groupOf(key);
        if (group != null) {
            options.groupId(group);
        }
        if (perKey) {
            options.fifo(true);
        }
        try {
            slots.acquire();
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
            return failed(e);
        }
        byte[] body = Stamp.wrap(System.currentTimeMillis(), payload);
        CompletableFuture<Void> done;
        try {
            done = client.publishAsync(topic, body, options).thenApply(response -> (Void) null);
        } catch (RuntimeException e) {
            slots.release();
            return failed(e);
        }
        return done.whenComplete((ok, failure) -> slots.release());
    }

    private static CompletableFuture<Void> failed(Throwable cause) {
        CompletableFuture<Void> future = new CompletableFuture<>();
        future.completeExceptionally(cause);
        return future;
    }

    @Override
    public void close() {
        // The client belongs to the driver.
    }
}
