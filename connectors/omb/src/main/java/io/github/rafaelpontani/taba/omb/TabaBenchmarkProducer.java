package io.github.rafaelpontani.taba.omb;

import io.github.rafaelpontani.taba.BatchMessage;
import io.github.rafaelpontani.taba.BatchResult;
import io.github.rafaelpontani.taba.PublishOptions;
import io.github.rafaelpontani.taba.TabaClient;
import io.openmessaging.benchmark.driver.BenchmarkProducer;
import java.util.ArrayList;
import java.util.List;
import java.util.Optional;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.Executors;
import java.util.concurrent.ScheduledExecutorService;
import java.util.concurrent.ScheduledFuture;
import java.util.concurrent.Semaphore;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicLong;

/**
 * Publishes to the distributor of the topic; the key is the group (see {@link TabaDriverConfig#ordering}). With
 * {@code producer.batchSize} above 1 the messages wait in a buffer and go in one request
 * ({@code POST /topics/{topic}/messages/batch}) when the buffer fills or {@code producer.lingerMs} passes, the same
 * idea as the {@code linger.ms} of the Kafka producer; the time of each message is taken when it is sent to the
 * buffer, so the wait counts in its latency. Like Kafka, the linger only sends a partial batch when a request is free:
 * with every one in flight the batch keeps filling, otherwise a saturated producer would send ever smaller batches,
 * each costing a request, and fall into a spiral.
 */
final class TabaBenchmarkProducer implements BenchmarkProducer {
    private final TabaClient client;
    private final String topic;
    private final boolean perKey;
    private final int groups;
    private final Semaphore slots;
    private final AtomicLong next = new AtomicLong();
    private final int batchSize;
    private final long lingerMs;
    private final ScheduledExecutorService timer;
    private final Object lock = new Object();
    private List<Pending> buffer = new ArrayList<>();
    private ScheduledFuture<?> lingering;

    /** A message in the buffer and who waits for it. */
    private static final class Pending {
        final BatchMessage message;
        final CompletableFuture<Void> done = new CompletableFuture<>();

        Pending(BatchMessage message) {
            this.message = message;
        }
    }

    TabaBenchmarkProducer(TabaClient client, String topic, boolean perKey, int groups, TabaDriverConfig.Producer config) {
        this.client = client;
        this.topic = topic;
        this.perKey = perKey;
        this.groups = Math.max(1, groups);
        this.slots = new Semaphore(config.inFlight);
        this.batchSize = config.batchSize;
        this.lingerMs = config.lingerMs;
        this.timer = batchSize > 1
                ? Executors.newSingleThreadScheduledExecutor(runnable -> {
                    Thread thread = new Thread(runnable, "taba-omb-linger-" + topic);
                    thread.setDaemon(true);
                    return thread;
                })
                : null;
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
        String group = groupOf(key);
        byte[] body = Stamp.wrap(System.currentTimeMillis(), payload);
        if (batchSize <= 1) {
            return sendOne(group, body);
        }
        BatchMessage message = new BatchMessage(body).contentType("application/octet-stream");
        if (group != null) {
            message.groupId(group);
        }
        Pending pending = new Pending(message);
        List<Pending> full = null;
        synchronized (lock) {
            buffer.add(pending);
            if (buffer.size() >= batchSize) {
                full = take();
            } else if (lingering == null) {
                lingering = timer.schedule(this::flushLingering, lingerMs, TimeUnit.MILLISECONDS);
            }
        }
        if (full != null) {
            send(full);
        }
        return pending.done;
    }

    /** Takes the buffer out (with the lock held) and cancels its timer. */
    private List<Pending> take() {
        List<Pending> taken = buffer;
        buffer = new ArrayList<>(batchSize);
        if (lingering != null) {
            lingering.cancel(false);
            lingering = null;
        }
        return taken;
    }

    /** The linger is over: the partial batch goes if a request is free; if not, it keeps filling and waits again. */
    private void flushLingering() {
        List<Pending> taken;
        synchronized (lock) {
            lingering = null;
            if (buffer.isEmpty()) {
                return;
            }
            if (!slots.tryAcquire()) {
                lingering = timer.schedule(this::flushLingering, Math.max(1, lingerMs), TimeUnit.MILLISECONDS);
                return;
            }
            taken = take();
        }
        sendAcquired(taken);
    }

    /** Sends one batch, waiting for a slot: {@code producer.inFlight} requests at most. */
    private void send(List<Pending> batch) {
        try {
            slots.acquire();
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
            batch.forEach(p -> p.done.completeExceptionally(e));
            return;
        }
        sendAcquired(batch);
    }

    /** Sends one batch with its slot already taken. */
    private void sendAcquired(List<Pending> batch) {
        List<BatchMessage> messages = new ArrayList<>(batch.size());
        batch.forEach(p -> messages.add(p.message));
        CompletableFuture<List<BatchResult>> done;
        try {
            done = client.publishBatchAsync(topic, messages);
        } catch (RuntimeException e) {
            slots.release();
            batch.forEach(p -> p.done.completeExceptionally(e));
            return;
        }
        done.whenComplete((results, failure) -> {
            slots.release();
            for (int i = 0; i < batch.size(); i++) {
                CompletableFuture<Void> future = batch.get(i).done;
                if (failure != null) {
                    future.completeExceptionally(failure);
                } else if (i < results.size() && results.get(i).isOk()) {
                    future.complete(null);
                } else {
                    String code = i < results.size() ? results.get(i).getErrorCode() : "no result";
                    future.completeExceptionally(new IllegalStateException("publication refused: " + code));
                }
            }
        });
    }

    /** One request per message ({@code producer.batchSize} 1). */
    private CompletableFuture<Void> sendOne(String group, byte[] body) {
        PublishOptions options = new PublishOptions().contentType("application/octet-stream");
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
        // What is still in the buffer goes; the client belongs to the driver.
        if (timer != null) {
            List<Pending> rest;
            synchronized (lock) {
                rest = buffer.isEmpty() ? null : take();
            }
            if (rest != null) {
                send(rest);
            }
            timer.shutdownNow();
        }
    }
}
