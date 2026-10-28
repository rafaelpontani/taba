package io.github.rafaelpontani.taba;

import static io.github.rafaelpontani.taba.ScriptedServer.HANG;
import static io.github.rafaelpontani.taba.ScriptedServer.reply;
import static org.junit.jupiter.api.Assertions.assertArrayEquals;
import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNotNull;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import io.github.rafaelpontani.taba.ScriptedServer.Recorded;
import java.io.IOException;
import java.net.InetAddress;
import java.net.ServerSocket;
import java.net.Socket;
import java.nio.charset.StandardCharsets;
import java.time.Duration;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.Collections;
import java.util.List;
import java.util.Map;
import java.util.concurrent.CancellationException;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.CopyOnWriteArrayList;
import java.util.concurrent.ExecutionException;
import java.util.concurrent.Semaphore;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicReference;
import java.util.function.BooleanSupplier;
import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;

class TabaClientTest {
    private ScriptedServer server;
    private List<Duration> waits;
    private TabaClient client;
    private final List<AutoCloseable> toClose = new ArrayList<>();

    @BeforeEach
    void setUp() {
        server = new ScriptedServer();
        waits = new CopyOnWriteArrayList<>();
        client = newClient(TabaConfig.of(server.url()), waits::add);
    }

    @AfterEach
    void tearDown() throws Exception {
        for (AutoCloseable closeable : toClose) {
            closeable.close();
        }
        client.close();
        server.close();
    }

    /** A client whose waits do not wait: the renewal and the backoff are recorded, not slept. */
    private TabaClient newClient(TabaConfig config, java.util.function.Consumer<Duration> onSleep) {
        TabaClient created = new TabaClient(config, duration -> onSleep.accept(duration));
        toClose.add(created);
        return created;
    }

    /** A client whose waits never end by themselves: the renewal does not spin in tests that do not look at it. */
    private TabaClient newBlockingClient(TabaConfig config) {
        TabaClient created = new TabaClient(config, duration -> Thread.sleep(Long.MAX_VALUE));
        toClose.add(created);
        return created;
    }

    private static String topicJson(String name) {
        return "{\"id\":\"t-" + name + "\",\"name\":\"" + name + "\",\"type\":\"queue\",\"createdAt\":\"2026-10-27T10:00:00Z\"}";
    }

    private static String messageJson(String id, String body, String... extra) {
        StringBuilder out = new StringBuilder("{\"id\":\"" + id + "\",\"body\":" + body
                + ",\"contentType\":\"application/json\",\"receiptToken\":\"rt-" + id + "\",\"deliveryCount\":1,"
                + "\"publishedAt\":\"2026-10-27T10:00:00Z\",\"invisibleUntil\":\"2026-10-27T10:02:00Z\"");
        for (String field : extra) {
            out.append(',').append(field);
        }
        return out.append('}').toString();
    }

    private static TabaException error(Runnable call) {
        return assertThrows(TabaException.class, call::run);
    }

    private static void waitFor(BooleanSupplier condition) throws InterruptedException {
        long deadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(5);
        while (!condition.getAsBoolean()) {
            if (System.nanoTime() > deadline) {
                throw new AssertionError("condition not met in 5 s");
            }
            Thread.sleep(5);
        }
    }

    private List<Recorded> requestsOf(String method) {
        List<Recorded> found = new ArrayList<>();
        synchronized (server.requests) {
            for (Recorded request : server.requests) {
                if (request.method.equals(method)) {
                    found.add(request);
                }
            }
        }
        return found;
    }

    // ---------------------------------------------------------------------------------------------
    // Operations
    // ---------------------------------------------------------------------------------------------

    /// UC: UC-CNT-001, UC-CNT-029
    @Test
    void theNineOperationsBuildTheExpectedRoutes() {
        server.fallback(reply(200, topicJson("orders")));
        client.createTopic(TopicRequest.queue("orders"));
        client.getTopic("orders");
        server.reply(reply(200, "{\"items\":[" + topicJson("orders") + "]}"));
        client.listTopics(10, "c1", "ord");
        client.updateTopic("orders", new TopicUpdate().idleTimeout(60));
        client.deleteTopic("orders");
        server.reply(reply(200, "{\"id\":\"m1\"}"));
        client.publish("orders", "{\"a\":1}".getBytes(StandardCharsets.UTF_8), new PublishOptions().groupId("g").deduplicationId("d"));
        server.reply(reply(200, "{\"messages\":[]}"));
        client.consume("orders", new ConsumeOptions().maxMessages(2));
        client.ack("orders", "m 1", "rt");
        server.reply(reply(200, "{\"id\":\"m1\",\"invisibleUntil\":\"x\"}"));
        client.changeVisibility("orders", "m1", "rt", 30);

        List<String> seen = new ArrayList<>();
        for (Recorded request : server.requests) {
            seen.add(request.method + " " + request.url);
        }
        assertEquals(
                Arrays.asList(
                        "POST /topics",
                        "GET /topics/orders",
                        "GET /topics?limit=10&cursor=c1&name=ord",
                        "PATCH /topics/orders",
                        "DELETE /topics/orders",
                        "POST /topics/orders/messages?groupId=g&deduplicationId=d",
                        "GET /topics/orders/messages?maxMessages=2",
                        "DELETE /topics/orders/messages/m%201?receiptToken=rt",
                        "PUT /topics/orders/messages/m1/visibility?receiptToken=rt"),
                seen);
        assertEquals("{\"visibilityTimeout\":30}", server.requests.get(8).text());
    }

    /// UC: UC-CNT-029
    @Test
    void updateTopicIsARealPatchWithoutAMethodOverrideHeader() {
        server.fallback(reply(200, topicJson("orders")));
        client.updateTopic("orders", new TopicUpdate().ttlDefault().maxDeliveries(5).tags(Map.of("a", "b")).prefixes(List.of("x-")));
        Recorded request = server.requests.get(0);
        assertEquals("PATCH", request.method);
        assertNull(request.header("X-HTTP-Method-Override"));
        assertEquals("{\"tags\":{\"a\":\"b\"},\"prefixes\":[\"x-\"],\"ttl\":null,\"maxDeliveries\":5}", request.text());
    }

    /// UC: UC-CNT-001
    @Test
    void createTopicSendsOnlyWhatWasFilled() {
        server.fallback(reply(201, topicJson("orders")));
        client.createTopic(TopicRequest.queue("orders"));
        client.createTopic(TopicRequest.distributor("d").prefixes(List.of("p-")).fifo(false).ttl(30L).tags(Collections.emptyMap()));
        assertEquals("{\"name\":\"orders\",\"type\":\"queue\"}", server.requests.get(0).text());
        assertEquals("{\"name\":\"d\",\"type\":\"distributor\",\"prefixes\":[\"p-\"],\"fifo\":false,\"ttl\":30}", server.requests.get(1).text());
    }

    /// UC: UC-CNT-002
    @Test
    void configDefaultsAndRequiredBaseUrl() {
        for (String bad : new String[] {null, "", "  ", "ftp://x", "not a url"}) {
            TabaException e = assertThrows(TabaException.class, () -> new TabaClient(TabaConfig.of(bad)));
            assertEquals("client.invalid-config", e.getCode());
            assertEquals(0, e.getStatus());
        }
        assertThrows(NullPointerException.class, () -> new TabaClient(null));
        TabaConfig defaults = new TabaClient(TabaConfig.of("http://localhost:8080/")).getConfig();
        assertEquals(Duration.ofSeconds(5), defaults.getConnectTimeout());
        assertEquals(Duration.ofSeconds(30), defaults.getRequestTimeout());
        assertEquals(3, defaults.getMaxAttempts());
        assertEquals("taba-java/" + TabaClient.VERSION, defaults.getUserAgent());
    }

    /// UC: UC-CNT-003
    @Test
    void aTokenGoesInEveryCallAndWithoutItNoCallCarriesAuthorization() {
        server.fallback(reply(200, topicJson("a")));
        newClient(TabaConfig.of(server.url()).token("secret"), waits::add).getTopic("a");
        client.getTopic("a");
        assertEquals("Bearer secret", server.requests.get(0).header("Authorization"));
        assertNull(server.requests.get(1).header("Authorization"));
    }

    /// UC: UC-CNT-018
    @Test
    void headersCarryTheAgentAndTheOptionalIds() {
        TabaClient own = newClient(TabaConfig.of(server.url()).userAgent("my-app taba-java/9"), waits::add);
        server.fallback(reply(200, "{\"id\":\"m1\"}"));
        own.publish("q", "x".getBytes(StandardCharsets.UTF_8), new PublishOptions().requestId("req-1").traceparent("00-abc-def-01"));
        own.publish("q", "x".getBytes(StandardCharsets.UTF_8));
        Recorded first = server.requests.get(0);
        Recorded second = server.requests.get(1);
        assertEquals("application/json", first.header("Accept"));
        assertEquals("my-app taba-java/9", first.header("User-Agent"));
        assertEquals("req-1", first.header("X-Request-Id"));
        assertEquals("00-abc-def-01", first.header("traceparent"));
        assertNull(second.header("X-Request-Id"));
        assertNull(second.header("traceparent"));
    }

    /// UC: UC-CNT-004, UC-CNT-025
    @Test
    void publishSendsRawBytesWithContentTypeAndQuery() {
        server.fallback(reply(200, "{\"id\":\"m1\",\"duplicate\":true}"));
        byte[] data = {0, 1, 2, (byte) 255, (byte) 254};
        PublishResponse response = client.publish("q", data, new PublishOptions().contentType("application/octet-stream").groupId("g1").deduplicationId("d1"));
        assertEquals("m1", response.getId());
        assertEquals(Boolean.TRUE, response.getDuplicate());
        Recorded request = server.requests.get(0);
        assertArrayEquals(data, request.body);
        assertEquals("application/octet-stream", request.header("Content-Type"));
        assertEquals("g1", request.query().get("groupId"));
        assertEquals("d1", request.query().get("deduplicationId"));

        client.publishText("q", "olá ✓");
        assertEquals("text/plain; charset=utf-8", server.requests.get(1).header("Content-Type"));
        assertEquals("olá ✓", server.requests.get(1).text());
        client.publishRawJson("q", "{\"n\": 1.0}");
        assertEquals("application/json", server.requests.get(2).header("Content-Type"));
        assertEquals("{\"n\": 1.0}", server.requests.get(2).text());
        client.publish("q", "{}".getBytes(StandardCharsets.UTF_8));
        assertEquals("application/json", server.requests.get(3).header("Content-Type"));
    }

    /// UC: UC-CNT-004
    @Test
    void aDistributorAnswerListsEachCopyAndThePartialFailures() {
        server.fallback(reply(200, "{\"matched\":2,\"deliveries\":[{\"topic\":\"a-1\",\"id\":\"m1\"},"
                + "{\"topic\":\"a-2\",\"error\":{\"code\":\"queue.full\",\"message\":\"full\"}}]}"));
        PublishResponse response = client.publish("dist", "{}".getBytes(StandardCharsets.UTF_8));
        assertNull(response.getId());
        assertEquals(Long.valueOf(2), response.getMatched());
        assertEquals(2, response.getDeliveries().size());
        assertTrue(response.getDeliveries().get(0).isSucceeded());
        assertEquals("m1", response.getDeliveries().get(0).getId());
        assertFalse(response.getDeliveries().get(1).isSucceeded());
        assertEquals("queue.full", response.getDeliveries().get(1).getErrorCode());
        assertEquals("full", response.getDeliveries().get(1).getErrorMessage());
    }

    /// UC: UC-CNT-005
    @Test
    void consumeSendsOnlyWhatWasGiven() {
        server.fallback(reply(200, "{\"messages\":[]}"));
        assertTrue(client.consume("q").isEmpty());
        client.consume("q", new ConsumeOptions().maxMessages(5).visibilityTimeout(60).waitTime(15));
        assertEquals("/topics/q/messages", server.requests.get(0).url);
        assertEquals("/topics/q/messages?maxMessages=5&visibilityTimeout=60&waitTime=15", server.requests.get(1).url);
        client.consume("q", new ConsumeOptions().autoCreate(true).volatileTopic(true).idleTimeout(30).fifo(false).tags(Map.of("env", "dev")));
        Map<String, String> query = server.requests.get(2).query();
        assertEquals("true", query.get("autoCreate"));
        assertEquals("true", query.get("volatile"));
        assertEquals("30", query.get("idleTimeout"));
        assertEquals("false", query.get("fifo"));
        assertEquals("dev", query.get("tag.env"));
    }

    /// UC: UC-CNT-005
    @Test
    void autoCreateParametersAreLeftOutWithoutAutoCreate() {
        ApiCall call = TabaClient.consumeCall("q", new ConsumeOptions().volatileTopic(true).idleTimeout(30).fifo(true).tags(Map.of("a", "b")));
        assertTrue(call.query.isEmpty());
    }

    /// UC: UC-CNT-008
    @Test
    void theConsumeReadTimeoutIsWaitTimePlusTenSecondsAndNotTheRequestTimeout() {
        assertEquals(Duration.ofSeconds(30), TabaClient.consumeCall("q", new ConsumeOptions().waitTime(20)).timeout);
        assertEquals(Duration.ofSeconds(35), TabaClient.consumeCall("q", new ConsumeOptions().waitTime(25)).timeout);
        assertEquals(Duration.ofSeconds(10), TabaClient.consumeCall("q", new ConsumeOptions()).timeout);
    }

    /// UC: UC-CNT-006
    @Test
    void ackAndVisibilityPutTheReceiptTokenInTheQuery() {
        server.fallback(reply(200, "{\"id\":\"m1\",\"invisibleUntil\":\"2026-10-27T10:05:00Z\"}"));
        client.ack("q", "m1", "a b&c=d");
        VisibilityResponse visibility = client.changeVisibility("q", "m1", "rt", 45);
        assertEquals("a b&c=d", server.requests.get(0).query().get("receiptToken"));
        assertEquals("rt", server.requests.get(1).query().get("receiptToken"));
        assertEquals("2026-10-27T10:05:00Z", visibility.getInvisibleUntil());
    }

    /// UC: UC-CNT-007
    @Test
    void listAllTopicsFollowsNextCursorUntilTheLastPage() {
        server.reply(
                reply(200, "{\"items\":[" + topicJson("a") + "," + topicJson("b") + "],\"nextCursor\":\"c2\"}"),
                reply(200, "{\"items\":[" + topicJson("c") + "],\"nextCursor\":null}"));
        List<Topic> topics = client.listAllTopics("x");
        assertEquals(Arrays.asList("a", "b", "c"), Arrays.asList(topics.get(0).getName(), topics.get(1).getName(), topics.get(2).getName()));
        assertFalse(server.requests.get(0).query().containsKey("cursor"));
        assertEquals("c2", server.requests.get(1).query().get("cursor"));
        assertEquals("x", server.requests.get(1).query().get("name"));
    }

    /// UC: UC-CNT-015, UC-CNT-016, UC-CNT-025, UC-CNT-026
    @Test
    void messagesComeBackAsBytesForJsonTextAndBase64() {
        String m1 = "{\"id\":\"m1\",\"body\":{\"pedido\": 9001, \"valor\": 1.0, \"grande\": 12345678901234567890},"
                + "\"contentType\":\"application/json\",\"receiptToken\":\"rt-m1\",\"deliveryCount\":3,\"publishedAt\":\"p\","
                + "\"invisibleUntil\":\"i\",\"groupId\":\"g\",\"deduplicationId\":\"d\",\"novo\":{\"campo\":[1,2]}}";
        String m2 = "{\"id\":\"m2\",\"body\":\"olá ✓ \\u00e9\",\"contentType\":\"text/plain; charset=utf-8\",\"receiptToken\":\"rt-m2\"}";
        String m3 = "{\"id\":\"m3\",\"body\":\"AAEC/w==\",\"contentType\":\"application/octet-stream\",\"bodyEncoding\":\"base64\",\"receiptToken\":\"rt-m3\"}";
        String m4 = "{\"id\":\"m4\",\"body\":{\"a\":1},\"contentType\":\"application/vnd.taba+json\",\"receiptToken\":\"rt-m4\","
                + "\"deadLetteredAt\":\"d\",\"deadLetterReason\":\"max-deliveries\"}";
        server.fallback(reply(200, "{\"messages\":[" + m1 + "," + m2 + "," + m3 + "," + m4 + "]}"));
        List<Message> messages = client.consume("q");
        // The JSON text comes exactly as the server sent it, not re-serialized.
        assertEquals("{\"pedido\": 9001, \"valor\": 1.0, \"grande\": 12345678901234567890}", messages.get(0).getBodyText());
        assertEquals(3, messages.get(0).getDeliveryCount());
        assertEquals("g", messages.get(0).getGroupId());
        assertEquals("d", messages.get(0).getDeduplicationId());
        assertEquals("rt-m1", messages.get(0).getReceiptToken());
        assertEquals("p", messages.get(0).getPublishedAt());
        assertEquals("i", messages.get(0).getInvisibleUntil());
        assertNull(messages.get(0).getDeadLetteredAt());
        assertEquals("olá ✓ é", messages.get(1).getBodyText());
        assertEquals("text/plain; charset=utf-8", messages.get(1).getContentType());
        assertArrayEquals(new byte[] {0, 1, 2, (byte) 255}, messages.get(2).getBody());
        assertEquals("{\"a\":1}", messages.get(3).getBodyText());
        assertEquals("d", messages.get(3).getDeadLetteredAt());
        assertEquals("max-deliveries", messages.get(3).getDeadLetterReason());
    }

    /// UC: UC-CNT-026
    @Test
    void anInvalidAnswerIsAnInvalidResponseErrorAndNotACrash() {
        server.reply(
                reply(200, "not json"),
                reply(200, "{\"messages\":[{\"id\":\"m1\",\"body\":\"AAA\",\"bodyEncoding\":\"base64\"}]}"),
                reply(200, "{\"messages\":[{\"id\":\"m1\",\"body\":\"x\""),
                reply(200, "[1,2]"));
        assertEquals("client.invalid-response", error(() -> client.getTopic("a")).getCode());
        assertEquals("client.invalid-response", error(() -> client.consume("q")).getCode());
        assertEquals("client.invalid-response", error(() -> client.consume("q")).getCode());
        assertEquals("client.invalid-response", error(() -> client.getTopic("a")).getCode());
    }

    /// UC: UC-CNT-028
    @Test
    void unknownFieldsInResponsesAreIgnoredAndTheContractVersionIsDeclared() {
        server.fallback(reply(200, topicJson("a").replace("}", ",\"novo\":{\"campo\":[1,{\"x\":\"y\"}]},\"outro\":null}")));
        assertEquals("a", client.getTopic("a").getName());
        assertEquals("1", TabaClient.CONTRACT_VERSION);
    }

    // ---------------------------------------------------------------------------------------------
    // Retries and errors
    // ---------------------------------------------------------------------------------------------

    /// UC: UC-CNT-009, UC-CNT-017
    @Test
    void aTimeoutIsNotRepeatedAndIsAClientSideError() {
        TabaClient quick = newClient(TabaConfig.of(server.url()).requestTimeout(Duration.ofMillis(300)), waits::add);
        server.fallback(HANG);
        TabaException e = error(() -> quick.getTopic("a"));
        assertEquals("client.timeout", e.getCode());
        assertEquals(0, e.getStatus());
        assertTrue(e.isClientSide());
        assertEquals(1, server.requests.size());
    }

    /// UC: UC-CNT-010
    @Test
    void retries429And503WaitingForRetryAfterThenBackoff() {
        server.reply(
                reply(429, "{\"code\":\"rate.limited\",\"message\":\"slow\"}").header("Retry-After", "7"),
                reply(503, "{\"code\":\"node.unavailable\",\"message\":\"x\"}").header("Retry-After", "2"),
                reply(200, topicJson("a")));
        assertEquals("a", client.getTopic("a").getName());
        assertEquals(3, server.requests.size());
        assertEquals(Arrays.asList(Duration.ofSeconds(7), Duration.ofSeconds(2)), waits);
    }

    /// UC: UC-CNT-010
    @Test
    void backoffWithoutRetryAfterIsOneTwoFourSecondsUpToMaxAttempts() {
        TabaClient four = newClient(TabaConfig.of(server.url()).maxAttempts(4), waits::add);
        server.fallback(reply(503, "{\"code\":\"node.unavailable\",\"message\":\"x\"}"));
        TabaException e = error(() -> four.getTopic("a"));
        assertEquals(503, e.getStatus());
        assertEquals(4, server.requests.size());
        assertEquals(Arrays.asList(Duration.ofSeconds(1), Duration.ofSeconds(2), Duration.ofSeconds(4)), waits);

        waits.clear();
        TabaClient once = newClient(TabaConfig.of(server.url()).maxAttempts(1), waits::add);
        server.fallback(reply(429, "{\"code\":\"rate.limited\",\"message\":\"x\"}"));
        error(() -> once.getTopic("a"));
        assertEquals(5, server.requests.size());
        assertTrue(waits.isEmpty());
    }

    /// UC: UC-CNT-011
    @Test
    void afterA503A409OnCreateAndA404OnDeleteCountAsSuccess() {
        server.reply(
                reply(503, "{\"code\":\"node.unavailable\",\"message\":\"x\"}").header("Retry-After", "0"),
                reply(409, "{\"code\":\"topic.already-exists\",\"message\":\"exists\"}"),
                reply(200, topicJson("orders")),
                reply(503, "{\"code\":\"node.unavailable\",\"message\":\"x\"}").header("Retry-After", "0"),
                reply(404, "{\"code\":\"topic.not-found\",\"message\":\"gone\"}"));
        assertEquals("orders", client.createTopic(TopicRequest.queue("orders")).getName());
        assertEquals(Arrays.asList("POST", "POST", "GET"), Arrays.asList(
                server.requests.get(0).method, server.requests.get(1).method, server.requests.get(2).method));
        client.deleteTopic("orders");
        assertEquals(5, server.requests.size());
    }

    /// UC: UC-CNT-011, UC-CNT-014
    @Test
    void withoutAPrevious503A409OrA404IsAPlainError() {
        server.reply(
                reply(409, "{\"code\":\"topic.already-exists\",\"message\":\"exists\"}"),
                reply(404, "{\"code\":\"topic.not-found\",\"message\":\"gone\"}"));
        assertEquals("topic.already-exists", error(() -> client.createTopic(TopicRequest.queue("a"))).getCode());
        assertEquals("topic.not-found", error(() -> client.deleteTopic("a")).getCode());
        assertEquals(2, server.requests.size());
    }

    /// UC: UC-CNT-012
    @Test
    void gatewayErrorsAreOnlyRepeatedWhenPublishingToAFifo() {
        server.reply(reply(502, "bad gateway"), reply(504, "timeout"), reply(200, "{\"id\":\"m1\"}"));
        PublishResponse response = client.publish("q", "{}".getBytes(StandardCharsets.UTF_8), new PublishOptions().fifo(true).deduplicationId("d"));
        assertEquals("m1", response.getId());
        assertEquals(3, server.requests.size());

        server.fallback(reply(502, "bad gateway"));
        assertEquals(502, error(() -> client.publish("q", "{}".getBytes(StandardCharsets.UTF_8))).getStatus());
        assertEquals(4, server.requests.size());
        server.fallback(reply(504, "timeout"));
        assertEquals(504, error(() -> client.getTopic("a")).getStatus());
        assertEquals(5, server.requests.size());
    }

    /// UC: UC-CNT-013
    @Test
    void networkFailureAfterSendingIsNotRepeated() throws Exception {
        // A server that reads the request and drops the connection with no answer.
        try (ServerSocket dropping = new ServerSocket(0, 5, InetAddress.getLoopbackAddress())) {
            AtomicReference<Integer> accepted = new AtomicReference<>(0);
            Thread thread = new Thread(() -> {
                while (!dropping.isClosed()) {
                    try (Socket socket = dropping.accept()) {
                        accepted.updateAndGet(n -> n + 1);
                        socket.getInputStream().read(new byte[4096]);
                    } catch (IOException e) {
                        return;
                    }
                }
            });
            thread.setDaemon(true);
            thread.start();
            TabaClient own = newClient(TabaConfig.of("http://127.0.0.1:" + dropping.getLocalPort()), waits::add);
            TabaException e = error(() -> own.publish("q", "{}".getBytes(StandardCharsets.UTF_8)));
            assertEquals("client.network", e.getCode());
            assertEquals(0, e.getStatus());
            assertEquals(1, accepted.get());
            assertTrue(waits.isEmpty());
        }
    }

    /// UC: UC-CNT-013
    @Test
    void aRefusedConnectionIsRepeatedAndThenANetworkError() {
        ScriptedServer closed = new ScriptedServer();
        String url = closed.url();
        closed.close();
        TabaClient own = newClient(TabaConfig.of(url).maxAttempts(3), waits::add);
        TabaException e = error(() -> own.publish("q", "{}".getBytes(StandardCharsets.UTF_8)));
        assertEquals("client.network", e.getCode());
        assertEquals(0, e.getStatus());
        assertEquals(Arrays.asList(Duration.ofSeconds(1), Duration.ofSeconds(2)), waits);
    }

    /// UC: UC-CNT-013
    @Test
    void aRefusedConnectionSucceedsWhenTheNodeComesBack() {
        ScriptedServer first = new ScriptedServer();
        int port = first.port();
        first.close();
        List<ScriptedServer> revived = new ArrayList<>();
        TabaClient own = newClient(TabaConfig.of("http://127.0.0.1:" + port), duration -> {
            if (revived.isEmpty()) {
                ScriptedServer node = new ScriptedServer(port);
                node.fallback(reply(200, topicJson("a")));
                revived.add(node);
                toClose.add(node);
            }
        });
        assertEquals("a", own.getTopic("a").getName());
    }

    /// UC: UC-CNT-014
    @Test
    void a4xxOtherThan429IsNeverRepeated() {
        for (int status : new int[] {400, 401, 403, 404, 409, 422}) {
            server.reply(reply(status, "{\"code\":\"x." + status + "\",\"message\":\"no\"}"));
            TabaException e = error(() -> client.getTopic("a"));
            assertEquals(status, e.getStatus());
            assertEquals("x." + status, e.getCode());
        }
        assertEquals(6, server.requests.size());
        assertTrue(waits.isEmpty());
    }

    /// UC: UC-CNT-017, UC-CNT-018
    @Test
    void theErrorCarriesTheErrorResponseFieldsAndTheHeaders() {
        server.reply(
                reply(422, "{\"code\":\"topic.invalid-name\",\"message\":\"bad name\",\"requestId\":\"body-id\","
                        + "\"details\":[{\"field\":\"name\",\"reason\":\"x\"}]}").header("Retry-After", "9").header("X-Request-Id", "header-id"),
                reply(400, "{\"code\":\"x.y\",\"message\":\"m\"}").header("X-Request-Id", "header-id"));
        TabaException e = error(() -> client.createTopic(TopicRequest.queue("!")));
        assertEquals(422, e.getStatus());
        assertEquals("topic.invalid-name", e.getCode());
        assertEquals("bad name", e.getMessage());
        assertEquals("body-id", e.getRequestId());
        assertEquals(Integer.valueOf(9), e.getRetryAfter());
        assertEquals("[{\"field\":\"name\",\"reason\":\"x\"}]", e.getDetails());
        assertTrue(e.getBody().length > 0);
        assertTrue(e.toString().contains("topic.invalid-name: bad name"));
        assertEquals("header-id", error(() -> client.getTopic("a")).getRequestId());
    }

    /// UC: UC-CNT-017
    @Test
    void aNonJsonErrorStillReportsAStatusCode() {
        server.reply(reply(500, "<html>boom</html>"), reply(500, ""));
        TabaException html = error(() -> client.getTopic("a"));
        assertEquals("http.500", html.getCode());
        assertEquals("<html>boom</html>", html.getMessage());
        TabaException empty = error(() -> client.getTopic("a"));
        assertEquals("http.500", empty.getCode());
        assertTrue(empty.getMessage().contains("no body"));
    }

    // ---------------------------------------------------------------------------------------------
    // Threads, async, cancellation
    // ---------------------------------------------------------------------------------------------

    /// UC: UC-CNT-020
    @Test
    void manyThreadsCanUseOneClientAtTheSameTime() throws Exception {
        server.fallback(reply(200, topicJson("a")));
        List<Thread> threads = new ArrayList<>();
        List<Throwable> failures = new CopyOnWriteArrayList<>();
        for (int i = 0; i < 10; i++) {
            Thread thread = new Thread(() -> {
                try {
                    for (int n = 0; n < 5; n++) {
                        assertEquals("a", client.getTopic("a").getName());
                    }
                } catch (Throwable t) {
                    failures.add(t);
                }
            });
            threads.add(thread);
            thread.start();
        }
        for (Thread thread : threads) {
            thread.join();
        }
        assertTrue(failures.isEmpty(), failures.toString());
        assertEquals(50, server.requests.size());
    }

    /// UC: UC-CNT-032
    @Test
    void theAsyncFormsDoTheSameAsTheSyncOnes() throws Exception {
        server.fallback(reply(200, "{\"items\":[" + topicJson("a") + "],\"messages\":[],\"id\":\"m1\",\"name\":\"a\"}"));
        Message message = new Message("m1", new byte[0], "application/json", "rt", 1, "", "", null, null, null, null, "");
        CompletableFuture.allOf(
                        client.createTopicAsync(TopicRequest.queue("a")),
                        client.getTopicAsync("a"),
                        client.listTopicsAsync(1, null, null),
                        client.listAllTopicsAsync(null),
                        client.updateTopicAsync("a", new TopicUpdate()),
                        client.deleteTopicAsync("a"),
                        client.publishAsync("a", new byte[] {1}, new PublishOptions().groupId("g")),
                        client.consumeAsync("a", new ConsumeOptions().waitTime(1)),
                        client.ackAsync("a", message),
                        client.changeVisibilityAsync("a", message, 5))
                .get(10, TimeUnit.SECONDS);
        assertEquals(10, server.requests.size());

        server.reply(reply(404, "{\"code\":\"topic.not-found\",\"message\":\"gone\"}"));
        ExecutionException e = assertThrows(ExecutionException.class, () -> client.getTopicAsync("zzz").get(10, TimeUnit.SECONDS));
        assertEquals("topic.not-found", ((TabaException) e.getCause()).getCode());
    }

    /// UC: UC-CNT-020
    @Test
    void interruptingTheThreadCancelsTheCallWithoutAClientError() throws Exception {
        server.fallback(HANG);
        AtomicReference<Throwable> outcome = new AtomicReference<>();
        AtomicReference<Boolean> flag = new AtomicReference<>();
        Thread thread = new Thread(() -> {
            try {
                client.consume("q", new ConsumeOptions().waitTime(20));
            } catch (Throwable t) {
                outcome.set(t);
                flag.set(Thread.currentThread().isInterrupted());
            }
        });
        thread.start();
        waitFor(() -> server.requests.size() == 1);
        thread.interrupt();
        thread.join(5000);
        assertFalse(thread.isAlive());
        assertTrue(outcome.get() instanceof CancellationException, String.valueOf(outcome.get()));
        assertEquals(Boolean.TRUE, flag.get());
        assertEquals(1, server.requests.size());
    }

    /// UC: UC-CNT-020
    @Test
    void cancellingTheFutureInterruptsTheAsyncCall() throws Exception {
        server.fallback(HANG);
        CompletableFuture<List<Message>> future = client.consumeAsync("q", new ConsumeOptions().waitTime(20));
        waitFor(() -> server.requests.size() == 1);
        assertTrue(future.cancel(true));
        assertTrue(future.isCancelled());
        assertEquals(1, server.requests.size());
    }

    // ---------------------------------------------------------------------------------------------
    // subscribe, JSON, consuming with a handler
    // ---------------------------------------------------------------------------------------------

    /// UC: UC-CNT-031
    @Test
    void subscribeAcksOnlyWhatTheHandlerAcceptedAndStopsOnClose() throws Exception {
        server.batches(
                "[" + messageJson("m1", "{\"n\":1}") + "]",
                "[" + messageJson("m2", "{\"n\":2}") + "]",
                "[" + messageJson("m3", "{\"n\":3}") + "]");
        List<String> handled = new CopyOnWriteArrayList<>();
        List<String> failures = new CopyOnWriteArrayList<>();
        Subscription subscription = client.subscribe("q", null, message -> {
            handled.add(message.getId());
            if (message.getId().equals("m2")) {
                throw new IllegalStateException("boom");
            }
        }, (message, e) -> failures.add(message.getId() + ":" + e.getMessage()));
        waitFor(() -> handled.size() == 3);
        waitFor(() -> requestsOf("DELETE").size() == 2);
        subscription.close();
        assertTrue(subscription.completion().isDone());
        assertFalse(subscription.completion().isCompletedExceptionally());

        assertEquals(Arrays.asList("m1", "m2", "m3"), handled.subList(0, 3));
        assertEquals(Collections.singletonList("m2:boom"), failures);
        List<String> acks = new ArrayList<>();
        for (Recorded ack : requestsOf("DELETE")) {
            acks.add(ack.url);
        }
        assertEquals(Arrays.asList("/topics/q/messages/m1?receiptToken=rt-m1", "/topics/q/messages/m3?receiptToken=rt-m3"), acks);
        Map<String, String> first = server.requests.get(0).query();
        assertEquals("20", first.get("waitTime"));
        assertEquals("1", first.get("maxMessages"));
    }

    /// UC: UC-CNT-031
    @Test
    void subscribeEndsWithoutAnErrorWhenClosedDuringTheLongPolling() throws Exception {
        server.fallback(HANG);
        Subscription subscription = client.subscribe("q", null, message -> { });
        waitFor(() -> server.requests.size() == 1);
        subscription.close();
        assertTrue(subscription.completion().isDone());
        assertFalse(subscription.completion().isCompletedExceptionally());
        assertEquals(1, server.requests.size());
    }

    /// UC: UC-CNT-031
    @Test
    void aServerErrorWhileSubscribingEndsTheLoopAndSurfacesInTheCompletion() throws Exception {
        server.fallback(reply(403, "{\"code\":\"auth.forbidden\",\"message\":\"no\"}"));
        Subscription subscription = client.subscribe("q", null, message -> { });
        TabaException e = assertThrows(TabaException.class, subscription::join);
        assertEquals("auth.forbidden", e.getCode());
        subscription.close();
    }

    private static class Order {
        int pedido;
    }

    private static TabaJsonSerializer pedidoSerializer() {
        return new TabaJsonSerializer() {
            @Override
            public String serialize(Object value) {
                return "{\"pedido\":" + ((Order) value).pedido + "}";
            }

            @Override
            @SuppressWarnings("unchecked")
            public <T> T deserialize(String json, Class<T> type) {
                if (!json.startsWith("{\"pedido\":")) {
                    throw new IllegalArgumentException("not an order");
                }
                Order order = new Order();
                order.pedido = Integer.parseInt(json.replaceAll("[^0-9]", ""));
                return (T) order;
            }
        };
    }

    /// UC: UC-CNT-033
    @Test
    void publishJsonSerializesTheObjectAndSendsItAsJson() {
        server.fallback(reply(200, "{\"id\":\"m1\"}"));
        TabaClient json = newClient(TabaConfig.of(server.url()).jsonSerializer(pedidoSerializer()), waits::add);
        Order order = new Order();
        order.pedido = 9001;
        json.publishJson("q", order);
        assertEquals("application/json", server.requests.get(0).header("Content-Type"));
        assertEquals("{\"pedido\":9001}", server.requests.get(0).text());

        assertEquals("client.invalid-config", error(() -> client.publishJson("q", order)).getCode());
        assertThrows(NullPointerException.class, () -> json.publishJson("q", null));
    }

    /// UC: UC-CNT-034
    @Test
    void consumeWithAHandlerDeletesByDefaultAndKeepsTheMessageWhenAskedOrWhenItFails() throws Exception {
        TabaClient quiet = newBlockingClient(TabaConfig.of(server.url()));
        server.batches(
                "[" + messageJson("m1", "{\"n\":1}") + "]",
                "[" + messageJson("m2", "{\"n\":2}") + "]",
                "[" + messageJson("m3", "{\"n\":3}") + "]");
        assertEquals(1, quiet.consume("q", (ConsumeOptions) null, context -> {
            assertEquals("m1", context.getMessage().getId());
            assertTrue(context.isDelete());
        }));
        assertEquals(1, quiet.consume("q", (ConsumeOptions) null, context -> context.setDelete(false)));
        IllegalStateException failure = assertThrows(IllegalStateException.class, () -> quiet.consume("q", context -> {
            throw new IllegalStateException("handler failed");
        }));
        assertEquals("handler failed", failure.getMessage());
        assertEquals(Arrays.asList("GET", "DELETE", "GET", "GET"), methods());
        Map<String, String> poll = server.requests.get(0).query();
        assertEquals("1", poll.get("maxMessages"));
        assertEquals("120", poll.get("visibilityTimeout"));
        assertEquals("20", poll.get("waitTime"));

        server.batches("[" + messageJson("m4", "{}") + "]");
        TabaClient.HandlerFailedException checked = assertThrows(TabaClient.HandlerFailedException.class, () -> quiet.consume("q", context -> {
            throw new IOException("disk");
        }));
        assertTrue(checked.getCause() instanceof IOException);
    }

    private List<String> methods() {
        List<String> methods = new ArrayList<>();
        synchronized (server.requests) {
            for (Recorded request : server.requests) {
                methods.add(request.method);
                if (methods.size() == 4) {
                    break;
                }
            }
        }
        return methods;
    }

    /// UC: UC-CNT-034
    @Test
    void consumeWithAHandlerRenewsTheVisibilityWhileItRunsAndLetsItChangeIt() throws Exception {
        server.batches("[" + messageJson("m1", "{\"n\":1}") + "]");
        // Each wait of the renewal ends only when the test says so (a "tick").
        Semaphore ticks = new Semaphore(0);
        TabaClient renewing = new TabaClient(TabaConfig.of(server.url()), duration -> ticks.acquire());
        toClose.add(renewing);
        renewing.consume("q", new ConsumeOptions().visibilityTimeout(90), context -> {
            ticks.release(2);
            waitFor(() -> requestsOf("PUT").size() == 2);
            assertEquals(90, context.getLeaseSeconds());
            context.changeVisibility(300);
            assertEquals(300, context.getLeaseSeconds());
            ticks.release(2);
            waitFor(() -> requestsOf("PUT").size() == 5);
        });
        List<String> bodies = new ArrayList<>();
        for (Recorded put : requestsOf("PUT")) {
            bodies.add(put.text());
        }
        assertEquals(
                Arrays.asList(
                        "{\"visibilityTimeout\":90}",
                        "{\"visibilityTimeout\":90}",
                        "{\"visibilityTimeout\":300}",
                        "{\"visibilityTimeout\":300}",
                        "{\"visibilityTimeout\":300}"),
                bodies);
        assertEquals("DELETE", server.requests.get(server.requests.size() - 1).method);
    }

    /// UC: UC-CNT-034
    @Test
    void aRefusedRenewalMarksTheLeaseAsLostAndTheDeleteIsTolerated() throws Exception {
        String gone = "{\"code\":\"message.receipt-expired\",\"message\":\"x\"}";
        server.reply(reply(200, "{\"messages\":[" + messageJson("m1", "{\"n\":1}") + "]}"), reply(409, gone));
        server.fallback(reply(409, gone));
        AtomicReference<Integer> rounds = new AtomicReference<>(0);
        TabaClient renewing = new TabaClient(TabaConfig.of(server.url()), duration -> {
            if (rounds.updateAndGet(n -> n + 1) > 1) {
                Thread.sleep(Long.MAX_VALUE);
            }
        });
        toClose.add(renewing);
        AtomicReference<Boolean> lost = new AtomicReference<>(false);
        renewing.consume("q", context -> {
            waitFor(context::isLeaseLost);
            lost.set(context.isLeaseLost());
        });
        assertEquals(Boolean.TRUE, lost.get());
    }

    /// UC: UC-CNT-034
    @Test
    void consumeOfTHandsOverTheDeserializedBodyAndRefusesABodyThatIsNotAT() throws Exception {
        TabaClient typed = newBlockingClient(TabaConfig.of(server.url()).jsonSerializer(pedidoSerializer()));
        server.batches("[" + messageJson("m1", "{\"pedido\":9001}") + "]", "[" + messageJson("m2", "\"not an order\"") + "]");
        AtomicReference<Integer> seen = new AtomicReference<>();
        typed.consume("q", Order.class, context -> {
            seen.set(context.getBody().pedido);
            assertEquals("m1", context.getMessage().getId());
        });
        assertEquals(Integer.valueOf(9001), seen.get());
        TabaException e = assertThrows(TabaException.class, () -> typed.consume("q", Order.class, context -> {
            throw new AssertionError("the handler must not run");
        }));
        assertEquals("client.invalid-response", e.getCode());
        assertEquals(Arrays.asList("GET", "DELETE", "GET"), methods().subList(0, 3));
        assertNotNull(seen.get());
    }
}
