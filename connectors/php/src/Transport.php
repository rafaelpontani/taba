<?php
/**
 * The client's HTTP transport: builds the request, sends it with `curl`, reads the body, turns the API error into a
 * `TabaException` and does the call retries of the common contract (doc 21).
 */

namespace Taba;

/**
 * The description of a request to the API: method, path, query, body, headers and the rules of the call
 * (timeout, retry of 502/504, responses that count as success after a 503).
 */
class ApiCall
{
    public $method;
    /** Path with the segments already encoded (`/topics/orders`). */
    public $path;
    /** @var array of array(key, value) */
    public $query = array();
    public $body = null;
    public $contentType = null;
    public $requestId = null;
    public $traceparent = null;
    /** Overrides the total time of the call in seconds (`consume` uses `waitTime + 10`). */
    public $timeout = null;
    /** Retries 502 and 504: only publishing to a FIFO queue, where the primary deduplicates. */
    public $retryGateway = false;
    /** After a 503, these array(status, code) count as success (the operation had already been applied). */
    public $recover = array();

    public function __construct($method, $path)
    {
        $this->method = $method;
        $this->path = $path;
    }

    public function withQuery($key, $value)
    {
        if (is_bool($value)) {
            $value = $value ? 'true' : 'false';
        }
        $this->query[] = array($key, (string) $value);
        return $this;
    }

    public function withJson($text)
    {
        $this->body = $text;
        return $this;
    }
}

class ApiReply
{
    public $status;
    public $body;
    public $recovered;

    public function __construct($status, $body, $recovered = false)
    {
        $this->status = $status;
        $this->body = $body;
        $this->recovered = $recovered;
    }
}

/**
 * A failure of the network. `$beforeSending` is true when it happened before any byte of the request went out
 * (refused, DNS): the call can be repeated like a 503.
 */
class NetworkFailure extends \Exception
{
    public $beforeSending;
    public $timedOut;
    public $connecting;

    public function __construct($message, $beforeSending = false, $timedOut = false, $connecting = false)
    {
        parent::__construct($message);
        $this->beforeSending = $beforeSending;
        $this->timedOut = $timedOut;
        $this->connecting = $connecting;
    }
}

/** Sends a request with `curl` and reuses the connection. */
class CurlExecutor
{
    private $handle;

    public function __construct()
    {
        if (!function_exists('curl_init')) {
            throw TabaException::client('client.invalid-config', 'the curl extension of PHP is required');
        }
    }

    public function __destruct()
    {
        $this->close();
    }

    public function close()
    {
        if ($this->handle !== null) {
            curl_close($this->handle);
            $this->handle = null;
        }
    }

    /**
     * @return array array(status, headers (lower-case names), body)
     * @throws NetworkFailure
     * @throws Cancelled
     */
    public function __invoke($url, $method, array $headerLines, $body, $timeout, Config $config, CancelToken $cancel)
    {
        if ($this->handle === null) {
            $this->handle = curl_init();
        } else {
            curl_reset($this->handle);
        }
        $headers = array();
        $options = array(
            CURLOPT_URL => $url,
            CURLOPT_CUSTOMREQUEST => $method,
            CURLOPT_RETURNTRANSFER => true,
            CURLOPT_HTTPHEADER => $headerLines,
            CURLOPT_CONNECTTIMEOUT_MS => (int) ceil($config->connectTimeout * 1000),
            CURLOPT_TIMEOUT_MS => (int) ceil($timeout * 1000),
            CURLOPT_FOLLOWLOCATION => false,
            CURLOPT_HTTP_VERSION => CURL_HTTP_VERSION_1_1,
            CURLOPT_SSL_VERIFYPEER => true,
            CURLOPT_SSL_VERIFYHOST => 2,
            CURLOPT_NOPROGRESS => false,
            CURLOPT_PROGRESSFUNCTION => function () use ($cancel) {
                return $cancel->isCancelled() ? 1 : 0;
            },
            CURLOPT_HEADERFUNCTION => function ($ch, $line) use (&$headers) {
                $parts = explode(':', $line, 2);
                if (count($parts) === 2) {
                    $headers[strtolower(trim($parts[0]))] = trim($parts[1]);
                }
                return strlen($line);
            },
        );
        if (defined('CURL_SSLVERSION_TLSv1_2')) {
            $options[CURLOPT_SSLVERSION] = CURL_SSLVERSION_TLSv1_2;
        }
        if ($config->caFile !== null) {
            $options[CURLOPT_CAINFO] = $config->caFile;
        }
        if ($config->caPath !== null) {
            $options[CURLOPT_CAPATH] = $config->caPath;
        }
        if ($body !== null) {
            $options[CURLOPT_POSTFIELDS] = $body;
        }
        curl_setopt_array($this->handle, $options);

        $text = curl_exec($this->handle);
        if ($text === false) {
            $errno = curl_errno($this->handle);
            $message = curl_error($this->handle);
            if ($errno === 42 || $cancel->isCancelled()) { // CURLE_ABORTED_BY_CALLBACK
                $this->close();
                throw new Cancelled();
            }
            // 5 and 6: could not resolve the proxy or the host; 7: could not connect.
            if ($errno === 5 || $errno === 6 || $errno === 7) {
                throw new NetworkFailure($message, true);
            }
            if ($errno === 28) { // CURLE_OPERATION_TIMEDOUT
                $connected = curl_getinfo($this->handle, CURLINFO_CONNECT_TIME) > 0;
                throw new NetworkFailure($message, false, true, !$connected);
            }
            $this->close();
            throw new NetworkFailure($message);
        }
        $status = (int) curl_getinfo($this->handle, CURLINFO_HTTP_CODE);
        return array($status, $headers, $text);
    }
}

final class Transport
{
    private $config;
    private $base;
    private $prefix;
    private $executor;
    private $sleep;

    /**
     * @param callable|null $sleep    `function ($seconds, CancelToken $cancel)`; tests replace it.
     * @param callable|null $executor same signature as `CurlExecutor::__invoke`; tests replace it.
     */
    public function __construct(Config $config, $sleep = null, $executor = null)
    {
        $this->config = $config;
        list($this->base, $this->prefix) = self::validate($config->baseUrl);
        $this->executor = $executor !== null ? $executor : new CurlExecutor();
        $this->sleep = $sleep !== null ? $sleep : array($this, 'defaultSleep');
    }

    public function config()
    {
        return $this->config;
    }

    public function close()
    {
        if ($this->executor instanceof CurlExecutor) {
            $this->executor->close();
        }
    }

    public function sleep($seconds, CancelToken $cancel)
    {
        call_user_func($this->sleep, $seconds, $cancel);
    }

    /** Waits in slices so a cancellation is noticed. */
    public function defaultSleep($seconds, CancelToken $cancel)
    {
        $end = microtime(true) + $seconds;
        while (($left = $end - microtime(true)) > 0) {
            $cancel->throwIfCancelled();
            usleep((int) (min($left, 0.05) * 1000000));
        }
        $cancel->throwIfCancelled();
    }

    private static function validate($baseUrl)
    {
        $parts = is_string($baseUrl) && trim($baseUrl) !== '' ? @parse_url(trim($baseUrl)) : false;
        if (!is_array($parts) || !isset($parts['scheme'], $parts['host'])
            || !in_array(strtolower($parts['scheme']), array('http', 'https'), true)) {
            $shown = is_string($baseUrl) && trim($baseUrl) !== '' ? $baseUrl : 'empty';
            throw TabaException::client(
                'client.invalid-config',
                'baseUrl must be an http:// or https:// address (received: ' . $shown . ')'
            );
        }
        $host = $parts['host'];
        if (strpos($host, ':') !== false) {
            $host = '[' . $host . ']';
        }
        $base = strtolower($parts['scheme']) . '://' . $host . (isset($parts['port']) ? ':' . $parts['port'] : '');
        $prefix = isset($parts['path']) ? rtrim($parts['path'], '/') : '';
        return array($base, $prefix);
    }

    public static function segment($value)
    {
        return rawurlencode((string) $value);
    }

    private function url(ApiCall $call)
    {
        $url = $this->base . $this->prefix . $call->path;
        if ($call->query) {
            $pairs = array();
            foreach ($call->query as $kv) {
                $pairs[] = rawurlencode($kv[0]) . '=' . rawurlencode($kv[1]);
            }
            $url .= '?' . implode('&', $pairs);
        }
        return $url;
    }

    private function headerLines(ApiCall $call)
    {
        $lines = array('Accept: application/json', 'User-Agent: ' . $this->config->userAgent, 'Expect:');
        if ($this->config->token !== null && $this->config->token !== '') {
            $lines[] = 'Authorization: Bearer ' . $this->config->token;
        }
        if ($call->requestId) {
            $lines[] = 'X-Request-Id: ' . $call->requestId;
        }
        if ($call->traceparent) {
            $lines[] = 'traceparent: ' . $call->traceparent;
        }
        if ($call->body !== null) {
            $lines[] = 'Content-Type: ' . ($call->contentType ? $call->contentType : 'application/json');
        }
        return $lines;
    }

    private function errorCodeOf($body)
    {
        try {
            return JsonFields::parse($body)->string('code');
        } catch (JsonError $e) {
            return null;
        }
    }

    private function errorFrom($status, array $headers, $body)
    {
        $code = $message = $requestId = $details = null;
        try {
            $fields = JsonFields::parse($body);
            $code = $fields->string('code');
            if ($code !== null) {
                $message = $fields->string('message');
                $requestId = $fields->string('requestId');
                $details = $fields->raw('details');
            }
        } catch (JsonError $e) {
            // Not an ErrorResponse: falls to the generic code below.
        }
        if ($code === null) {
            $code = 'http.' . $status;
            $trimmed = trim($body);
            $message = $trimmed === '' ? 'response ' . $status . ' with no body' : substr($trimmed, 0, 300);
        }
        $retryAfter = null;
        if (isset($headers['retry-after']) && ctype_digit(trim($headers['retry-after']))) {
            $retryAfter = (int) trim($headers['retry-after']);
        }
        if ($requestId === null && isset($headers['x-request-id'])) {
            $requestId = $headers['x-request-id'];
        }
        return new TabaException($status, $code, $message === null ? '' : $message, $requestId, $retryAfter, $details, $body);
    }

    /**
     * Makes the call, retrying only what the contract says to retry: 429 and 503 (waiting for the `Retry-After`,
     * or 1 s, 2 s, 4 s), 502 and 504 only with `retryGateway`, and a failure to connect (refused, DNS). Never a
     * 4xx (except 429), nor a failure after the body was sent, nor a timeout.
     */
    public function send(ApiCall $call, $cancel = null)
    {
        $cancel = $cancel !== null ? $cancel : new CancelToken();
        $timeout = $call->timeout !== null ? $call->timeout : $this->config->requestTimeout;
        $maximum = max(1, (int) $this->config->maxAttempts);
        $afterRetryable = false;
        $attempt = 0;
        while (true) {
            $attempt++;
            $backoff = (float) (1 << min($attempt - 1, 6));
            $cancel->throwIfCancelled();
            try {
                $executor = $this->executor;
                $reply = $executor(
                    $this->url($call),
                    $call->method,
                    $this->headerLines($call),
                    $call->body,
                    $timeout,
                    $this->config,
                    $cancel
                );
            } catch (NetworkFailure $e) {
                if ($e->timedOut && !$e->connecting) {
                    throw TabaException::client('client.timeout', 'timed out waiting for the response');
                }
                if ($e->timedOut) {
                    throw TabaException::client('client.network', 'timed out connecting to ' . $this->base);
                }
                $error = TabaException::client('client.network', $e->getMessage());
                if (!$e->beforeSending) {
                    throw $error;
                }
                if ($attempt >= $maximum) {
                    throw $error;
                }
                $afterRetryable = true;
                $this->sleep($backoff, $cancel);
                continue;
            }
            list($status, $headers, $body) = $reply;
            if ($status < 400) {
                return new ApiReply($status, $body);
            }
            if ($afterRetryable) {
                $code = $this->errorCodeOf($body);
                foreach ($call->recover as $rule) {
                    if ($rule[0] === $status && $rule[1] === $code) {
                        return new ApiReply($status, $body, true);
                    }
                }
            }
            $error = $this->errorFrom($status, $headers, $body);
            $retry = $status === 429 || $status === 503 || (($status === 502 || $status === 504) && $call->retryGateway);
            if (!$retry || $attempt >= $maximum) {
                throw $error;
            }
            $afterRetryable = $afterRetryable || $status === 503;
            $wait = $error->retryAfter !== null ? (float) $error->retryAfter : $backoff;
            $this->sleep($wait, $cancel);
        }
    }
}
