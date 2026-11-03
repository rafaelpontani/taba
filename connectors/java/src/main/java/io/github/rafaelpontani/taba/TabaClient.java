package io.github.rafaelpontani.taba;

import java.nio.charset.StandardCharsets;
import java.time.Duration;
import java.util.ArrayList;
import java.util.List;
import java.util.concurrent.Callable;
import java.util.concurrent.CancellationException;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.Future;
import java.util.concurrent.atomic.AtomicInteger;
import java.util.function.BiConsumer;

/**
 * The Taba client API client (doc 21): the nine operations, each in the blocking form and in the {@code ...Async}
 * form ({@link CompletableFuture}), {@code subscribe} and the consume with a handler. It is safe for concurrent
 * use: a single object serves several threads.
 *
 * <pre>{@code
 * try (TabaClient client = new TabaClient(TabaConfig.of("http://localhost:8080").token(token))) {
 *     client.createTopic(TopicRequest.queue("orders"));
 *     client.publishText("orders", "hello");
 *     for (Message message : client.consume("orders", new ConsumeOptions().waitTime(20))) {
 *         System.out.println(message.getBodyText());
 *         client.ack("orders", message);
 *     }
 * }
 * }</pre>
 *
 * <p>Cancelling a blocking call is interrupting its thread; cancelling the future of an {@code ...Async} call
 * interrupts the thread that runs it. Either way the call ends with a {@link CancellationException} (never a
 * {@link TabaException}) and the interrupted flag stays set.
 */
public class TabaClient implements AutoCloseable {
    /** The version of this connector. */
    public static final String VERSION = "0.1.0";

    /** The version of the API contract this connector implements. */
    public static final String CONTRACT_VERSION = "1";

    /** {@code consume} waits {@code waitTime} plus this slack, not the {@code requestTimeout}. */
    static final Duration LONG_POLL_SLACK = Duration.ofSeconds(10);

    static final int HANDLER_MAX_MESSAGES = 1;
    static final int HANDLER_VISIBILITY_TIMEOUT = 120;
    static final int HANDLER_WAIT_TIME = 20;

    private static final AtomicInteger THREADS = new AtomicInteger();

    private final TabaConfig config;
    private final HttpTransport transport;
    private final java.util.concurrent.Executor executor;
    private final ExecutorService ownedExecutor;

    public TabaClient(TabaConfig config) {
        this(config, null);
    }

    TabaClient(TabaConfig config, HttpTransport.Sleeper sleeper) {
        if (config == null) {
            throw new NullPointerException("config");
        }
        this.config = config.copy();
        this.transport = new HttpTransport(this.config, sleeper);
        if (this.config.getExecutor() != null) {
            this.executor = this.config.getExecutor();
            this.ownedExecutor = null;
        } else {
            this.ownedExecutor = Executors.newCachedThreadPool(runnable -> daemon(runnable, "taba-client-"));
            this.executor = ownedExecutor;
        }
    }

    private static Thread daemon(Runnable runnable, String prefix) {
        Thread thread = new Thread(runnable, prefix + THREADS.incrementAndGet());
        thread.setDaemon(true);
        return thread;
    }

    /** A copy of the configuration in use. */
    public TabaConfig getConfig() {
        return config.copy();
    }

    /** Stops the threads the client owns (the {@code ...Async} calls in flight are interrupted). */
    @Override
    public void close() {
        if (ownedExecutor != null) {
            ownedExecutor.shutdownNow();
        }
        transport.close();
    }

    private static String segment(String value) {
        return ApiCall.encode(value);
    }

    private HttpTransport.Reply send(ApiCall call) {
        return transport.send(call);
    }

    private <T> CompletableFuture<T> async(Callable<T> work) {
        CompletableFuture<T> future = new CompletableFuture<>();
        Future<?> task = submit(() -> {
            try {
                future.complete(work.call());
            } catch (Throwable t) {
                future.completeExceptionally(t);
            }
        });
        future.whenComplete((value, failure) -> {
            if (future.isCancelled()) {
                task.cancel(true);
            }
        });
        return future;
    }

    private Future<?> submit(Runnable runnable) {
        if (ownedExecutor != null) {
            return ownedExecutor.submit(runnable);
        }
        java.util.concurrent.FutureTask<Void> task = new java.util.concurrent.FutureTask<>(runnable, null);
        executor.execute(task);
        return task;
    }

    // -----------------------------------------------------------------------------------------
    // Topics
    // -----------------------------------------------------------------------------------------

    /** {@code POST /topics}. */
    public Topic createTopic(TopicRequest request) {
        if (request == null) {
            throw new NullPointerException("request");
        }
        ApiCall call = new ApiCall("POST", "/topics").json(request.toJson());
        call.recover.add(new ApiCall.Recover(409, "topic.already-exists"));
        HttpTransport.Reply reply = send(call);
        if (reply.recovered) {
            return getTopic(request.getName());
        }
        return ModelParser.parseTopic(reply.text());
    }

    public CompletableFuture<Topic> createTopicAsync(TopicRequest request) {
        return async(() -> createTopic(request));
    }

    /** {@code GET /topics/{topic}}. */
    public Topic getTopic(String name) {
        return ModelParser.parseTopic(send(new ApiCall("GET", "/topics/" + segment(name))).text());
    }

    public CompletableFuture<Topic> getTopicAsync(String name) {
        return async(() -> getTopic(name));
    }

    /** {@code GET /topics}. Null parameters do not go in the query. */
    public Page<Topic> listTopics(Integer limit, String cursor, String name) {
        ApiCall call = new ApiCall("GET", "/topics");
        if (limit != null) {
            call.with("limit", limit);
        }
        if (cursor != null) {
            call.with("cursor", cursor);
        }
        if (name != null) {
            call.with("name", name);
        }
        return ModelParser.parsePage(send(call).text());
    }

    public Page<Topic> listTopics() {
        return listTopics(null, null, null);
    }

    public CompletableFuture<Page<Topic>> listTopicsAsync(Integer limit, String cursor, String name) {
        return async(() -> listTopics(limit, cursor, name));
    }

    /** All topics, page by page. */
    public List<Topic> listAllTopics(String name) {
        List<Topic> all = new ArrayList<>();
        String cursor = null;
        do {
            Page<Topic> page = listTopics(null, cursor, name);
            all.addAll(page.getItems());
            cursor = page.getNextCursor();
        } while (cursor != null);
        return all;
    }

    public CompletableFuture<List<Topic>> listAllTopicsAsync(String name) {
        return async(() -> listAllTopics(name));
    }

    /** {@code PATCH /topics/{topic}}. It is a real {@code PATCH}, with no verb-override header. */
    public Topic updateTopic(String name, TopicUpdate update) {
        if (update == null) {
            throw new NullPointerException("update");
        }
        ApiCall call = new ApiCall("PATCH", "/topics/" + segment(name)).json(update.toJson());
        return ModelParser.parseTopic(send(call).text());
    }

    public CompletableFuture<Topic> updateTopicAsync(String name, TopicUpdate update) {
        return async(() -> updateTopic(name, update));
    }

    /** {@code DELETE /topics/{topic}}. */
    public void deleteTopic(String name) {
        ApiCall call = new ApiCall("DELETE", "/topics/" + segment(name));
        call.recover.add(new ApiCall.Recover(404, "topic.not-found"));
        send(call);
    }

    public CompletableFuture<Void> deleteTopicAsync(String name) {
        return async(() -> {
            deleteTopic(name);
            return null;
        });
    }

    // -----------------------------------------------------------------------------------------
    // Publishing
    // -----------------------------------------------------------------------------------------

    /** {@code POST /topics/{topic}/messages}. The body goes as bytes, without validation. */
    public PublishResponse publish(String topic, byte[] body, PublishOptions options) {
        PublishOptions o = options != null ? options : new PublishOptions();
        ApiCall call = new ApiCall("POST", "/topics/" + segment(topic) + "/messages");
        call.body = body != null ? body : new byte[0];
        call.contentType = o.getContentType() != null ? o.getContentType() : "application/json";
        call.requestId = o.getRequestId();
        call.traceparent = o.getTraceparent();
        call.retryGateway = o.isFifo();
        if (o.getGroupId() != null) {
            call.with("groupId", o.getGroupId());
        }
        if (o.getDeduplicationId() != null) {
            call.with("deduplicationId", o.getDeduplicationId());
        }
        return ModelParser.parsePublishResponse(send(call).text());
    }

    public PublishResponse publish(String topic, byte[] body) {
        return publish(topic, body, null);
    }

    public CompletableFuture<PublishResponse> publishAsync(String topic, byte[] body, PublishOptions options) {
        return async(() -> publish(topic, body, options));
    }

    /**
     * Serializes the object to JSON (with the {@link TabaConfig#jsonSerializer(TabaJsonSerializer) jsonSerializer})
     * and publishes it with {@code Content-Type: application/json}. To serialize on your own, use
     * {@link #publish(String, byte[], PublishOptions)} with the bytes.
     */
    public PublishResponse publishJson(String topic, Object value) {
        if (value == null) {
            throw new NullPointerException("value");
        }
        TabaJsonSerializer serializer = serializer();
        String json;
        try {
            json = serializer.serialize(value);
        } catch (Exception e) {
            throw TabaException.client("client.invalid-request", "cannot serialize the value: " + e.getMessage(), e);
        }
        return publishRawJson(topic, json);
    }

    public CompletableFuture<PublishResponse> publishJsonAsync(String topic, Object value) {
        return async(() -> publishJson(topic, value));
    }

    /** Publishes a JSON text the application already built, without serializing. */
    public PublishResponse publishRawJson(String topic, String json) {
        return publish(topic, json.getBytes(StandardCharsets.UTF_8), new PublishOptions().contentType("application/json"));
    }

    /** Publishes a text ({@code Content-Type: text/plain; charset=utf-8}). */
    public PublishResponse publishText(String topic, String text) {
        return publish(topic, text.getBytes(StandardCharsets.UTF_8), new PublishOptions().contentType("text/plain; charset=utf-8"));
    }

    private TabaJsonSerializer serializer() {
        TabaJsonSerializer serializer = config.getJsonSerializer();
        if (serializer == null) {
            throw TabaException.client(
                    "client.invalid-config",
                    "no JSON serializer: set TabaConfig.jsonSerializer (the JDK has no JSON library)");
        }
        return serializer;
    }

    /**
     * {@code POST /topics/{topic}/messages/batch}: publishes many messages in one request (1 to the server's
     * {@code api.batch.max-messages}, 1000 by default). Each item is answered as its single publication and one that
     * fails does not undo the others: look at {@link BatchResult#isOk()}. On a FIFO queue the items of a group are
     * published in the order of the list. Like the single publication outside FIFO, a failure of the network is not
     * retried (it could duplicate).
     */
    public List<BatchResult> publishBatch(String topic, List<BatchMessage> messages) {
        if (messages == null) {
            throw new NullPointerException("messages");
        }
        StringBuilder json = new StringBuilder("{\"messages\":[");
        for (int i = 0; i < messages.size(); i++) {
            if (i > 0) {
                json.append(',');
            }
            json.append(messages.get(i).toJson());
        }
        ApiCall call = new ApiCall("POST", "/topics/" + segment(topic) + "/messages/batch").json(json.append("]}").toString());
        return ModelParser.parseBatchResults(send(call).text());
    }

    public CompletableFuture<List<BatchResult>> publishBatchAsync(String topic, List<BatchMessage> messages) {
        return async(() -> publishBatch(topic, messages));
    }

    // -----------------------------------------------------------------------------------------
    // Consuming
    // -----------------------------------------------------------------------------------------

    static ApiCall consumeCall(String topic, ConsumeOptions options) {
        ConsumeOptions o = options != null ? options : new ConsumeOptions();
        ApiCall call = new ApiCall("GET", "/topics/" + segment(topic) + "/messages");
        if (o.getMaxMessages() != null) {
            call.with("maxMessages", o.getMaxMessages());
        }
        if (o.getVisibilityTimeout() != null) {
            call.with("visibilityTimeout", o.getVisibilityTimeout());
        }
        if (o.getWaitTime() != null) {
            call.with("waitTime", o.getWaitTime());
        }
        if (o.isAutoCreate()) {
            call.with("autoCreate", true);
            if (o.getVolatileTopic() != null) {
                call.with("volatile", o.getVolatileTopic());
            }
            if (o.getIdleTimeout() != null) {
                call.with("idleTimeout", o.getIdleTimeout());
            }
            if (o.getFifo() != null) {
                call.with("fifo", o.getFifo());
            }
            if (o.getTags() != null) {
                for (java.util.Map.Entry<String, String> tag : o.getTags().entrySet()) {
                    call.with("tag." + tag.getKey(), tag.getValue());
                }
            }
        }
        call.requestId = o.getRequestId();
        call.traceparent = o.getTraceparent();
        call.timeout = Duration.ofSeconds(o.getWaitTime() != null ? o.getWaitTime() : 0).plus(LONG_POLL_SLACK);
        return call;
    }

    /**
     * {@code GET /topics/{topic}/messages}. The read time is {@code waitTime + 10 s}, not the
     * {@code requestTimeout}. A timeout is not retried (the node may have captured messages, which come back by
     * themselves at the end of the {@code visibilityTimeout}). Returns an empty list if there are no messages.
     */
    public List<Message> consume(String topic, ConsumeOptions options) {
        return ModelParser.parseMessages(send(consumeCall(topic, options)).text());
    }

    public List<Message> consume(String topic) {
        return consume(topic, (ConsumeOptions) null);
    }

    public CompletableFuture<List<Message>> consumeAsync(String topic, ConsumeOptions options) {
        return async(() -> consume(topic, options));
    }

    /** {@code DELETE /topics/{topic}/messages/{id}?receiptToken=}. */
    public void ack(String topic, String messageId, String receiptToken) {
        ApiCall call = new ApiCall("DELETE", "/topics/" + segment(topic) + "/messages/" + segment(messageId))
                .with("receiptToken", receiptToken);
        send(call);
    }

    /** Acknowledges the message received from {@link #consume(String, ConsumeOptions)}. */
    public void ack(String topic, Message message) {
        ack(topic, message.getId(), message.getReceiptToken());
    }

    public CompletableFuture<Void> ackAsync(String topic, Message message) {
        return async(() -> {
            ack(topic, message);
            return null;
        });
    }

    /** {@code PUT /topics/{topic}/messages/{id}/visibility?receiptToken=}. */
    public VisibilityResponse changeVisibility(String topic, String messageId, String receiptToken, int seconds) {
        ApiCall call = new ApiCall("PUT", "/topics/" + segment(topic) + "/messages/" + segment(messageId) + "/visibility")
                .with("receiptToken", receiptToken)
                .json("{\"visibilityTimeout\":" + seconds + "}");
        return ModelParser.parseVisibilityResponse(send(call).text());
    }

    /** Changes the invisibility of the message received from {@link #consume(String, ConsumeOptions)}. */
    public VisibilityResponse changeVisibility(String topic, Message message, int seconds) {
        return changeVisibility(topic, message.getId(), message.getReceiptToken(), seconds);
    }

    public CompletableFuture<VisibilityResponse> changeVisibilityAsync(String topic, Message message, int seconds) {
        return async(() -> changeVisibility(topic, message, seconds));
    }

    private static String receipts(List<Message> messages, String extra) {
        if (messages == null) {
            throw new NullPointerException("messages");
        }
        StringBuilder json = new StringBuilder("{\"messages\":[");
        for (int i = 0; i < messages.size(); i++) {
            Message message = messages.get(i);
            if (i > 0) {
                json.append(',');
            }
            json.append("{\"id\":").append(Json.quote(message.getId()))
                    .append(",\"receiptToken\":").append(Json.quote(message.getReceiptToken()))
                    .append(extra).append('}');
        }
        return json.append("]}").toString();
    }

    /**
     * {@code POST /topics/{topic}/messages/ack}: acknowledges many messages received from
     * {@link #consume(String, ConsumeOptions)} in one request. Each item is answered as its single ack
     * ({@code 204}, or {@code 409 message.stale-receipt}, {@code 404 message.not-found}...) and one that fails does not
     * stop the others.
     */
    public List<BatchResult> ackBatch(String topic, List<Message> messages) {
        ApiCall call = new ApiCall("POST", "/topics/" + segment(topic) + "/messages/ack").json(receipts(messages, ""));
        return ModelParser.parseBatchResults(send(call).text());
    }

    public CompletableFuture<List<BatchResult>> ackBatchAsync(String topic, List<Message> messages) {
        return async(() -> ackBatch(topic, messages));
    }

    /**
     * {@code POST /topics/{topic}/messages/visibility}: sets the invisibility of many messages already delivered to
     * {@code seconds} from now, in one request. Each item is answered as its single call.
     */
    public List<BatchResult> changeVisibilityBatch(String topic, List<Message> messages, int seconds) {
        ApiCall call = new ApiCall("POST", "/topics/" + segment(topic) + "/messages/visibility")
                .json(receipts(messages, ",\"visibilityTimeout\":" + seconds));
        return ModelParser.parseBatchResults(send(call).text());
    }

    public CompletableFuture<List<BatchResult>> changeVisibilityBatchAsync(String topic, List<Message> messages, int seconds) {
        return async(() -> changeVisibilityBatch(topic, messages, seconds));
    }

    // -----------------------------------------------------------------------------------------
    // Consuming with a handler
    // -----------------------------------------------------------------------------------------

    private static ConsumeOptions withHandlerDefaults(ConsumeOptions options) {
        ConsumeOptions copy = options != null ? options.copy() : new ConsumeOptions();
        if (copy.getMaxMessages() == null) {
            copy.maxMessages(HANDLER_MAX_MESSAGES);
        }
        if (copy.getVisibilityTimeout() == null) {
            copy.visibilityTimeout(HANDLER_VISIBILITY_TIMEOUT);
        }
        if (copy.getWaitTime() == null) {
            copy.waitTime(HANDLER_WAIT_TIME);
        }
        return copy;
    }

    /** A third of the invisibility (and never all of it), so a failed renewal can be tried again in time. */
    static Duration renewalInterval(int leaseSeconds) {
        return Duration.ofMillis(Math.max(1, leaseSeconds) * 1000L / 3);
    }

    /**
     * Reads from the queue and hands each message to {@code handler}, with {@link MessageContext#setDelete delete}
     * (default yes) and {@link MessageContext#changeVisibility}. While the handler runs, a second thread renews the
     * invisibility every third of it. Defaults: one message at a time, 20 s of long polling and 120 s of
     * invisibility. If the handler throws, the message is not deleted (it goes back to the queue at the end of the
     * invisibility) and the exception propagates (a checked one inside a {@link HandlerFailedException}).
     * Returns how many messages were handled.
     */
    public int consume(String topic, ConsumeOptions options, ThrowingConsumer<MessageContext> handler) {
        if (handler == null) {
            throw new NullPointerException("handler");
        }
        ConsumeOptions o = withHandlerDefaults(options);
        int handled = 0;
        for (Message message : consume(topic, o)) {
            MessageContext context = new MessageContext(
                    message, o.getVisibilityTimeout(), seconds -> changeVisibility(topic, message, seconds));
            handle(topic, context, () -> handler.accept(context));
            handled++;
        }
        return handled;
    }

    public int consume(String topic, ThrowingConsumer<MessageContext> handler) {
        return consume(topic, (ConsumeOptions) null, handler);
    }

    /**
     * The {@link #consume(String, ConsumeOptions, ThrowingConsumer)} with the body already deserialized as
     * {@code type} (by the {@link TabaConfig#jsonSerializer(TabaJsonSerializer) jsonSerializer}) in
     * {@link TypedMessageContext#getBody()}. If the body is not a {@code type}, it throws
     * {@code client.invalid-response} before calling the handler, and the message is not deleted.
     */
    public <T> int consume(String topic, ConsumeOptions options, Class<T> type, ThrowingConsumer<TypedMessageContext<T>> handler) {
        if (handler == null) {
            throw new NullPointerException("handler");
        }
        TabaJsonSerializer serializer = serializer();
        ConsumeOptions o = withHandlerDefaults(options);
        int handled = 0;
        for (Message message : consume(topic, o)) {
            T body;
            try {
                body = serializer.deserialize(message.getBodyText(), type);
            } catch (Exception e) {
                throw TabaException.client(
                        "client.invalid-response",
                        "body of message " + message.getId() + " is not a " + type.getSimpleName() + ": " + e.getMessage(),
                        e);
            }
            TypedMessageContext<T> context = new TypedMessageContext<>(
                    message, body, o.getVisibilityTimeout(), seconds -> changeVisibility(topic, message, seconds));
            handle(topic, context, () -> handler.accept(context));
            handled++;
        }
        return handled;
    }

    public <T> int consume(String topic, Class<T> type, ThrowingConsumer<TypedMessageContext<T>> handler) {
        return consume(topic, null, type, handler);
    }

    private static boolean lostOrGone(TabaException e) {
        return e.getStatus() == 409 || e.getStatus() == 404;
    }

    private void handle(String topic, MessageContext context, ThrowingRunnable run) {
        Thread renewal = daemon(() -> keepAlive(topic, context), "taba-renewal-");
        renewal.start();
        try {
            run.run();
        } catch (RuntimeException | Error e) {
            throw e;
        } catch (Exception e) {
            throw new HandlerFailedException(e);
        } finally {
            renewal.interrupt();
            boolean interrupted = false;
            while (true) {
                try {
                    renewal.join();
                    break;
                } catch (InterruptedException e) {
                    interrupted = true;
                }
            }
            if (interrupted) {
                Thread.currentThread().interrupt();
            }
        }
        if (context.isDelete()) {
            try {
                ack(topic, context.getMessage());
            } catch (TabaException e) {
                // The message already went back to the queue (deadline expired) or is already gone.
                if (!lostOrGone(e)) {
                    throw e;
                }
            }
        }
    }

    /** The second thread: renews the invisibility until the handler finishes. Never throws. */
    private void keepAlive(String topic, MessageContext context) {
        try {
            while (!Thread.currentThread().isInterrupted()) {
                transport.sleep(renewalInterval(context.getLeaseSeconds()));
                try {
                    changeVisibility(topic, context.getMessage(), context.getLeaseSeconds());
                } catch (TabaException e) {
                    if (lostOrGone(e)) {
                        // The message already went back to the queue or was deleted: renewing is no longer of any use.
                        context.markLost();
                        return;
                    }
                    // Transient failure (network, 5xx): tries again on the next round, before the deadline expires.
                }
            }
        } catch (InterruptedException | CancellationException e) {
            // The handler finished.
        }
    }

    @FunctionalInterface
    private interface ThrowingRunnable {
        void run() throws Exception;
    }

    /** Thrown by the consume with a handler when the handler threw a checked exception (the cause). */
    public static final class HandlerFailedException extends RuntimeException {
        private static final long serialVersionUID = 1L;

        HandlerFailedException(Exception cause) {
            super(cause.getMessage(), cause);
        }
    }

    // -----------------------------------------------------------------------------------------
    // subscribe
    // -----------------------------------------------------------------------------------------

    /**
     * The consume loop, on its own thread: long polling ({@code waitTime} 20 and one message at a time, if not
     * set), calls {@code handler} and acknowledges by itself <b>only if it finishes without throwing</b> (a message
     * that fails goes back to the queue at the end of the {@code visibilityTimeout}, and the error goes to
     * {@code onError}). Stop it with {@link Subscription#close()}. A server error while consuming ends the loop;
     * see {@link Subscription#completion()}.
     */
    public Subscription subscribe(
            String topic,
            ConsumeOptions options,
            ThrowingConsumer<Message> handler,
            BiConsumer<Message, Exception> onError) {
        if (handler == null) {
            throw new NullPointerException("handler");
        }
        ConsumeOptions poll = options != null ? options.copy() : new ConsumeOptions();
        if (poll.getWaitTime() == null) {
            poll.waitTime(HANDLER_WAIT_TIME);
        }
        if (poll.getMaxMessages() == null) {
            poll.maxMessages(HANDLER_MAX_MESSAGES);
        }
        Subscription subscription = new Subscription();
        Thread thread = daemon(() -> loop(subscription, topic, poll, handler, onError), "taba-subscribe-");
        subscription.attach(thread);
        thread.start();
        return subscription;
    }

    public Subscription subscribe(String topic, ConsumeOptions options, ThrowingConsumer<Message> handler) {
        return subscribe(topic, options, handler, null);
    }

    private void loop(
            Subscription subscription,
            String topic,
            ConsumeOptions poll,
            ThrowingConsumer<Message> handler,
            BiConsumer<Message, Exception> onError) {
        Throwable failure = null;
        try {
            while (!subscription.isStopped()) {
                List<Message> messages;
                subscription.polling(true);
                try {
                    messages = consume(topic, poll);
                } catch (CancellationException e) {
                    return;
                } finally {
                    subscription.polling(false);
                }
                for (Message message : messages) {
                    boolean handled;
                    try {
                        handler.accept(message);
                        handled = true;
                    } catch (Exception e) {
                        handled = false;
                        if (onError != null) {
                            onError.accept(message, e);
                        }
                    }
                    if (handled) {
                        try {
                            ack(topic, message);
                        } catch (TabaException e) {
                            // The message already went back to the queue (deadline expired) or is already gone.
                            if (!lostOrGone(e)) {
                                throw e;
                            }
                        }
                    }
                    if (subscription.isStopped()) {
                        return;
                    }
                }
            }
        } catch (RuntimeException | Error e) {
            failure = e;
        } finally {
            subscription.finish(failure);
        }
    }
}
