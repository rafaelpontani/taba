---
title: Architecture
summary: How a Taba node is built inside — layers, direction of dependencies, the three ports, the test boundaries and the module map.
type: explanation
status: stable
verified-on: 2026-10-15 @ 56121c9
sources: [crates/taba-node/src/lib.rs, crates/taba-node/src/node.rs, crates/taba-node/src/main.rs, crates/taba-node/src/http/mod.rs, crates/taba-node/src/cluster/transport/mod.rs]
---

# Architecture

> Read first: [03-concepts.md](03-concepts.md).

## The view on one screen

A Taba cluster is made of **identical nodes**. Each node is **one process** (the `taba` binary) that:

1. stores messages **in memory**, with **overflow to disk** (SQLite and files);
2. opens **three HTTP ports** (client, operations, cluster);
3. talks to the other nodes through **HTTP calls** and a **permanent WebSocket channel**, protected by mutual TLS;
4. decides on its own who owns what, from the **member table** and the **hash ring**, with no coordinator.

There is no "master" node and no external service. What would be "the database" is local files on each node; what would be "the coordinator" is computed by everyone in the same way.

![A node inside: three ports, engine, cluster and storage](../diagrams/img/en-US/node-overview.svg)

## The layers and the direction of dependencies

The code is **one crate** (`taba-node`) organized in modules. The dependency rule goes in one direction only:

```
core  ←  storage  ←  engine  ←  cluster  ←  http  ←  node
                       config sits beside: reads file and environment and depends only on core
```

Each module knows only those on its left. This is verifiable: `core` imports no one else.

| Layer | Responsibility | Does not do |
|---|---|---|
| `core` | Domain types and pure rules: IDs (GUID v7), time, errors (`ErrorCode`), validations, the **hash ring**, topic, token, pagination | No I/O |
| `storage` | The node database (`_node.db`), messages on disk, the synchronization event database, tokens and losses | Delivery rules |
| `engine` | Topics, messages, ready and invisible lists, fair delivery, FIFO, dead-letter, tokens, statistics | Network |
| `cluster` | Members, node protocol, channel, voting, freezing, synchronization, replication, drain, version upgrade, recovery, volatile topics, health | Client HTTP |
| `http` | The three APIs, authentication, rate limit, client IP, forwarding, metrics | Domain rules (only reads the request and builds the response) |
| `telemetry` | Metrics, traces and logs; OTLP and Prometheus export | — |
| `config` | `.yml` file, `TABA_*` variables, the parameter table and the startup validations | — |
| `secrets` | The secrets folder: cluster certificate and root token (`taba init`, `taba cert generate`, `taba token generate`) | — |
| `cli` | The command line of the `taba` binary | — |
| `node.rs` | **The node startup**: prepares the disk, creates the state, wires the pieces and opens the listeners | — |

`main.rs` is only the **composition root**: it reads the configuration, calls what is in `node.rs` and waits for the stop signal. Everything else lives in the `taba_node` library so the integration tests start **real nodes**.

![The layers and the direction of dependencies](../diagrams/img/en-US/layers.svg)

## The three ports

Each one is its own *listener*, so the network can restrict access to each one separately.

| Port | Key | Default | Router (`http/mod.rs`) | Layers the call goes through |
|---|---|---|---|---|
| Client | `api.port` | 8080 | `client_router` | authenticate → rate limit → refusal by drain/synchronization/failure → authorize |
| Operations | `operations.port` | — | `operations_router` | IP filter (`operations.allowed-ips`), rate limit per IP |
| Cluster | `cluster.port` | 8081 | `cluster_router` | mutual TLS; only `/cluster/*` |

- Without `operations.port`, the operations routes are served **also on the client port**.
- A node alone (`replication.factor=1`) opens the cluster port the same way; no other node just uses it.
- `/tokens` routes only exist with authentication on; otherwise they answer `404`.
- A node in drain, synchronization or failure **does not serve** client calls: in a cluster it **forwards** them to another node (through the destination's cluster port) and, with no destination, answers `503` with `Retry-After`. The reads that say what the cluster is doing (`GET /auth/me`, `/nodes`, `/cluster/status`, `/cluster/backup` and the `/nodes/{nodeId}/…` calls, which include returning a node) are the exception: they are forwarded while another node can take them, and otherwise answered by the node itself, so a stopped cluster can still be inspected. In drain the node only validates the root token (the other tokens are data).

Route details in [15-api-overview.md](15-api-overview.md).

## Boundaries that make the node testable

What touches the outside world sits behind a *trait*, so the test can swap the implementation:

| Trait | Real implementation | Test implementation | What it is for |
|---|---|---|---|
| `Transport` (`cluster/transport`) | HTTP + WebSocket, with mTLS (`transport/http`) | In-memory network with programmable partition, loss, delay and mute node (`transport/memory`) | Calls and channel between nodes |
| `Clock` (`core/clock`) | System clock | Manual clock, advanced by the test | Invisibility, `ttl`, freezing, voting |
| `Handler` (`cluster/protocol`) | `ClusterHandler` over `ClusterService` | the same | What **receives** a node-to-node call, the same for the HTTP route and for the in-memory network |
| `ObjectStore` (`storage/backup/store`) | `S3Store` over `aws-sdk-s3` (AWS and MinIO) | `MemoryStore`, with programmable failures (down, no conditional write) | The store of the optional S3 backup |
| `BackupSink` (`engine/messages`) | `Backup` (`cluster/backup`) | a recorder, in the engine tests | What the engine **notifies** (published, removed, state changed, owner changed), never waiting for S3 |

That is why the cluster tests (level `C`, [25-testing.md](25-testing.md)) run several nodes **in the same process** and control time and the network.

## Concurrency

- **Runtime:** `tokio`, multiple threads.
- **Single writer per database:** every write to a SQLite database (each topic's and `_node.db`) goes through **a single writer**; long reads, such as the synchronization streams, use their own read connections (WAL mode).
- **Shared state:** member table, topic table and tokens are in memory behind locks; membership changes are **one at a time** per node.
- **Background tasks:** invisibility expiry, `ttl` sweep, volatile queue management, rejoin after a partition, overflow report.

## Module map

```
crates/taba-node/src/
  main.rs            composition: command line, configuration, signals
  lib.rs             the module list (the `taba_node` library)
  node.rs            node startup
  cli/               arguments of the `taba` binary
  config/            file, variables, parameter table, validations
    params/ source/ otlp.rs error.rs
  core/              clock content error id pagination ring time token topic validation
  secrets/           secrets folder (cluster certificate, root token)
  storage/           db (the `_node.db`) messages topics tokens state losses versions synclog
                     backup (store, keys, segment, journal, db: the `_backup.db`)
  engine/            topics tokens messages (blocks, index, rates, replicas, stats)
  cluster/           backup (upload, evict, definitions, heartbeat, restore, stop, topology)
                     channel drain health members protocol recovery replication restore
                     service state sync tls transport (http, memory) upgrade vote volatile
  http/              auth backup client_ip cluster_api common forward messages metrics observe
                     nodes operations ratelimit tokens topic_stats topics
  telemetry/         histogram metrics otlp process prometheus trace
```

Each folder has a `mod.rs` that starts with a `//!` comment saying what it does. Unit tests live in `tests.rs` next to the module, linked by `#[cfg(test)] mod tests;`.

## How the node starts (`node.rs`)

1. Reads and validates the configuration (for example, `storage.dir` must end in a folder named `taba`).
2. Prepares the disk: `_node.db`, the topic folders; the previous life's disk is set aside and comes back through the restoration ([07-cluster.md](07-cluster.md), section 10).
3. Creates the shared node state, the telemetry, the topic and token registries, and the messages.
4. In production: creates the `ClusterService`, the replication and the transport, and **joins the cluster** (bootstrap) or, if it is the first node (`cluster.first-node`), starts the cluster by itself.
5. Opens the listeners of the three ports and from then on serves. The node only becomes **ready** (`/health/ready`) when it finishes synchronizing.

The detail of joining and synchronization is in [07-cluster.md](07-cluster.md).

## Sources and evidence

- `crates/taba-node/src/lib.rs` (the dependency rule), `node.rs`, `http/mod.rs` (`client_router`, `operations_router`, `cluster_router`, `drain_guard`), `cluster/transport/mod.rs` (`Transport`), `core/clock/mod.rs`.
- Use cases: UC-SEG (ports and mutual TLS), UC-DEV (one-node profile), UC-FWD (forwarding), UC-CFG (configuration); see [26a](26a-implemented-use-cases.md).
- History: `02-arquitetura.md`, `18-estrutura-codigo.md`.
