package io.github.rafaelpontani.taba;

import java.io.IOException;
import java.net.ConnectException;
import java.net.NoRouteToHostException;
import java.net.URI;
import java.net.UnknownHostException;
import java.net.http.HttpClient;
import java.net.http.HttpConnectTimeoutException;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.net.http.HttpTimeoutException;
import java.nio.channels.UnresolvedAddressException;
import java.nio.charset.StandardCharsets;
import java.time.Duration;
import java.util.concurrent.CancellationException;

/**
 * The client's HTTP transport: builds the request, sends it, reads the body, turns the API error into a
 * {@link TabaException} and does the call retries of the common contract (doc 21).
 */
final class HttpTransport implements AutoCloseable {
    /** The wait between attempts and between renewals (tests replace it). */
    interface Sleeper {
        void sleep(Duration duration) throws InterruptedException;
    }

    static final class Reply {
        final int status;
        final byte[] body;
        final boolean recovered;

        Reply(int status, byte[] body, boolean recovered) {
            this.status = status;
            this.body = body;
            this.recovered = recovered;
        }

        String text() {
            return new String(body, StandardCharsets.UTF_8);
        }
    }

    private final TabaConfig config;
    private final String baseUrl;
    private final HttpClient http;
    private final Sleeper sleeper;

    HttpTransport(TabaConfig config, Sleeper sleeper) {
        this.config = config;
        this.baseUrl = validateBaseUrl(config.getBaseUrl());
        HttpClient.Builder builder = HttpClient.newBuilder()
                .version(HttpClient.Version.HTTP_1_1)
                .followRedirects(HttpClient.Redirect.NEVER)
                .connectTimeout(config.getConnectTimeout());
        if (config.getSslContext() != null) {
            builder.sslContext(config.getSslContext());
        }
        if (config.getExecutor() != null) {
            builder.executor(config.getExecutor());
        }
        this.http = builder.build();
        this.sleeper = sleeper != null ? sleeper : duration -> Thread.sleep(duration.toMillis());
    }

    void sleep(Duration duration) throws InterruptedException {
        sleeper.sleep(duration);
    }

    private static String validateBaseUrl(String baseUrl) {
        URI uri = null;
        if (baseUrl != null && !baseUrl.isBlank()) {
            try {
                uri = new URI(baseUrl.trim());
            } catch (java.net.URISyntaxException e) {
                uri = null;
            }
        }
        if (uri == null || uri.getHost() == null
                || (!"http".equalsIgnoreCase(uri.getScheme()) && !"https".equalsIgnoreCase(uri.getScheme()))) {
            throw TabaException.client(
                    "client.invalid-config",
                    "baseUrl must be an http:// or https:// address (received: "
                            + (baseUrl == null || baseUrl.isBlank() ? "empty" : baseUrl) + ")");
        }
        String trimmed = baseUrl.trim();
        while (trimmed.endsWith("/")) {
            trimmed = trimmed.substring(0, trimmed.length() - 1);
        }
        return trimmed;
    }

    private String url(ApiCall call) {
        StringBuilder url = new StringBuilder(baseUrl).append(call.path);
        char separator = '?';
        for (String[] pair : call.query) {
            url.append(separator).append(ApiCall.encode(pair[0])).append('=').append(ApiCall.encode(pair[1]));
            separator = '&';
        }
        return url.toString();
    }

    private HttpRequest wire(ApiCall call, Duration timeout) {
        HttpRequest.Builder request = HttpRequest.newBuilder(URI.create(url(call))).timeout(timeout);
        if (config.getToken() != null && !config.getToken().isEmpty()) {
            request.header("Authorization", "Bearer " + config.getToken());
        }
        request.header("Accept", "application/json");
        request.header("User-Agent", config.getUserAgent());
        if (call.requestId != null) {
            request.header("X-Request-Id", call.requestId);
        }
        if (call.traceparent != null) {
            request.header("traceparent", call.traceparent);
        }
        if (call.body != null) {
            request.header("Content-Type", call.contentType != null ? call.contentType : "application/json");
            request.method(call.method, HttpRequest.BodyPublishers.ofByteArray(call.body));
        } else {
            request.method(call.method, HttpRequest.BodyPublishers.noBody());
        }
        return request.build();
    }

    private static boolean isConnectFailure(Throwable e) {
        for (Throwable t = e; t != null; t = t.getCause()) {
            if (t instanceof HttpTimeoutException) {
                return false;
            }
            if (t instanceof ConnectException
                    || t instanceof UnknownHostException
                    || t instanceof NoRouteToHostException
                    || t instanceof UnresolvedAddressException) {
                return true;
            }
        }
        return false;
    }

    private static String describe(Throwable e) {
        String text = e.getClass().getSimpleName() + (e.getMessage() == null ? "" : ": " + e.getMessage());
        Throwable cause = e.getCause();
        if (cause != null && cause != e) {
            text += " (" + cause.getClass().getSimpleName() + (cause.getMessage() == null ? "" : ": " + cause.getMessage()) + ")";
        }
        return text;
    }

    private static String errorCodeOf(byte[] body) {
        try {
            return Json.Fields.parse(new String(body, StandardCharsets.UTF_8)).string("code");
        } catch (Json.JsonException e) {
            return null;
        }
    }

    /** The response's {@code ErrorResponse}; if the body is not one, the error carries the code {@code http.<status>}. */
    private static TabaException errorFrom(HttpResponse<byte[]> response, byte[] body) {
        String text = new String(body, StandardCharsets.UTF_8);
        int status = response.statusCode();
        String code = null;
        String message = null;
        String requestId = null;
        String details = null;
        try {
            Json.Fields fields = Json.Fields.parse(text);
            code = fields.string("code");
            if (code != null) {
                message = fields.string("message");
                requestId = fields.string("requestId");
                details = fields.raw("details");
            }
        } catch (Json.JsonException e) {
            code = null;
        }
        if (code == null) {
            code = "http." + status;
            String trimmed = text.trim();
            message = trimmed.isEmpty() ? "response " + status + " with no body" : trimmed.substring(0, Math.min(300, trimmed.length()));
        }
        Integer retryAfter = null;
        String retryHeader = response.headers().firstValue("Retry-After").orElse(null);
        if (retryHeader != null && retryHeader.trim().matches("[0-9]{1,9}")) {
            retryAfter = Integer.valueOf(retryHeader.trim());
        }
        String headerId = response.headers().firstValue("X-Request-Id").orElse(null);
        return new TabaException(status, code, message == null ? "" : message)
                .withResponse(requestId != null ? requestId : headerId, retryAfter, details, body);
    }

    private static boolean recovers(ApiCall call, int status, byte[] body) {
        for (ApiCall.Recover accepted : call.recover) {
            if (accepted.status == status && accepted.code.equals(errorCodeOf(body))) {
                return true;
            }
        }
        return false;
    }

    /**
     * Makes the call, retrying only what the contract says to retry: 429 and 503 (waiting for the
     * {@code Retry-After}, or 1 s, 2 s, 4 s), 502 and 504 only with {@code retryGateway}, and a failure to connect
     * (refused, DNS). Never a 4xx (except 429), nor a failure after the body was sent, nor a timeout.
     * An interruption cancels the call: the interrupted flag stays set and a {@link CancellationException} is thrown.
     */
    Reply send(ApiCall call) {
        Duration timeout = call.timeout != null ? call.timeout : config.getRequestTimeout();
        int max = Math.max(1, config.getMaxAttempts());
        boolean afterRetryable = false;
        for (int attempt = 1; ; attempt++) {
            Duration backoff = Duration.ofSeconds(1L << Math.min(attempt - 1, 6));
            Duration wait;
            try {
                HttpResponse<byte[]> response = http.send(wire(call, timeout), HttpResponse.BodyHandlers.ofByteArray());
                byte[] body = response.body();
                int status = response.statusCode();
                if (status < 400) {
                    return new Reply(status, body, false);
                }
                if (afterRetryable && recovers(call, status, body)) {
                    return new Reply(status, body, true);
                }
                TabaException error = errorFrom(response, body);
                boolean retry = status == 429 || status == 503 || ((status == 502 || status == 504) && call.retryGateway);
                if (!retry || attempt >= max) {
                    throw error;
                }
                afterRetryable |= status == 503;
                wait = error.getRetryAfter() != null ? Duration.ofSeconds(error.getRetryAfter()) : backoff;
            } catch (HttpConnectTimeoutException e) {
                throw TabaException.client("client.network", "timed out connecting: " + describe(e), e);
            } catch (HttpTimeoutException e) {
                // The time ran out. After the body is sent there is no knowing whether it was applied.
                throw TabaException.client("client.timeout", "timed out waiting for the response", e);
            } catch (IOException e) {
                if (!isConnectFailure(e) || attempt >= max) {
                    throw TabaException.client("client.network", describe(e), e);
                }
                afterRetryable = true;
                wait = backoff;
            } catch (InterruptedException e) {
                Thread.currentThread().interrupt();
                throw new CancellationException("the call was cancelled");
            }
            try {
                sleeper.sleep(wait);
            } catch (InterruptedException e) {
                Thread.currentThread().interrupt();
                throw new CancellationException("the call was cancelled");
            }
        }
    }

    @Override
    public void close() {
        // java.net.http.HttpClient has no close() before Java 21: its connections are released when idle.
    }
}
