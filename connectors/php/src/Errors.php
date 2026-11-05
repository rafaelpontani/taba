<?php
/** The only kind of error of the connector (doc 21, "The common contract"), and the cancellation. */

namespace Taba;

/**
 * The API `ErrorResponse`, or a failure of the client itself (`status` 0 and `errorCode` `client.network`,
 * `client.timeout`, `client.invalid-config`, `client.invalid-response` or `client.invalid-request`).
 * Test the `errorCode`, never the message. (`getCode()` of PHP's `Exception` is an integer, so the `group.reason`
 * code has its own property.)
 */
class TabaException extends \RuntimeException
{
    /** The HTTP status, or zero on a failure of the client itself. */
    public $status;
    /** The `group.reason` code of the error. */
    public $errorCode;
    /** The `requestId` of the `ErrorResponse`, or the `X-Request-Id` header. */
    public $requestId;
    /** The seconds of the `Retry-After`, if the response carried it (null otherwise). */
    public $retryAfter;
    /** The `details` of the `ErrorResponse`, as JSON text. */
    public $details;
    /** The response body, as it came (empty on a client failure). */
    public $body;

    public function __construct($status, $errorCode, $message, $requestId = null, $retryAfter = null, $details = null, $body = '')
    {
        parent::__construct($message);
        $this->status = $status;
        $this->errorCode = $errorCode === null ? '' : $errorCode;
        $this->requestId = $requestId;
        $this->retryAfter = $retryAfter;
        $this->details = $details;
        $this->body = $body;
    }

    public static function client($code, $message)
    {
        return new TabaException(0, $code, $message);
    }

    /** A failure of the client itself, not a server response. */
    public function isClientSide()
    {
        return $this->status === 0;
    }

    public function __toString()
    {
        $suffix = $this->requestId === null ? '' : ' (requestId ' . $this->requestId . ')';
        return $this->errorCode . ': ' . $this->getMessage() . $suffix;
    }
}

/** The call was cancelled through a `CancelToken`. It is not a `TabaException`: it is not a failure of the call. */
class Cancelled extends \RuntimeException
{
    public function __construct()
    {
        parent::__construct('the call was cancelled');
    }
}

/**
 * Cancels a call or a `subscribe` loop. PHP runs one thread, so nothing can call `cancel()` while a request is in
 * flight: the token is checked by the HTTP transfer (about once a second) and between messages. Cancel it from
 * the handler, or give it a deadline with `cancelAfter()`.
 *
 *     $token = new CancelToken();
 *     $token->cancelAfter(60);
 *     $client->subscribe('orders', $handler, null, $token);   // returns when the 60 s have passed
 */
final class CancelToken
{
    private $cancelled = false;
    private $deadline = null;

    public function cancel()
    {
        $this->cancelled = true;
    }

    /** Cancels by itself `$seconds` (a float) from now on. */
    public function cancelAfter($seconds)
    {
        $this->deadline = microtime(true) + $seconds;
        return $this;
    }

    public function isCancelled()
    {
        if (!$this->cancelled && $this->deadline !== null && microtime(true) >= $this->deadline) {
            $this->cancelled = true;
        }
        return $this->cancelled;
    }

    public function throwIfCancelled()
    {
        if ($this->isCancelled()) {
            throw new Cancelled();
        }
    }
}
