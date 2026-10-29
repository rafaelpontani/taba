package io.github.rafaelpontani.taba.omb;

import com.fasterxml.jackson.databind.ObjectMapper;
import com.fasterxml.jackson.dataformat.yaml.YAMLFactory;
import io.github.rafaelpontani.taba.TabaClient;
import io.github.rafaelpontani.taba.TabaConfig;
import io.github.rafaelpontani.taba.TabaException;
import io.github.rafaelpontani.taba.TopicRequest;
import io.openmessaging.benchmark.driver.BenchmarkConsumer;
import io.openmessaging.benchmark.driver.BenchmarkDriver;
import io.openmessaging.benchmark.driver.BenchmarkProducer;
import io.openmessaging.benchmark.driver.ConsumerCallback;
import java.io.File;
import java.io.IOException;
import java.time.Duration;
import java.util.Collections;
import java.util.List;
import java.util.Set;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.CopyOnWriteArrayList;
import org.apache.bookkeeper.stats.StatsLogger;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

/**
 * An OMB topic is a Taba distributor named like the topic, with the prefix {@code <topic>-}; each subscription is a
 * queue {@code <topic>-<subscription>} that the distributor copies every publication to. The consumers of a
 * subscription compete for the messages of its queue.
 */
public class TabaBenchmarkDriver implements BenchmarkDriver {
    private static final Logger LOG = LoggerFactory.getLogger(TabaBenchmarkDriver.class);
    private static final ObjectMapper MAPPER = new ObjectMapper(new YAMLFactory());
    /** The longest idleTimeout the server accepts (7 days): only a safety net for an aborted run. */
    private static final long SAFETY_NET_SECONDS = 7L * 24 * 3600;

    private TabaDriverConfig config;
    private TabaClient client;
    private final ConcurrentHashMap<String, Integer> partitions = new ConcurrentHashMap<>();
    private final Set<String> queues = ConcurrentHashMap.newKeySet();
    private final List<BenchmarkConsumer> consumers = new CopyOnWriteArrayList<>();
    private final List<BenchmarkProducer> producers = new CopyOnWriteArrayList<>();

    @Override
    public void initialize(File configurationFile, StatsLogger statsLogger) throws IOException {
        initialize(MAPPER.readValue(configurationFile, TabaDriverConfig.class));
    }

    void initialize(TabaDriverConfig loaded) {
        loaded.validate();
        this.config = loaded;
        TabaConfig tabaConfig = TabaConfig.of(loaded.baseUrl)
                .maxAttempts(1)
                .requestTimeout(Duration.ofSeconds(30))
                .userAgent("taba-omb-driver");
        if (loaded.token != null && !loaded.token.isEmpty()) {
            tabaConfig.token(loaded.token);
        }
        this.client = new TabaClient(tabaConfig);
    }

    @Override
    public String getTopicNamePrefix() {
        return "omb";
    }

    @Override
    public CompletableFuture<Void> createTopic(String topic, int partitionCount) {
        partitions.put(topic, partitionCount);
        return client.createTopicAsync(TopicRequest.distributor(topic).prefixes(Collections.singletonList(topic + "-")))
                .thenApply(created -> (Void) null);
    }

    @Override
    public CompletableFuture<BenchmarkProducer> createProducer(String topic) {
        TabaBenchmarkProducer producer = new TabaBenchmarkProducer(
                client, topic, config.isPerKey(), partitions.getOrDefault(topic, 1), config.producer);
        producers.add(producer);
        return CompletableFuture.completedFuture(producer);
    }

    @Override
    public CompletableFuture<BenchmarkConsumer> createConsumer(
            String topic, String subscriptionName, ConsumerCallback consumerCallback) {
        String queue = topic + "-" + subscriptionName;
        return client.createTopicAsync(queueRequest(queue))
                .handle((created, failure) -> {
                    if (failure != null && !alreadyExists(failure)) {
                        throw new IllegalStateException("creating queue " + queue, failure);
                    }
                    queues.add(queue);
                    BenchmarkConsumer consumer =
                            new TabaBenchmarkConsumer(client, queue, consumerCallback, config.consumer);
                    consumers.add(consumer);
                    return consumer;
                });
    }

    TopicRequest queueRequest(String queue) {
        return TopicRequest.queue(queue)
                .volatileTopic(true)
                .idleTimeout(SAFETY_NET_SECONDS)
                .fifo(config.isPerKey() ? Boolean.TRUE : null);
    }

    private static boolean alreadyExists(Throwable failure) {
        Throwable cause = failure;
        while (cause != null) {
            if (cause instanceof TabaException && ((TabaException) cause).getStatus() == 409) {
                return true;
            }
            cause = cause.getCause();
        }
        return false;
    }

    @Override
    public void close() throws Exception {
        for (BenchmarkConsumer consumer : consumers) {
            consumer.close();
        }
        for (BenchmarkProducer producer : producers) {
            producer.close();
        }
        if (client != null) {
            for (String queue : queues) {
                delete(queue);
            }
            for (String topic : partitions.keySet()) {
                delete(topic);
            }
            client.close();
        }
    }

    private void delete(String topic) {
        try {
            client.deleteTopic(topic);
        } catch (RuntimeException e) {
            LOG.warn("deleting {} failed: {}", topic, e.toString());
        }
    }
}
