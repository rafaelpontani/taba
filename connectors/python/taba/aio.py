"""
The `asyncio` Taba client.

Python's standard library has no asynchronous HTTP client, and the connector has no dependencies, so each call
runs the synchronous transport in a worker thread (`asyncio.to_thread`). Cancelling the task closes the
connection of the call in flight, so a long polling does not keep a thread for 30 s.
"""

import asyncio
import json
from typing import Any, Awaitable, Callable, Dict, List, Optional, Union

from . import _calls
from .cancel import CancelToken, Cancelled
from .client import Client, _lost_or_gone, make_config
from .errors import TabaError
from .models import (
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
)

Sleep = Callable[[float], Awaitable[None]]


class AsyncMessageContext(MessageContext):
    """The `MessageContext` of the asynchronous client: `change_visibility` is a coroutine."""

    async def change_visibility(self, seconds: int) -> None:  # type: ignore[override]
        await self._change(seconds)
        self._lease_seconds = seconds


class AsyncClient:
    """
    The Taba client API client for `asyncio`: the same nine operations, `subscribe` and the consume with a
    handler, as coroutines. Stop a `subscribe` by cancelling its task (`task.cancel()`, `asyncio.timeout`...).

    >>> async with AsyncClient("http://localhost:8080", token="...") as client:
    ...     await client.publish_json("orders", {"order": 9001})
    ...     for message in await client.consume("orders", wait_time=20):
    ...         await client.ack("orders", message)
    """

    def __init__(self, config: Union[Config, str, None] = None, *, _sleep: Optional[Sleep] = None, **options: Any) -> None:
        self._client = Client(make_config(config, options))
        self._sleep: Sleep = _sleep or asyncio.sleep
        self._serializer = self._client._serializer

    @property
    def config(self) -> Config:
        return self._client.config

    async def close(self) -> None:
        """Closes the worker-thread connections the client opened (they are closed with the threads)."""
        self._client.close()

    async def __aenter__(self) -> "AsyncClient":
        return self

    async def __aexit__(self, *exc_info: Any) -> None:
        await self.close()

    async def _run(self, function: Callable[..., Any], *args: Any, **kwargs: Any) -> Any:
        token = CancelToken()
        try:
            return await asyncio.to_thread(function, *args, cancel=token, **kwargs)
        except asyncio.CancelledError:
            token.cancel()  # closes the connection of the call in flight
            raise

    # -- topics --------------------------------------------------------------------------------------------

    async def create_topic(self, request: TopicRequest) -> Topic:
        return await self._run(self._client.create_topic, request)

    async def get_topic(self, name: str) -> Topic:
        return await self._run(self._client.get_topic, name)

    async def list_topics(
        self, limit: Optional[int] = None, cursor: Optional[str] = None, name: Optional[str] = None
    ) -> Page:
        return await self._run(self._client.list_topics, limit, cursor, name)

    async def list_all_topics(self, name: Optional[str] = None) -> List[Topic]:
        return await self._run(self._client.list_all_topics, name)

    async def update_topic(self, name: str, update: TopicUpdate) -> Topic:
        return await self._run(self._client.update_topic, name, update)

    async def delete_topic(self, name: str) -> None:
        await self._run(self._client.delete_topic, name)

    # -- publishing ----------------------------------------------------------------------------------------

    async def publish(self, topic: str, body: Union[bytes, bytearray, memoryview, str, None], **options: Any) -> PublishResponse:
        return await self._run(self._client.publish, topic, body, **options)

    async def publish_json(self, topic: str, value: Any, **options: Any) -> PublishResponse:
        return await self._run(self._client.publish_json, topic, value, **options)

    async def publish_raw_json(self, topic: str, text: str, **options: Any) -> PublishResponse:
        return await self._run(self._client.publish_raw_json, topic, text, **options)

    async def publish_text(self, topic: str, text: str, **options: Any) -> PublishResponse:
        return await self._run(self._client.publish_text, topic, text, **options)

    # -- consuming -----------------------------------------------------------------------------------------

    async def consume(self, topic: str, **options: Any) -> List[Message]:
        return await self._run(self._client.consume, topic, **options)

    async def ack(self, topic: str, message: Union[Message, str], receipt_token: Optional[str] = None) -> None:
        await self._run(self._client.ack, topic, message, receipt_token)

    async def change_visibility(
        self, topic: str, message: Union[Message, str], seconds: int, receipt_token: Optional[str] = None
    ) -> VisibilityResponse:
        return await self._run(self._client.change_visibility, topic, message, seconds, receipt_token)

    async def consume_with(self, topic: str, handler: Callable[[MessageContext], Awaitable[None]], **options: Any) -> int:
        """
        The `Client.consume_with` for `asyncio`: `handler` is a coroutine function. The invisibility is renewed
        by an `asyncio` task while it runs.
        """
        opts = _calls.handler_defaults(ConsumeOptions(**options))
        handled = 0
        for message in await self._run(self._client._consume, topic, opts):
            context = self._context(topic, opts, message)
            await self._handle(topic, context, handler)
            handled += 1
        return handled

    async def consume_json(self, topic: str, handler: Callable[[MessageContext], Awaitable[None]], **options: Any) -> int:
        """`consume_with` with the body already parsed in `context.body` (see `Client.consume_json`)."""
        opts = _calls.handler_defaults(ConsumeOptions(**options))
        handled = 0
        for message in await self._run(self._client._consume, topic, opts):
            try:
                body = self._serializer.loads(message.body_text)
            except ValueError as e:
                raise TabaError.client(
                    "client.invalid-response", "body of message %s is not valid JSON: %s" % (message.id, e)
                )
            context = self._context(topic, opts, message)
            context.body = body
            await self._handle(topic, context, handler)
            handled += 1
        return handled

    def _context(self, topic: str, options: ConsumeOptions, message: Message) -> AsyncMessageContext:
        return AsyncMessageContext(
            message,
            options.visibility_timeout or _calls.HANDLER_VISIBILITY_TIMEOUT,
            lambda seconds: self.change_visibility(topic, message, seconds),
        )

    async def _handle(self, topic: str, context: AsyncMessageContext, handler: Callable[[Any], Awaitable[None]]) -> None:
        renewal = asyncio.ensure_future(self._keep_alive(topic, context))
        try:
            await handler(context)
        finally:
            renewal.cancel()
            await asyncio.gather(renewal, return_exceptions=True)
        if context.delete:
            try:
                # Shielded: the message was already handled, and acknowledging it must not be cut short.
                await asyncio.shield(self.ack(topic, context.message))
            except TabaError as e:
                if not _lost_or_gone(e):
                    raise

    async def _keep_alive(self, topic: str, context: AsyncMessageContext) -> None:
        """The second task: renews the invisibility until the handler finishes (it is cancelled). Never raises."""
        while True:
            await self._sleep(_calls.renewal_interval(context.lease_seconds))
            try:
                await self.change_visibility(topic, context.message, context.lease_seconds)
            except TabaError as e:
                if _lost_or_gone(e):
                    context._lost = True
                    return
                # Transient failure (network, 5xx): tries again on the next round, before the deadline expires.

    async def subscribe(
        self,
        topic: str,
        handler: Callable[[Message], Awaitable[None]],
        *,
        on_error: Optional[Callable[[Message, Exception], None]] = None,
        **options: Any,
    ) -> None:
        """
        The consume loop (see `Client.subscribe`). It never returns by itself: stop it by cancelling the task,
        which raises `asyncio.CancelledError` as any `asyncio` coroutine does. A message already handled is
        acknowledged even if the task is cancelled right after.
        """
        poll = _calls.subscribe_defaults(ConsumeOptions(**options))
        while True:
            messages = await self._run(self._client._consume, topic, poll)
            for message in messages:
                handled = False
                try:
                    await handler(message)
                    handled = True
                except Exception as e:  # noqa: BLE001 - the handler is the application's code
                    if on_error is not None:
                        on_error(message, e)
                if handled:
                    try:
                        await asyncio.shield(self.ack(topic, message))
                    except TabaError as e:
                        if not _lost_or_gone(e):
                            raise
