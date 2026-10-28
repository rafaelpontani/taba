package io.github.rafaelpontani.taba;

import com.sun.net.httpserver.Headers;
import com.sun.net.httpserver.HttpExchange;
import com.sun.net.httpserver.HttpServer;
import java.io.IOException;
import java.io.OutputStream;
import java.net.InetAddress;
import java.net.InetSocketAddress;
import java.net.URLDecoder;
import java.nio.charset.StandardCharsets;
import java.util.ArrayDeque;
import java.util.ArrayList;
import java.util.Collections;
import java.util.Deque;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.Executors;
import java.util.concurrent.TimeUnit;
import java.util.function.Function;

/**
 * A local HTTP server that records every request and answers from a script (the {@code ScriptedHandler} of the C#
 * connector). A reply is a {@link Reply}, a function of the request returning one, {@link #HANG} (never answers).
 */
final class ScriptedServer implements AutoCloseable {
    static final class Recorded {
        final String method;
        final String url;
        final Headers headers;
        final byte[] body;

        Recorded(String method, String url, Headers headers, byte[] body) {
            this.method = method;
            this.url = url;
            this.headers = headers;
            this.body = body;
        }

        String text() {
            return new String(body, StandardCharsets.UTF_8);
        }

        String header(String name) {
            return headers.getFirst(name);
        }

        Map<String, String> query() {
            Map<String, String> query = new LinkedHashMap<>();
            int mark = url.indexOf('?');
            if (mark >= 0) {
                for (String pair : url.substring(mark + 1).split("&")) {
                    int eq = pair.indexOf('=');
                    query.put(
                            URLDecoder.decode(pair.substring(0, eq), StandardCharsets.UTF_8),
                            URLDecoder.decode(pair.substring(eq + 1), StandardCharsets.UTF_8));
                }
            }
            return query;
        }
    }

    static final class Reply {
        final int status;
        final String body;
        final Map<String, String> headers = new LinkedHashMap<>();
        final boolean hang;

        Reply(int status, String body, boolean hang) {
            this.status = status;
            this.body = body;
            this.hang = hang;
        }

        Reply header(String name, String value) {
            headers.put(name, value);
            return this;
        }
    }

    static final Reply HANG = new Reply(0, "", true);

    static Reply reply(int status, String body) {
        return new Reply(status, body, false);
    }

    final List<Recorded> requests = Collections.synchronizedList(new ArrayList<>());
    private final Deque<Object> script = new ArrayDeque<>();
    private Object fallback = reply(200, "{}");
    private final HttpServer server;
    private final CountDownLatch stop = new CountDownLatch(1);

    ScriptedServer() {
        this(0);
    }

    ScriptedServer(int port) {
        try {
            server = HttpServer.create(new InetSocketAddress(InetAddress.getLoopbackAddress(), port), 0);
        } catch (IOException e) {
            throw new IllegalStateException(e);
        }
        server.setExecutor(Executors.newCachedThreadPool(runnable -> {
            Thread thread = new Thread(runnable);
            thread.setDaemon(true);
            return thread;
        }));
        server.createContext("/", this::serve);
        server.start();
    }

    int port() {
        return server.getAddress().getPort();
    }

    String url() {
        return "http://127.0.0.1:" + port();
    }

    ScriptedServer reply(Object... answers) {
        synchronized (script) {
            for (Object answer : answers) {
                script.add(answer);
            }
        }
        return this;
    }

    ScriptedServer fallback(Object answer) {
        this.fallback = answer;
        return this;
    }

    /**
     * Answers the consume calls (GET) with these message batches (JSON arrays), in order, then with an empty one;
     * every other call (ack, visibility) gets a plain 200.
     */
    ScriptedServer batches(String... batches) {
        Deque<String> queue = new ArrayDeque<>(List.of(batches));
        Function<Recorded, Reply> answer = request -> {
            if (!request.method.equals("GET")) {
                return reply(200, "{\"id\":\"x\",\"invisibleUntil\":\"x\"}");
            }
            synchronized (queue) {
                return reply(200, "{\"messages\":" + (queue.isEmpty() ? "[]" : queue.poll()) + "}");
            }
        };
        this.fallback = answer;
        return this;
    }

    @SuppressWarnings("unchecked")
    private void serve(HttpExchange exchange) throws IOException {
        byte[] body = exchange.getRequestBody().readAllBytes();
        String url = exchange.getRequestURI().getRawPath()
                + (exchange.getRequestURI().getRawQuery() == null ? "" : "?" + exchange.getRequestURI().getRawQuery());
        Recorded record = new Recorded(exchange.getRequestMethod(), url, exchange.getRequestHeaders(), body);
        Object answer;
        synchronized (script) {
            requests.add(record);
            answer = script.isEmpty() ? fallback : script.poll();
        }
        if (answer instanceof Function) {
            answer = ((Function<Recorded, Reply>) answer).apply(record);
        }
        Reply reply = (Reply) answer;
        if (reply.hang) {
            try {
                stop.await(30, TimeUnit.SECONDS);
            } catch (InterruptedException e) {
                Thread.currentThread().interrupt();
            }
            exchange.close();
            return;
        }
        byte[] data = reply.body.getBytes(StandardCharsets.UTF_8);
        exchange.getResponseHeaders().set("Content-Type", "application/json");
        for (Map.Entry<String, String> header : reply.headers.entrySet()) {
            exchange.getResponseHeaders().set(header.getKey(), header.getValue());
        }
        exchange.sendResponseHeaders(reply.status, data.length == 0 ? -1 : data.length);
        if (data.length > 0) {
            try (OutputStream out = exchange.getResponseBody()) {
                out.write(data);
            }
        }
        exchange.close();
    }

    @Override
    public void close() {
        stop.countDown();
        server.stop(0);
    }
}
