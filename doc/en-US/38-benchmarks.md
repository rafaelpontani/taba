---
title: Benchmarks
summary: The local comparative benchmark (bench/run.sh): the OpenMessaging Benchmark against Taba, Kafka, Redpanda, RabbitMQ and NATS JetStream in Docker on one machine, how the comparison is kept fair, the durability levels, the workloads, the report and how to add a system. No number is published yet.
type: guide
status: partial
verified-on: 2026-10-28 @ 2447d5e
sources: [bench/cluster-trend.sh, bench/run.sh, bench/lib.sh, bench/report.mjs, bench/Dockerfile.runner, bench/run.test.mjs, bench/report.test.mjs, bench/test.sh, connectors/omb/OMB_VERSION]
---

# Benchmarks

> Read first: [21-connectors.md](21-connectors.md#openmessaging-benchmark-driver-connectorsomb) (the Taba driver for the OMB) and [11-durability.md](11-durability.md).

Taba's speed and cost goals (RNF-02) are measured by a script that anyone can run: `bench/run.sh`. It needs only Docker (Compose v2) with at least **4 cores and 8 GB**, no cloud account, and it compares Taba with Kafka, Redpanda, RabbitMQ and NATS JetStream using the [OpenMessaging Benchmark](https://github.com/openmessaging/benchmark) (OMB) at the commit pinned in `connectors/omb/OMB_VERSION`. **No result is published yet**: the tool is ready, and the numbers will come from a `standard` run made by the maintainer, with the machine described.

A result from one machine is **never an absolute number**. It compares the systems with each other, on that machine, in that run. Every `results.md` says so in its first lines.

## How to run

```bash
bench/run.sh                                   # profile quick (about 1 hour), every system
bench/run.sh --profile standard                # 3 rounds of 5 minutes, six workloads
bench/run.sh --systems taba,kafka --workloads fanout-1kb
bench/run.sh --durability both                 # default and fsync
bench/run.sh --taba-image rafaelpontani/taba:0.1.9   # default: builds the image of the current commit
bench/run.sh --dry-run                         # the plan and the estimated time; runs nothing
bench/run.sh --report-only bench/results/<folder>
```

`--cpus` and `--memory` override the limit given to each server. Windows runs it in Git Bash with Docker Desktop. `Ctrl+C` removes the containers and the volumes.

## The trend of the publish path: one node and three nodes

`bench/cluster-trend.sh <cores per node> <connections per node> <image>...` measures **only Taba**, to compare two builds of it (before and after a change), with one node (`replication.factor=1`) and with three nodes (factor 3), the same cores per node, `wrk` spread across the nodes inside one Docker network, and the production configuration (mutual TLS in the cluster, the root token of `taba init`). It prints, per round, the total publications per second, the p50 and p99 of each node and the failures (`non2xx`), and the CPU of each node. `BODY_BYTES`, `ROUNDS`, `SECONDS_PER_ROUND` and `NODES` change the message size, the rounds, their length and which sizes run. As everywhere in this document, a result from one machine is only comparable with another measured **in the same run**.

## How the comparison is kept fair

- **One system at a time.** The script starts the server, waits for it to be healthy, runs the workloads, removes the server **and its volume**, and only then starts the next one.
- **The same limits.** Every server gets the same `cpus` and `mem_limit` (half of the Docker cores, at most 4, and half of its memory, at most 8 GB). The OMB runs in another container with the rest, and its JVM heap follows its limit.
- **One node per system.** Three nodes on one machine would compete for the same cores and disks and give misleading numbers; the three-node cluster is a cloud measurement, outside this script.
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

Profiles are files `bench/profile-*`: `quick` (1 round of 2 minutes, the first four workloads), `standard` (3 rounds of 5 minutes, the six) and `smoke` (1 minute of `simple-1kb`, used only by `bench/test.sh`). The warm-up minute does not enter the percentiles.

## The report

`bench/results/<date>-<commit>/` (ignored by git) holds `raw/*.json` (one OMB result per round, with its `.log`), `machine.json`, `results.md`, `charts/*.svg` and, if a cell failed, `failed.txt`. `results.md` shows, per workload and durability, a table with the **median of the rounds and, in parentheses, the minimum and the maximum** of the published and consumed throughput and the end-to-end p50, p99 and p99.9. It does not name a winner; it ends with the known differences of each system.

## What the comparison does not hide

- The body Taba stores is **8 bytes larger** than the OMB payload (the publish time travels in it).
- Taba uses an HTTP/1.1 request per call and reads up to 100 messages per request; the others use a binary protocol with batching on the client.
- The Taba `fanout` copies the message to one queue per subscription; in the others the subscriptions are consumer groups on one log.
- One node: the replication cost of a cluster is not measured here.

## Adding a system

A system is a folder, `bench/systems/<name>/`, with `compose.yml` (a service called `server`, a pinned image, a healthcheck, `cpus: ${BENCH_CPUS}` and `mem_limit: ${BENCH_MEMORY}`), `env-default` and `env-fsync`, `driver-default.yaml` and `driver-fsync.yaml` (`driver-<level>-<workload>.yaml` overrides one workload) and a `README.md`. `--systems <name>` finds it by the folder, with no change in `run.sh`. Any system with a driver in the pinned OMB (Pulsar, Redis, Artemis, RocketMQ and the others) can be added this way; the limit is the machine that runs it.

## Tests

`node --test bench/run.test.mjs bench/report.test.mjs` needs no Docker: it checks the plan and its refusals, the shape of every system folder and workload, and the report on the sample results of `bench/testdata/`. `bench/test.sh` runs the `smoke` profile against Taba, end to end. The CI job `bench` runs `--dry-run` and the Node.js tests; benchmarks are not run on GitHub runners, which give no useful number.

## Sources and evidence

- `bench/run.sh`, `bench/lib.sh` (the plan, the limits, the rotation, the cells), `bench/Dockerfile.runner` (the OMB with all its drivers and the Taba driver), `bench/systems/`, `bench/workloads/`, `bench/profile-*`, `bench/report.mjs`.
- Use cases `UC-BNC-001` to `UC-BNC-004` in [26a](26a-implemented-use-cases.md) and [26b](26b-planned-use-cases.md); tests in `bench/run.test.mjs`, `bench/report.test.mjs` and `bench/test.sh`.
- Decision [D185](30-decisions.md).
