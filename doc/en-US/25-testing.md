---
title: Testing
summary: How Taba is tested — the levels (unit, one node, simulated cluster, connectors, end to end), the test tools (TestNode, TestCluster, in-memory network, manual clock, fake OTLP collector) and how to write and run a test.
type: guide
status: stable
verified-on: 2026-10-13 @ 71183b9
sources: [crates/taba-node/tests, crates/taba-node/tests/common, crates/taba-node/src/cluster/transport/memory, crates/taba-node/src/core/clock, connectors/rust, connectors/csharp/Taba.Client.Tests, docker/validate.sh]
---

# Testing

> Read first: [04-architecture.md](04-architecture.md) (why the node is testable) and [24-conventions.md](24-conventions.md).

## The idea

In a distributed system, the most valuable test **starts several nodes in the same process and injects failures**. Taba was designed for that: what touches the outside world sits behind a *trait* (`Transport` for the network between nodes, `Clock` for time), and the `taba_node` library exposes everything the binary uses. So the tests start **real nodes** (real HTTP on free ports, real SQLite in a temporary folder) with no mock of what matters, and **deterministically**: the clock and the network belong to the test.

## The levels

| Level | Acronym | What runs | Where it is |
|---|---|---|---|
| **Unit** | `U` | An isolated piece, with no real network or disk: domain, validation, ring, lists, quorum, JSON reader | `src/**/tests.rs`, **sibling of the module** |
| **One node** | `I` | A real node in **development mode**, real HTTP, real disk (temporary folder). What the client sees | `crates/taba-node/tests/*.rs` with `TestNode` |
| **Cluster** | `C` | **Several nodes in the same process**, in-memory network (programmable loss, delay, partition) and manual `Clock`. Quorum, rollback, failure, voting, synchronization, drain | `crates/taba-node/tests/*.rs` with `TestCluster` |
| **Connector** | `K` | The connector against a fake transport (unit) and against a real Taba (integration) | `connectors/rust/**/tests.rs`, `connectors/csharp/Taba.Client.Tests` |
| **End to end** | — | The real 3-node Compose | `docker/validate.sh`, `connectors/csharp/scripts/integration.sh` |

There are also **tests over real TCP and TLS** (`tests/mtls.rs`, `tests/common/real.rs`: nodes on `127.0.0.x`, HTTP `Transport` and mutual TLS) and **OTLP tests** (`tests/otlp.rs`, with a fake collector).

![The test levels](../diagrams/img/en-US/test-pyramid.svg)

## The integration files (`crates/taba-node/tests/`)

| File | Covers |
|---|---|
| `topics.rs`, `messages.rs` | Client API: topics, publishing, consumption, ack, invisibility (`I`) |
| `auth.rs`, `ratelimit.rs`, `secrets.rs` | Tokens, rate limit, `taba init` |
| `fifo.rs`, `volatile.rs` | FIFO queue and volatile queue on one node |
| `operations.rs`, `metrics.rs`, `topic_stats.rs`, `conditions.rs`, `otlp.rs` | Health, metrics, conditions, OTLP export |
| `startup.rs`, `listeners.rs` | Node startup, listeners (long polling) |
| `cluster.rs`, `cluster_api.rs`, `channel.rs`, `notify.rs`, `replication.rs`, `definitions.rs` | Simulated cluster: join, routes, channel, quorum, definitions (`C`) |
| `failures.rs`, `freezing.rs`, `sync.rs`, `recovery.rs`, `drain_cluster.rs` | Failure, voting, freezing, synchronization, return, drain |
| `fifo_cluster.rs`, `volatile_cluster.rs` | FIFO and volatile in the cluster |
| `mtls.rs` | Cluster security over real TCP and mutual TLS |
| `uc_coverage.rs` | **Checks** the use cases (see [26-use-cases.md](26-use-cases.md)) |

In numbers: ~330 tests in these files plus the unit tests of the modules; the cluster has **702 of 885** use cases with a test ([26a](26a-implemented-use-cases.md)).

## The tools (`tests/common/`)

| Tool | What it gives |
|---|---|
| **`TestNode`** (`mod.rs`) | Starts **one real node** on free ports, disk in a temporary folder. `TestNode::development()` and `::production()`; `builder()` with `.env(name, value)`, `.in_network(&network, host)`, `.clock(clock)`, `.real(ip, entry)`, `.production()`. Methods: `client_url()`, `operations_url()`, `cluster_url()`, `client_get()`, `metrics()`, `stats()`, `storage_dir()`, `shutdown()`. Helpers: `series()` (reads a `/metrics` series), `error_code()` (checks status and returns the `code`) |
| **`TestCluster`** (`cluster.rs`) | **Several nodes in the same process** (node `n` = `10.0.0.n`), linked by the **`MemoryNetwork`** and by **a shared manual `Clock`**; the load balancer (`balancer:8081`) distributes the calls. `start(n)`, `start_with(n, env)`, `add_node()`, `spawn_node()`, `respawn_node()`, `node(n)`, `table(n)`, `member_ids(n)`. Helpers: `eventually(…)` (waits for a condition), `settled()` (stable cluster), `create_queue()`, `name_owned_by()` (finds a topic name whose owner is node n), `pause_sync()`/`wait_phase()` (stops in the middle of a synchronization phase), `replication_of()`, `channels_of()`, `signals_of()` |
| **`MemoryNetwork`** (`src/cluster/transport/memory`) | The test `Transport`: **partition** between two nodes or isolation of one, **loss** of the next calls to a node (or of a kind), **delay** on all calls, **mute** node (accepts and does not answer), node that **goes down** (disappears from the network) and **load balancer** |
| **`ManualClock`** (`src/core/clock`) | The test clock: the test advances it (invisibility, `ttl`, freezing, voting, `idleTimeout`) |
| **`RealCluster`** (`real.rs`) | Nodes over **real TCP on `127.0.0.x`**, with the HTTP `Transport` and **mutual TLS**; certificate generation and rotation (`cert_dir`, `new_pair`, `add_named_pair`), connection with or without a certificate (`connect`, `anonymous_client`) |
| **Fake OTLP collector** (`otlp.rs`) | `Collector::http()` and `::grpc(tls)`, with `metrics()`, `spans()`, `log_records()`. The OTLP tests **run one at a time**, because the `tracing` subscriber belongs to the process |

## How to write a test

**Golden rule:** test **observable behavior** (status, body, disk state, metric, log, what the other node sees), at the **lowest level that proves it**.

```rust
/// UC: UC-TOP-001
#[tokio::test]
async fn creating_a_queue_returns_201_with_location_and_the_defaults() {
    let node = TestNode::development().await;
    // … POST /topics, checks status, Location and body …
    node.shutdown().await;
}
```

1. **Cite the use case** on a `/// UC: UC-XXX-NNN[, UC-YYY-NNN]` line **right above** the `#[test]` (or `#[tokio::test]`); there may be attributes between the line and the function. The code must exist in the catalogue ([26-use-cases.md](26-use-cases.md)).
2. **Name in English and descriptive** (the sentence says the expected result).
3. **Unit:** in `tests.rs` next to the module. **Integration:** in `tests/`, using `TestNode` or `TestCluster`. Do not create a new integration test if an existing file of the area serves.
4. **Cluster:** use the manual `Clock` (**advance time**, do not sleep) and the `MemoryNetwork` (program the failure). Use `eventually(…)` to wait for the state, not `sleep`.
5. **Language:** identifiers and test names in English; comments in Portuguese.
6. **Run and update the catalogue:** `cargo test --test uc_coverage` checks the cited codes; add the test to the case card in [26a](26a-implemented-use-cases.md) (a case that gets its first test moves from [26b](26b-planned-use-cases.md) to 26a), in both languages.

## How to run

```bash
cargo test                                          # everything (server)
cargo test --test messages                          # one integration file
cargo test --test uc_coverage -- --nocapture        # scoreboard: "use cases with at least one test: 702 of 885"
cargo clippy --all-targets -- -D warnings
cargo fmt --all --check

cd connectors/rust && cargo test --workspace --all-features
cd connectors/csharp && dotnet test Taba.Client.Tests          # unit; the integration ones require TABA_URL
connectors/csharp/scripts/integration.sh                       # starts Compose, creates tokens and queues, runs everything
docker/validate.sh                                             # end to end (≈ 5 min)
```

**`uc_coverage` in detail.** It fails when: (1) a test cites a code that **does not exist** or is `(retirado)`; (2) a code appears **twice** in the documentation. A third rule, "every documented case has a test", **only fails with `TABA_UC_STRICT=1`**: with 183 cases without a test, it prints the scoreboard instead of failing. `uc_coverage` reads the `### UC-…` cards of [26a](26a-implemented-use-cases.md) and [26b](26b-planned-use-cases.md).

## `docker/validate.sh` and `docker/cluster.sh`

- **`cluster.sh`** (library to `source`, or command): `taba_up`, `taba_down`, `taba_kill` (abrupt failure), `taba_stop` (orderly exit, with drain), `taba_start`, `taba_restart`, `taba_wait_healthy`, `taba_wait_members N`, `taba_wait_stable`, `taba_create_token`, `taba_ensure_topic`, `taba_cli …`, `taba_status`. The connector tests should **reuse it**.
- **`validate.sh`** (≈ 5 minutes): starts Compose from scratch and checks (1) the cluster formed (`health cluster` without conditions); (2) `taba-cli`: create topic, publish, consume; (3) abrupt failure of `taba-2` (`kill`; `docker/cluster.sh` maps `taba-N`, N ≥ 1, to replica N of the `taba` service); (4) orderly exit of `taba-1` (`stop`, with drain); (5) **no confirmed message was lost**; (6) final state. **Run before closing changes that affect cluster, channel, Compose or connectors.**

## Strategy: what each level protects

| Risk | Where it is caught |
|---|---|
| Wrong pure rule (quorum, ring, ID range) | `U` |
| Broken HTTP contract (status, body, error) | `I` |
| Message loss, quorum, split, voting, freezing, synchronization | `C` (deterministic, with injected failure) |
| TLS security and certificate rotation | `RealCluster` (real TCP) |
| Real integration (image, Compose, HAProxy, `SIGTERM`) | `validate.sh` |
| Connector diverging from the contract | `K` |

⬜ **Missing:** **performance and load** tests (the speed and cost goals have no benchmark), **Kubernetes** tests and the **shared conformance suite** between connectors ([32-roadmap.md](32-roadmap.md)). `docker/test-cpu.sh` is a CPU measurement script.

## Known issues

The tests turned off with `#[ignore]` because they fail intermittently in CI, with the cause and the planned fix of each, are in [36-known-issues.md](36-known-issues.md).

## Sources and evidence

- `crates/taba-node/tests/*.rs` and `tests/common/{mod,cluster,real,otlp}.rs`, `src/cluster/transport/memory`, `src/core/clock`, `tests/uc_coverage.rs`, `docker/{cluster,validate}.sh`, `connectors/rust/**/tests.rs`, `connectors/csharp/Taba.Client.Tests`.
- The "702 of 885" scoreboard and the "Tests" column failure were **run** on 2026-10-13 (`cargo test --test uc_coverage -- --nocapture`). Also **run and green** on that date: `cargo test --lib` (**509 unit tests**, 2.25 s) and the files `topics` (21), `messages` (41), `failures` (21), `freezing` (5) and `drain_cluster` (7). **The rest of the suite was not run** in this verification: the machine ran out of disk space at *link* (`LNK1201`), the `target/` problem described in [24-conventions.md](24-conventions.md).
- Use cases: all (each test cites its own); see [26a](26a-implemented-use-cases.md).
- History: `doc/20-casos-de-uso.md` (19.4, 19.5), `18-estrutura-codigo.md` (17.5).
