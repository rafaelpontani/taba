"""A local HTTP server that records every request and answers from a script (the `ScriptedHandler` of the C# connector)."""

import json
import socket
import threading
import time
import unittest
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from typing import Any, Callable, Dict, List, Optional
from urllib.parse import parse_qs, urlsplit

from taba import Client, TabaError


class Recorded:
    def __init__(self, method: str, url: str, headers: Dict[str, str], body: bytes) -> None:
        self.method = method
        self.url = url
        self.headers = headers
        self.body = body

    @property
    def text(self) -> str:
        return self.body.decode("utf-8")

    @property
    def query(self) -> Dict[str, str]:
        return {k: v[0] for k, v in parse_qs(urlsplit(self.url).query, keep_blank_values=True).items()}


def reply(status: int = 200, body: Any = "", headers: Optional[Dict[str, str]] = None) -> Dict[str, Any]:
    if not isinstance(body, (str, bytes)):
        body = json.dumps(body)
    return {"status": status, "body": body, "headers": headers or {}}


DESTROY = {"destroy": True}
HANG = {"hang": True}


class ScriptedServer:
    """Replies are dicts from `reply()`, functions of the request returning one, `DESTROY` or `HANG`."""

    def __init__(self, port: int = 0) -> None:
        self.requests: List[Recorded] = []
        self._script: List[Any] = []
        self._fallback: Any = reply(200, "{}")
        self._lock = threading.Lock()
        self._stop = threading.Event()
        owner = self

        class Handler(BaseHTTPRequestHandler):
            protocol_version = "HTTP/1.1"

            def log_message(self, *args: Any) -> None:
                pass

            def _serve(self) -> None:
                length = int(self.headers.get("Content-Length") or 0)
                body = self.rfile.read(length) if length else b""
                record = Recorded(self.command, self.path, {k.lower(): v for k, v in self.headers.items()}, body)
                with owner._lock:
                    owner.requests.append(record)
                    answer = owner._script.pop(0) if owner._script else owner._fallback
                if callable(answer):
                    answer = answer(record)
                if answer.get("destroy"):
                    self.close_connection = True
                    try:
                        self.connection.shutdown(socket.SHUT_RDWR)
                    except OSError:
                        pass
                    return
                if answer.get("hang"):
                    owner._stop.wait(30)
                    self.close_connection = True
                    return
                data = answer["body"]
                if isinstance(data, str):
                    data = data.encode("utf-8")
                self.send_response(answer["status"])
                headers = {"Content-Type": "application/json"}
                headers.update(answer["headers"])
                for key, value in headers.items():
                    self.send_header(key, value)
                self.send_header("Content-Length", str(len(data)))
                self.end_headers()
                self.wfile.write(data)

            do_GET = do_POST = do_PUT = do_DELETE = do_PATCH = _serve

        self._server = ThreadingHTTPServer(("127.0.0.1", port), Handler)
        self._server.daemon_threads = True
        self._thread = threading.Thread(target=self._server.serve_forever, daemon=True)
        self._thread.start()

    @property
    def port(self) -> int:
        return self._server.server_address[1]

    @property
    def url(self) -> str:
        return "http://127.0.0.1:%d" % self.port

    def reply(self, *answers: Any) -> "ScriptedServer":
        with self._lock:
            self._script.extend(answers)
        return self

    def fallback(self, answer: Any) -> "ScriptedServer":
        self._fallback = answer
        return self

    def batches(self, *batches: List[Dict[str, Any]]) -> "ScriptedServer":
        """Answers the consume calls (GET) with these message batches, in order, then with an empty one; the rest gets a plain 200."""
        queue = list(batches)

        def answer(request: Recorded) -> Dict[str, Any]:
            if request.method != "GET":
                return reply(200, {"id": "x", "invisibleUntil": "x"})
            with self._lock:
                batch = queue.pop(0) if queue else []
            return reply(200, {"messages": batch})

        self._fallback = answer
        return self

    def close(self) -> None:
        self._stop.set()
        self._server.shutdown()
        self._server.server_close()


def blocking_sleep(seconds: float, token: Any) -> None:
    """A `sleep` that never ends by itself: the renewal does not spin in tests that do not look at it."""
    token.wait(60)
    token.raise_if_cancelled()


class FakeSleep:
    """A `sleep` that does not wait and records what was asked."""

    def __init__(self) -> None:
        self.waits: List[float] = []

    def __call__(self, seconds: float, token: Any) -> None:
        self.waits.append(seconds)
        token.raise_if_cancelled()


class ClientTestCase(unittest.TestCase):
    """Starts a scripted server and a client with a `sleep` that does not wait."""

    config: Dict[str, Any] = {}

    def setUp(self) -> None:
        self.server = ScriptedServer()
        self.sleep = FakeSleep()
        self.client = Client(base_url=self.server.url, _sleep=self.sleep, **self.config)
        self.addCleanup(self.server.close)
        self.addCleanup(self.client.close)

    def assert_error(self, code: str, call: Callable[[], Any]) -> TabaError:
        with self.assertRaises(TabaError) as context:
            call()
        self.assertEqual(context.exception.code, code)
        return context.exception


def topic_json(name: str, **extra: Any) -> Dict[str, Any]:
    data = {"id": "t-" + name, "name": name, "type": "queue", "createdAt": "2026-10-27T10:00:00Z"}
    data.update(extra)
    return data


def message_json(message_id: str, body: Any, **extra: Any) -> Dict[str, Any]:
    data = {
        "id": message_id,
        "body": body,
        "contentType": "application/json",
        "receiptToken": "rt-" + message_id,
        "deliveryCount": 1,
        "publishedAt": "2026-10-27T10:00:00Z",
        "invisibleUntil": "2026-10-27T10:02:00Z",
    }
    data.update(extra)
    return data


def wait_for(condition: Callable[[], bool], timeout: float = 5.0) -> None:
    deadline = time.monotonic() + timeout
    while not condition():
        if time.monotonic() > deadline:
            raise AssertionError("condition not met in %s s" % timeout)
        time.sleep(0.005)
