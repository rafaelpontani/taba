---
title: Testing
summary: How Taba is tested — the levels (unit, one node, simulated cluster, connectors, end to end), the test tools (TestNode, TestCluster, in-memory network, manual clock, fake OTLP collector) and how to write and run a test.
type: guide
status: stable
verified-on: 2026-10-26 @ 75012bc
sources: [crates/taba-node/tests, crates/taba-node/tests/common, crates/taba-node/src/cluster/transport/memory, crates/taba-node/src/core/clock, connectors/rust, connectors/csharp/Taba.Client.Tests, docker/validate.sh, docker/seed-demo.sh, docker/hang-hunt.sh]
---

# Testing

> Read first: [04-architecture.md](04-architecture.md) (why the node is testable) and [24-conventions.md](24-conventions.md).

## The idea

In a distributed system, the most valuable test **starts several nodes in the same process and injects failures**. Taba was designed for that: what touches the outside world sits behind a *trait* (`Transport` for the network between nodes, `Clock` for time), and the `taba_node` library exposes everything the binary uses. So the tests start **real nodes** (real HTTP on free ports, real SQLite in a temporary folder) with no mock of what matters, and **deterministically**: the clock and the network belong to the test.

## The levels

| Level | Acronym | What runs | Where it is |
|---|---|---|---|
| **Unit** | `U` | An isolated piece, with no real network or disk: domain, validation, ring, lists, quorum, JSON reader | `src/**/tests.rs`, **sibling of the module** |
| **One node** | `I` | A real node in the **one-node profile**, real HTTP, real disk (temporary folder). What the client sees | `crates/taba-node/tests/*.rs` with `TestNode` |
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
| **`TestNode`** (`mod.rs`) | Starts **one real node** on free ports, disk in a temporary folder. `TestNode::single()` (a node in the one-node profile, already active and with its restoration done) and `::production()`; `builder()` (the one-node profile by default) with `.env(name, value)`, `.in_network(&network, host)`, `.clock(clock)`, `.real(ip, entry)`, `.single_node()`, `.production()`. Methods: `client_url()`, `operations_url()`, `cluster_url()`, `client_get()`, `metrics()`, `stats()`, `storage_dir()`, `shutdown()`. Helpers: `series()` (reads a `/metrics` series), `error_code()` (checks status and returns the `code`) |
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
    let node = TestNode::single().await;
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
docker/hang-hunt.sh nodes_api <test name>                         # a test that hangs only under load: logs + thread stacks
```

**`uc_coverage` in detail.** It fails when: (1) a test cites a code that **does not exist** or is `(retirado)`; (2) a code appears **twice** in the documentation. A third rule, "every documented case has a test", **only fails with `TABA_UC_STRICT=1`**: with 183 cases without a test, it prints the scoreboard instead of failing. `uc_coverage` reads the `### UC-…` cards of [26a](26a-implemented-use-cases.md) and [26b](26b-planned-use-cases.md).

### Investigating a test that hangs or fails only sometimes

**In one command:** `docker/hang-hunt.sh <test-file> [test name filter...]` (for example `docker/hang-hunt.sh nodes_api a_prune_that_fails a_node_that_does_not_answer_makes`). In a Linux container it builds that test binary, starts it in `PAR` processes at once (8) round after round (`ROUNDS`, 60), with the detailed node logs on, and stops at the first problem: a process that **fails** keeps its log; a process still running after `HANG_AFTER` seconds (70) is a **hang**, and the stack of every thread is taken with `gdb` before it is killed. With `KEEP_GOING=1` it does not stop at the first problem: it goes through all the rounds keeping every bad log, which is how a fix is validated (count the logs kept and read the message of each: a different failure, like a port taken by another test, is not the one being hunted). Logs and stacks go to `target/hang-hunt/` (`run_<round>_<n>.log`, `stack_<round>_<n>.txt`, `progress.txt`); the exit code is 1 when it captured something. The detailed logs are on **only in this script** and in the runs where you set `TABA_TEST_LOG` yourself: the normal runs and the CI stay quiet. The first run builds for about 6 minutes; the build volumes are kept for the next ones. The header of the script lists the other variables (`TEST_THREADS`, `TABA_TEST_LOG`, `OUT`, `PACKAGE`, `IMAGE`). What it does, step by step, is below, for when you need to do it by hand.

1. **Turn the logs on.** `TABA_TEST_LOG=<filter>` makes `TestCluster` start the node logs with that filter (the syntax of `RUST_LOG`: `debug`, or `taba_node=trace,hyper=warn,hyper_util=warn`); add `-- --nocapture`, or the test harness hides them. `TestNode` alone does not read it.
   ```bash
   TABA_TEST_LOG='taba_node=trace,hyper=warn,hyper_util=warn' \
     cargo test -p taba-node --test nodes_api <test name> -- --nocapture
   ```
2. **Reproduce under load, not alone.** Build the binary once (`cargo test --no-run`) and start it in 6 to 8 processes at once, round after round, each with a `timeout` of about 75 s and its own log file; delete the log of the runs that pass and keep the ones that hang. A race that costs 1 run in 50 shows up in a few minutes this way and never in a sequence.
3. **A hang with no output is a stuck runtime: take the stack.** In a Linux container (the Windows binary has no `gdb`): mount the repository read-only, build with `CARGO_TARGET_DIR` on a volume, `apt-get install gdb`, run the binary in the background, and when it does not end in 70 s run `gdb -p <pid> -batch -ex "set pagination off" -ex "thread apply all bt 40"`. Start the container with `--cap-add=SYS_PTRACE`. The `dev` profile keeps line tables, so the frames carry `file:line`.
4. **Read the stack.** A frame of the crate's own function under `Mutex::lock` (or `RwLock::read`/`write`) with no other thread holding that lock is a lock taken twice by the same thread; two threads, each inside a lock the other holds, is an inversion. With the `current_thread` runtime of `#[tokio::test]` there is a single thread to look at.
5. **Make the test fail instead of hang.** When the call under test can block for good, run it on its own `std::thread` (entering the runtime with `Handle::enter` if it spawns tasks) and wait with `mpsc::Receiver::recv_timeout`; see `tests/channel.rs::a_channel_from_a_node_the_peers_do_not_know_yet_is_refused_without_blocking_the_node`.

The case that taught this is in [36-known-issues.md](36-known-issues.md#a-channel-accept-that-locked-the-peers-twice).

## `docker/validate.sh` and `docker/cluster.sh`

- **`cluster.sh`** (library to `source`, or command): `taba_up`, `taba_down`, `taba_kill` (abrupt failure), `taba_stop` (orderly exit, with drain), `taba_start`, `taba_restart`, `taba_wait_healthy`, `taba_wait_members N`, `taba_wait_stable`, `taba_create_token`, `taba_ensure_topic`, `taba_cli …`, `taba_status`. The connector tests should **reuse it**.
- **`validate.sh`** (≈ 5 minutes): starts Compose from scratch and checks (1) the cluster formed (`taba-cli cluster status` without conditions); (2) `taba-cli`: create topic, publish, consume; (3) abrupt failure of `taba-2` (`kill`; `docker/cluster.sh` maps `taba-N`, N ≥ 1, to replica N of the `taba` service); (4) orderly exit of `taba-1` (`stop`, with drain); (5) **no confirmed message was lost**; (6) final state. **Run before closing changes that affect cluster, channel, Compose or connectors.**

## Demo data: `docker/seed-demo.sh`

For manual tests, load tests and demos (the TUI, the charts, the `noisy-group` condition), `docker/seed-demo.sh` brings up a populated cluster in a few minutes:

```bash
docker/seed-demo.sh                    # from scratch; leaves the cluster running at the end
docker/seed-demo.sh --no-build         # reuses the images already built
docker/seed-demo.sh --messages 10000   # another amount per distributor (default 100000; minimum 1000)
docker/seed-demo.sh --no-profile         # does not touch the taba-cli profile
PARALLEL=16 docker/seed-demo.sh        # more publisher containers at the same time (default 8)
```

What it does, in this order:

1. **Deletes everything:** `docker compose down -v` (volumes, secrets, node data and the backup bucket) and starts the three-node cluster building this repository's images (the same `taba_up` as `validate.sh`). Whatever was in that Compose is lost. It needs port 8080 free.
2. **Creates a local admin token.** The **root token** (full access, from the Compose `secrets` volume) only bootstraps: it is the only one that can create an `admin` token, and the script uses it for that (through the Compose `taba-cli`) and nowhere else on your machine. The script creates the token **`local-admin`** with the `admin` permission (it manages tokens and operates the cluster) and writes its secret, which only appears in the creation response, to **`docker/.local/admin-token`**, with no line break at the end and mode `0600` where the system allows it. Then it calls `GET /auth/me` to check that the API accepts it as an `admin` (`"root": false`). This is the way to use Taba: a named, revocable token, not the root one ([14-security.md](14-security.md)). The file is on your machine only: `docker/.local/` and every file called `admin-token` are in `.gitignore`. Every run **recreates** the volume, so the token is **new** and the file is overwritten (`ADMIN_NAME=` changes the token name). It shows up in the Tokens tab of the `taba-cli tui` and in `GET /tokens`; the root token does not, because it does not come from the table. Use it like this:

   ```bash
   curl -H "Authorization: Bearer $(cat docker/.local/admin-token)" http://localhost:8080/topics
   ```

   To use another path: `TOKEN_FILE=/path/token docker/seed-demo.sh`. The `taba-cli` of the Compose does not need the file (it reads the token from the volume itself).
3. **Configures the `taba-cli` profile.** If a `taba-cli` is installed on the machine (`TABA_CLI_BIN`, the `PATH`, or this repository's build in `connectors/rust/target/release`), the script points the profile **`local_admin`** (`PROFILE=` changes the name) to `http://localhost:8080` and to the new token: if the profile exists, only its `url` and `token` change (the token becomes `token-file`, pointing to `docker/.local/admin-token`); if not, it is created and becomes the **default**. Afterwards `taba-cli topic list` works without options. `--no-profile` skips the step; without a `taba-cli` it is skipped with the command to run by hand. The profile file is the one in [20-cli.md](20-cli.md#profiles) (`TABA_CLI_CONFIG` changes the path).
4. **Creates the topics** (the queues first, because a distributor only copies to the queues that exist when it publishes) and **publishes** the messages with `taba-cli publish --count`, in parallel containers.

| Topic | Type | Content |
|---|---|---|
| `demo-orders-billing` | queue | 100,000 messages (copies from `demo-orders`) |
| `demo-orders-stock` | queue | 100,000 messages (copies from `demo-orders`) |
| `demo-events-audit` | queue | 100,000 messages (copies from `demo-events`): **the noisy queue** |
| `demo-orders` | distributor, prefix `demo-orders-` | receives 100,000 publications; stores nothing |
| `demo-events` | distributor, prefix `demo-events-` | receives 100,000 publications; stores nothing |
| `demo-payments-fifo` | FIFO queue | 10,000 messages (10% of the amount), published directly: 20 accounts (`account-01` … `account-20`) × 500, numbered `seq` 1…500 |

So the three queues end up with 100,000 messages each (plus their `-dead-letter` queues, empty; the FIFO queue has the 10,000 above), and the two distributors are the ones that receive the 200,000 publications. The groups ([12-delivery.md](12-delivery.md)) are what differ:

- **`demo-payments-fifo` (FIFO):** each account is a group (`groupId`) and the queue keeps the order *within* the group: consuming it returns `seq` 1, 2, 3… of each account (`taba-cli consume demo-payments-fifo --max 10`). Every message has a `deduplicationId` (`account-NN-seq`), because FIFO drops a repeated body in the same group.
- **`demo-orders-*` (balanced):** 100 groups (`customer-001` … `customer-100`) with 1,000 messages each.
- **`demo-events-audit` (noisy):** 95% of the messages (95,000) belong to a single group, `noisy-integration`, and the other 5% are spread over 50 groups (`service-01` … `service-50`, 100 each). It is the noisy-neighbor example: delivery favors the groups with fewer messages, so a consumer of this queue receives the `service-NN` messages first. To see it: `taba-cli stats groups demo-events-audit` (or `/stats/topics/demo-events-audit/groups`) shows `noisy-integration` with `share` 0.95, and `/stats/topics/demo-events-audit?scope=node` shows `noisyShare` ≈ 0.95 and the `noisy-group` condition ([13-observability.md](13-observability.md)). The cluster-wide view (without `scope=node`) gives a **lower bound** of `noisyShare` (≈ 0.33 with three nodes, because each node holds a third of the messages) and does **not** raise the condition: measured with `--messages 2000`.

At the end the script prints the depth of each queue and leaves the cluster up: explore it (`docker compose run --rm taba-cli tui`, `stats topics`, `consume`) and delete everything with `docker/cluster.sh down`. With `--messages N`, the amounts scale (N per distributor, 95% noisy); the 100,000 above are the default.

> The publishing time depends on the machine (each publication waits for the quorum through HAProxy); the script prints how long it took. A failure of any publisher stops the script with an error.

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

The test turned off with `#[ignore]` because it fails intermittently in CI, with what is known of the cause and the next step, is in [36-known-issues.md](36-known-issues.md).

## Sources and evidence

- `crates/taba-node/tests/*.rs` and `tests/common/{mod,cluster,real,otlp}.rs`, `src/cluster/transport/memory`, `src/core/clock`, `tests/uc_coverage.rs`, `docker/{cluster,validate,seed-demo}.sh`, `connectors/rust/**/tests.rs`, `connectors/csharp/Taba.Client.Tests`.
- The "702 of 885" scoreboard and the "Tests" column failure were **run** on 2026-10-13 (`cargo test --test uc_coverage -- --nocapture`). Also **run and green** on that date: `cargo test --lib` (**509 unit tests**, 2.25 s) and the files `topics` (21), `messages` (41), `failures` (21), `freezing` (5) and `drain_cluster` (7). **The rest of the suite was not run** in this verification: the machine ran out of disk space at *link* (`LNK1201`), the `target/` problem described in [24-conventions.md](24-conventions.md).
- Use cases: all (each test cites its own); see [26a](26a-implemented-use-cases.md).
- History: `doc/20-casos-de-uso.md` (19.4, 19.5), `18-estrutura-codigo.md` (17.5).
