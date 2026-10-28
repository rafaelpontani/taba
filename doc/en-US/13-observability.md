---
title: Observability
summary: How to monitor Taba — OpenTelemetry (OTLP) and Prometheus, node and topic metrics, cluster and topic conditions, health checks, traces, logs and events.
type: reference
status: stable
verified-on: 2026-10-28 @ 2447d5e
sources: [crates/taba-node/src/telemetry, crates/taba-node/src/http/metrics, crates/taba-node/src/http/topic_stats, crates/taba-node/src/http/operations.rs, crates/taba-node/src/cluster/health, crates/taba-node/src/config/otlp.rs]
---

# Observability

> Read first: [04-architecture.md](04-architecture.md). The parameters cited are in [22-configuration.md](22-configuration.md); what to do about each condition, in [23-operations.md](23-operations.md).

## Overview

Taba telemetry is **OpenTelemetry** (metrics, traces and logs). There are two outputs for machines and one reading for people:

| Output | Model | Content | Typical use |
|---|---|---|---|
| **OTLP** | *push*: each node sends what is its own | Metrics, traces and logs | OpenTelemetry Collector or any OTLP backend (Prometheus accepts OTLP) |
| **`GET /metrics`** | *pull*, **this node only** | Metrics only | Prometheus and compatibles, or the Collector with the `prometheus` receiver |
| **`GET /health/cluster`**, **`/metrics/topics/*`** | on demand, JSON | State, conditions and topic metrics | Operator and scripts, diagnosis without a backend |

OTLP and `/metrics` come from the **same instruments**: the metrics are the same, only the names change in the Prometheus format.

**No node queries another to build a metric.** Each node reports its own work and its own view of the cluster; the cluster total is computed by the backend (PromQL or the Collector). There is no central target list or aggregation in the server. (The exception is the JSON of `/health/cluster` and `/metrics/topics`, which query the nodes on demand.)

![Telemetry paths](../diagrams/img/en-US/observability-flow.svg)

Telemetry has **two levels**, treated differently:

| Level | Cardinality | OTLP and `/metrics` | Query routes |
|---|---|---|---|
| **Node and cluster** | Bounded (nodes, peers, routes) | Always | `/health/cluster` |
| **Topic** | **Unbounded** (topics × tag values) | **Opt-in**, with aggregation and ceiling | `/metrics/topics`, `/metrics/topics/{topic}` |

That is why topic metrics live on their own, paginated routes: the telemetry cost must not grow with the number of topics.

## Conventions (OpenTelemetry)

| Rule | How Taba complies |
|---|---|
| Own namespace | What belongs to Taba starts with `taba.` |
| Semantic convention when one exists | `http.server.*`, `process.*`, `messaging.*`. Only what has no standard name is `taba.*` |
| `taba.cluster.*` | The **cluster view** (members, quorum, channels, voting): each node reports its own; in the backend use `max`/`min`, **not** `sum`. The other `taba.*` are node work and **add up** |
| Units (UCUM) | No unit in the name: `s`, `By`, and counts in braces (`{message}`, `{flag}`). `1` only for ratios |
| Instruments | `updowncounter`: goes up and down and adds across nodes; `gauge`: does not add (limit, age, state); `counter`: only grows; `histogram`: distribution |
| Resource | `service.name=taba`, `service.namespace` (cluster name), `service.instance.id` (node ID, new at each start), `service.version`, `deployment.environment.name` |
| Attributes | Bounded cardinality: `http.route` is always the **template** (`/topics/{topic}/messages`), never the path; the **message ID is never a metric attribute** (only of traces and logs) |

## How to connect a collector

Point the node at the OTLP endpoint with the **standard `OTEL_*` variables**. Without `OTEL_EXPORTER_OTLP_ENDPOINT`, OTLP is **off**.

| Variable | What for |
|---|---|
| `OTEL_EXPORTER_OTLP_ENDPOINT` | Collector address (e.g. `http://collector:4317`) |
| `OTEL_EXPORTER_OTLP_PROTOCOL` | `grpc` (default) or `http/protobuf` |
| `OTEL_EXPORTER_OTLP_HEADERS`, `OTEL_EXPORTER_OTLP_CERTIFICATE`… | Authentication and TLS of the backend |
| `OTEL_METRIC_EXPORT_INTERVAL` | Metric send interval, in ms (default 60000) |
| `OTEL_TRACES_SAMPLER`, `OTEL_TRACES_SAMPLER_ARG` | Trace sampling (default `parentbased_traceidratio`, fraction **0.1**, provisional: D117) |
| `OTEL_RESOURCE_ATTRIBUTES` | `service.namespace=<cluster>,deployment.environment.name=<environment>` |

**Three ways to use it**, from the simplest to the most manual:

1. **Collector (recommended).** Each node pushes by OTLP; the Collector receives and forwards:

   ```yaml
   receivers:
     otlp:
       protocols: { grpc: {}, http: {} }
   processors:
     batch: {}
   exporters:
     otlphttp: { endpoint: https://backend.example }
   service:
     pipelines:
       metrics: { receivers: [otlp], processors: [batch], exporters: [otlphttp] }
       traces:  { receivers: [otlp], processors: [batch], exporters: [otlphttp] }
       logs:    { receivers: [otlp], processors: [batch], exporters: [otlphttp] }
   ```

2. **Prometheus directly**: receives OTLP (`--web.enable-otlp-receiver`) or scrapes `GET /metrics` of each node.
3. **Only `/health/cluster`**: with nothing installed, the JSON is enough for diagnosis.

> **Limits of the OTLP export (PRB-050):** the node reads only the essentials of the `OTEL_*` variables: the deadline of each send is fixed (10 s) and the per-signal variables (`OTEL_EXPORTER_OTLP_METRICS_ENDPOINT` etc.) may not be read. **OTLP metrics are assembled by hand** (with `opentelemetry-proto`), because the SDK does not accept aggregated data from outside.

## Node metrics

Exported by **each node**, always, through OTLP and on `GET /metrics`. The list below is generated from the code registry (`telemetry/metrics/defs.rs`): it is what the node **actually** registers (84 metrics). Usual attributes: `http.route`, `http.request.method`, `http.response.status_code`, `server.port` (which of the three ports) and the `taba.*` ones (`taba.phase`, `taba.outcome`, `taba.peer`…). The descriptions are the ones in the code, translated here.


#### `process.*`

| Metric | Type | Unit | Description |
|---|---|---|---|
| `process.uptime` | gauge | `s` | Time since the process started. |
| `process.cpu.utilization` | gauge | `1` | CPU use of the process, from 0 to 1 of the total cores, since the previous reading. |
| `process.memory.usage` | updowncounter | `By` | Resident memory of the process. |
| `process.thread.count` | updowncounter | `{thread}` | Operating-system threads of the process. |
| `process.open_file_descriptor.count` | updowncounter | `{file}` | Open file descriptors. |

#### `taba.node.*`

| Metric | Type | Unit | Description |
|---|---|---|---|
| `taba.node.connections` | updowncounter | `{connection}` | Open HTTP connections, per port. |
| `taba.node.state` | gauge | `{flag}` | 1 on the current state of the node. |
| `taba.node.drain.state` | gauge | `{flag}` | 1 on the drain state of the node. |
| `taba.node.drain.pending` | updowncounter | `{message}` | Messages still in memory (reaches 0 when the node is drained). |

#### `taba.storage.*`

| Metric | Type | Unit | Description |
|---|---|---|---|
| `taba.storage.memory.usage` | updowncounter | `By` | Memory occupied by message bodies. |
| `taba.storage.memory.limit` | gauge | `By` | Message memory limit (memory.limit). |
| `taba.storage.memory.messages` | updowncounter | `{message}` | Messages kept in memory. |
| `taba.storage.disk.messages` | updowncounter | `{message}` | Messages on disk. |
| `taba.storage.disk.free` | gauge | `By` | Free space on the storage.dir volume. |
| `taba.storage.overflow` | counter | `{message}` | Messages that went from memory to disk for lack of space. |
| `taba.storage.reads` | counter | `{read}` | Message reads, by where they were served (memory or disk). |
| `taba.storage.disk.errors` | counter | `{error}` | Disk input and output failures. |

#### `http.*`

| Metric | Type | Unit | Description |
|---|---|---|---|
| `http.server.request.duration` | histogram | `s` | Latency of the received requests; the count is the total of requests. |
| `http.server.active_requests` | updowncounter | `{request}` | Requests in progress. |
| `http.server.request.body.size` | histogram | `By` | Size of the request body (when the size is known). |
| `http.server.response.body.size` | histogram | `By` | Size of the response body (when the size is known). |

#### `taba.ratelimit.*`

| Metric | Type | Unit | Description |
|---|---|---|---|
| `taba.ratelimit.rejected` | counter | `{request}` | Requests refused with 429. |

#### `taba.auth.*`

| Metric | Type | Unit | Description |
|---|---|---|---|
| `taba.auth.requests` | counter | `{request}` | Token authentication: accepted, refused with 401 and refused with 403. |
| `taba.auth.tokens` | gauge | `{token}` | Access tokens that this node knows. |

#### `taba.delivery.*`

| Metric | Type | Unit | Description |
|---|---|---|---|
| `taba.delivery.ready` | updowncounter | `{message}` | Messages ready for delivery on this node (all topics). |
| `taba.delivery.fair.wait` | histogram | `s` | Time between publication and first delivery, per group band. |
| `taba.delivery.redeliveries` | counter | `{message}` | Deliveries of messages already delivered before. |
| `taba.delivery.dead_lettered` | counter | `{message}` | Messages sent to the dead-letter for reaching maxDeliveries. |
| `taba.delivery.longpoll.waiting` | updowncounter | `{request}` | Long polling calls waiting on this node. |
| `taba.delivery.longpoll.outcome` | counter | `{request}` | How the long polling calls ended. |
| `taba.delivery.take` | counter | `{request}` | Delivery requests to another owner (take), sent and received. |

#### `taba.retention.*`

| Metric | Type | Unit | Description |
|---|---|---|---|
| `taba.retention.expired` | counter | `{message}` | Messages discarded by ttl. |

#### `taba.fifo.*`

| Metric | Type | Unit | Description |
|---|---|---|---|
| `taba.fifo.deduplicated` | counter | `{message}` | FIFO queue publications discarded by deduplication. |
| `taba.fifo.groups.busy` | updowncounter | `{group}` | FIFO queue groups with a message in flight on this node. |
| `taba.fifo.forward` | counter | `{request}` | FIFO publications forwarded to the group's primary. |

#### `taba.cluster.*`

| Metric | Type | Unit | Description |
|---|---|---|---|
| `taba.cluster.members` | gauge | `{node}` | Members in this node's table, per state. |
| `taba.cluster.members.version` | gauge | `{version}` | Version (epoch) of the member table; divergence between nodes indicates incomplete propagation. |
| `taba.cluster.membership.changes` | counter | `{change}` | Node registrations and deregistrations, per phase. |
| `taba.cluster.bootstrap.failures` | counter | `{failure}` | Bootstrap failures of this node, per reason. |
| `taba.cluster.channel.notifications` | counter | `{message}` | New-message notices sent and received. |
| `taba.cluster.forwarded` | counter | `{request}` | Client calls forwarded to another node (drain), per result. |
| `taba.cluster.channel.connections` | updowncounter | `{connection}` | Channels open with other nodes (expected: cluster nodes − 1). |
| `taba.cluster.channel.state` | gauge | `{flag}` | 1 if the channel with the peer is connected, 0 if down. |
| `taba.cluster.channel.rtt` | histogram | `s` | Latency of the channel call (heartbeat and frames), per peer. |
| `taba.cluster.channel.reconnects` | counter | `{attempt}` | Channel reconnection attempts and result. |
| `taba.cluster.channel.queue.out` | updowncounter | `{signal}` | Frames in the output queue to the peer. |
| `taba.cluster.channel.queue.in` | updowncounter | `{batch}` | Messages from the peer read from the channel and not yet processed (input queue). |
| `taba.cluster.channel.queue.dropped` | counter | `{signal}` | Frames replaced, discarded by age or by output queue overflow. |
| `taba.cluster.definitions.conflicts` | counter | `{conflict}` | Topic or token definitions received with the same version and content different from what the node has. |
| `taba.cluster.vote` | counter | `{vote}` | Exclusion votes opened or cast and the result. |
| `taba.cluster.freezing` | gauge | `{node}` | Nodes currently in a freezing period. |
| `taba.cluster.tls.expires_in` | gauge | `s` | Time until expiry (notAfter) of the cluster certificate in use. |
| `taba.cluster.quorum.available` | gauge | `{flag}` | 1 if there are enough healthy nodes to vote (cluster.vote.min-voters). |
| `taba.cluster.condition` | gauge | `{flag}` | 1 for each current cluster condition. |
| `taba.cluster.recover` | counter | `{message}` | Messages recovered when a node returns, per direction. |

#### `taba.ring.*`

| Metric | Type | Unit | Description |
|---|---|---|---|
| `taba.ring.virtual_nodes` | gauge | `{vnode}` | Virtual nodes of this node on the ring (ring.virtual-nodes). |
| `taba.ring.blocks` | updowncounter | `{block}` | Blocks the node serves, per role and integrity. |

#### `taba.write.*`

| Metric | Type | Unit | Description |
|---|---|---|---|
| `taba.write.coordinated` | counter | `{message}` | Message operations coordinated by this node (publishing, invisibility, ack, dead-letter). |
| `taba.write.retries` | counter | `{retry}` | Retries per phase and per destination node. |
| `taba.write.rollbacks` | counter | `{rollback}` | Rollbacks sent for failing to close the quorum (phase 1). |
| `taba.write.commit_timeouts` | counter | `{timeout}` | Times write.commit.timeout ran out without a confirmed commit on a replica. |
| `taba.write.received` | counter | `{message}` | Registers, commits and rollbacks received. |
| `taba.write.pending` | updowncounter | `{message}` | Pending registers (without commit). |
| `taba.write.pending.expired` | counter | `{message}` | Registers discarded by expiry (without commit or rollback). |

#### `taba.sync.*`

| Metric | Type | Unit | Description |
|---|---|---|---|
| `taba.sync.blocks.active` | updowncounter | `{block}` | Blocks synchronizing now (delivery suspended on those blocks). |
| `taba.sync.duration` | histogram | `s` | Time to make a block healthy. |
| `taba.sync.ids.listed` | counter | `{id}` | IDs consolidated (union) in the source lists. |
| `taba.sync.messages.pulled` | counter | `{message}` | Messages obtained per source node. |
| `taba.sync.bytes` | counter | `By` | Body volume transferred in synchronization. |
| `taba.sync.log.events` | updowncounter | `{event}` | Events in the event database (sync.db) still to process. |
| `taba.sync.log.bytes` | gauge | `By` | Size of the event database on disk. |
| `taba.sync.phase` | gauge | `{flag}` | 1 on the current synchronization phase of the node. |
| `taba.sync.failures` | counter | `{failure}` | Synchronization failures. |

#### `taba.recover.*`

| Metric | Type | Unit | Description |
|---|---|---|---|
| `taba.recover.messages` | counter | `{message}` | Messages sent, applied or ignored by the recovery of the returning node. |

#### `taba.topic.*`

| Metric | Type | Unit | Description |
|---|---|---|---|
| `taba.topic.published` | counter | `{message}` | Messages published to the topic. |
| `taba.topic.consumed` | counter | `{message}` | Messages delivered on GET. |
| `taba.topic.acked` | counter | `{message}` | Acks of the topic. |
| `taba.topic.visible` | updowncounter | `{message}` | Messages ready for delivery. |
| `taba.topic.invisible` | updowncounter | `{message}` | Messages in processing. |
| `taba.topic.depth` | updowncounter | `{message}` | Messages not yet confirmed. |
| `taba.topic.oldest_timestamp` | gauge | `s` | Unix seconds of the oldest unconfirmed message. |
| `taba.topic.memory_bytes` | updowncounter | `By` | Bytes of the topic in memory. |
| `taba.topic.disk_bytes` | updowncounter | `By` | Bytes of the topic on disk. |
| `taba.topic.condition` | gauge | `{flag}` | 1 for each current topic condition. |


> **Linux only:** `process.memory.usage`, `process.thread.count`, `process.open_file_descriptor.count` and `process.cpu.utilization` read `/proc` (PRB-026).

### Names in Prometheus

| Rule | Example |
|---|---|
| Dots become `_` | `taba.storage.memory.limit` → `taba_storage_memory_limit_bytes` |
| Unit becomes a suffix | `s` → `_seconds`; `By` → `_bytes`; `1` → `_ratio` |
| Unit in braces disappears | `taba.cluster.quorum.available` → `taba_cluster_quorum_available` |
| Counters gain `_total` | `taba.write.retries` → `taba_write_retries_total` |
| Histograms become `_bucket`/`_sum`/`_count` | `http.server.request.duration` → `http_server_request_duration_seconds_bucket` |
| Resource becomes `job`, `instance`, `target_info` | `service.instance.id` → `instance` |

One target per node: Prometheus keeps one series per node. The total comes from aggregation (`sum(rate(taba_write_coordinated_total[5m]))` adds work; `max(taba_cluster_members)` takes the cluster view). Adding on the server would break the counters.

### Alert examples (PromQL), to be calibrated

| Alert | Expression |
|---|---|
| Node down | `up{job="taba"} == 0` |
| Cluster without quorum | `min(taba_cluster_quorum_available) == 0` |
| Diverging member tables | `max(taba_cluster_members_version) != min(taba_cluster_members_version)` |
| Memory near the limit | `taba_storage_memory_usage_bytes / taba_storage_memory_limit_bytes > 0.9` |
| Write failures | `sum(rate(taba_write_coordinated_total{taba_outcome="failed"}[5m])) > 0` |
| Cluster certificate near expiry | `min(taba_cluster_tls_expires_in_seconds) < 30 * 86400` |

## Cluster conditions

`GET /health/cluster` returns a **list of conditions** (not a single state), derived from the signals of each node. It also exists as the `taba.cluster.condition` metric.

| Condition | Severity | When |
|---|---|---|
| `ok` | — | None of the below |
| `member-unreachable` | warning | Channel with some node down or reconnecting, still before the vote |
| `node-frozen` | warning | Node(s) in a freezing period |
| `syncing` | warning | Blocks synchronizing; delivery is suspended on them |
| `under-replicated` | warning | Blocks with fewer healthy replicas than `replication.factor`, but **at or above** `write.quorum` — transient and normal |
| `write-quorum-at-risk` | **critical** | Blocks with healthy replicas **below** `write.quorum`, or with the **primary** down and still in the table: writes may fail until the node is excluded |
| `quorum-lost` | **critical** | Fewer healthy nodes than `cluster.vote.min-voters`: no exclusion takes effect. It is **not** about message durability |
| `membership-divergent` | **critical** | Member tables with **different versions** between nodes |
| `storage-pressure` | warning | Free space of `storage.dir` below `stats.disk.free-min` (without a value, it never fires) |
| `upgrade-stalled` | warning | The upgrade that retires earlier versions stopped on a node that did not finish draining within `drain.timeout` plus 30 s |
| `members-missing` | warning | The time of the probe of the old IPs (`cluster.restore.probe.timeout`) ended and the cluster has fewer active members than the previous life's table. Goes away when the table completes |
| `reduced-profile` | warning | The node runs the one-node profile with something relaxed (`replication.factor=1`, `cluster.tls.enabled=false` or `auth.dev-token`). Informative: it does not bring `/health/ready` down; `health.single-node-condition=false` hides it |
| `previous-life-pending` | warning | The cluster is still being restored from the previous lives (the vote has not closed): the FIFO queues receive and do not deliver. Goes away when the vote closes or the operator forces it |

> **How the node computes it (provisional, D102–D104).** *Healthy* is this node plus the peers with a connected channel. `quorum-lost` holds when the healthy ones do not reach `min(cluster.vote.min-voters, members)` (with a single node there is no one to exclude). A replica is *healthy* when the block's owner is `active` in the table and the channel with it is up. `under-replicated` appears when there are more blocks below replication than below the quorum; `syncing` when some block owner is `syncing`. `membership-divergent` compares the table version of each node.

Replication degradation is a property of the **cluster**, not of the topic: since the blocks come from the message ID, the messages of a topic spread over all the blocks.

### State and response of `/health/cluster`

```bash
curl -s localhost:9090/health/cluster
# {"collectedAt":"…","conditions":[],"nodes":[{"nodeId":"…","reachable":true,"status":{
#    "drainState":"active","state":"active","version":"0.1.0",
#    "signals":{"blocksBelowQuorum":0,"blocksBelowReplication":0,"quorumAvailable":true,…},
#    "storage":{"diskFreeBytes":9322598400,"diskMessages":0,"memoryMessages":0,…}}}],
#  "partial":false,"scope":"cluster","summary":{"members":{"active":1},"membersVersion":0,
#  "quorumAvailable":true,"syncingBlocks":0},"worstCondition":"ok"}
```

- **`scope=cluster`** (default): queries **all the nodes** (`GET /cluster/status`) and returns the state of each, a summary and the conditions. A node that does not answer appears with `reachable: false` (*best-effort*).
- **`scope=node`:** only the node that serves, without querying others.
- `worstCondition` is the one of highest severity; `partial` indicates nodes that did not answer.

## Topic metrics

They stay **outside** the default export and are read through dedicated routes ([17-operations-api.md](17-operations-api.md)):

| Route | Description |
|---|---|
| `GET /metrics/topics` | **Paginated** list with the summary metrics of each topic |
| `GET /metrics/topics/{topic}` | Detail of **one** topic, with the largest groups |
| `GET /metrics/topics/{topic}/groups` | One group per line |

Example (a single node):

```bash
curl -s localhost:9090/metrics/topics/pedidos
# {"ackRate":0.0033,"acked":1,"consumed":1,"depth":0,"invisibleMessages":0,"visibleMessages":0,
#  "memoryMessages":0,"diskMessages":0,"published":1,"publishedBytes":15,
#  "publishLatency":{"p50":0.01,"p95":0.01,"p99":0.01},"consumeLatency":{…},
#  "groups":0,"topGroups":[],"noisyShare":0.0,"conditions":[],"worstCondition":"ok",…}
```

### Node view and cluster view

| Value | **Node** view | **Cluster** view |
|---|---|---|
| `visibleMessages`, `invisibleMessages`, `depth` | Only of the messages of which **this node is the primary** | **Sum** of all nodes |
| `oldestMessageAt` | The oldest among those of which this node is primary | **The oldest** among all (a node without messages of the topic does not count; absent, never zero) |

**Only the primary counts**: otherwise the sum would be multiplied by the replication factor. The default is the **cluster** view; `scope=node` gives the node's. A node that does not answer is left out and the result is marked `partial` (the sums become a **floor**). The values are an **approximate snapshot**, not a transactional value. With **a single node**, `scope=cluster` and `scope=node` coincide and the result is never `partial`.

> ⚠️ **Current limit (verified on a 3-node cluster):** the **aggregation between nodes is not implemented yet**. `scope=cluster` returns **the view of the node that serves** (and `partial` is always `false`), the same as `scope=node`. On a topic with 12 published messages, each node answered `depth: 4` (those of which it is primary), and the cluster response also gave 4, not 12. To get the total, **add in the backend** from the `/metrics` of each node (with `telemetry.topics.mode` on) or query **each node** with `scope=node` and add the quantities. The counters (`published`, `consumed`, `acked`) are **per node, since the process started**. The internal route `GET /cluster/metrics/topics` and the parameter `stats.topics.cache-ttl` (no effect today) are the missing part. Recorded in [31-pending-items.md](31-pending-items.md).

### Topic conditions

Also a **list**, with `worstCondition`. The limits come from `stats.topics.*`; **a limit without a value turns off the condition that depends on it**.

| Condition | Severity | When |
|---|---|---|
| `ok` | — | None of the below |
| `idle` | info | No publishing or consumption for `stats.topics.idle-after` |
| `backlog` | warning | `depth` > `stats.topics.backlog.depth` **or** `oldestAge` > `stats.topics.backlog.age` |
| `stalled` | warning | There are messages (`depth > 0`) but **no consumption** for `stats.topics.stalled-after`: consumer stopped or absent |
| `noisy-group` | info | Two or more groups and the largest occupies more than `stats.topics.noisy-group-share` of `depth` (default 50%). In FIFO it indicates the **hot group** |
| `group-stalled` | warning | FIFO only: the head of some group was delivered more than `stats.topics.group-stalled-deliveries` times without an ack |
| `memory-pressure` | info | Most of the topic's messages are on disk (`diskMessages` > `memoryMessages`) |

### Export of topic metrics (OTLP and Prometheus)

Opt-in through `telemetry.topics.mode`, with the **same effect** on OTLP and `/metrics`:

| Mode | Default | What it exports | Cardinality |
|---|---|---|---|
| `off` | ✔ | Nothing per topic | None |
| `by-tags` | | Metrics aggregated **by combination of tags** chosen in `telemetry.topics.group-by-tags`, in the attribute `taba.topic.tag.<key>` | By the tag values |
| `per-topic` | | One series per topic (`messaging.destination.name`), up to `telemetry.topics.max-topics` (1000 if absent); the excess becomes `_other` | By the ceiling |

Metrics exported in these modes: `taba.topic.published`, `consumed`, `acked` (counters), `visible`, `invisible`, `depth`, `memory_bytes`, `disk_bytes` (updowncounters), `oldest_timestamp` (gauge, Unix seconds; no series on a node that has no messages) and the `taba.topic.condition` condition. The SDK still limits the cardinality of any metric and marks the excess with `otel.metric.overflow`.

**Topic tags** are key/value pairs (`POST /topics`, `PATCH`). In **traces and logs** they become `taba.topic.tag.<key>` attributes; in metrics, only in the topic ones according to the mode; **never** in node metrics. They do not affect message behavior.

## Health checks

Light routes for orchestrators and load balancers, **without** fan-out:

| Route | Means | Failure → |
|---|---|---|
| `GET /health/live` | The process is up and answering | Restart the process |
| `GET /health/ready` | The node is **able to receive traffic**: registered, **synchronization finished**, outside bootstrap and return; **in drain it answers `503`** | Take the node out of rotation |

`/health/ready` only answers `200` when the whole synchronization finishes ([07-cluster.md](07-cluster.md)). While synchronizing, the node forwards client calls to another node, but it should still stay out of the load balancer's rotation.

## Access to the operations routes

The operations routes are `/metrics`, `/metrics/topics/*`, `/health/*` and `/node/drain`. They live on the **operations port** (`operations.port`, default 9090, always open) and have **no authentication** there (decisions D63 and D177): whoever installs the node protects the port with the network. One optional restriction on top of it:

| Parameter | Effect |
|---|---|
| `operations.allowed-ips` | List of authorized IPs and CIDR ranges. From outside the list: `403 ops.ip-not-allowed`. Applies to the operations port |

| Configuration | Behavior |
|---|---|
| Default | The operations port answers whoever reaches it: the network is the only protection |
| With `operations.allowed-ips` | Only the listed IPs access the operations port |

**Scraping the metrics: which port, with or without a token.**

| Where the collector runs | Port | Authentication |
|---|---|---|
| Inside the infrastructure (Prometheus, Telegraf, OpenTelemetry Collector) | `operations.port` (9090), `GET /metrics` | **None**: this is the door for collectors |
| Outside, or any place that only reaches the client port | `api.port` (8080), `GET /metrics` | **`Authorization: Bearer <token>`** with **any** valid token (no particular permission) |

The probes `GET /health/live` and `GET /health/ready` need no token on either port, because an orchestrator's probe cannot send one. The other operations routes (`/health/cluster`, `/metrics/topics/*`, `/node/drain`, backup) exist only on the operations port; outside, the client API has their equivalents behind a token (`GET /cluster/status`, `/stats/topics*`, `/nodes/{nodeId}/…`).

- **Production:** use the IP list (Prometheus, load balancer, operators) and keep the operations port out of any public network or ingress: `/metrics` exposes topic names and tags and `/health/cluster` exposes the topology.
- The list must include the **load balancer** that calls `/health/ready` and the **Prometheus or Collector** that scrapes.
- The IP evaluated is the **client IP resolved** by the same rule as the rate limit: the connection's or, with `client-ip.headers.enabled`, the header's (`X-Forwarded-For`), accepted only from `client-ip.trusted-proxies`. **With an empty proxy list, any client forges the header** and bypasses the IP list (PRB-015).
- OTLP sending is an **outgoing** connection and does not go through these restrictions. The `/cluster/*` routes stay on the cluster port and are not affected.
- The node **does not start** if one of the three ports is in use or repeated, or if the list has an invalid entry.
- Pending item **D76:** `kubelet` and the load balancer call `/health/*` from IPs that may not be in the list; today the list filters the health checks too.

## Traces

One trace per client operation, with internal spans per step. HTTP spans follow the convention (`{method} {route}`, `http.*`); messaging ones carry `messaging.system=taba`, `messaging.destination.name` (the topic), `messaging.operation.type` (`send`, `receive`, `settle`) and `messaging.message.id`.

| Operation | Trace structure |
|---|---|
| **Publishing** | Root `POST /topics/{topic}/messages` → `taba.write.register` (one per destination node) → `taba.write.commit` → `taba.write.rollback` if the quorum did not close. The root **ends successfully when the quorum confirms the commit**, without waiting for the other replicas |
| **Consumption and ack** | Request root → read from memory or disk |
| **Synchronization** | One trace per block (`taba.sync.block`, `taba.block.id`), with `taba.sync.fetch` per call to a source |
| **Join, leave and voting** | `taba.cluster.join`, `taba.cluster.unregister` and `taba.vote.open` as root, with a span per node queried |

- Context is propagated between nodes by the W3C `traceparent` header: the complete write appears in a single trace even passing through several nodes (D119).
- Attributes: `taba.topic.tag.<key>`, `messaging.message.id` (**high cardinality: only in traces and logs**), `taba.block.id`, `taba.node.id`, `taba.peer.id`, `taba.attempt`. The span that fails carries `otel.status_code=ERROR`.
- The `/health`, `/metrics`, `/node` and `/cluster` routes generate no trace; the channel heartbeat does not either.
- **Tail sampling:** the SDK samples by fraction, at the start. To **always keep** traces with a failure or durability event (error to the client, rollback, commit ceiling, message loss), the node marks those spans with `taba.durability_event=true` and the **Collector** applies `tail_sampling`.

## Logs

**Structured** logs: on `stdout` with `telemetry.logs.format` (`text`, default, `color` or `json`) and, with OTLP, as *log records* with `trace_id` and `span_id`. The level is `telemetry.logs.level` (`trace`, `debug`, `info`, `warn`, `error`; `info` if absent). Per-message logs on the hot path stay at `debug`.

> The `stdout` log **does not yet carry `trace_id`/`span_id`** (PRB-050); only OTLP does.

The node's spans (request, call to a node, cluster operation) exist for OTLP only: without an attached OTLP pipeline they are not even created, and the `stdout` lines do not carry their fields. The metrics registry is on the path of every request: an existing series is updated under read locks and atomics, and only a series seen for the first time takes the write lock.

| Event | Level |
|---|---|
| Message register/commit failure (per attempt) | `warn` |
| Write aborted with rollback (quorum did not close) | `error` |
| Pending register expired; commit ceiling exhausted on a replica | `warn` |
| Channel dropped/reconnected/exhausted; vote opened, cast and result | `warn` / `info` |
| Node registered, deregistered, rejected (freezing) | `info` |
| Token created, changed or revoked (with `id`, **never** the secret) | `info` |
| `401`/`403` (with the token `id`, if any; sampled at volume) | `warn` / `debug` |
| Drain started, finished, canceled, node returning as new | `info` |
| Drain that exceeded `drain.timeout` or had an I/O error | `error` |
| Start with the previous life's disk: set aside and restoration | `info` / `warn` |
| Bootstrap failure | `error` |
| Start and end of block synchronization | `info` |
| **Message loss** (ID, block, nodes queried) | `error` |
| Node return and recovered messages | `info` |
| Overflow summary (`taba.storage.overflow`, every 60 s, only if there was one; D118) | `info` |
| Disk I/O error | `error` |

## Events

Point-in-time events (**not** health conditions), emitted as *log records* with `event.name`. They always carry the topic in `messaging.destination.name`.

| `event.name` | When | Level |
|---|---|---|
| `taba.distributor.no_target` | Publishing to a distributor with no queue matching | `info` |
| `taba.distributor.partial` | At least one copy on a distributor failed | `warn` |
| `taba.volatile.deleted` | Volatile queue deleted for idleness | `info` |
| `taba.volatile.check_blocked` | The manager did not complete the final check: some node did not answer | — |
| `taba.message.dead_lettered` | Message sent to the dead-letter on reaching `maxDeliveries` | `warn` |
| `taba.message.deduplicated` | FIFO publication discarded by deduplication (at volume, use `taba.fifo.deduplicated`) | `debug` |
| `taba.message.expired` | Message discarded by `ttl` (at volume, use `taba.retention.expired`) | `info` |
| `taba.message.lost` | **Message loss** in synchronization. There is no metric or health condition: whoever wants to alert uses the event in the log backend | `error` |
| `taba.message.recovered` | Message returned by a node that came back | `info` |

The system **does not send notifications on its own**: alerts belong to the backend.

## Pending items in this area

D90 (defaults of the condition thresholds), D91 (ceiling and tags of topic metrics), D102 (`quorumAvailable`), PRB-026 (process metrics only on Linux), PRB-050 (OTLP and `stdout`): [31-pending-items.md](31-pending-items.md).

## Sources and evidence

- `telemetry/metrics/defs.rs` (the metrics table above is generated from here), `telemetry/otlp`, `telemetry/prometheus`, `telemetry/trace`, `telemetry/process`, `http/metrics`, `http/topic_stats`, `http/operations.rs`, `cluster/health/mod.rs` (the 8 conditions and the severity), `engine/messages/stats`.
- Examples of `/health/cluster` and `/metrics/topics/{topic}` copied from a real node.
- Use cases: UC-OPS (observability, statistics, metrics and health), UC-CFG; see [26a](26a-implemented-use-cases.md). Tests: `tests/metrics.rs`, `topic_stats.rs`, `otlp.rs`, `operations.rs`.
- History: `11-observabilidade.md`, `06b-api-operacao.md`.
