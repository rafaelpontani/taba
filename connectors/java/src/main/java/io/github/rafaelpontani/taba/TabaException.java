package io.github.rafaelpontani.taba;

/**
 * The only kind of error of the connector (doc 21, "The common contract"): the API {@code ErrorResponse}, or a
 * failure of the client itself, with {@link #getStatus() status} zero and {@link #getCode() code}
 * {@code client.network}, {@code client.timeout}, {@code client.invalid-config},
 * {@code client.invalid-response} or {@code client.invalid-request}. Test the {@code code}, never the message.
 */
public final class TabaException extends RuntimeException {
    private static final long serialVersionUID = 1L;

    private final int status;
    private final String code;
    private String requestId;
    private Integer retryAfter;
    private String details;
    private byte[] body = new byte[0];

    public TabaException(int status, String code, String message) {
        this(status, code, message, null);
    }

    public TabaException(int status, String code, String message, Throwable cause) {
        super(message, cause);
        this.status = status;
        this.code = code == null ? "" : code;
    }

    /** The HTTP status, or zero on a failure of the client itself. */
    public int getStatus() {
        return status;
    }

    /** The error's {@code group.reason} code. */
    public String getCode() {
        return code;
    }

    /** The {@code requestId} of the {@code ErrorResponse}, or the {@code X-Request-Id} header. May be null. */
    public String getRequestId() {
        return requestId;
    }

    /** The seconds of the {@code Retry-After}, if the response carried it. May be null. */
    public Integer getRetryAfter() {
        return retryAfter;
    }

    /** The {@code details} of the {@code ErrorResponse}, as JSON text. May be null. */
    public String getDetails() {
        return details;
    }

    /** The response body, as it came (empty on a client failure). */
    public byte[] getBody() {
        return body.clone();
    }

    /** A failure of the client itself, not a server response. */
    public boolean isClientSide() {
        return status == 0;
    }

    @Override
    public String toString() {
        return code + ": " + getMessage() + (requestId == null ? "" : " (requestId " + requestId + ")");
    }

    TabaException withResponse(String requestId, Integer retryAfter, String details, byte[] body) {
        this.requestId = requestId;
        this.retryAfter = retryAfter;
        this.details = details;
        this.body = body == null ? new byte[0] : body;
        return this;
    }

    static TabaException client(String code, String message) {
        return new TabaException(0, code, message);
    }

    static TabaException client(String code, String message, Throwable cause) {
        return new TabaException(0, code, message, cause);
    }
}
