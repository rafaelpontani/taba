package io.github.rafaelpontani.taba;

import java.net.URLEncoder;
import java.nio.charset.StandardCharsets;
import java.time.Duration;
import java.util.ArrayList;
import java.util.List;

/**
 * The description of a request to the API: method, path, query, body, headers and the rules of the call
 * (timeout, retry of 502/504, responses that count as success after a 503). The transport builds a new
 * {@code HttpRequest} from it on every attempt.
 */
final class ApiCall {
    static final class Recover {
        final int status;
        final String code;

        Recover(int status, String code) {
            this.status = status;
            this.code = code;
        }
    }

    final String method;
    /** Path with the segments already encoded ({@code /topics/orders}). */
    final String path;
    final List<String[]> query = new ArrayList<>();
    byte[] body;
    String contentType;
    String requestId;
    String traceparent;
    /** Overrides the total time of the call ({@code consume} uses {@code waitTime + 10 s}). */
    Duration timeout;
    /** Retries 502 and 504: only publishing to a FIFO queue, where the primary deduplicates. */
    boolean retryGateway;
    /** After a 503, these responses count as success (the operation had already been applied). */
    final List<Recover> recover = new ArrayList<>();

    ApiCall(String method, String path) {
        this.method = method;
        this.path = path;
    }

    ApiCall with(String key, Object value) {
        query.add(new String[] {key, String.valueOf(value)});
        return this;
    }

    ApiCall json(String json) {
        body = json.getBytes(StandardCharsets.UTF_8);
        return this;
    }

    static String encode(String value) {
        return URLEncoder.encode(value, StandardCharsets.UTF_8).replace("+", "%20");
    }
}
