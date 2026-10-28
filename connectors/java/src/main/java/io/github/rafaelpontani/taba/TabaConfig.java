package io.github.rafaelpontani.taba;

import java.time.Duration;
import java.util.concurrent.Executor;
import javax.net.ssl.SSLContext;

/**
 * The client configuration (doc 21). It does not read environment variables: {@code TABA_*} is the prefix of the
 * server configuration; whoever uses the connector reads their own and fills this object.
 *
 * <pre>{@code
 * TabaConfig config = TabaConfig.of("http://localhost:8080").token(System.getenv("TABA_TOKEN"));
 * }</pre>
 */
public final class TabaConfig {
    private String baseUrl;
    private String token;
    private Duration connectTimeout = Duration.ofSeconds(5);
    private Duration requestTimeout = Duration.ofSeconds(30);
    private int maxAttempts = 3;
    private String userAgent = "taba-java/" + TabaClient.VERSION;
    private TabaJsonSerializer jsonSerializer;
    private Executor executor;
    private SSLContext sslContext;

    public TabaConfig() {
    }

    public static TabaConfig of(String baseUrl) {
        return new TabaConfig().baseUrl(baseUrl);
    }

    /** Address of the client API load balancer ({@code http://host:port} or {@code https://...}). Required. */
    public TabaConfig baseUrl(String baseUrl) {
        this.baseUrl = baseUrl;
        return this;
    }

    /** Goes in {@code Authorization: Bearer}. Without it, the call goes without the header. */
    public TabaConfig token(String token) {
        this.token = token;
        return this;
    }

    /** Time to open the connection (5 s). */
    public TabaConfig connectTimeout(Duration connectTimeout) {
        this.connectTimeout = connectTimeout;
        return this;
    }

    /** Time to receive the response of an ordinary call (30 s). {@code consume} uses {@code waitTime + 10 s}. */
    public TabaConfig requestTimeout(Duration requestTimeout) {
        this.requestTimeout = requestTimeout;
        return this;
    }

    /** Attempts, counting the first (3). */
    public TabaConfig maxAttempts(int maxAttempts) {
        this.maxAttempts = maxAttempts;
        return this;
    }

    /** Goes in {@code User-Agent} ({@code taba-java/<version>}). Add the application name. */
    public TabaConfig userAgent(String userAgent) {
        this.userAgent = userAgent;
        return this;
    }

    /** Who serializes {@code publishJson} and deserializes the typed {@code consume}. Without it, those fail. */
    public TabaConfig jsonSerializer(TabaJsonSerializer jsonSerializer) {
        this.jsonSerializer = jsonSerializer;
        return this;
    }

    /** Where the {@code ...Async} calls and the renewals run. Default: daemon threads owned by the client. */
    public TabaConfig executor(Executor executor) {
        this.executor = executor;
        return this;
    }

    /** An own TLS context (a private CA, for example). Default: the JVM's. */
    public TabaConfig sslContext(SSLContext sslContext) {
        this.sslContext = sslContext;
        return this;
    }

    public String getBaseUrl() {
        return baseUrl;
    }

    public String getToken() {
        return token;
    }

    public Duration getConnectTimeout() {
        return connectTimeout;
    }

    public Duration getRequestTimeout() {
        return requestTimeout;
    }

    public int getMaxAttempts() {
        return maxAttempts;
    }

    public String getUserAgent() {
        return userAgent;
    }

    public TabaJsonSerializer getJsonSerializer() {
        return jsonSerializer;
    }

    public Executor getExecutor() {
        return executor;
    }

    public SSLContext getSslContext() {
        return sslContext;
    }

    TabaConfig copy() {
        TabaConfig c = new TabaConfig();
        c.baseUrl = baseUrl;
        c.token = token;
        c.connectTimeout = connectTimeout;
        c.requestTimeout = requestTimeout;
        c.maxAttempts = maxAttempts;
        c.userAgent = userAgent;
        c.jsonSerializer = jsonSerializer;
        c.executor = executor;
        c.sslContext = sslContext;
        return c;
    }
}
