"""Builds the `ApiCall` of each operation. Shared by the synchronous and the asynchronous client."""

import json
from typing import Any, Dict, Optional, Union

from ._transport import ApiCall, segment
from .errors import TabaError
from .models import UNSET, ConsumeOptions, Message, TopicRequest, TopicUpdate

#: `consume` waits `wait_time` plus this slack (seconds), not the `request_timeout`.
LONG_POLL_SLACK = 10.0

HANDLER_MAX_MESSAGES = 1
HANDLER_VISIBILITY_TIMEOUT = 120
HANDLER_WAIT_TIME = 20


def create_topic_call(request: TopicRequest) -> ApiCall:
    if request is None:
        raise TabaError.client("client.invalid-request", "request is required")
    body: Dict[str, Any] = {"name": request.name or "", "type": request.type or ""}
    if request.tags:
        body["tags"] = request.tags
    if request.prefixes:
        body["prefixes"] = request.prefixes
    for attribute, name in (
        ("volatile", "volatile"),
        ("idle_timeout", "idleTimeout"),
        ("fifo", "fifo"),
        ("max_message_bytes", "maxMessageBytes"),
        ("ttl", "ttl"),
        ("max_deliveries", "maxDeliveries"),
    ):
        value = getattr(request, attribute)
        if value is not None:
            body[name] = value
    call = ApiCall("POST", "/topics").with_json(json.dumps(body, separators=(",", ":")))
    call.recover.append((409, "topic.already-exists"))
    return call


def get_topic_call(name: str) -> ApiCall:
    return ApiCall("GET", "/topics/" + segment(name))


def list_topics_call(limit: Optional[int], cursor: Optional[str], name: Optional[str]) -> ApiCall:
    call = ApiCall("GET", "/topics")
    if limit is not None:
        call.with_query("limit", limit)
    if cursor is not None:
        call.with_query("cursor", cursor)
    if name is not None:
        call.with_query("name", name)
    return call


def update_topic_call(name: str, update: TopicUpdate) -> ApiCall:
    if update is None:
        raise TabaError.client("client.invalid-request", "update is required")
    body: Dict[str, Any] = {}
    if update.tags is not None:
        body["tags"] = update.tags
    if update.prefixes is not None:
        body["prefixes"] = update.prefixes
    if update.idle_timeout is not None:
        body["idleTimeout"] = update.idle_timeout
    # `UNSET` leaves the field out; `None` sends `null` (back to the server default).
    for value, key in (
        (update.max_message_bytes, "maxMessageBytes"),
        (update.ttl, "ttl"),
        (update.max_deliveries, "maxDeliveries"),
    ):
        if value is not UNSET:
            body[key] = value
    return ApiCall("PATCH", "/topics/" + segment(name)).with_json(json.dumps(body, separators=(",", ":")))


def delete_topic_call(name: str) -> ApiCall:
    call = ApiCall("DELETE", "/topics/" + segment(name))
    call.recover.append((404, "topic.not-found"))
    return call


def publish_call(
    topic: str,
    body: Union[bytes, bytearray, memoryview, str, None],
    content_type: Optional[str] = None,
    group_id: Optional[str] = None,
    deduplication_id: Optional[str] = None,
    request_id: Optional[str] = None,
    traceparent: Optional[str] = None,
    fifo: bool = False,
) -> ApiCall:
    if body is None:
        data = b""
    elif isinstance(body, str):
        data = body.encode("utf-8")
    else:
        data = bytes(body)
    call = ApiCall("POST", "/topics/%s/messages" % segment(topic))
    call.body = data
    call.content_type = content_type or "application/json"
    call.request_id = request_id
    call.traceparent = traceparent
    call.retry_gateway = bool(fifo)
    if group_id is not None:
        call.with_query("groupId", group_id)
    if deduplication_id is not None:
        call.with_query("deduplicationId", deduplication_id)
    return call


def consume_call(topic: str, options: Optional[ConsumeOptions]) -> ApiCall:
    o = options or ConsumeOptions()
    call = ApiCall("GET", "/topics/%s/messages" % segment(topic))
    if o.max_messages is not None:
        call.with_query("maxMessages", o.max_messages)
    if o.visibility_timeout is not None:
        call.with_query("visibilityTimeout", o.visibility_timeout)
    if o.wait_time is not None:
        call.with_query("waitTime", o.wait_time)
    if o.auto_create:
        call.with_query("autoCreate", True)
        if o.volatile is not None:
            call.with_query("volatile", o.volatile)
        if o.idle_timeout is not None:
            call.with_query("idleTimeout", o.idle_timeout)
        if o.fifo is not None:
            call.with_query("fifo", o.fifo)
        for key, value in (o.tags or {}).items():
            call.with_query("tag." + key, value)
    call.request_id = o.request_id
    call.traceparent = o.traceparent
    call.timeout = float(o.wait_time or 0) + LONG_POLL_SLACK
    return call


def ack_call(topic: str, message_id: str, receipt_token: str) -> ApiCall:
    call = ApiCall("DELETE", "/topics/%s/messages/%s" % (segment(topic), segment(message_id)))
    return call.with_query("receiptToken", receipt_token)


def change_visibility_call(topic: str, message_id: str, receipt_token: str, seconds: int) -> ApiCall:
    call = ApiCall("PUT", "/topics/%s/messages/%s/visibility" % (segment(topic), segment(message_id)))
    call.with_query("receiptToken", receipt_token)
    return call.with_json(json.dumps({"visibilityTimeout": int(seconds)}, separators=(",", ":")))


def reference(message: Union[Message, str], receipt_token: Optional[str]) -> "tuple[str, str]":
    """A message received from `consume`, or its `id` with the `receipt_token`."""
    if isinstance(message, str):
        if receipt_token is None:
            raise TabaError.client("client.invalid-request", "receipt_token is required with a message id")
        return message, receipt_token
    return message.id, message.receipt_token


def handler_defaults(options: Optional[ConsumeOptions]) -> ConsumeOptions:
    """One message at a time, 20 s of long polling and 120 s of invisibility, which the renewal keeps."""
    o = ConsumeOptions(**vars(options)) if options is not None else ConsumeOptions()
    if o.max_messages is None:
        o.max_messages = HANDLER_MAX_MESSAGES
    if o.visibility_timeout is None:
        o.visibility_timeout = HANDLER_VISIBILITY_TIMEOUT
    if o.wait_time is None:
        o.wait_time = HANDLER_WAIT_TIME
    return o


def subscribe_defaults(options: Optional[ConsumeOptions]) -> ConsumeOptions:
    o = ConsumeOptions(**vars(options)) if options is not None else ConsumeOptions()
    if o.wait_time is None:
        o.wait_time = HANDLER_WAIT_TIME
    if o.max_messages is None:
        o.max_messages = HANDLER_MAX_MESSAGES
    return o


def renewal_interval(lease_seconds: int) -> float:
    """A third of the invisibility (and never all of it), so a failed renewal can be tried again in time."""
    return max(1, lease_seconds) / 3.0
