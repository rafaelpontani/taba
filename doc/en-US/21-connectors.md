---
title: Connectors
summary: Taba's client libraries — the common contract (the nine operations, retry, errors, body), the real state of each language (Rust, C#, Java, Python, Node.js, Go, PHP and Ruby ready; two planned), how to use and how to write a new one.
type: reference
status: partial
verified-on: 2026-11-07 @ 4fbb808
sources: [connectors/README.md, connectors/rust/client, connectors/rust/cli, connectors/csharp, connectors/java, connectors/node, connectors/python, connectors/go, connectors/php, connectors/ruby, .github/scripts/package-connectors.sh, connectors/omb, connectors/contract, examples/csharp-console]
---

# Connectors

> Read first: [16-client-api.md](16-client-api.md). The connectors **add no protocol**: the API is HTTP, and any language already talks to Taba through the HTTP client it has. A connector only saves the lines of building the request, repeating calls, reading the `ErrorResponse` and decoding the body.

## Real state

| Language | Folder | State | Publication |
|---|---|---|---|
| **Rust** (`taba-client`) | `connectors/rust/client/` | ✅ **Synchronous ready.** The `async` form is missing | ✅ [crates.io](https://crates.io/crates/taba-client) |
| **`taba-cli`** (on top of Rust) | `connectors/rust/cli/` | ✅ Ready ([20-cli.md](20-cli.md)) | ✅ [crates.io](https://crates.io/crates/taba-cli), binaries and Docker image |
| **C#** (`Taba.Client`) | `connectors/csharp/` | ✅ **Ready**: unit, integration against Compose and the contract tests | ✅ [nuget.org](https://www.nuget.org/packages/Taba.Client) |
| **Java** (`taba-client`) | `connectors/java/` | ✅ **Ready** (unit tests and the contract tests) | ⬜ Not published (D70.1) |
| **Python** (`taba-client`) | `connectors/python/` | ✅ **Ready** (unit tests and the contract tests) | ⬜ Not published (D70.1) |
| **Node.js** (`taba-client`) | `connectors/node/` | ✅ **Ready** (unit tests and the contract tests) | ⬜ Not published (D70.1) |
| **Go** (`taba`) | `connectors/go/` | ✅ **Ready** (unit tests and the contract tests) | ⬜ Not published (D70.1) |
| **PHP** (`taba/client`) | `connectors/php/` | ✅ **Ready** (unit tests and the contract tests; no automatic renewal of the invisibility, D68.3) | ⬜ Not published (D70.1, D70.2) |
| **Ruby** (`taba-client`) | `connectors/ruby/` | ✅ **Ready** (unit tests and the contract tests) | ⬜ Not published (D70.1) |
| **OpenMessaging Benchmark driver** (on top of Java) | `connectors/omb/` | ✅ **Ready** (unit and integration tests against a node) | ⬜ Not published (a tool of the repository) |
| **Terraform and OpenTofu provider** (on top of Go) | `connectors/terraform/` | ✅ **Ready**: queues, distributors, tokens, `create_or_update`; unit tests and acceptance with Terraform and OpenTofu ([40-terraform.md](40-terraform.md)) | 🟡 Packaged in the Release; the registries wait for the mirror repository (D211) |
| **Pulumi provider** (the Terraform provider bridged) | `connectors/pulumi/` | ✅ **Ready**: queues, distributors, tokens, `createOrUpdate`; SDKs for TypeScript, Python, .NET and Go; unit tests and acceptance with TypeScript and Python ([41-pulumi.md](41-pulumi.md)) | 🟡 Packaged in the Release; the mirror repository and the npm, PyPI and NuGet packages wait for the maintainer's setup (D213) |
| C, C++ | — | ⏸ **Deferred**, with no date (D68.4): the folders do not exist | — |

> The goal of **ten languages** is a plan, not reality: today **8 of 10** exist, and the other two (C and C++) are deferred (D68.4). The [roadmap](32-roadmap.md) has the order. Since the API is HTTP, the other languages use their own HTTP client until the connector is out.

## The three principles

1. **Simple to use:** publishing, consuming and confirming fit in a few lines, with no configuration besides the address (and the token).
2. **Simple to implement:** small enough for a person to understand the whole code in an afternoon. That is why the surface is minimal, there is **no external dependency** and the behavior is the same in all languages.
3. **Works on old versions** (up to **15 years**, from 2011 on), **only on platforms that speak TLS 1.2 or higher**, do `PATCH` and verify the certificate. What does not meet this is not supported: Taba does not open a plain HTTP port or old TLS for them.

Beyond this, the connector **does not try to be clever**: it keeps no state, does no publication buffering, no reordering. The server decides (quorum, FIFO, fair delivery).

> **Reach.** The 15 years apply to the languages not done yet (C and C++) and to C#, Rust, Go and PHP (Go 1.7 is from 2016 and PHP 5.6 from 2014; Ruby 2.1 is from 2013, the 2.0 of the first plan has no Docker image left to test on, D68.3). For **Java 11 (2018), Python 3.9 (2020) and Node.js 18 (2022)** the base was set higher (D68.2): the rule of the table below gave Python 2.7.9/3.4.3 and Node.js 0.12, which would forbid `async`/`await`, `asyncio`, classes and type hints, and which the tools of today no longer run.

## The surface: nine operations

The same in all languages, with names adapted (`publish`, `Publish`, `publish_message`…).

| Operation | Route | Returns |
|---|---|---|
| `createTopic(request)` | `POST /topics` | `Topic` |
| `getTopic(name)` | `GET /topics/{topic}` | `Topic` |
| `listTopics(limit, cursor, name)` | `GET /topics` | `Page<Topic>` (and an iterator over all pages, where the language allows) |
| `updateTopic(name, request)` | `PATCH /topics/{topic}` | `Topic` |
| `deleteTopic(name)` | `DELETE /topics/{topic}` | — |
| `publish(topic, body, options)` | `POST …/messages` | `PublishResponse` (or `DistributorPublishResponse`) |
| `consume(topic, options)` | `GET …/messages` | list of `Message` (empty if none) |
| `ack(topic, message)` | `DELETE …/messages/{id}?receiptToken=` | — |
| `changeVisibility(topic, message, seconds)` | `PUT …/visibility?receiptToken=` | `VisibilityResponse` |

**In batch** (optional, next to the single calls, [D189](30-decisions.md)): `publishBatch(topic, messages)`, `ackBatch(topic, messages)` and `changeVisibilityBatch(topic, messages, seconds)` call `POST …/messages/batch`, `/ack` and `/visibility` and return one `BatchResult` per item, in order (`status`, `id`, `duplicate`, `matched`, `invisibleUntil`, `errorCode`, `errorMessage` and an `ok`/`isOk`); an item that failed does not throw. A message of `publishBatch` (`BatchMessage`) has the body bytes and the same options as `publish` (`contentType`, `groupId`, `deduplicationId`); the bytes go in base64, so any type fits. A network failure is not repeated, like the single publication. In C# they are in the interface `ITabaBatchClient`, which `TabaClient` also implements, so whoever implements `ITabaClient` is not broken. Names: `publish_batch`, `ack_batch`, `change_visibility_batch` in Rust, Python and Ruby; `publishBatch`, `ackBatch`, `changeVisibilityBatch` in PHP; `PublishBatch`, `AckBatch`, `ChangeVisibilityBatch` in Go; `PublishBatchAsync` (and `PublishBatch`) in C#.

**Declaring a topic** (optional, next to `createTopic`, which does not change; [D210](30-decisions.md#declaring-a-topic-d210-d214-to-d216)): `createOrUpdateTopic(request, recreate)` calls [`PUT /topics/{topic}`](16-client-api.md#create-or-update-a-topic-put-topicstopic) with the name in the encoded path and the body of `createTopic` without `name`, and returns the `Topic` and the **outcome** (`created` on `201`, `updated` on `200`, `recreated` on `201` with `Taba-Topic-Recreated: true`), so the caller looks at no status or header. What is left out goes back to the server default. `recreate` (`if-empty` or `always`) goes in the query only when given; another value is `client.invalid-request` and nothing is sent. `409 topic.immutable-mismatch` and `topic.not-empty` are errors with the code. After a `503` the `PUT` is repeated like the other calls, with no `recover` rule (it is idempotent). The removal "only if empty" (`DELETE ?ifEmpty=true`) goes in the form of each language without changing `deleteTopic`. Names: `create_or_update_topic` and `delete_topic_if_empty` in Rust; `CreateOrUpdateTopicAsync` (and the synchronous one) and `DeleteTopicIfEmptyAsync` in C#, in the interface `ITabaTopicPutClient`, which `TabaClient` implements, so whoever implements `ITabaClient` is not broken; `createOrUpdateTopic` and `deleteTopicIfEmpty` in Java (and the `…Async` forms), Go (`CreateOrUpdateTopic`, `DeleteTopicIfEmpty`) and PHP; `createOrUpdateTopic` and `deleteTopic(name, { ifEmpty: true })` in Node.js; `create_or_update_topic` and `delete_topic(name, if_empty=True)` in Python (also on `AsyncClient`) and Ruby (`if_empty: true`).

**Outside the surface, on purpose:** tokens (`/tokens`: the operator creates them, with `curl` or `taba-cli token`), the operations API and the cluster one.

**Additional** (where the language allows): `subscribe(handler)` (continuous long polling, calls the handler and confirms by itself **if it ends without an error**); `publishJson(topic, object)` (serializes and publishes as `application/json`) and `publishRawJson(topic, text)`; `consume` with a *handler* (the `delete` is `true` by default: the handler ends without an error, the message is deleted) and `consume<T>` (body already deserialized); automatic renewal of the invisibility while the handler runs. **Asynchronous** where there is an async model (`Task` in C#, `async`/`await` in Rust and Python 3.5+…), with the same contract.

**`publish`** receives the **bytes** and the `Content-Type`; the options are `groupId`, `deduplicationId`, `contentType` and `requestId`. The connector **does not validate the body**. **`consume`** has `maxMessages`, `visibilityTimeout`, `waitTime`, `autoCreate` (with `volatile`, `idleTimeout`, `fifo`, `tags`); it sends **only what the person informed** and **applies no defaults of its own**: whatever is missing gets the server default.

### Configuration

One object (no environment variables: `TABA_*` belongs to the server):

| Field | Default | Description |
|---|---|---|
| `baseUrl` | — (required) | Address of the client API load balancer (`http://host:port` or `https://…`) |
| `token` | none | Goes in `Authorization: Bearer` |
| `connectTimeout` | 5 s | Open the connection |
| `requestTimeout` | 30 s | Ordinary call |
| `maxAttempts` | 3 | Attempts, counting the first |
| `userAgent` | `taba-<language>/<version>` | — |

## The common contract

Identical in **all** connectors; it is what allows a single conformance suite.

**Long polling.** In `consume`, the read timeout is **`waitTime + 10 s`**, never the `requestTimeout` (a `waitTime` of 20 s with a limit of 30 s would drop the call for nothing). A timeout in `consume` is **not safe to repeat blindly** (the node may have captured messages): the error rises and the messages come back by themselves at the end of the `visibilityTimeout`.

**Retrying calls:**

| Response | Retry? |
|---|---|
| `429`, `503` | **Yes**, up to `maxAttempts`, waiting for the `Retry-After` (without it: 1 s, 2 s, 4 s). On a `503` of an operation that creates or deletes a topic, `409 topic.already-exists` and `404` on the retry count as success (not on `createOrUpdateTopic`, whose `PUT` is simply repeated) |
| `502`, `504` | **Only** on publishing to a **FIFO** queue (the primary deduplicates). Outside it, **no** (it would duplicate) |
| Network failure **before** sending the body (refused, DNS) | Yes, like a `503` |
| Network failure **after** sending the body | **No** (it is not known whether it was applied) |
| Any `4xx`, except `429` | **Never** |

**The delivered message.** The connector always returns the body as **bytes**:

| `contentType` / `bodyEncoding` | What comes in `body` |
|---|---|
| `application/json`, `*/*+json` | The **JSON text** of the `body` field, as is (the application decodes it) |
| `text/*` in UTF-8 | The string, in UTF-8 |
| `bodyEncoding: base64` | The bytes already **decoded** |

Plus `id`, `contentType`, `receiptToken`, `deliveryCount`, `publishedAt`, `invisibleUntil` and, when present, `groupId`, `deduplicationId`, `deadLetteredAt`, `deadLetterReason`. **Unknown fields in the response are ignored.**

**Error.** A single form per connector (exception, `Result`, return code), with the fields of `ErrorResponse`: `status`, `code`, `message`, `requestId`, `retryAfter`, `details`. The application tests the **`code`**, never the `message`. The client's own failures have `status` zero and code **`client.network`**, **`client.timeout`**, **`client.invalid-config`**, **`client.invalid-response`** (and `client.invalid-request`, when the application's object cannot be serialized): codes that **only exist in the client**.

**Headers.** Sends `Authorization`, `Content-Type`, `Accept: application/json`, `User-Agent` and, if informed, `X-Request-Id` and `traceparent`; reads `Retry-After` and `X-Request-Id`.

**Connection and threads.** HTTP/1.1, with a persistent connection when the library offers it at no cost; **does not require HTTP/2**. The client is **safe for concurrent use** in Java, C#, Go, Rust and Node; in C, C++ and PHP, **one client per thread** (per process, in PHP) applies; Python and Ruby keep one connection per thread, so one client can be shared.

## No dependencies: the minimal JSON reader

An external dependency is almost always what breaks on an old version. The rule: **only the language's standard library**, except `libcurl` in C and `rustls` in Rust.

- **HTTP:** the language's own client. Three do not have one: **Rust** (minimal HTTP/1.1 implementation over `TcpStream`, with `rustls`), **C++** (uses `libtaba`) and **C** (`libcurl`).
- **JSON:** what the connector reads is **small and fixed** (the DTOs and the `ErrorResponse`). Instead of depending on a library, each connector carries a **minimal JSON reader** (150 to 250 lines) that walks the text once, knows how to **skip** any value, delivers the fields of an object as name → text-fragment pairs and decodes *strings* (escapes and `\uXXXX`). This solves the `body` field, which is **any** JSON: the connector returns the fragment **without interpreting it**.
- **Base64:** standard library, or ~30 lines.

## Rust (`taba-client`)

- **Own HTTP/1.1** over `std::net::TcpStream`; **TLS 1.2+** through `rustls` with the system roots (and `ca-file`); own JSON and base64. Dependencies: `rustls`, `rustls-native-certs`; `serde` + `serde_json` **only** with the optional `serde` feature.
- `Client` is `Send + Sync`. Retry according to the contract.

```rust
use taba_client::{Client, Config, ConsumeOptions, PublishOptions, TopicRequest};

let mut config = Config::new("http://127.0.0.1:8080");
config.token = std::env::var("TABA_TOKEN").ok();
let client = Client::new(config)?;

client.create_topic(&TopicRequest { name: "pedidos".into(), topic_type: "queue".into(), ..Default::default() })?;
client.publish("pedidos", br#"{"pedido": 9001}"#, &PublishOptions::default())?;

let options = ConsumeOptions { wait_time: Some(20), ..Default::default() };
for message in client.consume("pedidos", &options)? {
    println!("{}", String::from_utf8_lossy(&message.body));
    client.ack("pedidos", &message.id, &message.receipt_token)?;
}
```

With the `serde` feature: `publish_object` and `consume_typed::<T>` (long polling, body already deserialized, **deletes if the handler returns `Ok`**, renews the invisibility while it runs). Without `serde`: `consume_with`. Tests: `cd connectors/rust && cargo test --workspace --all-features`.

## C# (`Taba.Client`)

- **Targets:** `net461` (.NET Framework 4.6.1 to 4.8), `net5.0`, `net6.0`, `net8.0` and, with SDK ≥ 10, `net10.0`. **C# 7.3** on all targets. **No dependencies** (`HttpClient`, `Task`); own minimal JSON.
- **Packages:** `Taba.Client` (the `TabaClient`), `Taba.Client.Abstractions` (`ITabaClient`, models, `TabaException`) and `Taba.Client.Extensions.DependencyInjection` (the `AddTabaClient`, **separate** so the main one stays dependency-free).
- **Async first:** the nine operations have `…Async` (the implementation) and the synchronous one. All accept `CancellationToken`. A single `TabaClient` serves several tasks.

```csharp
var config = new TabaConfig("http://localhost:8080") { Token = Environment.GetEnvironmentVariable("TABA_TOKEN") };
using (var client = new TabaClient(config))
{
    await client.CreateTopicAsync(new TopicRequest { Name = "pedidos", Type = "queue" });
    await client.PublishJsonAsync("pedidos", new { pedido = 9001 });

    foreach (Message message in await client.ConsumeAsync("pedidos", new ConsumeOptions { WaitTime = 20 }))
    {
        Console.WriteLine(message.BodyText);
        await client.AckAsync("pedidos", message);
    }
}
```

- **`ConsumeAsync<T>(topic, handler)`:** 20 s long polling, one message at a time, 120 s of invisibility, deserialized body, **ack only if the handler ends without an exception**, automatic renewal of the invisibility (every third).
- **`SubscribeAsync`:** the whole loop.
- **Dependency injection:** `builder.Services.AddTabaClient("Taba")` reads `BaseUrl`, `Token`… from the `appsettings.json` section and registers `ITabaClient` as a *singleton*.
- **Tests:** `cd connectors/csharp && dotnet test Taba.Client.Tests` (NUnit; the integration ones only run with `TABA_URL`). `connectors/csharp/scripts/integration.sh` starts Compose, creates tokens and queues and runs everything. Besides it, every connector (this one too) runs the **contract tests**: `connectors/csharp/contract/run.sh`, or `connectors/contract/run.sh` for all eight ([contract tests](#the-contract-tests)).
- **Example:** [`examples/csharp-console/`](../../examples/csharp-console/): creates a topic, publishes, consumes with long polling and confirms; reads the address and the root token from environment variables.

## Java (`io.github.rafaelpontani:taba-client`)

- **Java 11 or later** (`java.net.http.HttpClient`, HTTP/1.1, redirects off). **No runtime dependencies**: own minimal JSON; JUnit only in the tests. Maven project in `connectors/java/`.
- **Blocking and `...Async`** (`CompletableFuture`, on a thread pool the client owns or on `TabaConfig.executor`). A blocking call is cancelled by interrupting its thread and an async one by `future.cancel(true)`; both end with a `CancellationException`, never with a `TabaException`. `TabaException` is unchecked. Handlers are a `ThrowingConsumer` (a checked exception comes out as `HandlerFailedException`).
- **JSON:** the JDK has no JSON library, so `publishJson` and the typed `consume` need a `TabaJsonSerializer` in the configuration (four lines with Jackson, in the README); without it they fail with `client.invalid-config`.
- **`subscribe`** returns a `Subscription` (`AutoCloseable`) that runs the loop on its own thread; `close()` stops it, interrupting a long polling and never the handler.

```java
TabaConfig config = TabaConfig.of("http://localhost:8080").token(System.getenv("TABA_TOKEN"));
try (TabaClient client = new TabaClient(config)) {
    client.createTopic(TopicRequest.queue("pedidos"));
    client.publishText("pedidos", "ola");

    for (Message message : client.consume("pedidos", new ConsumeOptions().waitTime(20))) {
        System.out.println(message.getBodyText());
        client.ack("pedidos", message);
    }
}
```

Tests: `cd connectors/java && mvn test` (JUnit 5 and the JDK's own `com.sun.net.httpserver`); CI runs JDK 11 and 21.

## OpenMessaging Benchmark driver (`connectors/omb`)

- A driver for the [OpenMessaging Benchmark](https://github.com/openmessaging/benchmark) (OMB) written on the Java connector, on **Java 11**; it is a tool to measure Taba with the same workloads the OMB runs against Kafka, NATS or RabbitMQ, and it goes into no package. The OMB commit it is built against is in `connectors/omb/OMB_VERSION` (the last one that still builds on Java 11).
- **Mapping:** an OMB topic is a Taba **distributor** named like the topic, with the prefix `<topic>-`; each subscription is a **queue** `<topic>-<subscription>` created (volatile, with the longest `idleTimeout`, as a safety net) before the first publication, and deleted when the driver closes. Consumers of one subscription compete for its messages. The OMB partition count creates nothing in Taba.
- **Key and order:** the OMB message key is the `groupId`. `ordering: none` (default) uses an ordinary queue; `ordering: per-key` uses a FIFO queue, and a message without a key takes `p0`..`p<partitions-1>` in rotation.
- **Batches:** the producer keeps the messages in a buffer and sends them in one `POST …/messages/batch` when `producer.batchSize` (100) fill or `producer.lingerMs` (1 ms) pass, the same idea as the `linger.ms` of the Kafka producer; the time of a message is taken when it enters the buffer, so the wait counts in its latency. Like Kafka, the linger only sends a partial batch when a request is free: with every request in flight the batch keeps filling (otherwise a saturated producer sends ever smaller batches, each costing a request, and the rate falls in a spiral: measured, 600 messages per second against a node that takes 45 thousand). `producer.inFlight` (1000) counts requests; `bench/run.sh` uses 64, and the consumer reads 500 per `GET`. The consumer acks what one `GET` delivered in one `POST …/messages/ack` (`consumer.ackBatch`, `true`). `consumer.pollers` (1) threads long-poll the same queue at the same time, each with its own `GET`, the idea of the background fetch of the Kafka consumer: while one waits for its answer another is already asking; `bench/run.sh` uses 16 (with 4, the four `GET`s in flight, not the node, held one node at about 20 thousand messages per second while the node had cores to spare, and the rate controller of the OMB took the rest as a backlog). `batchSize: 1` and `ackBatch: false` go back to one request per message.
- **Publish time:** Taba keeps no attribute besides the body, so the producer writes its clock in the first 8 bytes of the body and the consumer removes it; the body in the server is 8 bytes larger than the OMB payload, and a result must say so.
- **Build and tests:** `connectors/omb/build.sh` (OMB `driver-api`, the connector and the driver in a Java 11 container; one JAR with the connector inside, `target/omb-driver-taba-<version>-all.jar`) and `connectors/omb/test.sh` (one node in Docker, then the unit and the integration tests). The CI job `connector-omb` runs the build and the unit tests; `connector-contract` also runs `test.sh`. Configuration: `connectors/omb/driver-taba.yaml`. Cases: UC-CNT-036 to 041. The script that runs it against other systems is `bench/run.sh` ([38-benchmarks.md](38-benchmarks.md)).

## Node.js (`taba-client`)

- **Node.js 18 or later**, CommonJS (works with `require` and `import`), types in `index.d.ts`, **no dependencies** (`node:http` and `node:https`).
- **Promises only:** Node has no synchronous HTTP, so there is no `...Sync` twin. Cancellation is an `AbortSignal`; options are objects; names are `camelCase`. In a `PATCH`, `undefined` leaves a field out and `null` goes back to the server default.
- `consumeWith(topic, options, handler)` and `consumeJson` (the `consume<T>` of the other languages: `context.body` is the parsed body); `subscribe(topic, options, handler, { signal, onError })` ends without an error when the signal is aborted.
- A pooled connection the server closed while idle (`ECONNRESET` before any byte of the answer) is repeated like a refused connection (PRB-081).

```js
const { TabaClient } = require('taba-client');

const client = new TabaClient({ baseUrl: 'http://localhost:8080', token: process.env.TABA_TOKEN });
await client.createTopic({ name: 'pedidos', type: 'queue' });
await client.publishJson('pedidos', { pedido: 9001 });
await client.consumeWith('pedidos', { waitTime: 20 }, async ({ message }) => console.log(message.bodyText));
client.close();
```

Tests: `cd connectors/node && npm test` (`node:test` and a local HTTP server); CI runs Node 18 and 24.

## Python (`taba-client`, `import taba`)

- **Python 3.9 or later**, **no dependencies**, typed (`py.typed`). `taba.Client` is synchronous (one connection per thread, so it is safe for concurrent use); `taba.aio.AsyncClient` is the `asyncio` one. The standard library has no asynchronous HTTP client, so the latter runs the same transport in worker threads (PRB-080).
- `snake_case`; options are keyword arguments (`client.consume("q", wait_time=20)`); `iter_topics()` is a generator; `UNSET` and `None` tell `TopicUpdate` to leave a field out or send `null`. `consume_with` and `consume_json` are the handler forms.
- **Cancelling:** the synchronous client takes a `CancelToken` (it closes the connection in flight, so a long polling does not wait for its timeout) and raises `taba.Cancelled`; `subscribe` returns without an exception. The `asyncio` client is cancelled the `asyncio` way (`task.cancel()`), which raises `asyncio.CancelledError`.

```python
from taba import Client, TopicRequest

with Client("http://localhost:8080", token="...") as client:
    client.create_topic(TopicRequest("pedidos", "queue"))
    client.publish_json("pedidos", {"pedido": 9001})
    client.consume_with("pedidos", lambda context: print(context.message.body_text), wait_time=20)
```

Tests: `cd connectors/python && python -m unittest discover -s tests -t .` (the standard `unittest` and a local HTTP server); CI runs Python 3.9 and 3.13.

> **Java, Node.js and Python are not a copy of the C# one.** The contract (nine operations, retry, errors, body) is the same; the forms are each language's own, recorded in D68.2. All three run the contract tests against a real Taba ([contract tests](#the-contract-tests), D183), and none is published (PRB-079).

## Go (`github.com/rafaelpontani/taba/connectors/go`, `package taba`)

- **Go 1.7 or later**, **standard library only** (`net/http`, `encoding/json`); it builds and tests on 1.7 and on current versions. No `go.mod` features beyond the module line, so it works in a `GOPATH` too.
- `taba.Client` is safe for concurrent use. **Every call takes a `context.Context`** and returns `(value, error)`; cancelling the context ends the call in flight (a long polling does not wait for its timeout) and the call returns the context's own error, never a `*taba.Error`. `Subscribe` returns `nil` when its context is cancelled.
- Optional fields are pointers (`taba.Int(20)`, `taba.Bool(true)`), so `nil` means "not set" and the server default applies; in a `TopicUpdate`, `taba.SetInt(n)` / `taba.ClearInt()` / `nil` are "send `n`" / "send `null`" / "leave out". The error is a `*taba.Error` (`Status`, `Code`, `Message`, `RequestID`, `RetryAfter`, `Details`, `Body`).
- `ConsumeWith(ctx, topic, options, func(*MessageContext) error)`: a `nil` return deletes the message (unless `Delete = false`), an error keeps it and is returned; the invisibility is renewed by another goroutine. Go 1.7 has no generics, so there is no typed `consume<T>`: the handler reads the body with `Decode(&v)`.
- **Only in Go so far** (the Terraform provider, and the Pulumi one through it, use them; PRB-109): the administration of the access tokens (`CreateToken`, `GetToken`, `ListTokens`/`ListAllTokens`, `UpdateToken`, `RevokeToken`; a `503 cluster.quorum-unavailable` on `CreateToken` comes back without a new attempt, because the owner may have created the token), and `Config.RetryMaxWait`: the total of the waits between the attempts of one call; no attempt is made whose wait would pass it, and `MaxAttempts` still bounds them (D220).

```go
client, _ := taba.NewClient(taba.Config{BaseURL: "http://localhost:8080", Token: os.Getenv("TABA_TOKEN")})
ctx := context.Background()
client.CreateTopic(ctx, &taba.TopicRequest{Name: "pedidos", Type: "queue"})
client.PublishJSON(ctx, "pedidos", map[string]int{"pedido": 9001})
client.ConsumeWith(ctx, "pedidos", &taba.ConsumeOptions{WaitTime: taba.Int(20)}, func(c *taba.MessageContext) error {
	fmt.Println(c.Message.BodyText())
	return nil
})
```

Tests: `cd connectors/go && go test -race .` (the standard `testing` and `httptest`).

## PHP (`taba/client`, namespace `Taba`)

- **PHP 5.6 or later**, `ext-curl` and `ext-json` only; the same code runs on 5.6 and on 8.x. TLS 1.2 and the certificate check are always on. Autoload by Composer (`classmap`) or by `src/autoload.php`.
- **One thread**, so **one client per process**, and **no automatic renewal of the invisibility** while a handler runs: `consumeWith` takes the message for `visibilityTimeout` (120 s by default) and the handler can call `$context->changeVisibility()`. This is the only place where a connector does less than the contract allows (D68.3); the contract scenario prints that line as `SKIP`.
- `camelCase` methods; options are arrays (`array('waitTime' => 20)`) or the matching objects, and an unknown key is a `client.invalid-request` error. The error is a `Taba\TabaException` with `status`, `errorCode` (PHP's `getCode()` is an integer), `requestId`, `retryAfter`, `details`, `body`.
- **Cancelling:** a `CancelToken` is checked by the `curl` transfer (about once a second) and between messages, and has `cancelAfter($seconds)`, since nothing else runs while a request is in flight; a cancelled call throws `Taba\Cancelled`.

```php
$client = new Taba\Client(new Taba\Config('http://localhost:8080', getenv('TABA_TOKEN')));
$client->createTopic(array('name' => 'pedidos', 'type' => 'queue'));
$client->publishJson('pedidos', array('pedido' => 9001));
$client->consumeWith('pedidos', function (Taba\MessageContext $context) {
    echo $context->message->bodyText();
}, array('waitTime' => 20));
```

Tests: `cd connectors/php && php tests/run.php` (a small runner of its own, no PHPUnit: a fake `curl` for the requests, and the built-in PHP server and raw sockets for the real transport).

## Ruby (`taba-client`, `require 'taba'`)

- **Ruby 2.1 or later**, **standard library only** (`net/http`, `json`); the same code runs on 2.1 and on 3.x. `Taba::Client` is safe for concurrent use (one connection per thread). The 2.0 of the first plan could not be tested (its Docker image is gone), so the base is 2.1 (D68.3).
- `snake_case`; options are keyword arguments, handlers are blocks (`consume_with`, `consume_json`, `subscribe`); `each_topic` is an `Enumerator` over the pages. In a `PATCH`, a key left out is not sent and `nil` sends `null`. The error is a `Taba::Error` (`status`, `code`, `reason`, `request_id`, `retry_after`, `details`, `body`).
- **Cancelling:** a `Taba::CancelToken` cancelled from another thread ends the call in flight at once (the exchange runs in a helper thread when a token is given, because closing a socket does not wake a blocked read on every Ruby version) and raises `Taba::Cancelled`. `Net::HTTP` repeats an idempotent request by itself after a timeout; the connector switches that off, so only the retries of the contract happen.

```ruby
client = Taba::Client.new('http://localhost:8080', token: ENV['TABA_TOKEN'])
client.create_topic(name: 'pedidos', type: 'queue')
client.publish_json('pedidos', pedido: 9001)
client.consume_with('pedidos', wait_time: 20) { |context| puts context.message.body_text }
```

Tests: `cd connectors/ruby && ruby -Ilib -Itest -e 'Dir["test/*_test.rb"].each { |f| require File.expand_path(f) }'` (minitest and a local TCP server).

> **Go, PHP and Ruby are not a copy of the C# one either** (D68.3): each uses its language's own forms, and the three run the contract tests against a real Taba, on Go 1.7, PHP 5.6 and Ruby 2.1. None is published (PRB-079).

## Using the connectors before they are published

Until a connector reaches its registry (PRB-079), its package is a **file attached to every GitHub Release** (D70.3), built by `.github/scripts/package-connectors.sh`, listed in `SHA256SUMS` and attested like the binaries:

| Connector | File of the [Release](https://github.com/rafaelpontani/taba/releases) | Install |
|---|---|---|
| Node.js | `taba-client-<v>.tgz` | `npm install <URL of the file>` |
| Python | `taba_client-<v>-py3-none-any.whl` | `pip install <URL of the file>` |
| Ruby | `taba-client-<v>.gem` | download, then `gem install ./taba-client-<v>.gem` |
| Java | `taba-client-<v>.jar` | `mvn install:install-file`, or the jar on the classpath |
| PHP | `taba-client-php-<v>.zip` | a Composer `package` repository whose `dist.url` is the file, or unzip and `require src/autoload.php` |
| Go | no file | `go get github.com/rafaelpontani/taba/connectors/go@release` (the module path is the repository) |

`<v>` is the version of the **connector** (the connectors version on their own, D70.2), not the Release's. Without a Release, some can also be installed from the source: Python (`pip install "git+https://github.com/rafaelpontani/taba.git@release#subdirectory=connectors/python"`), Ruby (Bundler, `glob: 'connectors/ruby/*.gemspec'`) and pnpm (`#path:/connectors/node`); npm and Composer cannot read a folder of a repository. The exact commands are in the README of each connector.

## The deferred ones: base version and publication

C and C++ are **deferred with no date** (D68.4): this is the design kept for when they are taken up again, not a commitment (proposals, subject to D68, D70.1, D70.2). Java, Python and Node.js left this table when they were done (D68.2), and Go, PHP and Ruby after them (D68.3). "Base version" is the oldest one that speaks TLS 1.2, has an HTTP client with `PATCH` and verifies the certificate.

| Language | Base version | Why | HTTP / JSON | Publication |
|---|---|---|---|---|
| C | **C89** + `libcurl` ≥ 7.34 | `libcurl` 7.34 is the first that requires TLS 1.2 and does `PATCH` | `libcurl` / minimal reader | GitHub releases |
| C++ | **C++03** | A header over `libtaba` (C) | via `libtaba` | GitHub releases |

**Code rules for old versions:** no syntax or library later than the base version (no `?.` in C# 7.3; no `record` or text block in Java 11; no `match` in Python 3.9; no `Object.groupBy` in Node.js 18); CI compiles and tests **on the base version and the latest**; text in **UTF-8 always**; the connector **does not turn off certificate verification** and **does not depend on the local clock**. Taba does **not** accept `X-HTTP-Method-Override`.

## How to write a new connector

1. **Read** [16-client-api.md](16-client-api.md) and this contract; **use `taba-cli`** as the executable reference (each command is an API call, and what it does is what the connector repeats).
2. **Self-contained folder** `connectors/<language>/`: `README.md` (installation, a 10-line example, base version), the language's packaging, `src/`, `test/` and `examples/`. No folder imports code from another (except `cpp/` over `c/`).
3. **Implement** the nine operations, the retry in the table above, the minimal JSON reader and the `client.*` errors.
4. **Test**: unit tests with a fake transport (Rust has `taba_client::testing`; C# has `ScriptedHandler`) checking the **built request** and the reading of ready-made responses; integration against a one-node Taba (`replication.factor=1`), or Compose; and a **driver** for the contract tests ([contract tests](#the-contract-tests)): the scenario is already written, the connector only needs to run it.
5. **Cite the use cases** in the tests on a `UC: UC-CNT-NNN` line (the server's `uc_coverage` reads the citations from the connector folders too) ([26-use-cases.md](26-use-cases.md)).

> Planned and **not yet existing**: the request/response fixtures and the JSON reader documents of the conformance suite ([32-roadmap.md](32-roadmap.md)); the scenarios and the drivers already exist ([contract tests](#the-contract-tests)). The OpenAPI documents of the client API exist ([15-api-overview.md](15-api-overview.md)) and the connectors can be generated from them or checked against them; besides them, the "source of truth" of the contract is this text, `taba-cli` and the tests of the two connectors.

## The contract tests

The same scenario runs against a real Taba through **every connector** (C#, Java, Node.js, Python, Rust, Go, PHP and Ruby), each with a small *driver* written in its own language that uses only the public API ([connectors/contract/README.md](https://github.com/rafaelpontani/taba/blob/main/connectors/contract/README.md) has the scenario language and the operations). `connectors/contract/run.sh` starts **one Taba node** in Docker (the one-node profile) and runs, one after the other, the driver of each connector **in the container of its oldest supported runtime** (Node 18, Python 3.9, JDK 11, .NET 8, Rust stable, Go 1.7, PHP 5.6, Ruby 2.1); each connector can also run alone (`connectors/<language>/contract/run.sh`).

- **`scenarios/core.scn`**, in every connector: topics (and their `404`, `409` and `400`), publish, consume, `changeVisibility` and `ack`, and the same in batch (`publishBatch`, `settleBatch`: one result per item, a bad item and an old receipt failing alone, the order of a FIFO group); text with accents, JSON and bytes coming back byte for byte; FIFO (duplicate and order); distributor; consuming with a handler (deleted, failed, kept and **renewal of the invisibility** while a slow handler holds the message); `subscribe`; a long polling **cancelled** from outside (not in Rust, which is synchronous: the line is printed as `SKIP`; and the `slow` handler that needs the **renewal** is also a `SKIP` in PHP, which has no threads); a wrong token and a refused connection.
- **`scenarios/cross-*.scn`**, the eight together: each connector publishes to a distributor and each one reads its own queue, finding what the eight sent, exactly.
- **Not covered**: `503` and the retries with a node going down, and the cluster behavior (one node cannot show them): they stay with `docker/validate.sh` and the unit tests of each connector. The job `connector-contract` of `ci.yml` runs it on every pull request to `release`.

## Pending items in this area

D68 (confirm the base versions of the languages still to do: C and C++), D70.1 (organization and package identifiers in each registry), D70.2 (PHP mirror; versions independent of the server's): [31-pending-items.md](31-pending-items.md).

## Sources and evidence

- `connectors/rust/client/src` (`client`, `http`, `json`, `base64`, `transport`, `testing`), `connectors/rust/client/README.md`, `connectors/csharp/Taba.Client*`, `connectors/csharp/Taba.Client.Tests`, `connectors/java/src`, `connectors/node/src`, `connectors/python/taba`, `connectors/go`, `connectors/php/src`, `connectors/ruby/lib`, `examples/csharp-console`, `connectors/README.md`.
- Use cases: UC-CNT (43 cases, level `K`; connectors; 042 and 043: declaring a topic); see [26a](26a-implemented-use-cases.md) and [26b](26b-planned-use-cases.md). Tests: `connectors/rust/client/**/tests.rs`, `connectors/csharp/Taba.Client.Tests`, `connectors/java/src/test`, `connectors/node/test`, `connectors/python/tests`, `connectors/go/*_test.go`, `connectors/php/tests`, `connectors/ruby/test`, `connectors/omb/src/test`; contract tests: `connectors/contract` (`run.sh`, `scenarios/`) and the drivers in `connectors/{csharp,java,node,python,rust,go,php,ruby}/contract`.
- History: `19-conectores.md`, `19a-conectores-linguagens.md`.
