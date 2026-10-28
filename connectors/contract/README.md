# Connector contract tests

The same scenario, run against a real Taba by **every connector** (C#, Java, Node.js, Python and Rust), through a small *driver* written in the connector's own language. The scenario is data, not code, so the contract is written once and each connector proves it.

```
connectors/contract/run.sh                # all five, then the five talking to each other
connectors/contract/run.sh node python    # only these (no cross-language step)
connectors/node/contract/run.sh           # one connector alone (starts its own node)
```

Needs Docker. `run.sh` starts **one Taba node** from the image `taba:local` (built from the root `Dockerfile` if it does not exist; `TABA_IMAGE=rafaelpontani/taba:latest` uses a published one) in the one-node profile (`replication.factor=1`, no TLS, a development token), on its own Docker network. Each driver then runs **in the container of the oldest runtime the connector supports** (Node 18, Python 3.9, JDK 11, .NET 8 SDK, current Rust), reaching the node by name. Everything is removed at the end (`--keep` leaves the node).

## What it checks

| Scenario | What |
|---|---|
| `scenarios/core.scn` (every connector) | topics (create, get, list, update, delete, the `404`/`409`/`400` errors); publish, consume, `changeVisibility`, `ack`; text with accents, JSON and bytes coming back byte for byte; FIFO (duplicate reported, order inside a group); distributor; consuming with a handler (deleted, failed, kept, **invisibility renewed** while a slow handler holds the message); `subscribe` (acknowledges what the handler accepted, gives back what it refused); a **long polling cancelled** from outside; a wrong token and a refused connection (`client.network`) |
| `scenarios/cross-*.scn` (all five together) | `setup` creates one queue per connector and a distributor over them; every connector **publishes** a text with accents, a JSON and bytes to the distributor; every connector **drains its own queue** and finds what each of the five published, exactly |

A line the connector cannot do is printed as `SKIP`, never silently ignored (today: `cancelConsume` in Rust, whose client is synchronous and has no cancellation).

Not covered, because one node cannot show it: `503` and the retries with a node going down, and the cluster behaviour. That is `docker/validate.sh` with `taba-cli` and the chaos run (`docker/cluster.sh` has `taba_kill`), and the retry rules have their unit tests in each connector.

## The scenario language

One operation per line; `#` starts a comment.

```
op key=value key=value ... [as=name] [=> expectation expectation ...]
```

- **Values** have no spaces: `%20` is a space and any `%XX` run is a UTF-8 byte sequence (`caf%C3%A9`). The decoding applies to what is written in the file, never to what a variable brings.
- **Variables:** `{p}` is the prefix of the run (it makes topic names unique), `{lang}` is the connector, and `$name.field` is a field of the result of an earlier line stored with `as=name`.
- **Expectations** after ` => `: `key=value` (equal), `key=*` (not empty), `key~value` (contains). `error=<code> status=<n>` expects the call to fail with that API error; a line that fails when it should not, or succeeds when an error was expected, fails.
- **Any operation** accepts `url=`, `token=` and `attempts=` to use another client (another address, another token, another number of attempts).

| Operation | Parameters | Result fields |
|---|---|---|
| `createTopic` | `name type fifo prefixes tags ttl maxDeliveries maxMessageBytes idleTimeout volatile` (lists: `a,b`; tags: `k:v,k:v`) | `id name type fifo volatile raw` |
| `getTopic` | `name` | same |
| `listTopics` | `name` (prefix) | `count names` (sorted) |
| `updateTopic` | `name prefixes tags idleTimeout ttl maxDeliveries maxMessageBytes` (`null` = back to the server default) | same as `createTopic` |
| `deleteTopic` | `name` | |
| `publishText` / `publishJson` / `publishBytes` | `topic body` / `topic body` (raw JSON) / `topic base64 [contentType]`; all: `groupId deduplicationId fifo` | `id duplicate matched` |
| `consume` | `topic max wait visibility` | `count bodies` and, of the first message, `id receiptToken deliveryCount contentType body` |
| `ack` | `topic id receipt` | |
| `changeVisibility` | `topic id receipt seconds` | `id invisibleUntil` |
| `handle` | `topic mode max wait visibility [hold]`; mode `ok`, `fail`, `keep` or `slow` | `handled failed bodies leaseLost` |
| `subscribe` | `topic count timeout visibility [failFirst=true]` | `count bodies` |
| `cancelConsume` | `topic after` | `cancelled fast` |
| `drain` | `topic count timeout` | `count bodies` (sorted) |
| `sleep` | `seconds` | |

`bodies` is the text of each message separated by `,`; a message whose content type is `application/octet-stream` shows as `b64:<base64>`. `handle` is consuming **with a handler** (`consume_with`, `consumeWith`, `consume(topic, handler)`...), so `slow` only works if the connector renews the invisibility by itself.

## A driver

`connectors/<language>/contract/` holds the driver (`driver.js`, `driver.py`, `ContractDriver.java`, `Program.cs`, `src/main.rs`) and `run.sh`. The driver takes `--url --token --prefix --lang --scenario FILE...`, runs the lines in order and prints `PASS`, `FAIL` (with what differed) or `SKIP` for each one, then `<lang>: N passed, N failed, N skipped`; the exit code is not zero if any line failed. It uses **only the connector's public API**, in the language's own style (a `Promise` and an `AbortSignal` in Node, a `CancelToken` in Python, a `Subscription` in Java, a `CancellationToken` in C#). It is not published with the package.

To add an operation: describe it in the table above, add it to every driver (or let a connector `SKIP` it on purpose) and use it in a scenario. To add a connector: a folder `connectors/<language>/contract/` with a driver and a `run.sh` like the others (they all end in `contract_main <language> "$@"` from `lib.sh`), and its name in `run.sh` and in `scenarios/cross-*.scn`.
