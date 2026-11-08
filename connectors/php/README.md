# taba/client (PHP)

The PHP client of [Taba](https://github.com/rafaelpontani/taba): the nine operations of the client API, `subscribe`, consuming with a handler and the call retries of the common contract. **No dependencies** (only the `curl` and `json` extensions that PHP ships with).

> Other languages: [Português (Brasil)](https://github.com/rafaelpontani/taba/blob/main/doc/pt-BR/21-conectores.md) · [all languages](https://github.com/rafaelpontani/taba/blob/main/doc/README.md)

- **PHP 5.6 or later** (it runs the same code on 5.6 and on 8.x). Needs `ext-curl` and `ext-json`; TLS 1.2 and the certificate check are on, and cannot be turned off.
- PHP runs one thread per request, so use **one client per process**: it keeps one connection open and reuses it.

```php
use Taba\Client;
use Taba\Config;

$client = new Client(new Config('http://localhost:8080', getenv('TABA_TOKEN')));

$client->createTopic(array('name' => 'orders', 'type' => 'queue'));
$client->publishJson('orders', array('order' => 9001));

// Consume with a handler: the message is deleted if the handler does not throw.
$client->consumeWith('orders', function (Taba\MessageContext $context) {
    echo $context->message->bodyText(), "\n";
}, array('waitTime' => 20));

// Or the whole loop, until the token is cancelled.
// $token = new Taba\CancelToken(); $token->cancelAfter(60);
// $client->subscribe('orders', function (Taba\Message $message) { ... }, null, $token);
```

## No automatic renewal of the invisibility

The other connectors renew the invisibility of a message while its handler runs, on a second thread. PHP has no threads, so `consumeWith` and `consumeJson` **cannot**: the message is taken for `visibilityTimeout` seconds (120 by default) and a handler that runs longer than that gives it back to the queue. Size the `visibilityTimeout` for the handler, or call `$context->changeVisibility($seconds)` from inside it.

## Installing (not in a registry yet)

Not in a registry yet (the identifiers are still open). Until then, each [Release](https://github.com/rafaelpontani/taba/releases) has the installable file, with its checksum in `SHA256SUMS` and its provenance (`gh attestation verify <file> -R rafaelpontani/taba`). `<release>` below is the version of the Release; the version in the file name is the connector's own.

Composer cannot read a package from a folder of a Git repository, so declare the file of the Release as a `package` repository in your `composer.json`:

```json
{
  "repositories": [
    { "type": "package", "package": {
        "name": "taba/client", "version": "0.1.0",
        "dist": { "url": "https://github.com/rafaelpontani/taba/releases/download/v<release>/taba-client-php-0.1.0.zip", "type": "zip" },
        "require": { "php": ">=5.6" },
        "autoload": { "classmap": ["src/"] } } }
  ],
  "require": { "taba/client": "0.1.0" }
}
```

Without Composer: unzip the file and `require 'taba-client-php/src/autoload.php';`.

## Declaring a topic

`createOrUpdateTopic($request, $recreate = null)` (`PUT /topics/{topic}`) creates the topic or brings it to the declared state, and returns a `TopicPut` with `topic` and `outcome` (`TopicPut::CREATED`, `UPDATED` or `RECREATED`). What is left out goes back to the server default (a full replacement, unlike the update); another type, `fifo` or `volatile` is the error `409 topic.immutable-mismatch` unless `recreate` is `if-empty` (only when the queue and its dead-letter are empty, `409 topic.not-empty` otherwise) or `always` (drops the messages; new `id`). After a `503` the call is repeated like the others. `deleteTopicIfEmpty($name)` only removes a queue with no message. `createTopic` does not change (`409` when it exists). Details: [Create or update a topic](https://github.com/rafaelpontani/taba/blob/main/doc/en-US/16-client-api.md#create-or-update-a-topic-put-topicstopic).

## Batches

Many messages in one request, next to the single calls (optional): `publishBatch($topic, array(new BatchMessage($body, array('contentType' => ...))))`, `ackBatch($topic, $messages)` and `changeVisibilityBatch($topic, $messages, $seconds)` return one `BatchResult` per item (`ok()`, `status`, `id`, `errorCode`). Each item is answered as its single call, in order, and an item that failed comes back with its `status` and error code instead of throwing. The bodies go in base64, so any type fits. Details: [Messages in batch](https://github.com/rafaelpontani/taba/blob/main/doc/en-US/16-client-api.md#messages-in-batch-post-messagesbatch-ack-and-visibility).

## Configuration

`new Config($baseUrl, $token)`, or `Config::from(array('baseUrl' => ..., 'token' => ..., ...))`. The client does not read environment variables: `TABA_*` belongs to the server.

| Field | Default | |
|---|---|---|
| `baseUrl` | required | `http://host:port` or `https://...` |
| `token` | none | `Authorization: Bearer` |
| `connectTimeout` | `5.0` | seconds |
| `requestTimeout` | `30.0` | seconds; `consume` uses `waitTime + 10` |
| `maxAttempts` | `3` | counting the first |
| `userAgent` | `taba-php/<version>` | |
| `caFile`, `caPath` | system's | an own CA bundle (a private CA, for example) |

## What differs from the other connectors

- Methods are `camelCase`; options go in an array (`array('waitTime' => 20)`) or in the matching object (`new ConsumeOptions(array(...))`); an unknown key is a `client.invalid-request` error, so a typo is not ignored. `ack` and `changeVisibility` take the `Message` you received, or its id plus the receipt token.
- In a `TopicUpdate`, a key left out is not sent and a key with `null` sends `null` (back to the server default) for `maxMessageBytes`, `ttl` and `maxDeliveries`.
- Errors are a `Taba\TabaException` with `status`, `errorCode`, `requestId`, `retryAfter`, `details`, `body` (PHP's own `getCode()` is an integer, so the `group.reason` code has its own property). Test the `errorCode`, never the message.
- Cancelling: nothing else runs while a request is in flight, so a `CancelToken` is checked by the transfer itself (about once a second) and between messages. Give it a deadline with `cancelAfter($seconds)`, or call `cancel()` from the handler. A cancelled call throws `Taba\Cancelled` (not a `TabaException`); `subscribe` returns without an exception.
- Message bodies are PHP strings: bytes for any type, the JSON text as it came for JSON (`$message->json()` is a `json_decode` of it).

## Tests

```
cd connectors/php
php tests/run.php
```

The unit tests use a fake `curl` that records every request and answers from a script (the equivalent of the C# `ScriptedHandler`), and a few tests talk to the built-in PHP server and to raw sockets to exercise the real `curl` transport. The **contract tests** run the same scenario against a real Taba (one node in Docker), on PHP 5.6: `contract/run.sh` (`../contract/run.sh` for all the connectors; [how it works](https://github.com/rafaelpontani/taba/blob/main/connectors/contract/README.md)).
