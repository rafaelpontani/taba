---
title: Connectors
summary: Taba's client libraries — the common contract (the nine operations, retry, errors, body), the real state of each language (Rust, C#, Java, Python and Node.js ready; five planned), how to use and how to write a new one.
type: reference
status: partial
verified-on: 2026-10-27 @ 9511db3
sources: [connectors/README.md, connectors/rust/client, connectors/rust/cli, connectors/csharp, connectors/java, connectors/node, connectors/python, connectors/contract, examples/csharp-console]
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
| C, C++, PHP, Ruby, Go | — | ⬜ **Planned** (the folders do not exist) | — |

> The goal of **ten languages** is a plan, not reality: today **5 of 10** exist. The [roadmap](32-roadmap.md) has the order. Since the API is HTTP, the other languages use their own HTTP client until the connector is out.

## The three principles

1. **Simple to use:** publishing, consuming and confirming fit in a few lines, with no configuration besides the address (and the token).
2. **Simple to implement:** small enough for a person to understand the whole code in an afternoon. That is why the surface is minimal, there is **no external dependency** and the behavior is the same in all languages.
3. **Works on old versions** (up to **15 years**, from 2011 on), **only on platforms that speak TLS 1.2 or higher**, do `PATCH` and verify the certificate. What does not meet this is not supported: Taba does not open a plain HTTP port or old TLS for them.

Beyond this, the connector **does not try to be clever**: it keeps no state, does no publication buffering, no reordering. The server decides (quorum, FIFO, fair delivery).

> **Reach.** The 15 years apply to the languages not done yet and to C# and Rust. For **Java 11 (2018), Python 3.9 (2020) and Node.js 18 (2022)** the base was set higher (D68.2): the rule of the table below gave Python 2.7.9/3.4.3 and Node.js 0.12, which would forbid `async`/`await`, `asyncio`, classes and type hints, and which the tools of today no longer run.

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
| `429`, `503` | **Yes**, up to `maxAttempts`, waiting for the `Retry-After` (without it: 1 s, 2 s, 4 s). On a `503` of an operation that creates or deletes a topic, `409 topic.already-exists` and `404` on the retry count as success |
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

**Connection and threads.** HTTP/1.1, with a persistent connection when the library offers it at no cost; **does not require HTTP/2**. The client is **safe for concurrent use** in Java, C#, Go, Rust and Node; in C, C++, Python, PHP and Ruby, **one client per thread** applies.

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
- **Tests:** `cd connectors/csharp && dotnet test Taba.Client.Tests` (NUnit; the integration ones only run with `TABA_URL`). `connectors/csharp/scripts/integration.sh` starts Compose, creates tokens and queues and runs everything. Besides it, every connector (this one too) runs the **contract tests**: `connectors/csharp/contract/run.sh`, or `connectors/contract/run.sh` for all five ([contract tests](#the-contract-tests)).
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

## The planned ones: base version and publication

They count as a **plan** (proposals, subject to D68, D70.1, D70.2). Java, Python and Node.js left this table when they were done (D68.2). "Base version" is the oldest one that speaks TLS 1.2, has an HTTP client with `PATCH` and verifies the certificate.

| Language | Base version | Why | HTTP / JSON | Publication |
|---|---|---|---|---|
| PHP | **5.6** | TLS 1.2 and verification on by default | `curl` / `json_decode` | Packagist |
| Ruby | **2.0** | `Net::HTTP::Patch` and `json` in the standard library | `Net::HTTP` / `json` | RubyGems |
| Go | **1.7** | `context.Context` and `net/http` with `PATCH` | `net/http` / `encoding/json` | Module proxy (tag `connectors/go/vX`) |
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

The same scenario runs against a real Taba through **every connector** (C#, Java, Node.js, Python and Rust), each with a small *driver* written in its own language that uses only the public API ([connectors/contract/README.md](https://github.com/rafaelpontani/taba/blob/main/connectors/contract/README.md) has the scenario language and the operations). `connectors/contract/run.sh` starts **one Taba node** in Docker (the one-node profile) and runs, one after the other, the driver of each connector **in the container of its oldest supported runtime** (Node 18, Python 3.9, JDK 11, .NET 8, Rust stable); each connector can also run alone (`connectors/<language>/contract/run.sh`).

- **`scenarios/core.scn`**, in every connector: topics (and their `404`, `409` and `400`), publish, consume, `changeVisibility` and `ack`; text with accents, JSON and bytes coming back byte for byte; FIFO (duplicate and order); distributor; consuming with a handler (deleted, failed, kept and **renewal of the invisibility** while a slow handler holds the message); `subscribe`; a long polling **cancelled** from outside (not in Rust, which is synchronous: the line is printed as `SKIP`); a wrong token and a refused connection.
- **`scenarios/cross-*.scn`**, the five together: each connector publishes to a distributor and each one reads its own queue, finding what the five sent, exactly.
- **Not covered**: `503` and the retries with a node going down, and the cluster behavior (one node cannot show them): they stay with `docker/validate.sh` and the unit tests of each connector. The job `connector-contract` of `ci.yml` runs it on every pull request to `release`.

## Pending items in this area

D68 (confirm the base versions of the languages still to do), D70.1 (organization and package identifiers in each registry), D70.2 (PHP mirror; versions independent of the server's): [31-pending-items.md](31-pending-items.md).

## Sources and evidence

- `connectors/rust/client/src` (`client`, `http`, `json`, `base64`, `transport`, `testing`), `connectors/rust/client/README.md`, `connectors/csharp/Taba.Client*`, `connectors/csharp/Taba.Client.Tests`, `connectors/java/src`, `connectors/node/src`, `connectors/python/taba`, `examples/csharp-console`, `connectors/README.md`.
- Use cases: UC-CNT (35 cases, level `K`; connectors); see [26a](26a-implemented-use-cases.md) and [26b](26b-planned-use-cases.md). Tests: `connectors/rust/client/**/tests.rs`, `connectors/csharp/Taba.Client.Tests`, `connectors/java/src/test`, `connectors/node/test`, `connectors/python/tests`; contract tests: `connectors/contract` (`run.sh`, `scenarios/`) and the drivers in `connectors/{csharp,java,node,python,rust}/contract`.
- History: `19-conectores.md`, `19a-conectores-linguagens.md`.
