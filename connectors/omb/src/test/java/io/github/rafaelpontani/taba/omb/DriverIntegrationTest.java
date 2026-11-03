package io.github.rafaelpontani.taba.omb;

import static org.junit.jupiter.api.Assertions.assertArrayEquals;
import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertTrue;

import io.openmessaging.benchmark.driver.BenchmarkConsumer;
import io.openmessaging.benchmark.driver.BenchmarkProducer;
import io.openmessaging.benchmark.driver.ConsumerCallback;
import java.nio.ByteBuffer;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.Collections;
import java.util.List;
import java.util.Optional;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.TimeUnit;
import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.Assumptions;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;

/** Against a real node; runs only with TABA_URL and TABA_TOKEN (bench/omb/test.sh sets them). */
class DriverIntegrationTest {
    private TabaBenchmarkDriver driver;

    private static final class Collector implements ConsumerCallback {
        final List<String> received = Collections.synchronizedList(new ArrayList<>());
        final List<Long> stamps = Collections.synchronizedList(new ArrayList<>());

        @Override
        public void messageReceived(byte[] payload, long publishTimestamp) {
            received.add(new String(payload, StandardCharsets.UTF_8));
            stamps.add(publishTimestamp);
        }

        @Override
        public void messageReceived(ByteBuffer payload, long publishTimestamp) {
            byte[] bytes = new byte[payload.remaining()];
            payload.get(bytes);
            messageReceived(bytes, publishTimestamp);
        }

        void awaitCount(int count) throws InterruptedException {
            long deadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(60);
            while (received.size() < count && System.nanoTime() < deadline) {
                Thread.sleep(50);
            }
        }
    }

    private TabaBenchmarkDriver start(String ordering) {
        Assumptions.assumeTrue(System.getenv("TABA_URL") != null, "TABA_URL not set");
        TabaDriverConfig config = new TabaDriverConfig();
        config.baseUrl = System.getenv("TABA_URL");
        config.token = System.getenv("TABA_TOKEN");
        config.ordering = ordering;
        config.consumer.waitTime = 2;
        TabaBenchmarkDriver started = new TabaBenchmarkDriver();
        started.initialize(config);
        driver = started;
        return started;
    }

    private static String topic(String what) {
        return "omb-it-" + what + "-" + System.currentTimeMillis();
    }

    @AfterEach
    void stop() throws Exception {
        if (driver != null) {
            driver.close();
        }
    }

    /// UC: UC-CNT-036, UC-CNT-039
    @Test
    void everySubscriptionReceivesEveryMessageAndTheTimestampComesBack() throws Exception {
        TabaBenchmarkDriver d = start("none");
        String topic = topic("fanout");
        d.createTopic(topic, 4).get(30, TimeUnit.SECONDS);
        Collector a = new Collector();
        Collector b = new Collector();
        d.createConsumer(topic, "sub-a", a).get(30, TimeUnit.SECONDS);
        d.createConsumer(topic, "sub-b", b).get(30, TimeUnit.SECONDS);
        BenchmarkProducer producer = d.createProducer(topic).get(30, TimeUnit.SECONDS);

        long before = System.currentTimeMillis();
        List<CompletableFuture<Void>> sent = new ArrayList<>();
        for (int i = 0; i < 50; i++) {
            sent.add(producer.sendAsync(Optional.empty(), ("m" + i).getBytes(StandardCharsets.UTF_8)));
        }
        CompletableFuture.allOf(sent.toArray(new CompletableFuture[0])).get(30, TimeUnit.SECONDS);
        a.awaitCount(50);
        b.awaitCount(50);

        assertEquals(50, a.received.size());
        assertEquals(50, b.received.size());
        for (int i = 0; i < 50; i++) {
            assertTrue(a.received.contains("m" + i), "sub-a lacks m" + i);
            assertTrue(b.received.contains("m" + i), "sub-b lacks m" + i);
        }
        for (long stamp : a.stamps) {
            assertTrue(stamp >= before && stamp <= System.currentTimeMillis(), "stamp " + stamp);
        }
    }

    /// UC: UC-CNT-037
    @Test
    void consumersOfTheSameSubscriptionShareTheMessages() throws Exception {
        TabaBenchmarkDriver d = start("none");
        String topic = topic("share");
        d.createTopic(topic, 1).get(30, TimeUnit.SECONDS);
        Collector first = new Collector();
        Collector second = new Collector();
        BenchmarkConsumer c1 = d.createConsumer(topic, "sub", first).get(30, TimeUnit.SECONDS);
        BenchmarkConsumer c2 = d.createConsumer(topic, "sub", second).get(30, TimeUnit.SECONDS);
        BenchmarkProducer producer = d.createProducer(topic).get(30, TimeUnit.SECONDS);

        for (int i = 0; i < 40; i++) {
            producer.sendAsync(Optional.empty(), ("s" + i).getBytes(StandardCharsets.UTF_8)).get(30, TimeUnit.SECONDS);
        }
        long deadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(60);
        while (first.received.size() + second.received.size() < 40 && System.nanoTime() < deadline) {
            Thread.sleep(50);
        }
        assertEquals(40, first.received.size() + second.received.size());
        c1.close();
        c2.close();
    }

    /// UC: UC-CNT-038
    @Test
    void perKeyKeepsTheOrderOfEachKey() throws Exception {
        TabaBenchmarkDriver d = start("per-key");
        String topic = topic("order");
        d.createTopic(topic, 2).get(30, TimeUnit.SECONDS);
        Collector sub = new Collector();
        d.createConsumer(topic, "sub", sub).get(30, TimeUnit.SECONDS);
        BenchmarkProducer producer = d.createProducer(topic).get(30, TimeUnit.SECONDS);

        for (int i = 0; i < 20; i++) {
            producer.sendAsync(Optional.of("k1"), ("k1-" + i).getBytes(StandardCharsets.UTF_8)).get(30, TimeUnit.SECONDS);
            producer.sendAsync(Optional.of("k2"), ("k2-" + i).getBytes(StandardCharsets.UTF_8)).get(30, TimeUnit.SECONDS);
        }
        sub.awaitCount(40);

        assertEquals(40, sub.received.size());
        int next1 = 0;
        int next2 = 0;
        for (String message : new ArrayList<>(sub.received)) {
            if (message.startsWith("k1-")) {
                assertEquals("k1-" + next1++, message);
            } else {
                assertEquals("k2-" + next2++, message);
            }
        }
        assertArrayEquals(new int[] {20, 20}, new int[] {next1, next2});
    }

    /// UC: UC-CNT-041
    @Test
    void aProducerInBatchesDeliversEveryMessageTheFullBatchesAndTheRestByTheLinger() throws Exception {
        TabaBenchmarkDriver d = start("none");
        String topic = topic("batch");
        d.createTopic(topic, 1).get(30, TimeUnit.SECONDS);
        Collector sub = new Collector();
        d.createConsumer(topic, "sub", sub).get(30, TimeUnit.SECONDS);
        TabaDriverConfig.Producer config = new TabaDriverConfig.Producer();
        config.batchSize = 10;
        config.lingerMs = 5;
        TabaBenchmarkProducer producer = new TabaBenchmarkProducer(new io.github.rafaelpontani.taba.TabaClient(
                io.github.rafaelpontani.taba.TabaConfig.of(System.getenv("TABA_URL")).token(System.getenv("TABA_TOKEN"))),
                topic, false, 1, config);
        List<java.util.concurrent.CompletableFuture<Void>> sent = new ArrayList<>();
        for (int i = 0; i < 25; i++) {
            sent.add(producer.sendAsync(Optional.empty(), ("b-" + i).getBytes(StandardCharsets.UTF_8)));
        }
        for (java.util.concurrent.CompletableFuture<Void> future : sent) {
            future.get(30, TimeUnit.SECONDS);
        }
        sub.awaitCount(25);
        assertEquals(25, new java.util.HashSet<>(sub.received).size());
        producer.close();
    }

    @BeforeEach
    void reset() {
        driver = null;
    }
}
