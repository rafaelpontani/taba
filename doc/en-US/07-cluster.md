---
title: Cluster
summary: The life cycle of the cluster — how a node joins, how failure is detected and voted on, freezing, return, call forwarding, synchronization, drain, development mode.
type: explanation
status: stable
verified-on: 2026-10-14 @ efeb47a
sources: [crates/taba-node/src/cluster/members, cluster/service, cluster/channel, cluster/vote, cluster/health, cluster/sync, cluster/recovery, cluster/drain, cluster/state, http/forward]
---

# Cluster

> Read first: [03-concepts.md](03-concepts.md) and [06-ring-and-replication.md](06-ring-and-replication.md).

This document describes **how the nodes organize themselves**: joining, being monitored, leaving (by failure or by choice), returning and synchronizing. There is no coordinator: all decisions come from rules that all nodes apply in the same way.

## Central idea: one protocol, two uses

The two-phase **register + commit** serves two purposes with the same mechanics and different decision rules:

| Use | Who decides | Why |
|---|---|---|
| Messages ([10-publishing.md](10-publishing.md)) | The **quorum** (`write.quorum`, with the primary) | It is the latency-critical path |
| Membership change (join, leave) | **All** the nodes | They are rare, heavy operations: they change everyone's ring |

Rule common to both: **before** the decision, the operation can be aborted (*rollback*); **after** it is decided, it is never undone, and the coordinator **insists** (within `write.commit.timeout`) until it is confirmed. Each phase is idempotent by `operationId`.

## Node states

The node has a **state** (`taba.node.state`) and, separately, a **drain state**:

| State | Means |
|---|---|
| `joining` | Asked to join; not in the table yet |
| `syncing` | In the table, loading topics and blocks; **does not serve clients** |
| `active` | Ready; serves |
| `returning` | Was excluded (or left by drain) and is recovering messages before rejoining |
| `failed` | Disk full or corrupted database: only restarting the process fixes it |

`drainState`: `active` → `draining` → `drained`.

![States of a node](../diagrams/img/en-US/node-states.svg)

> **Freezing** is not a node state: it is a property of an excluded member, exposed as the `node-frozen` condition ([13-observability.md](13-observability.md)).

## 1. Communication between nodes

- **Own port** (`cluster.port`, default 8081), separate from the client port, with **mutual TLS**: the node only talks to whoever presents the cluster certificate ([14-security.md](14-security.md)).
- **HTTP calls** (`/cluster/*`): register, commit, member table, replication, synchronization streams ([18-cluster-api.md](18-cluster-api.md)).
- **Permanent channel** (`GET /cluster/channel`, WebSocket): **one per pair of nodes**, in a full mesh, shared by all topics. It carries only **small signals** (never messages): the heartbeat and delivery notices ([12-delivery.md](12-delivery.md)).
- The caller is recognized by the connection's **source IP** (they all use the same certificate, and the table stores the IPs).

## 2. How a node joins

![A node joining](../diagrams/img/en-US/node-join.svg)

![Failure registering a new node](../diagrams/img/en-US/node-register-failure.svg)

1. **Discovery.** The node calls the node listing (`GET /cluster/nodes`) at the load balancer address (`cluster.entry-address`, host and **cluster port**). If the key is missing, the node does not start (except the first node and development mode).
2. **First node.** Only with `cluster.first-node=true`, and only if **no one answers over the network** (connection refused or timeout), the node starts the cluster by itself as the only member. Whoever answers, even with an error (`503`), was found: the node tries again every `cluster.bootstrap.retry.interval` (default 5 s), up to `cluster.bootstrap.retry.max-attempts` (default 0 = no limit).
3. **Join request.** The node picks a member at random and sends, in a single synchronous call, `POST /cluster/nodes/join` with its own IPs and the cluster port; that member starts to **coordinate** the join.
4. **Connection test.** The coordinator tries to call the new node back at each reported IP. If it fails, the join is aborted.
5. **Phase 1 (register)** to all nodes. A `409` (another change is pending) is repeated until `write.commit.timeout`.
6. **Phase 2 (commit)** to all. Each node that takes effect queues the `membership` frame for its peers, so a late or down node fixes its table when it returns.
7. **Response** to the new node with the **member table**. The topic table is **not** in the response (it grows with the number of topics): the node pulls it afterwards.
8. The new node opens the permanent channel with each node in the table. When a node goes down and restarts, **it is the one that reopens** the channels.
9. **Synchronization** (section 5) and only then does the state become `active` and `/health/ready` answers `200`.

**Failure before the commit:** rollback on the ones that already confirmed and the node does **not join**; it redoes the bootstrap from the start at the retry interval. **Failure at the commit** (register already confirmed by all): it is not undone; the coordinator insists until it is confirmed.

### One membership change at a time

After a node joins, no other **join, leave or rebalance** takes effect until it is **ready**. Also, while an exclusion vote is open, a join request **waits**. The exception is the node in synchronization itself: if it goes into failure, the vote for its deregistration is released.

If two changes are coordinated at the same time by different members, the one with the lowest `operationId` (the oldest GUID v7) wins: the other coordinator **yields**, undoes what it registered and tries again after a random wait of 100 to 200 ms.

## 3. How failure is detected

![Removal of a node by failure](../diagrams/img/en-US/node-removal.svg)

1. The permanent channel carries a **heartbeat** (*keepalive*) every `cluster.channel.keepalive.interval` (default **2 s**). **There is no separate ping.**
2. The channel with a peer **drops** when a call fails, or nothing arrives for `2 × keepalive.interval` (at least 500 ms). The following calls, every `cluster.channel.reconnect.backoff` (default 1 s), are the retries; when `cluster.channel.reconnect.retries` (default **3**) are exhausted, the **vote** opens.
3. Meanwhile, the signals to the node pile up in an in-memory **output queue**. If it exceeds `cluster.channel.queue.max-size` (default 10,000) or `max-age` (default 60 s), the node on the other side is treated as **failed** and the vote opens without waiting for the end of the retries.
4. **Exclusion vote.** Each vote is `yes` if the voter also cannot reach the candidate. It requires `ceil(S × cluster.vote.quorum-percent)` votes from **S healthy nodes** (default 51%), with at least `cluster.vote.min-voters` voters (default 2). A **healthy** node is one that the node opening the vote sees with the channel active and that answers the request; nodes that are down **do not vote and are not in the base**. Without enough healthy nodes, no exclusion takes effect (condition `quorum-lost`).
5. **Who coordinates the exclusion:** the one with the **lowest ID among those who voted**. The one of order `k` waits `k × write.commit.timeout` before trying, if the node is still in the table.
6. Once the quorum is reached, the **deregistration** follows the same two-phase protocol. The coordinator insists until it is confirmed on all remaining nodes, **except those it sees as down** (waiting for a node that is down would prevent the exclusion of another; whoever is left with the old table shows up as `membership-divergent`).
7. At the commit, whoever becomes primary of the removed node's positions takes over the **volatile queues** it managed, and the ring is recomputed.

### Node in failure

A node in `failed` (**disk full** or **corrupted database**, both in the topic databases and in `_node.db`) stops serving **client calls**: in a cluster it forwards them to a healthy node; with no other node, it answers `503 node.failed`. The operations routes and the node-to-node calls continue until it is excluded. **Only restarting the process** takes it out of `failed`: there is no re-evaluation at runtime, so the node does not oscillate between `failed` and active.

### Losing two nodes in a row (verified)

With 3 nodes, `docker compose kill taba-2` leads in ~8 s to the exclusion of the node (table at the next version, condition `node-frozen`). If **before it returns** a second node goes down, **one** node is left, fewer than `cluster.vote.min-voters` (2): the `quorum-lost` condition appears and the exclusion **does not take effect**. The two restarted nodes **cannot rejoin**: the join requires registration on **all** nodes of the table, and the table still lists the dead node (`cluster.registration-failed` and `cluster.node-syncing` in a loop in the log). **There is no automatic recovery and no manual route** (deferred decision: "forced exclusion"). What worked was **restarting the whole cluster**, at the cost of **losing messages and topics** ([11-durability.md](11-durability.md)). Therefore: restart and upgrade **one node at a time**, waiting for `healthy` between them.

### With the S3 backup: alive is not reachable

With `backup.s3.bucket`, "unreachable" is no longer enough: each node writes a **heartbeat** to the bucket, and the exclusion vote **only opens after the candidate's heartbeat has expired** (`backup.dead-after`). An unreachable node **with a fresh heartbeat** is alive in another partition, and the others **do not take over** its blocks. When **only one node** is left and all the others are dead as far as the bucket is concerned, it excludes the dead by itself, even below `cluster.vote.min-voters` (without this the cluster would be left in `quorum-lost` with no way out). And when the dead members leave the table, the survivors fetch from S3 the messages that only existed on them. The start of a node that finds no one also consults the bucket (the single survivor restores). All in [33-s3-backup.md](33-s3-backup.md); without S3, or with it down, what is here applies.

### Accepted risk: partitions

The voting base is only whoever the voter sees. In a **network partition** where both sides have enough healthy nodes, each side may exclude the other. `cluster.vote.min-voters` only prevents an **isolated** node from excluding all the others; whoever operates many nodes can raise it to a majority of the cluster. When both sides excluded each other, each node keeps the addresses of the excluded ones and, every `keepalive.interval × 10`, queries their table: if it belongs to **another cluster** and this side **loses** (fewer members; on a tie, the lowest `nodeId`), the node redoes the join from scratch.

## 4. Freezing and return of the excluded node

![Return of an excluded node](../diagrams/img/en-US/excluded-node-return.svg)

![Process restart](../diagrams/img/en-US/process-restart.svg)

**Freezing.** A node excluded for **failure** that **remains the same process** (same ID) is prevented from rejoining for `cluster.freezing.duration` (default **5 min**), so it does not join and leave in a loop. During freezing the join request is **rejected** with `Retry-After`; the node keeps trying in the normal bootstrap loop. Whoever executed the exclusion commit stores `(ID, instant)` and passes it on to its peers through the channel (`frozen` frame), so a member that was down also refuses the `join` of the excluded node. **Drain** and a process **restart** do **not** cause freezing (they are voluntary, or the ID is new).

**How the node finds out it was excluded.** If three consecutive channel calls to a peer return `403 cluster.not-member` **and** the peer's table is newer and does not have it, the node knows it was excluded.

**Recovery before rejoining** (state `returning`):

1. Queries **all the nodes it can reach** (`GET /cluster/message-loss`, NDJSON stream) and merges the **loss lists**: the (topic, ID) pairs that some synchronization declared lost.
2. Looks for each ID **on disk** (and in memory, if it is the same process).
3. Sends those found to the node responsible for the block (`POST /cluster/messages/recover`). Whoever accepts writes directly to the responsible nodes, **idempotent by ID**, and clears the loss.
4. Tries again every `cluster.bootstrap.retry.interval` up to `cluster.recovery.timeout` (default 5 min); only then **clears the local state** and redoes the join from scratch, **as a new node**.

**Process restart.** A process that restarts is born as a **new node (new ID)**, but the disk of the previous life is **not erased at start**: it is set aside (`${storage.dir}/_previous-life`), the node joins with an empty disk and, **after becoming active and in the background**, returns to the cluster what that disk has of the losses. Recovery does not delay the join.

## 5. Synchronization of a node that joins

![Synchronization phases](../diagrams/img/en-US/sync-phases.svg)

The new node is already in the table and **receives** the normal protocol, but **cannot process** the events until it finishes. Synchronization is **scoped to the blocks it took over**; the rest of the cluster continues normally.

1. **Topic and token load** from a member (`GET /cluster/topics`, `GET /cluster/tokens`, NDJSON streams ordered by `id`); the source is the node with the lowest load, or a random one.
2. **Block load.** For each block, it asks the responsible nodes for the **list of IDs** (`GET /cluster/blocks/{id}/ids`), makes the **union** (not the intersection) by ordered *merge*, splits it into **ranges** and fetches them (`GET /cluster/blocks/{id}/messages?from=&to=`) **balancing across the sources**. Whatever is missing is fetched from another source; the single-ID route is the last resort. A cut stream is resumed from the last ID read.
3. **Event database** (`${storage.dir}/_sync/sync.db`). Everything that arrives by register/commit in this interval is stored **in order of arrival**; the node confirms the register **after writing**, so it counts toward the quorum.
4. **Replay.** Processes the database **on top of the load**: always reads the oldest row, applies it (idempotent; the event wins over the load) and deletes it, until it is empty.
5. **Final check.** Repeats the listings and discards what no longer appears in any source (covers an ack that never arrived).
6. **Switch to normal mode.** With the database empty, it starts processing in real time; `sync.db` is discarded; the node becomes `active` and **ready**.

**A message that does not come.** Before declaring a loss, the node tries: another source, the single-ID route and a re-listing. If the ID **disappeared** from the lists, it was an ack and the case is silently discarded. Only what remains **listed and without content** in all the sources is a **loss**: it generates an `error` log with topic, ID and block, and a record in the `losses` table; it does **not** compromise the node's health.

**Synchronization failures.** A full disk, exceeding `cluster.sync.log.max-bytes` (default 1 GiB) or a corrupted database take the node to `failed`. With no source for a load, the node waits and tries again until `cluster.sync.timeout` (default 60 s); when it runs out, it falls into `failed`. If, while waiting, a member of the table it received answers with a newer table that no longer has the node (its join was undone, for example by the reunion after a symmetric partition), the node does not wait: it forgets the table and redoes the join from scratch.

While it synchronizes, the node **forwards** client calls to another node (section 6), and the **cleanup** of blocks it no longer has only happens with the cluster healthy ([06-ring-and-replication.md](06-ring-and-replication.md)).

## 6. Forwarding client calls

A node that is not yet healthy (synchronizing, in drain or in failure) may receive client calls. Instead of an error, it **forwards** the call:

- **Destination:** in synchronization, a member with an active channel, at random; in **drain**, the **freest** node (lowest load by the heartbeat, tie drawn at random), chosen **at each call**.
- **Transparent:** method, path, query, body and headers go as they came, and the response comes back as the destination gave it (including `409`, `413`, `429`).
- **Through the cluster port** (`/cluster/forward/{path}`): that way the client API does not have to accept any internal marker that a client could forge. It accepts only from members of the table and from nodes that left by drain; whoever receives **never forwards again**.
- **Authentication:** `Authorization` goes as it came; **the destination checks** the token.
- **Destination failure:** before sending the body (connection refused), it tries another, up to a small ceiling. **After it is sent, it does not repeat**: a repeated `POST` would duplicate the message. It answers `502`/`504`.
- **Rate limit:** counted once, on the node that received, **per IP**.
- **No destination:** `503` with `Retry-After`.
- **FIFO:** publishing to a FIFO queue goes to the **group's primary** through its own call (`POST /cluster/topics/{topic}/fifo/publish`), not through the generic forwarding.

## 7. Drain: leaving without losing what is in memory

**What it is for.** Maintenance, upgrade or container relocation. Without drain, a stop loses what was only in memory, the channel drops and the cluster only removes the node after the reconnections and the vote.

**How it starts:**

- **Signal** `SIGTERM` or `SIGINT` (what Docker and Kubernetes send), with `drain.on-stop` on (default). When it finishes, the process exits with code 0. A **second signal** exits immediately.
- **API** `POST /node/drain`: does the same **without exiting**; the node stays `drained` until someone stops the process or calls `DELETE /node/drain`. It only applies to the node that receives the call: use the **operations port** directly, not the load balancer.

**What the node does, in order:**

1. **Leaves the rotation:** `/health/ready` becomes `503`; it keeps receiving client calls but **forwards all of them** to the freest node; long pollings in progress answer empty.
2. **Notifies the peers** through the channel ("in drain"): they stop asking it for `take` and stop choosing it as a load source.
3. **Writes everything to disk:** the effective memory limit drops to **zero**; every new message (published or replica) goes straight to disk.
4. **Empties memory:** moves to disk the messages in memory **with the metadata** (`deliveryCount`, `invisibleUntil`, `receiptToken`), topics with the lowest read rate first.
5. **With the S3 backup, before leaving:** sends to S3 everything on disk, the `outbox`, the journal and the pending definitions, within `backup.drain.timeout` ([33-s3-backup.md](33-s3-backup.md)). With S3 down the node does not wait forever.
6. **Leaves the cluster:** coordinates its **own deregistration** through the two-phase protocol (`POST /cluster/nodes/unregister` and `commit`), **without a vote**. It rebalances, and the new primaries take over its volatile queues.
7. **Closes the channel.** State `drained`. If it was by signal, waits for the forwarded calls to finish (within `drain.timeout`) and exits.

`drain.timeout` (default **25 s**) covers everything (except the upload to S3, which has its own deadline `backup.drain.timeout`, 30 s) and must be **shorter** than the orchestrator's stop deadline (`stop_grace_period` in Compose, `terminationGracePeriodSeconds` in Kubernetes, 30 s by default). If exceeded, the node records what is left in memory and the step where it stopped and exits anyway (if by signal): it falls back to the behavior without drain. An I/O error in step 4 does **not interrupt** the drain.

**Returning from drain** (`DELETE /node/drain`):

| When | Result |
|---|---|
| Before leaving (`draining`) | Cancels: the node goes back to `active` and to writing in memory. `200` |
| During deregistration | `409`; try again when it is `drained` |
| After leaving (`drained`) | The node **joins as a new node** (new ID), redoes the bootstrap, first does the **loss recovery**. `202` |

Leaving by drain does **not** cause freezing. Pending item **D75**: if the drain requested through the API exceeds `drain.timeout`, the node stays `draining` until someone calls `DELETE` ([31-pending-items.md](31-pending-items.md)).

## 8. Cluster size and development mode

| Environment | Minimum nodes | Why |
|---|---|---|
| Test and development | **1** (`mode=development`) | Run locally without the weight of the cluster |
| Production | **3** | Allows a vote with one node down (2 healthy voters) and avoids a decision by a single node |

In **development mode** the node is a cluster of one and everything that exists to coordinate several nodes is **disabled**: bootstrap, channel, vote, freezing, synchronization, replication and the **cluster port** (it is not opened; `cluster.port` is ignored). `replication.factor` and `write.quorum` are **1** (configured values are ignored, with a warning). **Not for production:** losing the node or the disk means losing the messages. The log warns at startup and `GET /health/cluster` shows `"mode":"development"`. The mode does not change at runtime.

**Drain as a *save*.** In development, drain writes everything to disk and marks `save_completed` in `_node.db`. At the next start, with `development.resume-on-start` on (default), the node **resumes whatever it finds in `storage.dir`**: definitions, messages with metadata, rebuilt lists. The `ttl` counts from the ID, so the stopped time counts. **Without a completed save** (crash, `kill -9`), it resumes what is on disk anyway and warns that what was only in memory was lost. With `development.resume-on-start=false`, it deletes `storage.dir` and starts clean.

## 9. The cluster conditions

`GET /health/cluster` returns a **list** of conditions (not a single state), derived from the signals that each node collects:

| Condition | Severity | Means |
|---|---|---|
| `write-quorum-at-risk` | critical | Some block cannot form the write quorum |
| `quorum-lost` | critical | There are fewer healthy nodes than `cluster.vote.min-voters`: no exclusion takes effect |
| `membership-divergent` | critical | Nodes with different versions of the member table |
| `member-unreachable` | warning | A member is unreachable |
| `node-frozen` | warning | There are nodes in freezing |
| `syncing` | warning | There are node(s) synchronizing |
| `under-replicated` | warning | Some block is below the full replication factor |
| `storage-pressure` | warning | Free disk space below `stats.disk.free-min` (without a value, it never fires) |
| `upgrade-stalled` | warning | The retirement of earlier versions stopped on a node that did not finish draining (D165) |

What to do for each one is in [23-operations.md](23-operations.md).

## Known limits

- **D99:** the vote when the candidate announced that it went into failure is still provisional.
- **D102:** `quorumAvailable` and `quorum-lost` do not define whose point of view it is.
- **D109, D120:** constants of the channel and the transport (10 s of notice validity, connection deadlines) are in the code, not parameters.
- **PRB-005:** during drain the node refuses **every** client call, including ack: with no other node, the consumer cannot confirm what it already received.
- **PRB-051:** the cluster recognizes the caller by the source IP, and the `Transport` does not pin the outgoing IP: on a node with several interfaces it may fail.

Full list: [31-pending-items.md](31-pending-items.md).

## Sources and evidence

- `cluster/members` (two-phase register and table), `cluster/service` (bootstrap, join, forwarding), `cluster/channel` (channel and output queue), `cluster/vote` (voting), `cluster/health` (conditions), `cluster/sync` (phases), `cluster/recovery` (losses), `cluster/drain`, `cluster/state`, `http/forward`.
- Use cases: UC-ING, UC-FAL, UC-FRZ, UC-CNC, UC-SIN, UC-STR, UC-RTN, UC-FWD, UC-DRN, UC-DEV; see [26a](26a-implemented-use-cases.md). Tests: `tests/cluster.rs`, `failures.rs`, `freezing.rs`, `sync.rs`, `recovery.rs`, `drain_cluster.rs`.
- History: `04-cluster.md`, `05-particionamento-replicacao.md` (7.3).
