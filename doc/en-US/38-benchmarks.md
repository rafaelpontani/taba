---
title: Benchmarks
summary: The local comparative benchmark (bench/run.sh): the OpenMessaging Benchmark against Taba, Kafka, Redpanda, RabbitMQ and NATS JetStream in Docker on one machine, which systems are the target (brokers of the same class) and which are the reference (streaming logs), how the comparison is kept fair, the durability levels, the workloads, the report and how to add a system. The results of the last run, with their charts.
type: guide
status: partial
verified-on: 2026-11-03 @ 3735b5f
sources: [bench/cluster-trend.sh, bench/profile.sh, bench/profile.awk, bench/run.sh, bench/lib.sh, bench/report.mjs, bench/charts.mjs, doc/diagrams/scripts/generate-bench.mjs, bench/Dockerfile.runner, bench/run.test.mjs, bench/report.test.mjs, bench/test.sh, connectors/omb/OMB_VERSION]
---

# Benchmarks

> Read first: [21-connectors.md](21-connectors.md#openmessaging-benchmark-driver-connectorsomb) (the Taba driver for the OMB) and [11-durability.md](11-durability.md).

Taba's speed and cost goals (RNF-02) are measured by a script that anyone can run: `bench/run.sh`. It needs only Docker (Compose v2) with at least **4 cores and 7 GB**, no cloud account, and it compares Taba with Kafka, Redpanda, RabbitMQ and NATS JetStream using the [OpenMessaging Benchmark](https://github.com/openmessaging/benchmark) (OMB) at the commit pinned in `connectors/omb/OMB_VERSION`. **No result is published yet**: the tool is ready, and the numbers will come from a `standard` run made by the maintainer, with the machine described.

A result from one machine is **never an absolute number**. It compares the systems with each other, on that machine, in that run. Every `results.md` says so in its first lines.

## What the comparison measures

Taba is a message broker, not a streaming platform ([01-overview.md](01-overview.md#what-it-is)): it acks, redelivers, dead-letters and expires each message in the broker, and its goal is to do that with few resources. So the systems in the run play two roles (D195):

| Role | Systems | What it answers |
|---|---|---|
| **Target** (same class) | RabbitMQ (quorum queue), NATS JetStream | Does Taba do the same work per message with more throughput, or with less CPU and memory, under the same limits? The performance goal is set against them |
| **Reference** (pure log) | Kafka, Redpanda | How fast the same machine moves bytes when the broker does no work per message. A ceiling to know where the cost is, not a target |

A gap to Kafka or Redpanda is read as the price of the work per message (the delivery state, the ack of each message, the redelivery), not as a failure by itself; a gap to RabbitMQ or NATS is a problem to solve.

## How to run

```bash
bench/run.sh                                   # profile quick (about 1 hour), every system
bench/run.sh --profile standard                # 3 rounds of 5 minutes, six workloads
bench/run.sh --systems taba,kafka --workloads fanout-1kb
bench/run.sh --durability both                 # default and fsync
bench/run.sh --nodes 1,3                       # one node and a cluster of three (the systems with compose-cluster.yml)
bench/run.sh --profile showcase                # Taba, RabbitMQ and NATS, one and three nodes, fsync, with a node killed
bench/run.sh --taba-image rafaelpontani/taba:0.1.9   # default: builds the image of the current commit
bench/run.sh --dry-run                         # the plan and the estimated time; runs nothing
bench/run.sh --report-only bench/results/<folder>
```

`--cpus` and `--memory` override the limit given to each server. Windows runs it in Git Bash with Docker Desktop. `Ctrl+C` removes the containers and the volumes.

## The trend of the publish path: one node and three nodes

`bench/cluster-trend.sh <cores per node> <connections per node> <image>...` measures **only Taba**, to compare two builds of it (before and after a change), with one node (`replication.factor=1`) and with three nodes (factor 3), the same cores per node, `wrk` spread across the nodes inside one Docker network, and the production configuration (mutual TLS in the cluster, the root token of `taba init`). It prints, per round, the total publications per second, the p50 and p99 of each node and the failures (`non2xx`), and the CPU of each node. `BODY_BYTES`, `ROUNDS`, `SECONDS_PER_ROUND` and `NODES` change the message size, the rounds, their length and which sizes run. As everywhere in this document, a result from one machine is only comparable with another measured **in the same run**.

**Consumption, one node and three nodes.** `MODE=consume bench/cluster-trend.sh ...` measures the read side the same way: per round it fills a fresh queue for `FILL_SECONDS` (default 20) and then drains it for `DRAIN_SECONDS` (default 10) with one consumer per connection, spread across the nodes. A consumer takes up to 100 messages (`GET`) and acks **each one** (`DELETE`), like the Taba driver of the OMB. It prints the backlog filled, the messages acked and the acks per second; when the drain emptied the queue it says so, because the rate is then diluted (raise `FILL_SECONDS`). This comparison of one node against three is **Taba against itself**; the comparison with the other systems is the one-node `consume` profile below, the one that counts.

**CPU profile of a run.** `bench/profile.sh` runs beside `bench/run.sh`: it waits for the OMB runner, writes to `<output>/monitor.log` (with the time) the rates the OMB prints, live, and the CPU and memory of the server, and when the backlog reaches `--backlog` thousand messages (or after `--after` seconds) takes `--repeat` profiles of `--duration` seconds with `perf record -g` in a privileged container in the process space of the server. Each profile goes to `<output>/perf-N/` (`children.txt`, `self.txt`, `threads.txt`, `tree.txt`) with, for Taba, the mean latency and the calls per second of each route in the same interval (`routes.txt`, from `http.server.request.duration`), followed by a wait profile of `--offcpu` seconds (`offcpu.txt`: the time the threads of the server spent off the CPU, by call chain; a wait on a futex of the static musl stops at `syscall`); `--server-grep` copies the matching lines of the server log to the same log (a temporary measurement, a panic). The summaries shorten the Rust names with `bench/profile.awk`. The call chains are only readable with an image built with frame pointers (`-C force-frame-pointers=yes`).

## How the comparison is kept fair

- **One system at a time.** The script starts the server, waits for it to be healthy, runs the workloads, removes the server **and its volume**, and only then starts the next one.
- **The same limits.** Every server gets the same `cpus` and `mem_limit` (half of the Docker cores, at most 4, and half of its memory, at most 8 GB). The OMB runs in another container with the rest, and its JVM heap follows its limit.
- **One node or three (`--nodes 1,3`, D197).** With three nodes each node gets half of what a single node gets (2 cores and 1.5 GB on a machine of 12 cores and 7 GB), the same for every system, from `compose-cluster.yml` (services `node-0` to `node-2`): Taba with `replication.factor=3`, mutual TLS and the root token, behind an HAProxy of 1 core counted in its resources, as in the Compose of production; RabbitMQ with a quorum queue that has a member on each node; NATS JetStream with streams of 3 replicas. RabbitMQ and NATS clients connect straight to the nodes. The three nodes share the cores and the disk of one machine: the cluster numbers compare the systems with each other, not with a real cluster. A system without `compose-cluster.yml` (Kafka, Redpanda) stays with one node.
- **The order rotates** between rounds (A B C, then B C A, then C A B), so thermal drift and the disk cache do not always favour the same system.
- **The same runner, the same OMB, the same payload files.** Taba is measured by the driver of `connectors/omb`; Kafka and Redpanda by the OMB Kafka driver; RabbitMQ and NATS by their own. Every image has a fixed version.
- The report records the Taba commit, the OMB commit, the images, the limits and the machine (`machine.json`).

## Durability

`--durability default|fsync|both` applies one level to all the systems and writes it next to each result.

| System | `default` | `fsync` |
|---|---|---|
| Taba | `storage.db.synchronous=NORMAL`; durability by the replica in memory | `synchronous=FULL` and `memory.limit=64MB`, so the excess goes through the disk |
| Kafka | `acks=all`, no fsync per message | `flush.messages=1`, `flush.ms=0` |
| Redpanda | `acks=all`, topic write caching on | write caching off (fsync before the ack) |
| RabbitMQ | quorum queue, persistent messages, publisher confirms | the same (the quorum queue already fsyncs per confirmation) |
| NATS JetStream | file storage, default `sync_interval` | `sync_interval: always` |

A system that does not tell the levels apart shows the same numbers in both, with a footnote saying why. With one node there is no replica: the Taba column `default` has **no copy** besides the node itself, as `replication.factor=1` says ([11-durability.md](11-durability.md)).

## Workloads and profiles

| Workload | Shape | Measures |
|---|---|---|
| `simple-1kb` | 1 topic, 1 producer, 1 consumer, 1 KB, 10 000 msg/s | end-to-end latency p50 to p99.9 under moderate load |
| `max-rate-1kb` | the same, with the rate found by the OMB | sustained throughput |
| `fanout-1kb` | 5 subscriptions, 1 KB, 5 000 msg/s | the cost of the copy per subscription |
| `keyed-1kb` | 16 keys (in Taba, `ordering: per-key`), 1 KB, 10 000 msg/s | order per key |
| `small-100b` | 100 bytes, 20 000 msg/s | fixed cost per message (HTTP/1.1 against a binary protocol) |
| `backlog-1kb` | 1 GB produced with the consumer stopped, then released | reading a backlog |
| `backlog-100b` | the same with 100 bytes | the fixed cost of reading and acking a message |
| `backlog-1kb-4c` | the same, 4 partitions and 4 consumers | consumption by several consumers of the same queue |

A workload with a file `<workload>.faults` (lines `<seconds> <kill|start> <service>`, counted from the start of the measured phase) only runs with three nodes: `failover-1kb` publishes 5 000 messages of 1 KB per second, kills `node-1` at 60 s and starts it again at 150 s. Profiles are files `bench/profile-*`: `quick` (1 round of 2 minutes, the first four workloads), `standard` (3 rounds of 5 minutes, the six) and `smoke` (1 minute of `simple-1kb`, used only by `bench/test.sh`). `showcase` is the comparison with the brokers of the same class (Taba, RabbitMQ and NATS, one and three nodes, `fsync`, `simple-1kb`, `max-rate-1kb`, `keyed-1kb`, `backlog-1kb` and `failover-1kb`). `consume` is the consumption profile: 3 rounds of 3 minutes of the three `backlog-*` workloads, `bench/run.sh --profile consume`. The warm-up minute does not enter the percentiles.

In a `backlog-*` workload the report shows the **drain rate of the backlog** (the mean consumption after the peak of the backlog, when the consumer is released, while there is backlog left) in place of the end-to-end latency, which there is only the time spent in the queue. The OMB keeps the producer at the configured rate during the drain.

## The report

`bench/results/<date>-<commit>/` (ignored by git) holds `raw/*.json` (one OMB result per round, with its `.log`, the CPU and memory of the server containers sampled every few seconds in `.stats` the start of the measured phase in `.t0` and the logs of the server containers during the cell in `.server.log`), `machine.json`, `startup.csv` (seconds until healthy and memory at rest), `results.md`, `summary.json`, `charts/*.svg` and, if a cell failed, `failed.txt`. The charts (`bench/charts.mjs`) follow the color of each system, with a light and a dark theme: bars of throughput and latency, and over time the messages published and consumed per second, the messages processed so far, CPU and memory, with the kill and the start of a node marked. `node doc/diagrams/scripts/generate-bench.mjs --from <folder>` copies the `summary.json` of a run to `doc/diagrams/data/bench-results.json` and draws the charts of the documentation in every language. `results.md` shows, per workload and durability, a table with the **median of the rounds and, in parentheses, the minimum and the maximum** of the published and consumed throughput and the end-to-end p50, p99 and p99.9. It does not name a winner; it ends with the known differences of each system.

## Results

Run of 2026-11-03, profile `showcase` with the workloads `simple-1kb` and `max-rate-1kb` (rounds of 3 minutes after 1 minute of warm-up), durability fsync, on a laptop (Intel i7-10750H, 6 physical cores and 12 threads, Docker Desktop with 12 cores and 7 GB): one node with 4 cores and 3 GB, three nodes with 2 cores and 1.5 GB each. All the cells are of the same code; they came from four runs of the same day on the same machine, joined in one folder (`MERGED.txt` in it says which): the three-node cells of the first run did not start (the network of Docker failed in the same minute for RabbitMQ, and Taba hit [PRB-092](30-decisions.md)). Taba on three nodes at the maximum rate has three rounds (the median, with the minimum and the maximum); every other cell, one. The data is `doc/diagrams/data/bench-results.json` (the `summary.json` of the folder) and the charts are drawn from it by `node doc/diagrams/scripts/generate-bench.mjs`. A local run compares the systems with each other; it is not an absolute number.

| Cell | Taba | RabbitMQ (quorum queue) |
|---|---|---|
| Maximum rate, one node | 29,173 msg/s, p50 68 ms, 12,619 msg/s per core | 14,796 msg/s, p50 1.4 s, 7,176 msg/s per core |
| Maximum rate, three nodes | 9,955 msg/s (9,627 to 10,966), p50 246 ms | 5,617 msg/s, p50 2.4 s |
| 10,000 msg/s, one node | sustained, p50 2 ms, p99 17 ms | sustained, p50 517 ms, p99 1 s |
| 10,000 msg/s, three nodes | 9,754 msg/s, p50 232 ms, p99 1.5 s | 5,892 msg/s (does not keep up), p50 1.2 s, p99 2 s |
| Memory at rest, one node / three nodes | 21 MB / 87 MB | 227 MB / 534 MB |

Taba consumes 2 times what RabbitMQ does on one node and 1.8 times on three; the goal of [D195](30-decisions.md), twice the brokers of the same class, is met on one node and not yet on three ([PRB-087](31-pending-items.md)). On three nodes at the maximum rate Taba uses the 6 cores its nodes have (5.8 on average) and the laptop runs at about 90 % of its CPU; about one round in five, the whole virtual machine stalls for a second and the rate of that round falls for 10 to 20 s ([PRB-091](31-pending-items.md)): one of the three rounds above has it in its last 10 s, which the median does not show and the minimum does.

**NATS JetStream does not sustain the load with the same guarantee.** With `sync_interval: always` it makes one fsync per message, about 7 ms on the disk of Docker Desktop; its server warns `Readloop processing time: 2s` and `JetStream stream ... has high message lag`, and the Java client cancels every publication that waits more than 2 s (`response not registered in time`), hundreds per second. At the maximum rate the OMB rate controller drops the rate to zero, so `bench/systems/nats/max-rate-sweep` replaces `max-rate-1kb` by fixed rates of 500, 1 000, 2 000 and 5 000 per second: it sustained none, and its highest 10-second rate was about 380 per second. The charts leave it out of the bars and say so in a second line of the note (`generate-bench.mjs` finds that from the data); on a server disk with a cheaper fsync its number would be higher, and it depends far more on the disk than Taba's and RabbitMQ's, which group their fsyncs.

![Maximum throughput](../diagrams/img/en-US/bench-throughput.svg)

![End-to-end latency at 10,000 msg/s](../diagrams/img/en-US/bench-latency.svg)

![Messages per CPU core](../diagrams/img/en-US/bench-efficiency.svg)

![Server memory](../diagrams/img/en-US/bench-memory.svg)

![Messages per second over time, one node](../diagrams/img/en-US/bench-timeline-1.svg)

![Messages per second over time, three nodes](../diagrams/img/en-US/bench-timeline-3.svg)

![Messages processed over time, one node](../diagrams/img/en-US/bench-processed-1.svg)

![Messages processed over time, three nodes](../diagrams/img/en-US/bench-processed-3.svg)

## What the comparison does not hide

- The body Taba stores is **8 bytes larger** than the OMB payload (the publish time travels in it).
- Taba uses an HTTP/1.1 request per call and reads up to 100 messages per request; the others use a binary protocol with batching on the client.
- The Taba `fanout` copies the message to one queue per subscription; in the others the subscriptions are consumer groups on one log.
- **The `consume` profile needs memory for the backlog.** Taba keeps the backlog in memory, about 2.4 KB per message of 1 KB: the 1 GB backlog of the OMB (about 1.07 million messages) does not fit in a 3 GB server, and consumption collapses ([PRB-086](31-pending-items.md)). Run it with `--memory` large enough, and treat a cell that stops at the limit as a finding, not as a number.
- **The ack in consumption is not the same.** RabbitMQ and NATS ack each message; Taba acks each message too, but the OMB driver sends the acks of one read in **one request** (`POST …/messages/ack`, [D189](30-decisions.md)) and publishes in batches of up to 100 with 1 ms of `linger`, like the Kafka producer; Kafka and Redpanda commit an offset for a batch (`enable.auto.commit=false`, a commit after each read of the consumer, as the OMB driver does). That is cheaper per message, so the drain rate of Kafka and Redpanda is not comparable point to point with the others. They are not forced to a commit per message because that would slow them down artificially; the report says it in every `backlog-*` workload.

## Adding a system

A system is a folder, `bench/systems/<name>/`, with `compose.yml` (a service called `server`, a pinned image, a healthcheck, `cpus: ${BENCH_CPUS}` and `mem_limit: ${BENCH_MEMORY}`), `env-default` and `env-fsync`, `driver-default.yaml` and `driver-fsync.yaml` (`driver-<level>-<workload>.yaml` overrides one workload) and a `README.md`. `--systems <name>` finds it by the folder, with no change in `run.sh`. Any system with a driver in the pinned OMB (Pulsar, Redis, Artemis, RocketMQ and the others) can be added this way; the limit is the machine that runs it.

## Tests

`node --test bench/run.test.mjs bench/report.test.mjs` needs no Docker: it checks the plan and its refusals, the shape of every system folder and workload, and the report on the sample results of `bench/testdata/`. `bench/test.sh` runs the `smoke` profile against Taba, end to end. The CI job `bench` runs `--dry-run` and the Node.js tests; benchmarks are not run on GitHub runners, which give no useful number.

## Sources and evidence

- `bench/run.sh`, `bench/lib.sh` (the plan, the limits, the rotation, the cells), `bench/Dockerfile.runner` (the OMB with all its drivers and the Taba driver), `bench/systems/`, `bench/workloads/`, `bench/profile-*`, `bench/report.mjs`.
- Use cases `UC-BNC-001` to `UC-BNC-005` in [26a](26a-implemented-use-cases.md) and [26b](26b-planned-use-cases.md); tests in `bench/run.test.mjs`, `bench/report.test.mjs` and `bench/test.sh`.
- Decisions [D185](30-decisions.md) and [D195](30-decisions.md) (target and reference).
