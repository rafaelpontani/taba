"""
The client's HTTP transport: builds the request, sends it, reads the body, turns the API error into a
`TabaError` and does the call retries of the common contract (doc 21).
"""

import errno
import http.client
import socket
import ssl
import threading
import time
from dataclasses import dataclass, field
from typing import Callable, Dict, List, Optional, Tuple
from urllib.parse import quote, urlsplit

from ._json import JsonError, JsonFields
from .cancel import Cancelled, CancelToken
from .errors import TabaError
from .models import Config

#: The signature of the wait between attempts (tests replace it): `sleep(seconds, token)` raises `Cancelled`.
Sleep = Callable[[float, CancelToken], None]


def default_sleep(seconds: float, token: CancelToken) -> None:
    if token.wait(seconds):
        raise Cancelled()


def segment(value: str) -> str:
    return quote(str(value), safe="")


@dataclass
class ApiCall:
    """
    The description of a request to the API: method, path, query, body, headers and the rules of the call
    (timeout, retry of 502/504, responses that count as success after a 503).
    """

    method: str
    #: Path with the segments already encoded (`/topics/orders`).
    path: str
    query: List[Tuple[str, str]] = field(default_factory=list)
    body: Optional[bytes] = None
    content_type: Optional[str] = None
    request_id: Optional[str] = None
    traceparent: Optional[str] = None
    #: Overrides the total time of the call in seconds (`consume` uses `wait_time + 10`).
    timeout: Optional[float] = None
    #: Retries 502 and 504: only publishing to a FIFO queue, where the primary deduplicates.
    retry_gateway: bool = False
    #: After a 503, these responses count as success (the operation had already been applied).
    recover: List[Tuple[int, str]] = field(default_factory=list)

    def with_query(self, key: str, value: object) -> "ApiCall":
        if isinstance(value, bool):
            value = "true" if value else "false"
        self.query.append((key, str(value)))
        return self

    def with_json(self, text: str) -> "ApiCall":
        self.body = text.encode("utf-8")
        return self


@dataclass
class ApiReply:
    status: int
    body: bytes
    recovered: bool = False

    @property
    def text(self) -> str:
        return self.body.decode("utf-8", errors="replace")


class _Retry(Exception):
    """A failure before the body was sent (refused, DNS): the call can be repeated like a 503."""

    def __init__(self, error: TabaError) -> None:
        super().__init__(str(error))
        self.error = error


def _is_connect_failure(e: BaseException) -> bool:
    if isinstance(e, (ConnectionRefusedError, socket.gaierror)):
        return True
    return isinstance(e, OSError) and e.errno in (errno.ENETUNREACH, errno.EHOSTUNREACH)


def _describe(e: BaseException) -> str:
    return "%s: %s" % (type(e).__name__, e) if str(e) else type(e).__name__


def _error_code_of(body: bytes) -> Optional[str]:
    try:
        return JsonFields.parse(body.decode("utf-8", errors="replace")).string("code")
    except JsonError:
        return None


def _error_from(status: int, headers: Dict[str, str], body: bytes) -> TabaError:
    text = body.decode("utf-8", errors="replace")
    code = message = request_id = details = None
    try:
        fields = JsonFields.parse(text)
        code = fields.string("code")
        if code is not None:
            message = fields.string("message") or ""
            request_id = fields.string("requestId")
            details = fields.raw("details")
    except JsonError:
        pass  # Not an ErrorResponse: falls to the generic code below.
    if code is None:
        code = "http.%d" % status
        trimmed = text.strip()
        message = "response %d with no body" % status if not trimmed else trimmed[:300]
    retry_header = headers.get("retry-after")
    retry_after = None
    if retry_header is not None and retry_header.strip().isdigit():
        retry_after = int(retry_header.strip())
    return TabaError(
        status,
        code,
        message or "",
        request_id=request_id or headers.get("x-request-id"),
        retry_after=retry_after,
        details=details,
        body=body,
    )


class Transport:
    def __init__(self, config: Config, sleep: Optional[Sleep] = None) -> None:
        self._config = config
        self._scheme, self._host, self._port, self._prefix = self._validate(config.base_url)
        self._context = config.ssl_context
        if self._scheme == "https" and self._context is None:
            self._context = ssl.create_default_context()
            self._context.minimum_version = ssl.TLSVersion.TLSv1_2
        self._local = threading.local()
        self.sleep: Sleep = sleep or default_sleep

    @staticmethod
    def _validate(base_url: object) -> Tuple[str, str, int, str]:
        parts = None
        if isinstance(base_url, str) and base_url.strip():
            try:
                parts = urlsplit(base_url.strip())
                port = parts.port
            except ValueError:
                parts = None
        if parts is None or parts.scheme not in ("http", "https") or not parts.hostname:
            shown = base_url if isinstance(base_url, str) and base_url.strip() else "empty"
            raise TabaError.client(
                "client.invalid-config",
                "base_url must be an http:// or https:// address (received: %s)" % shown,
            )
        default_port = 443 if parts.scheme == "https" else 80
        return parts.scheme, parts.hostname, port or default_port, parts.path.rstrip("/")

    # -- connection (one per thread: `http.client` connections are not thread-safe) --------------------

    def _new_connection(self) -> http.client.HTTPConnection:
        timeout = self._config.connect_timeout
        if self._scheme == "https":
            return http.client.HTTPSConnection(self._host, self._port, timeout=timeout, context=self._context)
        return http.client.HTTPConnection(self._host, self._port, timeout=timeout)

    def _drop(self) -> None:
        conn = getattr(self._local, "conn", None)
        self._local.conn = None
        if conn is not None:
            try:
                conn.close()
            except OSError:
                pass

    def close(self) -> None:
        """Closes the calling thread's connection."""
        self._drop()

    def _url(self, call: ApiCall) -> str:
        url = self._prefix + call.path
        if call.query:
            url += "?" + "&".join("%s=%s" % (quote(k, safe=""), quote(v, safe="")) for k, v in call.query)
        return url

    def _headers(self, call: ApiCall) -> Dict[str, str]:
        headers = {"Accept": "application/json", "User-Agent": self._config.user_agent}
        if self._config.token:
            headers["Authorization"] = "Bearer " + self._config.token
        if call.request_id:
            headers["X-Request-Id"] = call.request_id
        if call.traceparent:
            headers["traceparent"] = call.traceparent
        if call.body is not None:
            headers["Content-Type"] = call.content_type or "application/json"
        return headers

    def _once(self, call: ApiCall, timeout: float, cancel: CancelToken) -> Tuple[int, Dict[str, str], bytes]:
        deadline = time.monotonic() + timeout
        for stale_retry in (True, False):
            cancel.raise_if_cancelled()
            conn = getattr(self._local, "conn", None)
            reused = conn is not None and conn.sock is not None
            if conn is None:
                conn = self._new_connection()
                self._local.conn = conn

            def abort(c: http.client.HTTPConnection = conn) -> None:
                sock = c.sock
                if sock is not None:
                    try:
                        sock.shutdown(socket.SHUT_RDWR)
                    except OSError:
                        pass
                    # `close()` only marks the socket closed while the response still holds a `makefile`
                    # reference, and on Windows `shutdown` does not wake a thread blocked in `recv`: closing
                    # the descriptor for real does.
                    getattr(socket.socket, "_real_close", socket.socket.close)(sock)

            cancel._attach(abort)
            sent = False
            try:
                if conn.sock is None:
                    conn.timeout = self._config.connect_timeout
                    conn.connect()
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    raise socket.timeout()
                conn.sock.settimeout(remaining)
                conn.request(call.method, self._url(call), body=call.body, headers=self._headers(call))
                sent = True
                response = conn.getresponse()
                chunks = []
                while True:
                    remaining = deadline - time.monotonic()
                    if remaining <= 0:
                        raise socket.timeout()
                    if conn.sock is not None:
                        conn.sock.settimeout(remaining)
                    chunk = response.read(65536)
                    if not chunk:
                        break
                    chunks.append(chunk)
                headers = {k.lower(): v for k, v in response.getheaders()}
                if response.will_close:
                    self._drop()
                return response.status, headers, b"".join(chunks)
            except Exception as e:  # noqa: BLE001 - classified below
                self._drop()
                if cancel.is_set():
                    raise Cancelled()
                if isinstance(e, (socket.timeout, TimeoutError)):
                    if conn.sock is None and not sent:
                        raise TabaError.client("client.network", "timed out connecting to %s" % self._host)
                    raise TabaError.client("client.timeout", "timed out waiting for the response")
                error = TabaError.client("client.network", _describe(e))
                if not sent and not reused and _is_connect_failure(e):
                    raise _Retry(error)
                # A pooled connection the server closed while idle fails before anything is read.
                stale = reused and stale_retry and isinstance(
                    e, (http.client.RemoteDisconnected, ConnectionResetError, BrokenPipeError)
                )
                if stale:
                    continue
                raise error
            finally:
                cancel._detach(abort)
        raise AssertionError("unreachable")  # pragma: no cover

    def send(self, call: ApiCall, cancel: Optional[CancelToken] = None) -> ApiReply:
        """
        Makes the call, retrying only what the contract says to retry: 429 and 503 (waiting for the
        `Retry-After`, or 1 s, 2 s, 4 s), 502 and 504 only with `retry_gateway`, and a failure to connect
        (refused, DNS). Never a 4xx (except 429), nor a failure after the body was sent, nor a timeout.
        """
        cancel = cancel or CancelToken()
        timeout = call.timeout if call.timeout is not None else self._config.request_timeout
        maximum = max(1, self._config.max_attempts)
        after_retryable = False
        attempt = 0
        while True:
            attempt += 1
            backoff = float(1 << min(attempt - 1, 6))
            try:
                status, headers, body = self._once(call, timeout, cancel)
                if status < 400:
                    return ApiReply(status, body)
                if after_retryable and any(
                    s == status and c == _error_code_of(body) for s, c in call.recover
                ):
                    return ApiReply(status, body, recovered=True)
                error = _error_from(status, headers, body)
                retry = status in (429, 503) or (status in (502, 504) and call.retry_gateway)
                if not retry or attempt >= maximum:
                    raise error
                after_retryable = after_retryable or status == 503
                wait = float(error.retry_after) if error.retry_after is not None else backoff
            except _Retry as r:
                if attempt >= maximum:
                    raise r.error
                after_retryable = True
                wait = backoff
            self.sleep(wait, cancel)
