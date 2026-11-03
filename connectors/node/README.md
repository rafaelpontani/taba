# taba-client (Node.js)

The Node.js client of [Taba](https://github.com/rafaelpontani/taba): the nine operations of the client API, `subscribe`, consuming with a handler (with automatic renewal of the invisibility) and the call retries of the common contract. **No dependencies** (only `node:http`/`node:https`); types included.

> Other languages: [Português (Brasil)](https://github.com/rafaelpontani/taba/blob/main/doc/pt-BR/21-conectores.md) · [all languages](https://github.com/rafaelpontani/taba/blob/main/doc/README.md)

- **Node.js 18 or later.** CommonJS (works with `require` and `import`).
- **Everything is asynchronous** (`Promise`): Node has no synchronous HTTP, so there is no `...Sync` twin.
- Safe for any number of concurrent calls on one `TabaClient`.

```js
const { TabaClient } = require('taba-client');

const client = new TabaClient({ baseUrl: 'http://localhost:8080', token: process.env.TABA_TOKEN });

await client.createTopic({ name: 'orders', type: 'queue' });
await client.publishJson('orders', { order: 9001 });

// Consume with a handler: the message is deleted if the handler does not throw.
await client.consumeWith('orders', { waitTime: 20 }, async ({ message }) => {
  console.log(message.bodyText);
});

// Or the whole loop, until the signal is aborted.
const controller = new AbortController();
await client.subscribe('orders', undefined, async (message) => {
  console.log(message.json());
}, { signal: controller.signal, onError: (message, error) => console.error(message.id, error) });

client.close();
```

## Batches

Many messages in one request, next to the single calls (optional): `publishBatch(topic, [{ body, contentType, groupId, deduplicationId }])`, `ackBatch(topic, messages)` and `changeVisibilityBatch(topic, messages, seconds)` resolve one result per item (`ok`, `status`, `id`, `errorCode`). Each item is answered as its single call, in order, and an item that failed comes back with its `status` and error code instead of throwing. The bodies go in base64, so any type fits. Details: [Messages in batch](https://github.com/rafaelpontani/taba/blob/main/doc/en-US/16-client-api.md#messages-in-batch-post-messagesbatch-ack-and-visibility).

## Configuration

One object (the client does not read environment variables: `TABA_*` belongs to the server).

| Field | Default | |
|---|---|---|
| `baseUrl` | required | `http://host:port` or `https://...` |
| `token` | none | `Authorization: Bearer` |
| `connectTimeout` | `5000` | milliseconds |
| `requestTimeout` | `30000` | milliseconds; `consume` uses `waitTime + 10 s` |
| `maxAttempts` | `3` | counting the first |
| `userAgent` | `taba-node/<version>` | |
| `serializer` | `JSON` | `{ stringify, parse }` for `publishJson` and `consumeJson` |
| `agent` | own keep-alive agent | an `http.Agent`/`https.Agent` (a proxy, for example) |

## What differs from the other connectors

- Names are `camelCase` and options are objects; cancellation is an `AbortSignal` (`{ signal }` in the last argument, or inside the options).
- `ack(topic, message)` and `changeVisibility(topic, message, seconds)` take the `Message` you received (or any `{ id, receiptToken }`).
- A `PATCH` field is sent when set; `undefined` leaves it out and `null` goes back to the server default.
- `consumeJson` is the `consume<T>` of the other languages: `context.body` is the parsed body; a body that is not JSON gives `client.invalid-response` and the message is not deleted.
- Errors are always a `TabaError` with `status`, `code`, `message`, `requestId`, `retryAfter`, `details`. Test the `code` (`client.network`, `client.timeout`, `topic.not-found`...), never the `message`. Cancelling with a signal rejects with the signal's `AbortError`, not a `TabaError`.
- TLS is Node's own, with `minVersion` TLS 1.2; the certificate is always verified.

## Tests

```
cd connectors/node
npm test
```

The tests start a local HTTP server that records every request and answers from a script (the equivalent of the C# `ScriptedHandler`). The **contract tests** run the same scenario against a real Taba (one node in Docker), on the oldest supported version: `contract/run.sh` (`../contract/run.sh` for all the connectors; [how it works](https://github.com/rafaelpontani/taba/blob/main/connectors/contract/README.md)).
