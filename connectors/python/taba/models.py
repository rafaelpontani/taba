"""The models of the client API and the reading of its JSON responses."""

import base64
import binascii
import json
import ssl
from dataclasses import dataclass, field
from typing import Any, Dict, List, Optional, Union

from ._json import JsonError, JsonFields, array_items, decode_string
from ._version import VERSION
from .errors import TabaError


@dataclass
class Config:
    """
    The client configuration (doc 21). It does not read environment variables: `TABA_*` is the prefix of the
    server configuration; whoever uses the connector reads their own and fills this object.
    """

    #: Address of the client API load balancer (`http://host:port` or `https://...`). Required.
    base_url: str = ""
    #: Goes in `Authorization: Bearer`. Without it, the call goes without the header.
    token: Optional[str] = None
    #: Seconds to open the connection.
    connect_timeout: float = 5.0
    #: Seconds of an ordinary call. `consume` uses `wait_time + 10`.
    request_timeout: float = 30.0
    #: Attempts, counting the first.
    max_attempts: int = 3
    #: Goes in `User-Agent`. Add the application name.
    user_agent: str = "taba-python/" + VERSION
    #: Who serializes `publish_json` and parses `consume_json`: an object with `dumps` and `loads` (default: `json`).
    serializer: Any = None
    #: An own `ssl.SSLContext` (a private CA, for example). Default: the system's, TLS 1.2 or later.
    ssl_context: Optional[ssl.SSLContext] = None


@dataclass(frozen=True)
class Topic:
    id: str
    name: str
    #: `queue` or `distributor`.
    type: str
    #: Creation date, as RFC 3339 text.
    created_at: str
    fifo: Optional[bool] = None
    volatile: Optional[bool] = None
    #: The topic JSON, as the server returned it.
    raw: str = ""


@dataclass(frozen=True)
class Page:
    items: List[Topic]
    #: The cursor of the next page, or `None` on the last one.
    next_cursor: Optional[str]
    raw: str = ""


@dataclass
class TopicRequest:
    """`POST /topics`. Only what was filled in is sent."""

    name: str
    #: `queue` or `distributor`.
    type: str
    tags: Optional[Dict[str, str]] = None
    #: Distributor only: the name prefixes of the queues each publication is copied to.
    prefixes: Optional[List[str]] = None
    volatile: Optional[bool] = None
    idle_timeout: Optional[int] = None
    fifo: Optional[bool] = None
    max_message_bytes: Optional[int] = None
    ttl: Optional[int] = None
    max_deliveries: Optional[int] = None


class _Unset:
    def __repr__(self) -> str:
        return "UNSET"

    def __bool__(self) -> bool:
        return False


#: A `TopicUpdate` field left out of the request. `None` sends `null`: goes back to the server default.
UNSET: Any = _Unset()


@dataclass
class TopicUpdate:
    """`PATCH /topics/{topic}`: only what was filled in is sent."""

    #: Replaces the whole map (an empty map clears the tags).
    tags: Optional[Dict[str, str]] = None
    prefixes: Optional[List[str]] = None
    idle_timeout: Optional[int] = None
    max_message_bytes: Any = UNSET
    ttl: Any = UNSET
    max_deliveries: Any = UNSET


@dataclass
class ConsumeOptions:
    """The `consume` options. The connector sends only what was set: whatever is missing takes the server default."""

    max_messages: Optional[int] = None
    #: Seconds of invisibility for the delivered messages.
    visibility_timeout: Optional[int] = None
    #: Seconds of long polling.
    wait_time: Optional[int] = None
    auto_create: bool = False
    #: The next four only apply with `auto_create`.
    volatile: Optional[bool] = None
    idle_timeout: Optional[int] = None
    fifo: Optional[bool] = None
    tags: Optional[Dict[str, str]] = None
    request_id: Optional[str] = None
    traceparent: Optional[str] = None


@dataclass(frozen=True)
class Message:
    """A delivered message. The `body` always comes out as bytes (doc 21, "The delivered message")."""

    id: str
    #: JSON: the JSON text as it came. `text/*`: the UTF-8 text. base64: the bytes already decoded.
    body: bytes
    content_type: str
    receipt_token: str
    delivery_count: int = 0
    #: As RFC 3339 text.
    published_at: str = ""
    invisible_until: str = ""
    group_id: Optional[str] = None
    deduplication_id: Optional[str] = None
    dead_lettered_at: Optional[str] = None
    dead_letter_reason: Optional[str] = None
    #: The message JSON, as the server returned it.
    raw: str = ""

    @property
    def body_text(self) -> str:
        """The `body` as UTF-8 text."""
        return self.body.decode("utf-8")

    def json(self) -> Any:
        """The `body` read as JSON."""
        return json.loads(self.body_text)


@dataclass(frozen=True)
class DistributorDelivery:
    """The copy of a publication made by a distributor to a queue."""

    topic: str
    id: Optional[str] = None
    error_code: Optional[str] = None
    error_message: Optional[str] = None

    @property
    def succeeded(self) -> bool:
        return self.error_code is None


@dataclass(frozen=True)
class PublishResponse:
    #: The message `id` (queue). `None` in a distributor's response.
    id: Optional[str] = None
    #: FIFO queue only: `True` when it repeated a publication within the deduplication window.
    duplicate: Optional[bool] = None
    #: Distributor only: how many queues matched.
    matched: Optional[int] = None
    deliveries: List[DistributorDelivery] = field(default_factory=list)
    raw: str = ""


@dataclass(frozen=True)
class VisibilityResponse:
    id: str
    invisible_until: str
    raw: str = ""


@dataclass
class BatchMessage:
    """One message of `publish_batch`: the same fields as `publish`. The bytes travel in base64."""

    body: Union[bytes, bytearray, memoryview, str]
    #: Default `application/json`.
    content_type: Optional[str] = None
    group_id: Optional[str] = None
    deduplication_id: Optional[str] = None


@dataclass(frozen=True)
class BatchResult:
    """
    The result of one item of a batch, in the order of the request: the `status` the single call would have
    answered and what it would have returned. An item that failed does not raise: `ok` is `False`.
    """

    #: `201` published, `200` a FIFO duplicate, a distributor or a new invisibility, `204` acked, or the error's.
    status: int
    id: Optional[str] = None
    duplicate: Optional[bool] = None
    matched: Optional[int] = None
    invisible_until: Optional[str] = None
    error_code: Optional[str] = None
    error_message: Optional[str] = None
    raw: str = ""

    @property
    def ok(self) -> bool:
        return self.error_code is None and self.status < 300


class MessageContext:
    """
    What the `consume_with` handler receives: the `message`, the `delete` flag (the message is deleted at the end
    if the handler returns without raising; the default is `True`) and `change_visibility`. While the handler
    runs, the connector renews the invisibility by itself, so no other consumer takes the message.
    """

    def __init__(self, message: Message, lease_seconds: int, change: Any) -> None:
        self.message = message
        self.delete = True
        #: `consume_json` only: the parsed body.
        self.body: Any = None
        self._lease_seconds = lease_seconds
        self._lost = False
        self._change = change

    @property
    def lease_seconds(self) -> int:
        """The seconds of invisibility the automatic renewal requests on each round."""
        return self._lease_seconds

    @property
    def lease_lost(self) -> bool:
        """`True` when the server refused the renewal (the message already went back to the queue or was deleted)."""
        return self._lost

    def change_visibility(self, seconds: int) -> None:
        """Changes the invisibility to `seconds` from now on. The value becomes the one of the automatic renewal."""
        self._change(seconds)
        self._lease_seconds = seconds


# -- reading the responses ---------------------------------------------------------------------------------


def _invalid(what: str, error: JsonError) -> TabaError:
    return TabaError.client("client.invalid-response", "%s: %s" % (what, error))


def _fields(raw: str, what: str) -> JsonFields:
    try:
        return JsonFields.parse(raw)
    except JsonError as e:
        raise _invalid(what, e)


def _is_json_type(content_type: str) -> bool:
    media = content_type.split(";")[0].strip().lower()
    return media == "application/json" or media.endswith("+json")


def parse_topic(raw: str) -> Topic:
    f = _fields(raw, "topic")
    try:
        return Topic(
            id=f.string("id") or "",
            name=f.string("name") or "",
            type=f.string("type") or "",
            created_at=f.string("createdAt") or "",
            fifo=f.boolean("fifo"),
            volatile=f.boolean("volatile"),
            raw=raw,
        )
    except JsonError as e:
        raise _invalid("topic", e)


def parse_message(raw: str) -> Message:
    f = _fields(raw, "message")
    try:
        content_type = f.string("contentType") or "application/json"
        body = f.raw("body")
        if body is None:
            data = b""
        elif f.string("bodyEncoding") == "base64":
            try:
                data = base64.b64decode(decode_string(body), validate=True)
            except (binascii.Error, ValueError):
                raise TabaError.client("client.invalid-response", "invalid base64 body")
        elif _is_json_type(content_type):
            data = body.encode("utf-8")
        else:
            data = decode_string(body).encode("utf-8")
        return Message(
            id=f.string("id") or "",
            body=data,
            content_type=content_type,
            receipt_token=f.string("receiptToken") or "",
            delivery_count=f.integer("deliveryCount") or 0,
            published_at=f.string("publishedAt") or "",
            invisible_until=f.string("invisibleUntil") or "",
            group_id=f.string("groupId"),
            deduplication_id=f.string("deduplicationId"),
            dead_lettered_at=f.string("deadLetteredAt"),
            dead_letter_reason=f.string("deadLetterReason"),
            raw=raw,
        )
    except JsonError as e:
        raise _invalid("message", e)


def parse_messages(raw: str) -> List[Message]:
    f = _fields(raw, "consume")
    try:
        return [parse_message(item) for item in array_items(f.raw("messages") or "[]")]
    except JsonError as e:
        raise _invalid("messages", e)


def parse_page(raw: str) -> Page:
    f = _fields(raw, "page")
    try:
        items = [parse_topic(item) for item in array_items(f.raw("items") or "[]")]
        return Page(items=items, next_cursor=f.string("nextCursor"), raw=raw)
    except JsonError as e:
        raise _invalid("page items", e)


def parse_publish_response(raw: str) -> PublishResponse:
    f = _fields(raw, "publication")
    try:
        deliveries: List[DistributorDelivery] = []
        listing = f.raw("deliveries")
        if listing is not None:
            for item in array_items(listing):
                d = JsonFields.parse(item)
                error = d.raw("error")
                e = JsonFields.parse(error) if error is not None and error.startswith("{") else None
                deliveries.append(
                    DistributorDelivery(
                        topic=d.string("topic") or "",
                        id=d.string("id"),
                        error_code=None if e is None else (e.string("code") or ""),
                        error_message=None if e is None else e.string("message"),
                    )
                )
        return PublishResponse(
            id=f.string("id"),
            duplicate=f.boolean("duplicate"),
            matched=f.integer("matched"),
            deliveries=deliveries,
            raw=raw,
        )
    except JsonError as e:
        raise _invalid("publication", e)


def parse_batch_results(raw: str) -> List[BatchResult]:
    """The `results` of a batch, one per item in the order of the request."""
    f = _fields(raw, "batch")
    try:
        results: List[BatchResult] = []
        for item in array_items(f.raw("results") or "[]"):
            r = JsonFields.parse(item)
            error = r.raw("error")
            e = JsonFields.parse(error) if error is not None and error.startswith("{") else None
            results.append(
                BatchResult(
                    status=r.integer("status") or 0,
                    id=r.string("id"),
                    duplicate=r.boolean("duplicate"),
                    matched=r.integer("matched"),
                    invisible_until=r.string("invisibleUntil"),
                    error_code=None if e is None else (e.string("code") or ""),
                    error_message=None if e is None else e.string("message"),
                    raw=item,
                )
            )
        return results
    except JsonError as e:
        raise _invalid("batch results", e)


def parse_visibility_response(raw: str) -> VisibilityResponse:
    f = _fields(raw, "visibility")
    try:
        return VisibilityResponse(
            id=f.string("id") or "", invisible_until=f.string("invisibleUntil") or "", raw=raw
        )
    except JsonError as e:
        raise _invalid("visibility", e)
