---
title: Cluster
summary: The life cycle of the cluster — how a node joins, how failure is detected and voted on, freezing, return, call forwarding, synchronization, drain, one-node profile and resuming from disk after a total fall.
type: explanation
status: stable
verified-on: 2026-10-26 @ 5b85d85
sources: [crates/taba-node/src/cluster/members, cluster/service, cluster/channel, cluster/vote, cluster/health, cluster/sync, cluster/recovery, cluster/drain, cluster/state, cluster/restore, cluster/upgrade, cluster/replication/definitions.rs, http/forward]
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

1. **Discovery.** The node calls the node listing (`GET /cluster/nodes`) at the load balancer address (`cluster.entry-address`, host and **cluster port**). If the key is missing, the node does not start (except the first node and the one-node profile).
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

The voting base is only whoever the voter sees. In a **network partition** where both sides have enough healthy nodes, each side may exclude the other. `cluster.vote.min-voters` only prevents an **isolated** node from excluding all the others; whoever operates many nodes can raise it to a majority of the cluster. When both sides excluded each other, each node keeps the addresses of the excluded ones and, every `keepalive.interval × 10`, queries their table: if it belongs to **another cluster** and this side **loses** (fewer members; on a tie, the lowest `nodeId`), the node regroups as the loser of the probe of the old IPs does ([section 10.11](#1011-probe-of-the-old-ips-and-the-regrouping-of-groups)): it moves what it holds in memory to the disk, the disk becomes the folder of a life, and it joins the other cluster with a new ID. What it accepted while apart is restored and checked in the merged cluster (D181).

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

**Process restart.** A process that restarts is born as a **new node (new ID)**, but the disk of the previous life is **not erased at start**: it is set aside in a **folder named with the old ID** (`${storage.dir}/_lives/<old ID>`, with the manifest of the life; a disk with no manifest, from an earlier version, goes to `_previous-life`), the node joins with an empty disk and, **after becoming active and in the background**, returns to the cluster what that disk has of the losses. Recovery does not delay the join and does **not delete** the folder of the life ([07-cluster.md](07-cluster.md), section 10.4).

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

## 8. Cluster size and the one-node profile

| Environment | Minimum nodes | Why |
|---|---|---|
| Test and development | **1** (the one-node profile: `replication.factor=1`) | Run locally without the weight of the cluster |
| Production | **3** | Allows a vote with one node down (2 healthy voters) and avoids a decision by a single node |

In the **one-node profile** (`replication.factor=1`, no `cluster.entry-address`) the node is a cluster of one: it starts alone, is the primary of every block and the quorum is 1, but the path is the production one (empty cluster, cluster port, register and commit). What the profile relaxes (`cluster.tls.enabled=false`, `auth.dev-token`) only holds with factor 1 ([22-configuration.md](22-configuration.md)); the log lists what is relaxed at start and `/health` shows the `reduced-profile` condition. **Not for production:** losing the node or the disk means losing the messages.

**Restart.** Every restart is a new node: the old disk is set aside in `_lives/<old ID>` and the node restores it for itself, as new messages, ready, with the same `deliveryCount` and a new `receiptToken` (section 10). The `development` mode and `development.resume-on-start` no longer exist: to start clean, delete the volume or the directory.

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
| `members-missing` | warning | The probe of the old IPs ended and nodes of the previous life's table are missing (section 10.11) |
| `reduced-profile` | warning | The node runs the one-node profile with something relaxed: factor 1, TLS off or `auth.dev-token`. Informative |
| `previous-life-pending` | warning | The restoration of the previous lives has not ended: the FIFO queues receive and do not deliver (section 10.8) |

What to do for each one is in [23-operations.md](23-operations.md).

## 10. Resuming after a total fall

When **all** the nodes fall and come back, the cluster resumes from disk: the definitions, the messages that were on disk and the order of the FIFO queues. What was **only in memory** on every node is lost ([11-durability.md](11-durability.md)). When only **some** nodes fall none of this is used: the replicas have the message and the new node synchronizes (sections 4 and 5).

**Central idea: availability first, truth later.** The cluster comes up and serves **new** messages at once. The **old** ones stay in quarantine on disk and are only delivered again after the block's new primary checks that they are not dead.

A **life** is the run of a process, with the ID it had. The disk of a previous life becomes a folder (`_lives/<old ID>`) and is restored by the rules below.

### 10.1 Why the ID is still new, and what persists

Every process is born with a new ID. It is a defence: a node that was down and went out of sync must not bring back data that was already excluded or is out of date. Instead of reusing the ID, the node restores the previous life through the common path, checking each message. The price is that every restart resynchronizes the node's blocks (the new ID changes its position on the ring).

| Data | Persists between lives? | Where |
|---|---|---|
| Topics and tokens, with version and tombstone | **Yes**; read at start only when the node starts alone (10.2) | `definition_versions` in `_node.db` ([08-storage.md](08-storage.md)) |
| Messages | **Yes**, in the folder of the life that wrote them, in quarantine | `_lives/<old ID>` |
| Log of excluded messages | **Yes** | `excluded` table of each topic's database (10.3) |
| The life's member table, `last_seen`, watermark | **Yes** | `manifest.json` in the life's folder (10.4) |

### 10.2 Definitions

- Each definition has a **version** and an owner on the ring, with the fingerprint as tie-breaker. When two states are merged the **highest version** wins per definition, and a tombstone of a higher version wins over the definition. The `topicId` goes in the tombstone, so removing a topic does not erase one that was recreated with the same name.
- The node **reads** the definitions from disk only when it **starts alone** (10.5). If it joins a cluster that is already up, it keeps what the disk had, clears it, receives the cluster's definitions and then **offers** them: the highest version of each wins, tombstone included, and a disk older than the cluster imposes nothing. What wins goes to **every member of the table** with a `definition-apply`, like a change decided by an owner, besides the frame of the channel (which only reaches the peers the node knows at that instant): a node that registered a few milliseconds later and is still synchronizing records it with the events and applies it at the end.
- A tombstone expires after 1 hour. A definition that the cluster deleted longer ago and that is still alive on a disk that joins comes back to the cluster (PRB-064).

### 10.3 The log of excluded messages

- Every node that keeps a copy of a block, primary or not, records the `id` of each message excluded from it (ack, `prune`, sending to the dead-letter) in the `excluded` table of the topic's database, **in the same transaction** that deletes the message. Expiry by `ttl` is not recorded: a copy that comes back expires again by the same rule. The ack gets no extra `fsync` (`storage.db.synchronous` stays `NORMAL`).
- The log has a size and an age limit: `storage.excluded.max-size` (100 MB, summing the topics) and `storage.excluded.max-age` (24 h), both greater than zero, applied every 30 s with the oldest record going first.
- Each node keeps the **start of the continuous recording** (`state.excluded_since`, in `_node.db`): the instant from which the log guarantees it has the exclusions. It begins with the log and only advances when a record is dropped by the limit. A new node copies the log of each source along with each block (`GET /cluster/blocks/{id}/excluded`) and **inherits** the most recent date among the sources.
- The **cluster's cut-off date** is the most **recent** of those dates among the active members (the weakest link): every exclusion after it is recorded on all the owners that record it. It is visible in `summary.excludedCutoff` of `GET /health/cluster` (each node reports its own as `excludedSince` in the `NodeStatus`). A node that deletes early because of the size shortens the window of the whole cluster: size the limit for that.
- The logs of past lives are merged into the current one, and the start date only goes back if the life's interval and the sources' meet with no gap.
- A life is only restorable if its `last_seen` is later than the cut-off date plus the `cluster.restore.clock-slack` slack (5 s). With no cut-off date, or outside the window, the life follows the common path: a new identity and only the losses the peers list. If the real clock skew is larger than the slack the risk is accepted: the consequence is the residual redelivery (10.10).
- S3 is only **proof of death**, never of life: the journal lags by up to `backup.meta.interval`, so it does not widen the window ([33-s3-backup.md](33-s3-backup.md)).

### 10.4 One folder per life

- The disk of the previous life **always** becomes a folder `_lives/<old ID>`, whether the node found the cluster or not, and several lives coexist (a second restart does not delete the first). A disk **without a manifest** (a previous version of the program) is not migrated: it follows the old path (`_previous-life`, only the listed losses, folder deleted).
- The **manifest** (`manifest.json`) holds the ID, the active members with IPs and port, the table version, the `last_seen`, the watermark of the highest `id` and the start of the excluded log. It is written by atomic replacement when the node becomes active, on each table change, every 10 s and on a clean stop. A folder without a readable manifest is not a restorable life.
- The recovery of losses does **not** delete the life's folder: only the end of the restoration (10.8) deletes it, and only if everything it holds was confirmed.
- There is no space ceiling of its own for the folders: if the disk fills up, the usual behaviour applies (the node goes to `failed`).

### 10.5 The start

| # | Step | How it is |
|---|---|---|
| 1 | Identity | A new ID per process; the old one becomes the name of the life's folder |
| 2 | `_node.db` | Opens and reads the definitions into memory, but only uses them if the node starts alone |
| 3 | Messages on disk | The disk becomes the life's folder (10.4); nothing is deleted before the end of the restoration |
| 4 | `_backup.db` | Zeroed on every start |
| 5 | Discovery | Listing at `cluster.entry-address`. A node **with a previous life** also tries the IPs of the manifest's table: whoever answers with a table is the cluster it joins; whoever answers `cluster.node-syncing` is alive and also looking |
| 6 | Starting alone | After `cluster.restore.cold-start.wait` (15 s), the node whose **old ID** is the lowest among the live ones and itself starts alone. It takes over the disk: loads topics, tokens, versions and tombstones, continues the excluded log (the start date goes back to the manifest's) and begins the restoration. With no previous life only `cluster.first-node` starts alone (with S3, the bucket's decision still holds) |
| 7 | Join and synchronization | The other nodes keep trying and join the one that started, by the normal join, as new, clean nodes (10.2 for their definitions) |
| 8 | Restoration | In the background, each node sends the lives it keeps to the new primary of each block (10.6) |
| 9 | Queues | The **common** queues receive and deliver as soon as the definitions are distributed; the **FIFO** only receives until the end of the restoration (10.9) |
| 10 | End | Vote of the present nodes, **restored from disk** status and folders deleted (10.8) |
| 11 | In the background | The probe of the old IPs (10.11) and the writing of `last_seen` |

**Accepted risk window.** A late node that starts alone serves with the definitions it has, and a token revoked while it was away would be accepted. The window ends as soon as some node answers (the higher version wins) and is bounded by token expiry.

### 10.6 Restoring the messages

- Each node reads the `_lives/<ID>` folders whose `last_seen` is inside the window (10.3), topic by topic, and **sends** the messages to the block's primary on the current ring, **with the original ID** (`POST /cluster/topics/{topic}/messages/restore`, NDJSON). The sending node decides nothing.
- The primary checks each message and **discards** it if: the topic was deleted or recreated with another `topicId` (stored in `topic_meta`); the `id` is in its local log; it is in the log of **any** present node (`POST …/messages/dead`, in batches per block, to **all**, not just the owners, because the logs are on the nodes and not on the ring); or the S3 journal records the removal. S3 unreachable is missing evidence: it decides with the other sources and warns.
- The rest is **activated**: replicated to the current owners with the quorum, **on disk**, and made available. A message that is only in the life, inside the window and with no exclusion record, is activated: it was a confirmed message or a publication that did not reach the quorum, and delivering it is acceptable under *at-least-once*. If the primary already has it (`Existing`) it skips it; the highest `deliveryCount` among the copies stays on the owners.
- Invisibility and receipt are zeroed (the message comes back ready, with a new `receiptToken`). Exclusion is an absorbing state: if any copy says dead, dead wins.
- The primary only confirms after writing to disk and replicating with the quorum. Only then does the origin node mark the message as **sent**, deleting the row from the life's folder; what was not confirmed is resent up to `cluster.recovery.timeout`. So a second fall does not lose a message that was saved.
- With the S3 backup the pass has a **last step**, the scan of the bucket ([33-s3-backup.md](33-s3-backup.md)); the node's "I finished" (10.8) includes it.

### 10.7 Nodes that do not come back

- The restoration of the messages only **begins** when all the **expected nodes** (the active members of the largest table among the announced manifests) have announced their lives, or after `G` (`cluster.restore.grace`, 5 min). This does not delay the definitions or the common queues. It waits for **all**, not a majority, because the exclusion vote uses the healthy nodes as its base and a healthy pair can exclude the majority of the table.
- After `G`, in the default mode it **goes on with a warning** and uses the present nodes. In **strict mode** (`cluster.restore.strict`) it stays held until someone forces it.
- **Forcing** is `POST /cluster/restore/force` (root token on the client API, no token on the operations port): it changes the status, releases what is held, and is spread to every node. The risk is declared: messages already consumed may reappear (10.10). It is what releases the restoration when a node of a cluster that got stuck by the loss of two nodes does not come back ([23-operations.md](23-operations.md)).
- The `previous-life-pending` condition of `/health/cluster` stays visible while the restoration has not ended, and `GET /cluster/restore` says which old nodes are still expected.

### 10.8 The end of the restoration

- Each node has a status: `unknown`, `restoring` or `restored` (`GET /cluster/restore`). A node that joins asks the others: if any is `restoring`, it joins the restoration; otherwise it restores its own lives without a vote.
- In `restoring` the nodes poll each other every 500 ms (manifests, `finished`, `forced`). Each node announces `finished` when everything it sent was confirmed. The **coordinator** (the lowest ID among the active nodes) calls `POST /cluster/restore/decided` on all of them when **all** the active members have finished. The decision requires every present node, because each one has disk to send.
- Whoever decides releases the FIFO (10.9), marks the cluster **restored from disk** (a name not to be confused with the S3 restore) and deletes the lives' folders, if everything was confirmed.
- If the vote does not close (in strict mode, while an expected node is missing), the FIFO stays without delivery and `previous-life-pending` stays visible until the force operation.
- A node that comes back **after** the release joins as a new, clean node, and its lives follow the path of 10.6. Its FIFO messages are activated in `id` order, and their order relative to the group's newer messages, already delivered, is not guaranteed. It is the exception.

### 10.9 FIFO queues during the restoration

- The FIFO **receives and does not deliver** until the status changes to restored from disk, because only then is everything that was on disk at its primary. `take` delivers nothing.
- New publications go to a **provisional list** at the group's primary, with no effective position in the queue, and no deduplication happens: all are accepted. The ID generator stays above the **watermark** (the highest `id` of the previous lives, announced between the nodes), so new IDs are always greater than the old ones and no `id` already returned to the producer changes later.
- Restored messages reach the group's primary through 10.6 and also stay undelivered.
- At the release, the primary deduplicates the provisional list against the restored keys and within itself, in `id` order, drops the duplicates with an ack without a receipt (`discard`) and only then releases delivery. The producer of a message later dropped as a duplicate received `duplicate=false`: it is the exception.
- Limits (PRB-063): the deduplication holds within the `fifo.dedup.window` window, and the provisional list is in the memory of the group's primary.

### 10.10 Guarantees and limits

- The restoration keeps *at-least-once*: the messages that were on disk come back, and those already consumed do **not**, except in two cases: (a) a node that has exclusion records is not present (after `G`, or only nodes that confirm each other came back); (b) a system crash made the log lose its last entries. This is the **residual redelivery**, and the consumer must be idempotent, as is already required for the S3 restore.
- With **all the expected nodes present** redelivery by (a) does not happen. It is the property the tests prove ([25-testing.md](25-testing.md)).
- What was only in memory on every node is lost.
- While a node of a previous version is in the cluster it does not write the excluded log; by the cut-off date rule (10.3), no life is restorable until it leaves ([23-operations.md](23-operations.md)).

### 10.11 Probe of the old IPs and the regrouping of groups

Two groups of nodes that came up without reaching each other each take over the disk they have. To find each other:

- Every node reads the previous life's table (the manifest) when it starts and, **while the cluster has fewer active members than it**, tries its missing IPs every `cluster.restore.probe.interval` (10 s), up to `cluster.restore.probe.timeout` (2 h).
- The probe ends when the table is complete. If time runs out with nodes missing, the log warns and the `members-missing` condition stays visible while the table is incomplete; the missing node joins later by the normal join.
- When one group reaches another, the partition reunion tie-break applies: **the side with fewer members loses** and, on a tie, the one with the lowest `nodeId`. The loser **deletes nothing**: it writes the manifest, moves the disk to `_lives/<ID>`, keeps the definitions to offer them to the winner, changes the ID and redoes the join. What it accepted while separated lives in the life's folder and is restored and checked against the dead (10.6).
- Limits (PRB-066): after the time is up, two groups that meet later do not join on their own; completeness is by count, so a cluster that was reduced on purpose shows `members-missing` until the time runs out; the loser is out of service during the regrouping.

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
