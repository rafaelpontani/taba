package io.github.rafaelpontani.taba.omb;

import io.github.rafaelpontani.taba.ConsumeOptions;
import io.github.rafaelpontani.taba.Message;
import io.github.rafaelpontani.taba.TabaClient;
import io.openmessaging.benchmark.driver.BenchmarkConsumer;
import io.openmessaging.benchmark.driver.ConsumerCallback;
import java.util.List;
import java.util.concurrent.atomic.AtomicLong;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

/** One thread that long-polls the queue of the subscription, calls the OMB callback and acks. */
final class TabaBenchmarkConsumer implements BenchmarkConsumer {
    private static final Logger LOG = LoggerFactory.getLogger(TabaBenchmarkConsumer.class);

    private final TabaClient client;
    private final String queue;
    private final ConsumerCallback callback;
    private final ConsumeOptions options;
    private final boolean ackBatch;
    private final Thread thread;
    private final AtomicLong redelivered = new AtomicLong();
    private volatile boolean running = true;

    TabaBenchmarkConsumer(TabaClient client, String queue, ConsumerCallback callback, TabaDriverConfig.Consumer config) {
        this.client = client;
        this.queue = queue;
        this.callback = callback;
        this.ackBatch = config.ackBatch;
        this.options = new ConsumeOptions()
                .maxMessages(config.maxMessages)
                .visibilityTimeout(config.visibilityTimeout)
                .waitTime(config.waitTime);
        this.thread = new Thread(this::loop, "taba-omb-consumer-" + queue);
        this.thread.setDaemon(true);
        this.thread.start();
    }

    /** Messages delivered more than once; the OMB cannot see them. */
    long redelivered() {
        return redelivered.get();
    }

    private void loop() {
        while (running) {
            List<Message> messages;
            try {
                messages = client.consume(queue, options);
            } catch (RuntimeException e) {
                if (running) {
                    LOG.warn("consume on {} failed: {}", queue, e.toString());
                    pause();
                }
                continue;
            }
            for (Message message : messages) {
                if (message.getDeliveryCount() > 1) {
                    redelivered.incrementAndGet();
                }
                byte[] body = message.getBody();
                if (body.length >= Stamp.SIZE) {
                    callback.messageReceived(Stamp.payload(body), Stamp.millis(body));
                }
                if (!ackBatch) {
                    client.ackAsync(queue, message).exceptionally(failure -> {
                        LOG.warn("ack on {} failed: {}", queue, failure.toString());
                        return null;
                    });
                }
            }
            if (ackBatch && !messages.isEmpty()) {
                client.ackBatchAsync(queue, messages).whenComplete((results, failure) -> {
                    if (failure != null) {
                        LOG.warn("ack of {} messages on {} failed: {}", messages.size(), queue, failure.toString());
                    } else {
                        long refused = results.stream().filter(r -> !r.isOk()).count();
                        if (refused > 0) {
                            LOG.warn("{} of {} acks on {} were refused", refused, messages.size(), queue);
                        }
                    }
                });
            }
        }
    }

    private static void pause() {
        try {
            Thread.sleep(200);
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
        }
    }

    @Override
    public void close() {
        running = false;
        thread.interrupt();
        if (redelivered.get() > 0) {
            LOG.warn("{}: {} messages were delivered more than once", queue, redelivered.get());
        }
    }
}
