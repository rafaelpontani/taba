"""The synchronous Taba client."""

import json
import threading
from typing import Any, Callable, Dict, Iterator, List, Optional, Union

from . import _calls
from ._transport import Sleep, Transport
from .cancel import Cancelled, CancelToken
from .errors import TabaError
from .models import (
    BatchMessage,
    BatchResult,
    Config,
    ConsumeOptions,
    Message,
    MessageContext,
    Page,
    PublishResponse,
    Topic,
    TopicRequest,
    TopicUpdate,
    VisibilityResponse,
    parse_batch_results,
    parse_messages,
    parse_page,
    parse_publish_response,
    parse_topic,
    parse_visibility_response,
)


def make_config(config: Union[Config, str, None], options: Dict[str, Any]) -> Config:
    if isinstance(config, Config):
        if options:
            raise TypeError("pass either a Config or keyword options, not both")
        return config
    if isinstance(config, str):
        options = dict(options, base_url=config)
    return Config(**options)


def _lost_or_gone(error: TabaError) -> bool:
    """The message already went back to the queue (deadline expired) or is already gone."""
    return error.status in (409, 404)


class Client:
    """
    The Taba client API client (doc 21): the nine operations, `subscribe` and the consume with a handler.
    It is safe for concurrent use: each thread has its own connection. For `asyncio`, use `taba.aio.AsyncClient`.

    >>> client = Client("http://localhost:8080", token="...")
    >>> client.publish_json("orders", {"order": 9001})
    >>> for message in client.consume("orders", wait_time=20):
    ...     client.ack("orders", message)
    """

    def __init__(self, config: Union[Config, str, None] = None, *, _sleep: Optional[Sleep] = None, **options: Any) -> None:
        self._config = make_config(config, options)
        self._transport = Transport(self._config, _sleep)
        self._serializer = self._config.serializer or json

    @property
    def config(self) -> Config:
        return self._config

    def close(self) -> None:
        """Closes the calling thread's connection. The client can be used again afterwards."""
        self._transport.close()

    def __enter__(self) -> "Client":
        return self

    def __exit__(self, *exc_info: Any) -> None:
        self.close()

    def _send(self, call: Any, cancel: Optional[CancelToken]) -> Any:
        return self._transport.send(call, cancel)

    # -- topics --------------------------------------------------------------------------------------------

    def create_topic(self, request: TopicRequest, *, cancel: Optional[CancelToken] = None) -> Topic:
        reply = self._send(_calls.create_topic_call(request), cancel)
        if reply.recovered:
            return self.get_topic(request.name, cancel=cancel)
        return parse_topic(reply.text)

    def get_topic(self, name: str, *, cancel: Optional[CancelToken] = None) -> Topic:
        return parse_topic(self._send(_calls.get_topic_call(name), cancel).text)

    def list_topics(
        self,
        limit: Optional[int] = None,
        cursor: Optional[str] = None,
        name: Optional[str] = None,
        *,
        cancel: Optional[CancelToken] = None,
    ) -> Page:
        """`GET /topics`. Parameters left `None` do not go in the query."""
        return parse_page(self._send(_calls.list_topics_call(limit, cursor, name), cancel).text)

    def iter_topics(self, name: Optional[str] = None, *, cancel: Optional[CancelToken] = None) -> Iterator[Topic]:
        """All topics, page by page, as a generator."""
        cursor = None
        while True:
            page = self.list_topics(cursor=cursor, name=name, cancel=cancel)
            yield from page.items
            cursor = page.next_cursor
            if cursor is None:
                return

    def list_all_topics(self, name: Optional[str] = None, *, cancel: Optional[CancelToken] = None) -> List[Topic]:
        return list(self.iter_topics(name, cancel=cancel))

    def update_topic(self, name: str, update: TopicUpdate, *, cancel: Optional[CancelToken] = None) -> Topic:
        """A real `PATCH`, with no verb-override header."""
        return parse_topic(self._send(_calls.update_topic_call(name, update), cancel).text)

    def delete_topic(self, name: str, *, cancel: Optional[CancelToken] = None) -> None:
        self._send(_calls.delete_topic_call(name), cancel)

    # -- publishing ----------------------------------------------------------------------------------------

    def publish(
        self,
        topic: str,
        body: Union[bytes, bytearray, memoryview, str, None],
        *,
        content_type: Optional[str] = None,
        group_id: Optional[str] = None,
        deduplication_id: Optional[str] = None,
        request_id: Optional[str] = None,
        traceparent: Optional[str] = None,
        fifo: bool = False,
        cancel: Optional[CancelToken] = None,
    ) -> PublishResponse:
        """
        `POST /topics/{topic}/messages`. The body goes as bytes (a `str` is sent as UTF-8), without validation.
        Set `fifo=True` when the topic is a FIFO queue: only there can a 502/504 be repeated.
        """
        call = _calls.publish_call(
            topic, body, content_type, group_id, deduplication_id, request_id, traceparent, fifo
        )
        return parse_publish_response(self._send(call, cancel).text)

    def publish_json(self, topic: str, value: Any, **options: Any) -> PublishResponse:
        """Serializes the value (with the configured `serializer`, or `json`) and publishes as `application/json`."""
        if value is None:
            raise TabaError.client("client.invalid-request", "value is required")
        try:
            text = self._serializer.dumps(value)
        except (TypeError, ValueError) as e:
            raise TabaError.client("client.invalid-request", "cannot serialize the value: %s" % e)
        options["content_type"] = "application/json"
        return self.publish(topic, text.encode("utf-8"), **options)

    def publish_raw_json(self, topic: str, text: str, **options: Any) -> PublishResponse:
        """Publishes a JSON text the application already built, without serializing."""
        options["content_type"] = "application/json"
        return self.publish(topic, text.encode("utf-8"), **options)

    def publish_text(self, topic: str, text: str, **options: Any) -> PublishResponse:
        """Publishes a text (`Content-Type: text/plain; charset=utf-8`)."""
        options["content_type"] = "text/plain; charset=utf-8"
        return self.publish(topic, text.encode("utf-8"), **options)

    def publish_batch(
        self, topic: str, messages: List[BatchMessage], *, cancel: Optional[CancelToken] = None
    ) -> List[BatchResult]:
        """
        `POST /topics/{topic}/messages/batch`: publishes many messages in one request (1 to the server's
        `api.batch.max-messages`, 1000 by default). Each item is answered as its single publication and one that
        fails does not undo the others (look at `ok`). On a FIFO queue the items of a group are published in the
        order of the list. A failure of the network is not repeated (it could duplicate).
        """
        return parse_batch_results(self._send(_calls.publish_batch_call(topic, messages), cancel).text)

    # -- consuming -----------------------------------------------------------------------------------------

    def consume(self, topic: str, *, cancel: Optional[CancelToken] = None, **options: Any) -> List[Message]:
        """
        `GET /topics/{topic}/messages`. The read time is `wait_time + 10` seconds, not the `request_timeout`.
        A timeout is not repeated (the node may have captured messages, which come back by themselves at the end
        of the `visibility_timeout`). Returns an empty list if there are no messages. Options: `max_messages`,
        `visibility_timeout`, `wait_time`, `auto_create`, `volatile`, `idle_timeout`, `fifo`, `tags`,
        `request_id`, `traceparent`.
        """
        return self._consume(topic, ConsumeOptions(**options), cancel)

    def _consume(self, topic: str, options: ConsumeOptions, cancel: Optional[CancelToken]) -> List[Message]:
        return parse_messages(self._send(_calls.consume_call(topic, options), cancel).text)

    def ack(
        self,
        topic: str,
        message: Union[Message, str],
        receipt_token: Optional[str] = None,
        *,
        cancel: Optional[CancelToken] = None,
    ) -> None:
        """`DELETE /topics/{topic}/messages/{id}?receiptToken=`. `message` is what `consume` returned, or its id."""
        message_id, token = _calls.reference(message, receipt_token)
        self._send(_calls.ack_call(topic, message_id, token), cancel)

    def change_visibility(
        self,
        topic: str,
        message: Union[Message, str],
        seconds: int,
        receipt_token: Optional[str] = None,
        *,
        cancel: Optional[CancelToken] = None,
    ) -> VisibilityResponse:
        """`PUT /topics/{topic}/messages/{id}/visibility?receiptToken=`."""
        message_id, token = _calls.reference(message, receipt_token)
        reply = self._send(_calls.change_visibility_call(topic, message_id, token, seconds), cancel)
        return parse_visibility_response(reply.text)

    def ack_batch(
        self, topic: str, messages: List[Message], *, cancel: Optional[CancelToken] = None
    ) -> List[BatchResult]:
        """`POST /topics/{topic}/messages/ack`: acknowledges many messages `consume` returned, in one request."""
        return parse_batch_results(self._send(_calls.receipts_call(topic, "ack", messages), cancel).text)

    def change_visibility_batch(
        self, topic: str, messages: List[Message], seconds: int, *, cancel: Optional[CancelToken] = None
    ) -> List[BatchResult]:
        """`POST /topics/{topic}/messages/visibility`: the invisibility of each message, `seconds` from now."""
        call = _calls.receipts_call(topic, "visibility", messages, seconds)
        return parse_batch_results(self._send(call, cancel).text)

    def consume_with(
        self,
        topic: str,
        handler: Callable[[MessageContext], None],
        *,
        cancel: Optional[CancelToken] = None,
        **options: Any,
    ) -> int:
        """
        Reads from the queue and hands each message to `handler(context)`. The message is deleted if the handler
        returns without raising (`context.delete = False` keeps it). While the handler runs, the invisibility is
        renewed every third of it, by another thread. Defaults: one message at a time, 20 s of long polling,
        120 s of invisibility. If the handler raises, the message is not deleted and the exception propagates.
        Returns how many messages were handled.
        """
        opts = _calls.handler_defaults(ConsumeOptions(**options))
        handled = 0
        for message in self._consume(topic, opts, cancel):
            context = self._context(topic, opts, message, cancel)
            self._handle(topic, context, lambda c=context: handler(c))
            handled += 1
        return handled

    def consume_json(
        self,
        topic: str,
        handler: Callable[[MessageContext], None],
        *,
        cancel: Optional[CancelToken] = None,
        **options: Any,
    ) -> int:
        """
        `consume_with` with the body already parsed (with the configured `serializer`, or `json`) in
        `context.body`. A body that does not parse gives `client.invalid-response` before the handler is called,
        and the message is not deleted.
        """
        opts = _calls.handler_defaults(ConsumeOptions(**options))
        handled = 0
        for message in self._consume(topic, opts, cancel):
            try:
                body = self._serializer.loads(message.body_text)
            except ValueError as e:
                raise TabaError.client(
                    "client.invalid-response", "body of message %s is not valid JSON: %s" % (message.id, e)
                )
            context = self._context(topic, opts, message, cancel)
            context.body = body
            self._handle(topic, context, lambda c=context: handler(c))
            handled += 1
        return handled

    def _context(self, topic: str, options: ConsumeOptions, message: Message, cancel: Optional[CancelToken]) -> MessageContext:
        return MessageContext(
            message,
            options.visibility_timeout or _calls.HANDLER_VISIBILITY_TIMEOUT,
            lambda seconds: self.change_visibility(topic, message, seconds, cancel=cancel),
        )

    def _handle(self, topic: str, context: MessageContext, run: Callable[[], None]) -> None:
        stop = CancelToken()
        renewal = threading.Thread(target=self._keep_alive, args=(topic, context, stop), daemon=True, name="taba-renewal")
        renewal.start()
        try:
            run()
        finally:
            stop.cancel()
            renewal.join()
        if context.delete:
            try:
                # Without the token: the message was already handled, and acknowledging it must not be cut short.
                self.ack(topic, context.message)
            except TabaError as e:
                if not _lost_or_gone(e):
                    raise

    def _keep_alive(self, topic: str, context: MessageContext, stop: CancelToken) -> None:
        """The second thread: renews the invisibility until the handler finishes. Never raises."""
        try:
            while not stop.is_set():
                self._transport.sleep(_calls.renewal_interval(context.lease_seconds), stop)
                try:
                    self.change_visibility(topic, context.message, context.lease_seconds, cancel=stop)
                except TabaError as e:
                    if _lost_or_gone(e):
                        # The message already went back to the queue or was deleted: renewing is no longer of any use.
                        context._lost = True
                        return
                    # Transient failure (network, 5xx): tries again on the next round, before the deadline expires.
        except Cancelled:
            pass
        finally:
            self._transport.close()  # this thread's own connection

    def subscribe(
        self,
        topic: str,
        handler: Callable[[Message], None],
        *,
        cancel: Optional[CancelToken] = None,
        on_error: Optional[Callable[[Message, Exception], None]] = None,
        **options: Any,
    ) -> None:
        """
        The consume loop: long polling (`wait_time` 20 and one message at a time, if not set), calls
        `handler(message)` and acknowledges by itself only if it returns without raising (a message that fails
        goes back to the queue at the end of the `visibility_timeout`, and the error goes to
        `on_error(message, error)`). Blocks until `cancel` is cancelled and then returns without an exception
        (a `KeyboardInterrupt` propagates). A server error while consuming propagates.
        """
        cancel = cancel or CancelToken()
        poll = _calls.subscribe_defaults(ConsumeOptions(**options))
        while not cancel.is_set():
            try:
                messages = self._consume(topic, poll, cancel)
            except Cancelled:
                return
            for message in messages:
                handled = False
                try:
                    handler(message)
                    handled = True
                except Exception as e:  # noqa: BLE001 - the handler is the application's code
                    if on_error is not None:
                        on_error(message, e)
                if handled:
                    try:
                        # Without the token: the message was already handled.
                        self.ack(topic, message)
                    except TabaError as e:
                        if not _lost_or_gone(e):
                            raise
                if cancel.is_set():
                    return
