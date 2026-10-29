# Local comparative benchmark

`bench/run.sh` runs the [OpenMessaging Benchmark](https://github.com/openmessaging/benchmark) (OMB) against Taba and
other messaging systems on **your own machine**, everything in Docker. The only requirement is Docker (Compose v2)
with at least **4 cores and 8 GB** available to it, on Linux, macOS or Windows (Git Bash).

```
bench/run.sh                                   # profile quick (about 1 hour), every system
bench/run.sh --profile standard                # 3 rounds of 5 minutes, six workloads
bench/run.sh --systems taba,kafka --workloads fanout-1kb
bench/run.sh --durability both                 # default and fsync
bench/run.sh --taba-image rafaelpontani/taba:0.1.9   # default: builds the image of the current commit
bench/run.sh --dry-run                         # prints the plan and the estimated time, runs nothing
bench/run.sh --report-only bench/results/<folder>
```

A result from a local machine is **never an absolute number**: it compares the systems with each other, on this
machine, in this run. Every report says so in its first line.

## How the comparison is kept fair

- One system at a time: the script starts its server, waits for it to be healthy, runs the workloads, removes the
  server and its volume, and only then starts the next one.
- The same `cpus` and `mem_limit` for every server (half of the cores, at most 4, and half of the memory, at most
  8 GB); the OMB runs in another container with the rest. `--cpus` and `--memory` override the split.
- One node per system. Three nodes on one machine would compete for the same cores and disks.
- The order of the systems rotates between rounds.
- The same OMB, the same runner image and the same payload files for all of them. Taba is measured by the driver of
  `connectors/omb`; Kafka and Redpanda by the OMB Kafka driver; RabbitMQ and NATS JetStream by their own.
- Two durability levels (`--durability default|fsync|both`), written next to each result:

| System | `default` | `fsync` |
|---|---|---|
| Taba | `storage.db.synchronous=NORMAL`, replica in memory | `synchronous=FULL` and `memory.limit=64MB` (overflow to disk) |
| Kafka | `acks=all`, no fsync per message | `flush.messages=1` |
| Redpanda | `acks=all`, topic write caching on | write caching off |
| RabbitMQ | quorum queue, persistent messages | the same (a quorum queue already fsyncs per confirmation) |
| NATS JetStream | file storage, default `sync_interval` | `sync_interval: always` |

## Workloads and profiles

| Workload | Shape | Measures |
|---|---|---|
| `simple-1kb` | 1 topic, 1 producer, 1 consumer, 1 KB, 10 000 msg/s | end-to-end latency under moderate load |
| `max-rate-1kb` | the same, with the OMB rate discovery | sustained throughput |
| `fanout-1kb` | 5 subscriptions, 5 000 msg/s | the cost of the copy per subscription |
| `keyed-1kb` | 16 keys, per-key order, 10 000 msg/s | order per key |
| `small-100b` | 100 bytes, 20 000 msg/s | fixed cost per message |
| `backlog-1kb` | 1 GB produced with the consumer stopped, then released | reading a backlog |

`quick` (default): 1 round of 2 minutes (+1 of warm-up) of the first four. `standard`: 3 rounds of 5 minutes of the six.
`smoke` (used by `bench/test.sh`): 1 minute of `simple-1kb`.

## Output

`bench/results/<date>-<commit>/` (ignored by git): `raw/*.json` (one OMB result per round, with its `.log`),
`machine.json`, `results.md` (median and min–max of the rounds per workload and system), `charts/*.svg` and, when a
cell failed, `failed.txt`.

## Adding a system

Create `bench/systems/<name>/` with `compose.yml` (a service named `server`, a pinned image, a healthcheck, and
`cpus: ${BENCH_CPUS}` / `mem_limit: ${BENCH_MEMORY}`), `env-default` and `env-fsync` (environment of the server for
each level; `env_file: env-${BENCH_DURABILITY}`), `driver-default.yaml` and `driver-fsync.yaml` (the OMB driver file,
with the server at `server:<port>`; `driver-<level>-<workload>.yaml` overrides it for one workload) and a `README.md`.
`--systems <name>` finds it by the folder; `run.sh` does not change. Any system with a driver in the pinned OMB
(`connectors/omb/OMB_VERSION`) can be added this way.

## Tests

```
node --test bench/run.test.mjs bench/report.test.mjs   # no Docker: the plan, the refusals, the shape of each system, the report
bench/test.sh                                          # Docker: the smoke profile against Taba, end to end
```

The CI job `bench` runs only `--dry-run` and the Node.js tests. Benchmarks are not run on GitHub runners.

## The trend of the publish path

`bench/cluster-trend.sh <cores per node> <connections per node> <image>...` compares builds of Taba itself with one node (factor 1) and three nodes (factor 3), `wrk` inside a Docker network, mutual TLS in the cluster ([doc 38](../doc/en-US/38-benchmarks.md#the-trend-of-the-publish-path-one-node-and-three-nodes)). A result from one machine only compares with another measured in the same run.
