---
title: Operations API
summary: Manual of the routes for the operator, Prometheus and orchestrator — health, node and topic metrics, drain and S3 backup — with parameters, DTOs and examples.
type: reference
status: stable
verified-on: 2026-10-15 @ 56121c9
sources: [crates/taba-node/src/http/backup.rs, crates/taba-node/src/http/operations.rs, crates/taba-node/src/http/metrics, crates/taba-node/src/http/topic_stats, crates/taba-node/src/cluster/drain, crates/taba-node/src/cluster/health, crates/taba-node/src/cluster/state]
---

# Operations API

> Common conventions: [15-api-overview.md](15-api-overview.md). The meaning of the metrics and conditions: [13-observability.md](13-observability.md). What to do about them: [23-operations.md](23-operations.md).

**Port:** `operations.port` or, if absent, the **client port**. **Authentication: none**; protection is by network (own port, `operations.allowed-ips`). IP outside the list: `403 ops.ip-not-allowed`. Without `operations.port`, these routes **do not use the client token**.

> The `/node/drain` routes apply **only to the node that receives the call**: the operator calls them on the node (operations port directly), **not through the load balancer**.

> **A client with a token has copies of these routes in the client API** ([16-client-api.md](16-client-api.md)): `GET /cluster/status`, `/nodes/{nodeId}/metrics`, `/nodes/{nodeId}/drain`, `/nodes/{nodeId}/backup`..., addressed by `nodeId` (they work through the load balancer) and for the **root token**. The routes of this document do not change: no token, for continuous monitoring.

| Verb | Route | Rate limit | Success |
|---|---|---|---|
| `GET` | `/health/live` | exempt | `200` `HealthResponse` |
| `GET` | `/health/ready` | exempt | `200` / `503` `HealthResponse` |
| `GET` | `/health/cluster` | exempt with `scope=node`; `ratelimit.ops.rps` with `scope=cluster` | `200` `ClusterStatus` |
| `GET` | `/metrics` | exempt | `200` Prometheus text (this node only) |
| `GET` | `/metrics/topics` | `ratelimit.ops.rps` | `200` `Page<TopicStats>` |
| `GET` | `/metrics/topics/{topic}` | same | `200` `TopicStatsDetail` |
| `GET` | `/metrics/topics/{topic}/groups` | same | `200` `Page<GroupStats>` |
| `POST` | `/node/drain` | admin | `202` (or `200` if already in drain) |
| `GET` | `/node/drain` | admin | `200` `DrainStatus` |
| `DELETE` | `/node/drain` | admin | `200` or `202` `DrainStatus` |
| `GET` | `/node/backup` | exempt | `200` `BackupStatus` (or `422 backup.not-configured`) |
| `POST` | `/node/backup/flush` | admin | `200` `FlushReport` (or `422 backup.not-configured`) |
| `POST` | `/cluster/backup/flush` | admin | `200` `ClusterFlushReport` (or `422 backup.not-configured`) |
| `POST` | `/cluster/stop` | admin | `200` `StopReport` |
| `POST` | `/cluster/restore/force` | admin (root token on the client API) | `200` `RestoreForce` |

"Exempt" = never answers `429`: they are light and called by machines; a `429` on `/health/ready` would make the load balancer remove the node from rotation by mistake.

---

## Health

### `GET /health/live`

The process is up and answering. Failed → the orchestrator **restarts** the process. `200 {"health":"live"}`.

### `GET /health/ready`

The node is **able to receive traffic**: registered, with **synchronization finished**, outside bootstrap and return. **In drain** (`draining` or `drained`) it answers `503`.

| Status | Body |
|---|---|
| `200` | `{"health":"ready"}` |
| `503` | `{"health":"not-ready","reason":"…"}` |

`reason`: `bootstrapping` (joining), `syncing` (synchronizing), `returning` (coming back after being excluded), `draining`, `drained` or `failed` (disk full or corrupted database). The two endpoints do **not** return `ErrorResponse`: the body is always `HealthResponse`, so the orchestrator can read the reason without handling errors.

### `GET /health/cluster`

Cluster state for **people and scripts**: conditions, summary and the state of each node. **It does not repeat the metrics.**

| Parameter | Default | Description |
|---|---|---|
| `scope` | `cluster` | `cluster`: queries **all nodes** (`GET /cluster/status`), *best-effort*: a node that does not answer appears with `reachable: false`. `node`: **local** read, without querying other nodes |

On a single node (development mode) the two scopes are the same. Errors: `400 validation.failed` (unknown `scope`), `403`, `429` (only with `scope=cluster`).

```json
{"collectedAt":"2026-10-13T17:13:10.812Z","partial":false,"scope":"cluster","worstCondition":"ok","conditions":[],
 "summary":{"members":{"active":3},"membersVersion":3,"quorumAvailable":true,"syncingBlocks":0},
 "nodes":[{"nodeId":"01a13640-7774-…","reachable":true,"status":{ "…": "NodeStatus" }}, …]}
```

During a real failure (a node brought down with `kill`), the same route showed: `conditions: [write-quorum-at-risk, member-unreachable, under-replicated]`, then `[node-frozen]` (excluded node).

**`ClusterStatus`:**

| Field | Description |
|---|---|
| `scope`, `collectedAt` | As requested; instant of collection |
| `partial` | `true` if some node did not answer |
| `worstCondition` | The one of highest severity (`ok` if none) |
| `conditions` | List of `{condition, severity}` ([13-observability.md](13-observability.md)) |
| `summary` | `members` (count per state), `membersVersion`, `quorumAvailable`, `syncingBlocks`, `excludedCutoff` (the cut-off date of the log of excluded messages: the most recent `excludedSince` among the active members; `null` with none) |
| `nodes` | One per member: `nodeId`, `reachable`, `status` (`NodeStatus`, or `null`) |

**`NodeStatus`:** `nodeId`, `version`, `clusterName`/`environment` (if configured), `mode` (`production`/`development`), `state` (`joining`, `active`, `syncing`, `returning`, `failed`), `drainState`, `startedAt`, `collectedAt`, `excludedSince` (since when the node records the log of excluded messages), `storage` (`memoryUsedBytes`, `memoryLimitBytes`, `memoryMessages`, `diskMessages`, `diskFreeBytes`) and `signals` (`membersVersion`, `quorumAvailable`, `channels`, `syncingBlocks`, `blocksBelowReplication`, `blocksBelowQuorum`, `frozenNodes`).

---

## Metrics

### `GET /metrics`

The metrics of **this node**, in Prometheus text (`text/plain; version=0.0.4`; `Accept: application/openmetrics-text` asks for OpenMetrics). **One target per node**: the node does not query others. No parameters. Errors: `403 ops.ip-not-allowed`; `404 route.not-found` if `telemetry.prometheus.enabled` is `false`. The list of metrics is in [13-observability.md](13-observability.md).

### `GET /metrics/topics`

**Paginated** list of per-topic metrics, **always by name** (there is no `sort`: the metrics change all the time).

| Parameter | Description |
|---|---|
| `limit`, `cursor` | Pagination |
| `scope` | `cluster` (default) or `node`. ⚠️ **Today `cluster` does not aggregate across nodes**: it returns the view of the node that serves ([13-observability.md](13-observability.md)) |
| `tag.<key>` | Filters by tag |
| `condition` | Filters by topic condition (`backlog`, `stalled`…): "which topics have a problem?" |
| `fields` | Restricts the fields (`fields=name,depth,oldestAge`); `name` and `id` always come |
| `groupBy` | Name of a tag: **one row per tag value** (`TopicStatsGroup`) instead of one per topic |

### `GET /metrics/topics/{topic}`

One topic, in detail: `TopicStats` plus `topGroups` (up to `stats.topics.top-groups`, default 10). Parameter `scope`. `404 topic.not-found`. On a distributor, only its metrics (`distributor`: `received`, `copies`, `copyErrors`).

### `GET /metrics/topics/{topic}/groups`

Metrics **per `groupId`** to find the noisy producer. Paginated by `groupId` (no `sort`).

| Parameter | Description |
|---|---|
| `limit`, `cursor` | Pagination |
| `groupId` | Only that group |
| `top` | The `N` largest by `messages`, **without pagination** (does not combine with `cursor`: `400`) |
| `scope` | `cluster` or `node` |

A group with no messages and no activity in the window disappears from the response. The shared counter of the groups above `delivery.fair.max-groups` comes **without** `groupId` and with `overflow: true`.

**`TopicStats` (main fields):** `id`, `name`, `type`, `tags`, `createdAt`, `published`, `publishedBytes`, `publishErrors`, `publishRate`, `publishLatency`/`consumeLatency` (`{p50,p95,p99}` in s), `consumed`, `acked`, `consumeRate`, `ackRate`, `visibleMessages`, `invisibleMessages`, `depth`, `oldestMessageAt`/`oldestAge` (absent without messages), `bytes`, `memoryMessages`, `diskMessages`, `memoryBytes`, `diskBytes`, `memoryShare`, `groups`, `noisyShare`, `worstCondition`, `conditions`, `partial`, `collectedAt`.

**`GroupStats`:** `groupId`, `messages`, `visibleMessages`, `invisibleMessages`, `share`, `band` (only with `scope=node`), `oldestMessageAt`/`oldestAge`, `publishRate`, `consumeRate`, `ackRate`.

---

## Drain

Prepares the node to **leave the cluster**: writes everything to disk, leaves the rotation and **deregisters itself, without a vote** ([07-cluster.md](07-cluster.md), section 7). States: `active` → `draining` → `drained`.

**`DrainStatus`:**

| Field | Description |
|---|---|
| `drainState` | `active`, `draining` or `drained` |
| `trigger` | `signal` (`SIGTERM`/`SIGINT`) or `api`; `null` in `active` |
| `startedAt` | Start of the drain; `null` in `active` |
| `pendingMessages` | Messages **still in memory**; reaches 0 when `drained` |

### `POST /node/drain`: start

Puts **this node** in drain, **without exiting** the process. `202` with `trigger: "api"`; `200` if it was already in drain (no effect).

### `GET /node/drain`: query

```bash
taba-cli drain status -o json     # or GET http://node:9090/node/drain
# {"drainState":"active","trigger":null,"startedAt":null,"pendingMessages":13}
# after the start (4 s): {"drainState":"drained","trigger":"api","startedAt":"2026-10-13T17:24:55.438Z","pendingMessages":0}
```

### `DELETE /node/drain`: cancel or return

| Node state | Effect | Status |
|---|---|---|
| `draining`, before leaving the cluster | **Cancels** the drain; the node goes back to `active` and stays a member | `200` |
| `drained`, after leaving | The node **returns as a new node** (new ID, full bootstrap, recovers lost messages before cleaning). `drainState` goes back to `active` | `202` |
| `active` | Nothing to cancel | `200` |
| During deregistration, or process already stopping by signal | — | `409 drain.conflict` |

Verified on a 3-node cluster: `drain start` took the node to `drained` in ~4 s and `/health/ready` became `503`; `drain cancel` returned it as a new node, and in ~20 s the cluster went back to **3 active, no conditions**.

> While in drain, the node **refuses every client call** (including ack): it forwards to another node or, with no other, `503 cluster.node-draining` (PRB-005). Pending item **D75:** a drain requested through the API that exceeds `drain.timeout` stays `draining` until someone calls `DELETE`. **D77:** the `Retry-After` of the drain `503` is fixed at 5 s.

## S3 backup

The feature is optional ([33-s3-backup.md](33-s3-backup.md)). These routes, like `/node/drain`, do not use a token: protection is by network. Without `backup.s3.bucket`, `GET /node/backup` and the two *flush* routes answer `422` `backup.not-configured`; `POST /cluster/stop` still drains the nodes.

> These routes live in the **operations API** (the operations port, or the client port without `operations.port`), **not** on the cluster port: node-to-node calls use `/cluster/backup/flush`, `/cluster/backup/stop` and `/cluster/backup/saved` on the cluster port (mTLS), described in [18-cluster-api.md](18-cluster-api.md).

### `GET /node/backup`: the state

```bash
curl -s http://node:9090/node/backup
# {"s3Reachable":true,"lastError":null,"lastSuccessAt":"2026-10-14T15:00:03.512Z",
#  "outboxPending":0,"journalPending":0,"definitionsPending":0,"pending":0,
#  "segments":3,"segmentBytes":182011,"saved":412,"segmentsUploaded":9,
#  "messagesUploaded":412,"bodiesEvicted":0,"errors":0}
```

| Field | Meaning |
|---|---|
| `s3Reachable` / `lastError` / `lastSuccessAt` | Did S3 answer on the last call? If not, the reason and when the last successful one was |
| `outboxPending` | Messages that are not yet in a segment in the bucket (includes those waiting for `backup.delay`) |
| `journalPending` | State changes still outside the journal |
| `definitionsPending` | Topic or token definitions still to be written |
| `pending` | The sum of the three: **zero is "all saved"** |
| `segments` / `segmentBytes` | Segments that **this node** wrote and keeps in the bucket |
| `saved` | Messages that this node (owner or replica) knows how to read from the bucket |
| `bodiesEvicted` | Bodies that eviction deleted from disk since the start |

### `POST /node/backup/flush`: force the backup on this node

Does, in sequence, the three steps: **memory → disk** (through the drain path, without stopping the node), **disk → S3** (segments and journal, with priority) and **`outbox` → S3**. Answers when the three finish **or** when `backup.drain.timeout` runs out. It does not deregister or stop the node: it is safe in production. During the flush the node writes new messages to disk (the memory limit drops to zero) and returns to normal when it finishes.

```bash
curl -s -X POST http://node:9090/node/backup/flush
# {"complete":true,"memoryMoved":128,"memoryRemaining":0,"segmentsUploaded":4,
#  "messagesUploaded":128,"journalsUploaded":2,"outboxPending":0,"journalPending":0,
#  "timedOut":false,"s3Reachable":true,"nodeId":"01a1…"}
```

`complete: true` only when nothing was left behind (`memoryRemaining`, `outboxPending` and `journalPending` at zero and no timeout).

### `POST /cluster/backup/flush`: force on all nodes

The node that receives the call forwards it to all the others (through the cluster port) and gathers the responses. **It does not change the order of any block.**

```bash
curl -s -X POST http://node:9090/cluster/backup/flush
# {"complete":false,"nodes":[
#   {"nodeId":"01a1…","complete":true,"outboxPending":0, …},
#   {"nodeId":"01a2…","complete":false,"pending":true,"error":"prazo esgotado"}]}
```

A slow or down node appears with `"pending": true` and the reason; the set's `complete` is `false` and the operator repeats the call.

### `POST /cluster/stop`: stop the cluster with backup

Does the drain on **all** nodes through the same sending step as the stop by signal; the others in parallel, and the node that received the call last. The coordinator writes `cluster/stop.json` with the nodes that stopped and the order in which they stopped. The processes **keep running** in `drained`: stopping the processes is up to the orchestrator (`docker compose stop`).

```bash
curl -s -X POST http://node:9090/cluster/stop
# {"stopped":["01a2…","01a3…","01a1…"],"stopList":true,"warnings":[],
#  "nodes":[{"nodeId":"01a2…","drained":true,"outboxPending":0}, …]}
```

`stopList: false` with `warnings` when S3 was unavailable: the disk drain was done, but the backup was **not sent**. A node that did not answer stays out of the list and is treated as dead on start.

### `POST /cluster/restore/force`: force the restoration of the previous lives

After a total fall the cluster restores the old messages and only then releases the FIFO queues ([37-cluster-cold-start.md](37-cluster-cold-start.md), sections 4.7 and 4.8). While the vote does not close, `GET /health/cluster` shows `previous-life-pending`. The call declares the cluster **restored from disk** with the nodes that are present: the node that receives it spreads it to the others (`POST /cluster/restore/force` on the cluster port), the FIFO queues are released and a restoration held by `cluster.restore.strict` goes on. Messages already consumed may come back (doc 37, section 4.11). On the client API it needs the **root token**; on the operations port, like `/cluster/stop`, it uses no token.

```bash
curl -s -X POST http://node:9090/cluster/restore/force
# {"wasRestoring":true,"status":"restored","silentNodes":[]}
```

`wasRestoring: false` when the cluster was not restoring (nothing changes); `silentNodes` lists the nodes that did not answer the spread (they learn it on their next poll).

## Sources and evidence

- `http/backup.rs` (the backup routes), `http/operations.rs` (the routes and `HealthResponse`), `http/metrics`, `http/topic_stats`, `cluster/drain` (states and transitions), `cluster/health` (conditions), `cluster/state`.
- Responses copied from a real node (a development node and a 3-node cluster through Compose).
- Use cases: UC-OPS (health, metrics and statistics), UC-DRN (drain), UC-LIM (limits of the operations routes); see [26a](26a-implemented-use-cases.md). Tests: `tests/operations.rs`, `metrics.rs`, `topic_stats.rs`, `drain_cluster.rs`.
- History: `06b-api-operacao.md`, `11-observabilidade.md`.
