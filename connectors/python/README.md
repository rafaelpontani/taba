# taba-client (Python)

The Python client of [Taba](https://github.com/rafaelpontani/taba): the nine operations of the client API, `subscribe`, consuming with a handler (with automatic renewal of the invisibility) and the call retries of the common contract. **No dependencies** (only the standard library); typed.

> Other languages: [Português (Brasil)](https://github.com/rafaelpontani/taba/blob/main/doc/pt-BR/21-conectores.md) · [all languages](https://github.com/rafaelpontani/taba/blob/main/doc/README.md)

- **Python 3.9 or later.**
- `taba.Client` is synchronous and safe for concurrent use (each thread has its own connection); `taba.aio.AsyncClient` is the `asyncio` one.

```python
from taba import Client, TopicRequest

with Client("http://localhost:8080", token="...") as client:
    client.create_topic(TopicRequest("orders", "queue"))
    client.publish_json("orders", {"order": 9001})

    # Consume with a handler: the message is deleted if the handler does not raise.
    def handle(context):
        print(context.message.body_text)

    client.consume_with("orders", handle, wait_time=20)

    # Or the whole loop, until the token is cancelled.
    # client.subscribe("orders", lambda message: print(message.json()), cancel=token)
```

```python
import asyncio
from taba.aio import AsyncClient

async def main():
    async with AsyncClient("http://localhost:8080") as client:
        for message in await client.consume("orders", wait_time=20):
            await client.ack("orders", message)

asyncio.run(main())
```

## Configuration

`Client(Config(...))`, or `Client(base_url, **options)` with the same fields. The client does not read environment variables: `TABA_*` belongs to the server.

| Field | Default | |
|---|---|---|
| `base_url` | required | `http://host:port` or `https://...` |
| `token` | none | `Authorization: Bearer` |
| `connect_timeout` | `5.0` | seconds |
| `request_timeout` | `30.0` | seconds; `consume` uses `wait_time + 10` |
| `max_attempts` | `3` | counting the first |
| `user_agent` | `taba-python/<version>` | |
| `serializer` | `json` | an object with `dumps` and `loads`, for `publish_json` and `consume_json` |
| `ssl_context` | system's, TLS 1.2+ | an own `ssl.SSLContext` (a private CA, for example) |

## What differs from the other connectors

- Names are `snake_case`; options are keyword arguments (`client.consume("q", max_messages=5, wait_time=20)`), and `ack`/`change_visibility` take the `Message` you received or its id plus `receipt_token`.
- A `TopicUpdate` field left at its default is not sent; `None` sends `null` (back to the server default) for `max_message_bytes`, `ttl` and `max_deliveries`.
- Cancelling: the synchronous client takes a `CancelToken` (`cancel=token`; it closes the connection in flight, so a long polling does not wait for its timeout) and raises `taba.Cancelled`; `subscribe` returns without an exception. The `asyncio` client is cancelled the `asyncio` way (`task.cancel()`), which raises `asyncio.CancelledError`.
- `iter_topics()` is a generator over all the pages; `list_all_topics()` returns the list.
- Errors are always a `TabaError` with `status`, `code`, `message`, `request_id`, `retry_after`, `details`. Test the `code`, never the `message`.
- The standard library has no asynchronous HTTP client, so the `asyncio` client runs the synchronous transport in worker threads (`asyncio.to_thread`).
- TLS is Python's own, with TLS 1.2 as the minimum; the certificate is always verified.

## Tests

```
cd connectors/python
python -m unittest discover -s tests -t .
```

The tests start a local HTTP server that records every request and answers from a script (the equivalent of the C# `ScriptedHandler`). There are no integration tests against a real Taba yet.
