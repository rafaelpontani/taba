---
title: Optional S3 backup
summary: How Taba keeps a serious-failure backup in an S3 (or SeaweedFS, MinIO) bucket without ever waiting for it — what goes to the bucket, who writes, how the node restores, how to force the backup and stop and start the cluster, the configuration, the limits and the simulated scenarios.
type: explanation
status: stable
verified-on: 2026-10-14 @ efeb47a
sources: [crates/taba-node/src/storage/backup, crates/taba-node/src/cluster/backup, crates/taba-node/src/engine/messages/backup.rs, crates/taba-node/src/http/backup.rs, crates/taba-node/src/cluster/protocol/backup.rs, crates/taba-node/src/cluster/drain/mod.rs, crates/taba-node/src/cluster/vote/mod.rs, crates/taba-node/src/cluster/service/mod.rs, crates/taba-node/src/config/mod.rs, crates/taba-node/tests/backup.rs, docker-compose.yml, examples/backup-s3]
---

# Optional S3 backup

> Read first: [08-storage.md](08-storage.md), [11-durability.md](11-durability.md) and [07-cluster.md](07-cluster.md). The configuration is in [22-configuration.md](22-configuration.md); the step by step for whoever operates, in [23-operations.md](23-operations.md).

Taba keeps everything in **memory and SQLite** and guarantees the message through **replication** between nodes ([11-durability.md](11-durability.md)). That does not cover the **loss of all nodes** (general outage, total stop, a datacenter disaster): in production every restart is a new node, with an empty disk. The **S3 backup** covers exactly that.

> **S3 is only a backup for total or serious failure** (cluster below the quorum). SQLite and memory remain the primary storage. **S3 never enters the path of publishing, consumption or synchronization**: nothing waits for it.

## Principles

| Principle | How it is met |
|---|---|
| **Optional** | Without `backup.s3.bucket` the feature does not exist: the node is the usual one and the backup routes answer `422 backup.not-configured` |
| **Never delays or refuses** | The commit of a message only **inserts a row** (the `id`) in the local `_backup.db`. A definition change is **never refused** because of an S3 failure |
| **S3 down = feature off** | The cluster works exactly as without it; only IDs pile up on disk (the `outbox`). The node **starts with a warning** if S3 is unreachable at start |
| **The guarantee remains replication** | The backup accepts a lag (RPO, below) and this is documented; a receipt or an invisibility of the last window may be behind, so delivery is *at-least-once* and the consumer must be idempotent |
| **All queues** | Volatile and dead-letter ones too |
| **Official client** | The `aws-sdk-s3` (talks to AWS and MinIO). Optional credentials: without them, the default AWS chain applies (IAM, profile, task role) |

> 🖼️ **DIAGRAMA PLANEJADO** — tipo: sequence; objetivo: show the message commit inserting only a row in the outbox, the upload thread assembling the segment after `backup.delay`, the PUT confirming and only then the outbox being emptied; reaproveita: `doc/diagrams/source/write-architecture.json`

## What goes to the bucket, and who writes

```text
<prefix>/                                   backup.s3.prefix (default `taba`)
  topics/<topic>.json                        definition (or tombstone), written by the managing node
  tokens/<id>.json                           token, written by the managing node
  nodes/<id>.json                            the heartbeat of each node
  messages/<topic>/<topicId>/<firstId>_<writer>_<seq>.seg   message segments
  journal/<topic>/<topicId>/<instant>_<writer>_<seq>.jrn    journals of the mutable state
  restore/lock.json                          the lock of whoever restores
  cluster/stop.json                          the list of live nodes at the planned stop
```

| Data | Who writes | Format |
|---|---|---|
| Topic definition | The topic's **managing node** (the definition owner on the ring, as in [09-topics.md](09-topics.md)) | 1 `.json` per topic; removal writes a **tombstone** |
| Token | The token's manager (the token's definition owner) | 1 `.json` per token; revocation deletes the object |
| Heartbeat | Each node, its own | `nodes/<id>.json` |
| Messages | The block's **primary**, sole writer | `.seg` segments |
| Mutable state | The block's primary | `.jrn` journal |

The `topicId` in the path prevents the messages of a **deleted** topic from resurrecting in a new topic with the same name. The segment name starts with the `id` of the first message (a GUID v7 embeds the instant): the names sort by time.

> ⚠️ The bucket holds the **salted hash** of the tokens (never the secret), like `_node.db`. Protect the bucket as you protect the data folder.

## Messages: from commit to segment

1. **Local outbox.** The commit only inserts the `id` in `outbox` in `_backup.db` (SQLite, next to `_node.db`). Only the **owner** (block primary) enqueues. There is no ceiling: if the disk fills up, the node goes into `failed`, as with any lack of space (the outbox only stores IDs, and grows only with the backlog that already occupies the disk).
2. **Delay before sending (`backup.delay`, 60 s).** A message confirmed and consumed **before the window closes never goes to S3**, which cuts the PUT volume: almost all the fast-consumption traffic never reaches the bucket.
3. **Segment.** A background task assembles, per topic, a segment that **closes at whichever comes first**: `backup.segment.max-bytes` (8 MiB), `backup.segment.max-messages` (5000) or `backup.segment.max-age` (60 s). The file ends in an **index footer** (id, offset, size): the footer can be read without downloading the segment (`Range` from the end) and a message by `Range`.
4. **Only after the PUT confirms** do the batch rows leave the `outbox`, and the node records for each message where it is (`saved`: segment, offset, size) and how many live ones each segment has (`segments`).
5. **Notice to the replicas.** The primary tells the replicas, in a batch (`POST /cluster/backup/saved`), which messages were already saved and **when** (`saved_at`, the instant the segment PUT was confirmed, on the primary's clock; information only, not compared with another node's clock). With the reference, the replica can also do the eviction. Any node that stores the message queries where it is and when it was saved through a single function (`Backup::saved_info`); the reference stays in each node's `_backup.db`, **not** in `_topic.db` (D142), and is **not** exposed by the client API. In a restore `saved_at` is the instant of the restore.

**Example.** The primary confirms 10 messages in the `pedidos` topic. They enter the outbox. Within 8 seconds the consumer confirms 6, and those 6 leave the outbox **without generating an upload**. The other 4 pass `backup.delay`, enter a segment, the segment is sent, and only then do the 4 get a reference and `_backup.db` records the segment with 4 alive.

A message that **dies while the segment uploads** (ack or `ttl`) does not count as alive: the segment commit detects it (it is no longer in the outbox) and writes the removal in the journal, so the restore does not resurrect it.

### Mutable state: the journal

`delivery_count`, `invisible_until`, `receipt_token`, dead-letter and deduplication are **not in the segments**; they go in a batch **journal** (`.jrn`, JSON lines).

- Every `backup.meta.interval` (60 s) only the **last state** of each changed message is written: one delivered 3 times in the window generates 1 record.
- An **ack enters as a removal**. **A loose message is never deleted from the bucket.**
- The primary deletes a **segment from the bucket only when all its messages have died** (confirmed, expired or in the dead-letter). A partially dead segment stays until it dies completely; **there is no compaction**. For what the primary that changed cannot see, the topic's manager deletes segments whose messages have all passed the `ttl` (sweep every `backup.reconcile.interval`).
- Restore = segments + journals, from oldest to newest; the last state wins and the removals discard the message.

## Definitions and tokens

Whoever **receives** the change (any node) applies it to the cluster normally; the definition's **manager**, on seeing the commit, writes the object. If the write fails, the change **still holds** and goes into a retry queue. Every `backup.reconcile.interval` the manager compares what the bucket has with memory and rewrites what diverges.

**Example.** The manager of the `pedidos` topic receives the creation, the change is applied to the cluster and it writes `topics/pedidos.json`. S3 is down: the creation holds normally and `pedidos` stays in the retry queue until S3 returns.

**Topic removal:** the manager writes the tombstone (`{"deleted": true, "id": …}`) and deletes in the background the segment and journal prefixes of the `topicId`.

## Heartbeat and death detection

Each node writes `nodes/<id>.json` every `backup.heartbeat.interval` (15 s) with **id**, **access ips**, the cluster port, the state (`member`, `joining`, `draining`) and a **sequence number** (`seq`) that grows at each heartbeat. The `seq` is mandatory: without it the content repeats, the ETag does not change and whoever observes does not notice the heartbeat.

**Without a synchronized clock:** whoever observes **does not compare times**. It keeps the ETag of each file and marks, on its own monotonic clock, when it last changed. If it did not change for `backup.dead-after` (60 s), the node is **dead to whoever observes**. A file seen for the first time counts as fresh until it completes `dead-after` without changing (without history there is no way to know). If S3 cannot be read, the result is **unknown** and the usual behavior applies.

**Alive is not the same as reachable.** The heartbeat in S3 shows that the node is alive, whatever the network between the nodes; the network only answers "can I talk to it right now?":

| Network to Y | Y's heartbeat in S3 | Conclusion | X's action |
|---|---|---|---|
| reachable | fresh | alive | normal |
| unreachable | fresh | **alive in another partition** | **does not take over** Y's blocks: the exclusion vote waits |
| unreachable | expired (or file removed) | dead | may exclude Y by vote and take over the blocks |

The exclusion vote only **opens** after the heartbeat expires (`backup.dead-after`); the channel of a node that dies "blinks", so the wait only gives up if the channel stays open in several consecutive checks. The voter's vote remains "I cannot reach it either".

### Islanding and survivor

**A: partition {1,2} | {3,4,5}.** Node 1 sees 3, 4 and 5 with a fresh heartbeat: takes over nothing; node 3 sees 1 and 2 fresh: takes over nothing. Each side continues with the vote quorum that already exists. S3 is the tiebreaker: whoever is alive shows up in the bucket.

**B: 5 nodes, 3 go down at the same time.** Nodes 1 and 2 see 3, 4 and 5 with an expired heartbeat: exclude the dead by today's vote and take over the blocks. On seeing members leave the table, each survivor **looks in the bucket** for the messages of the blocks it started to hold and does not have (only the footer of each segment and, of what is missing, the entry by `Range`; the journal says what was already confirmed), and synchronization also looks in S3 before declaring a loss (`Syncer::settle_missing`). Only if S3 does not have it either does it become a declared loss.

**Single survivor running.** When **one** node is left (below `cluster.vote.min-voters`), today the cluster would stay in `quorum-lost` with no way out. With the backup, if **all the other members are dead to S3**, the node excludes the dead by itself.

**C: a node that starts alone (start).** When the node listing at `cluster.entry-address` finds no one, the node queries the bucket:

| The bucket shows | Decision |
|---|---|
| S3 unreachable | The usual behavior applies: starts alone only with `cluster.first-node` |
| Some `member` node beating | **Not alone**: keeps trying to join through the normal path (the ips stay in the log) |
| Nodes that stopped beating (`dead-after`) | **Single survivor**: takes the lock and restores |
| No node file | Only starts alone with `first-node`, **or** if `cluster/stop.json` exists (planned stop) |

A node that is also starting (`joining`) **does not prove** that the cluster exists; without this, a simultaneous start of all would lock.

### The restore lock

Two restorers at the same time would diverge. `restore/lock.json` is created with a conditional write (`If-None-Match: *`): **whoever creates it wins**, whoever loses waits and joins as a normal node. The lock has a validity (`backup.restore.lock-ttl`): the holder renews it; if the ETag stays still for that long, another takes it (`If-Match`). At the end of the restore the lock is released and `cluster/stop.json` is deleted. **This requires an S3 with conditional write** (recent AWS S3 and MinIO support it); without it the node warns in the log and continues, and **two restorers may diverge** (accepted risk).

### Identity and cleanup

A dead node that returns joins as a **new node**, with a new id; it does not resume the old id. The `nodes/<id>.json` of a dead node stays until the cluster works again; then any node deletes the files of ids that **do not belong to the cluster and** whose heartbeat is expired. **A fresh file is never deleted** (it may be a live node in another partition). A node that leaves cleanly (drain) removes its own file.

## Full disk: eviction

- Above `backup.disk.evict-above` (85%) the node deletes the **body file** (`.body`) of messages that already have a reference in the bucket, until use falls to `backup.disk.evict-to` (70%). The **row** in `_topic.db` stays, so the "file first, row after" order of [08-storage.md](08-storage.md) still holds.
- Only what is on **disk** is freed: a message only in memory frees nothing.
- **During flush and drain**, on reaching `backup.disk.pause-at` (90%), the move from memory to disk **pauses** until eviction frees space, at most for `backup.drain.timeout`; after the deadline, the node follows the usual behavior (failure for a full disk). If S3 is down, nothing is freed.
- On read, the missing file becomes an S3 lookup by `Range` on the segment; without S3, or without the message there, the current behavior applies (a vanished body is a loss). The read waits for S3 holding the topic lock: it is the rare full-disk case, and only that topic waits.

**Example.** Node 2's disk goes past 85%. It deletes 500 `.body` of messages already in segments. A consumer asks for one of them: the node does not find the file, reads `saved` (segment, offset, size) and does a `Range` on the segment.

## Planned shutdown (`SIGTERM`)

The stop signal triggers **drain**; with the backup, it gains a step **before the node leaves the cluster** (after leaving, the node is no longer the owner of what it would send):

1. Stops accepting publications (as today) and moves memory to disk.
2. **Sends to S3, with priority**, everything on disk and the `outbox`, and the pending journal and definitions, within `backup.drain.timeout`.
3. Only after the upload does it leave the cluster, write `save_completed` and remove its own `nodes/<id>.json`.

The process **does not exit** before the end of step 2, **but does not wait forever**: after `backup.drain.timeout` (30 s) with S3 down the node exits anyway, and what stayed in the outbox depends on the replicas. **Docker's `stop_grace_period` (and `terminationGracePeriodSeconds`) must stay above `drain.timeout` + `backup.drain.timeout`.** The repository's `docker-compose.yml` uses 70 s.

## Operation: force the backup, stop and start the cluster

These routes are in the operations API (like `/node/drain`: no token, filtered by `operations.allowed-ips`, *admin* category of the rate limit). Details and responses in [17-operations-api.md](17-operations-api.md).

| Route | What it does |
|---|---|
| `GET /node/backup` | The backup state on this node: S3 up, what is left to send (`pending`), segments, evicted bodies |
| `POST /node/backup/flush` | Forces the backup **on this node**, in three steps: **1.** memory → disk (through the drain path, without stopping the node); **2.** disk → S3: pending segments and journal, with priority; **3.** `outbox` → S3. Answers when the three finish or when `backup.drain.timeout` runs out. **Does not deregister or stop the node**: it is safe in production |
| `POST /cluster/backup/flush` | The node that receives forwards to **all the others** and gathers the responses. A slow node appears as pending (`pending: true`): repeat the call. Does not change the order of any block |
| `POST /cluster/stop` | Does the drain on **all** nodes (the others in parallel; the one that received the call last) through the same upload step. The coordinator writes `cluster/stop.json` with the nodes that stopped and the **order** in which they stopped. With S3 unavailable it still does the disk drain and answers with a **warning** that the backup was not sent |

Without S3 configured, the two *flush* routes answer `422 backup.not-configured` (nobody thinks they made a backup without having done so); `POST /cluster/stop` still drains the nodes and warns that there is no list.

**Starting the cluster** is not a route: the API does not start stopped processes. "Start" is starting the processes (`docker compose start`, scaling Kubernetes). Restoration happens **by itself** at start: the first node to take the lock becomes the single survivor, restores definitions, tokens and messages, and the others join as normal nodes. Without the stop list (a crash, not a planned stop), the death rule applies, with the delay of `backup.dead-after`.

**Example.** Before a maintenance window, the operator runs `POST /cluster/stop`. The five nodes drain, the coordinator writes the list `[2,3,4,5,1]` in the order they stopped and the cluster stays stopped. On return, `docker compose start` starts the processes, the first node reads the list, restores and the restore does not need to wait for the death deadline.

A step by step with the commands and a ready configuration are in [`examples/backup-s3`](../../examples/backup-s3/README.md).

## Configuration

All keys accept an environment variable (`TABA_BACKUP_…`): see [22-configuration.md](22-configuration.md). The minimum:

```yaml
backup:
  s3:
    bucket: taba-backup          # turning the feature on is just this (AWS uses the default credential chain)
    region: sa-east-1
```

With SeaweedFS, MinIO (or another compatible S3), `backup.s3.endpoint`, `access-key` and `secret-key` (`path-style` turns on by itself with `endpoint`).

> **The S3 must honor conditional writes** (`If-None-Match: *` and `If-Match`, `412` when the condition fails, atomically): this is what guarantees a single restorer. Verified: AWS S3, SeaweedFS (tested: 20 simultaneous creations, 1 winner) and MinIO. **Garage does not work**: it ignores the headers and two nodes could restore at the same time.

## Simulated scenarios and consequences

| # | Scenario | What happens | Consequence |
|:-:|---|---|---|
| 1 | S3 down for all | Feature off; the cluster goes on as today | No loss beyond what already existed without the backup |
| 2 | S3 down only for one (live) node | That node stops renewing the heartbeat; the others may find it dead | **Accepted risk:** only becomes a problem with a partition at the same time; messages accepted only by the losing side may be lost |
| 3 | Primary goes down before sending | The heir takes over the `outbox` at the owner change (`reown`) | The pending messages are sent by the new primary |
| 4 | Primary changes with partially dead segments | The heir does not have the old one's `segments` | The segment is only deleted when all die **by the journal and the `ttl`** (manager sweep). Slower, no loss |
| 5 | Partially dead segment | Stays in the bucket until it dies completely | Spends S3 on dead bytes; accepted, no compaction |
| 6 | Ack arrives during the upload of a segment | The segment commit detects it and writes the removal in the journal | On restore the message is discarded |
| 7 | Message confirmed and consumed before `backup.delay` | Never goes to S3 | If **all** nodes go down before the end of the window, it is lost (accepted: the window is short) |
| 8 | Total stop with up to 60 s of state | Restores the messages, but the journal has up to `backup.meta.interval` of lag | A receipt or invisibility of the last window may be behind; the message may become deliverable before the deadline. *At-least-once*: the consumer must be idempotent |
| 9 | `receipt_token` stored in the bucket | The in-flight consumer confirms the ack after the restore | The token is not a secret; it only avoids duplicate processing |
| 10 | S3 **without conditional write** | The restore lock is not safe | Two restorers may diverge. Accepted risk, warned in the log |
| 11 | Several nodes go down, some remain | The survivors take over the blocks and fetch from S3 what no live node has | Confirmed messages stay available; those that only existed in S3 may be behind by `backup.meta.interval` |
| 12 | S3 down for a long time with high traffic | The `outbox` accumulates IDs | If the backlog fills the disk, the node goes into `failed`: the same risk that already existed, because the backlog already occupies the disk |
| 13 | `aws-sdk-s3` uses `aws-lc-rs` | It built on Windows (MSVC) without extra tools (CMake/NASM) | Nothing to do; the build gets slower |
| 14 | Single survivor excludes the dead and restores | If the dead were only in a partition, they return as new nodes | Their memory-only data may diverge; the reunion uses the one that already exists (`check_reunion`) |
| 15 | SIGTERM and the upload finishes in time | Exits with `save_completed` and the heartbeat removed | Nothing is left pending in the bucket |
| 16 | SIGTERM and the upload goes past the deadline (`stop_grace_period`) | Docker sends SIGKILL; what was left in the outbox stays pending | The heir resends the pending ones; what was only on disk and outside S3 depends on the replicas |
| 17 | SIGKILL or fatal failure | Same as 16 | Same as 16 |
| 18 | S3 down at stop | The upload does not finish | **Does not block forever:** exits after `backup.drain.timeout` |
| 19 | `POST /cluster/stop` with a node that does not answer | The others' drain finishes; the silent one does not enter the list | It is treated as dead on start; no confirmed loss if the replicas exist |
| 20 | `POST /cluster/backup/flush` with a slow node | The other responses come back; the slow one appears as pending | The operator repeats the call |
| 21 | Route called without S3 configured | `422 backup.not-configured` | Nobody thinks they made a backup without having done so |
| 22 | Stop interrupted before writing the list | The list does not exist | The death rule applies, with the delay of `backup.dead-after` |
| 23 | Flush or drain with the disk reaching 90% | Step 1 pauses; the upload and the eviction free space | With S3 down nothing is freed: the wait goes up to `backup.drain.timeout` and the node follows the current behavior |

## Cost on publishing (measured)

The backup adds to the commit **one query and one insert** in the local `_backup.db`, never a call to S3. Measured on a 3-node cluster in the same process (in-memory network), 300 sequential publications after warming up (`cargo test --test backup publish_latency -- --ignored --nocapture`):

| | p50 | p99 |
|---|---:|---:|
| without backup | 0.92 ms | 1.49 ms |
| with backup, S3 up | 1.07 ms | 1.77 ms |
| with backup, S3 **down** | 1.07 ms | 1.82 ms |

S3 being down **does not change** the latency (the difference from no backup, ~0.15 ms, is the local SQLite). The numbers are from a development machine, they only show that S3 is not on the path; they are not a performance goal.

## Known limits

The items below are in [31-pending-items.md](31-pending-items.md): the journal is not compacted (**PRB-053**); a segment of a primary that stopped being one is only reclaimed by the `ttl` (**PRB-054**); restore reads whole segments (**PRB-055**); a definition removed while the manager was down may resurrect on restore (**PRB-056**); the cost of searching for messages in the bucket grows with the bucket (**PRB-057**).

## Sources and evidence

- `storage/backup` (`ObjectStore` store with S3 and the in-memory fake, segment, journal, names, `_backup.db`), `cluster/backup` (`upload`, `evict`, `definitions`, `heartbeat`, `restore`, `stop`), `engine/messages/backup.rs` (the `BackupSink`), `http/backup.rs`, `cluster/protocol/backup.rs`, and the hooks in `cluster/drain`, `cluster/vote`, `cluster/service` (start) and `cluster/sync`.
- Use cases: **UC-BKP** ([26a-implemented-use-cases.md](26a-implemented-use-cases.md#bkp-optional-s3-backup)). Tests: `tests/backup.rs` (level C, with the shared in-memory S3 and the manual `Clock`: upload, window, journal, eviction, disk pause, partition, three of five, single survivor, stop and restore) and the unit tests next to the modules.
