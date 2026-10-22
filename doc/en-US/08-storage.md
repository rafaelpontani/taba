---
title: Storage
summary: Where each piece of node data lives — memory first, overflow to disk, one SQLite database per topic, the node database, the event database — and what happens on a restart.
type: explanation
status: stable
verified-on: 2026-10-14 @ efeb47a
sources: [crates/taba-node/src/storage, crates/taba-node/src/engine/messages/mod.rs, crates/taba-node/src/engine/messages/rates, crates/taba-node/src/engine/topics/mod.rs, crates/taba-node/src/engine/tokens/mod.rs]
---

# Storage

> Read first: [03-concepts.md](03-concepts.md). Read next: [11-durability.md](11-durability.md).

## General rule

**Messages live in memory.** Disk comes in through **overflow** (memory filled up), through **drain** (the node is about to leave) and to keep what **must survive**: topic definitions, tokens and node state. The durability of a confirmed message comes from **replication across several nodes** ([10-publishing.md](10-publishing.md)), not from one node's disk.

Each message is in **one place only** on the node (memory **or** disk), and all operations (delivery, ack, invisibility, `ttl`, dead-letter) work the same on both.

## Disk map (`storage.dir`)

The last folder of the path **must be named `taba`** (startup validation). Default: `/var/lib/taba`.

```
${storage.dir}/
  _node.db                 node database (SQLite, WAL)
  _backup.db               S3 backup database (only with `backup.s3.bucket`): send queue, references, segments
  _sync/sync.db            event database; only exists during synchronization
  manifest.json            manifest of the running life: ID, active members with IPs, table version, `last_seen` (every 10 s), watermark of the IDs (the largest instant the generator used, which the next life starts above), start of the log of excluded messages
  _lives/<old ID>/         one folder per previous life: its topics and its `manifest.json`; they stay until the restoration ends
  _previous-life/          disk with no manifest (earlier version), set aside on a restart; deleted when the recovery ends
  <topic>/                 one folder per topic (uppercase becomes "~" + lowercase: Pedidos → ~pedidos/)
    _topic.db              topic database (SQLite, WAL), only with what is on disk
    <id>.body              the body of each message on disk, only the bytes as they came
```

Everything is **strictly local**: none of these files is copied between nodes. What travels are the messages and metadata through the register + commit protocol.

![Disk map of a node](../diagrams/img/en-US/disk-storage.svg)

## Memory and overflow

- **Limit:** `memory.limit` (no default). **Without it, nothing goes to disk by overflow.**
- On reaching the limit, the excess messages are written to disk: first the **body** in `<topic>/<id>.body`, then the **row** in the topic database. When reading a message that is not in memory, it is fetched from disk.
- **Division among topics.** Memory is not divided equally. Each topic has a **read rate** (moving average of messages consumed per second in the `memory.rate.window` window, default **5 min**; the same window computes `publishRate` and `consumeRate`). Each topic is entitled to a **reserve** (`memory.topic.reserve`, default **1000** messages) regardless of rate, so a new topic does not start by throwing messages to disk. Above the reserves, whoever has the lowest read rate loses memory first.
- **What stays in memory:** the ready list (the next ones to deliver) and the newly arrived. LRU/LFU is not used: each message is read **once** and deleted by the ack, so access history predicts nothing.
- **Overflow summary:** an `info` log (`taba.storage.overflow`) every 60 s, only if there was overflow.
- **Drain:** the effective limit drops to **zero** and everything goes to disk ([07-cluster.md](07-cluster.md)).

## The node database (`_node.db`)

Local SQLite, **one connection behind a `Mutex`** (single writer). Tables:

| Table | Key | Stores |
|---|---|---|
| `topics` | `name` | The **definition** of each topic (JSON). One row per topic, written at the commit, in a transaction |
| `tokens` | `id` | The access tokens: name, permissions, prefixes, dates, **salt** (128 bits) and **salted hash** of the secret. **Never the secret** |
| `state` | `key` | Node state; today the `save_completed` mark (the end of drain) |
| `losses` | `(topic, id)` | The messages declared lost in a synchronization, queried by whoever returns from an exclusion. They expire at `cluster.message-loss.ttl` (default 14 d) |
| `definition_versions` | `(kind, key)` | The **version** of each topic and token the node applied and the **tombstone** of each removal, with the topic's `topicId` and the instant it was applied. One row per definition, written together with the definition; a tombstone leaves after 1 hour ([37-cluster-cold-start.md](37-cluster-cold-start.md)) |

The schema version is written to the database (`SCHEMA_VERSION`); a database of a **newer** version than the node's is not touched.

## The topic database (`<topic>/_topic.db`)

Table `messages`, **one row per message on disk**: `id`, `content_type`, `group_id`, `published_at`, `delivery_count`, `invisible_until`, `receipt_token`, `age_base` (where the `ttl` counts from: publication, or entry into the dead-letter), `dead_lettered_at`, `dead_letter_reason`, `position` (the position on the ring) and, in FIFO, `deduplication_id` and `delete_at`. Indexes by `invisible_until`, `age_base`, `position` and `(group_id, deduplication_id)` and `delete_at`.

It answers: the oldest message, the next ones to expire, the messages of a block (range by position, for the synchronization streams and cleanup) and the counts.

**`topic_meta`.** Each topic database keeps the `topicId` the folder was created for (key `topic_id`); the restoration of a previous life discards the messages of a topic recreated with another `topicId` ([37-cluster-cold-start.md](37-cluster-cold-start.md), section 4.6). A folder from before the record does not have it, and the topic counts as the same.

**Log of excluded messages.** The `excluded` table (`id`, `position`, `at_ms`) keeps the `id` of every message that **left the topic dead** (ack, `prune`, sent to the dead-letter; the `ttl` is not included), with the ring position (the block is found by it). The record is written **in the same transaction** that deletes the message, with no extra fsync (`storage.db.synchronous` stays as configured). The log has an age limit (`storage.excluded.max-age`, 24 h) and a size limit (`storage.excluded.max-size`, 100 MB, adding all topics); the oldest rows go first, and the **start of the continuous recording** (key `excluded_since` of the `state` table in `_node.db`) moves forward to the instant of the newest discarded row ([37-cluster-cold-start.md](37-cluster-cold-start.md)).

**Single writer.** Every write to a topic's database goes through **one command queue**, executed one at a time. Long reads (the synchronization streams) use their own read connections (WAL mode) and do not block the writer.

**Write order** (so a crash does not corrupt): on write, **first the file, then the row**; on ack, **first the row, then the file**. A crash leaves at most a **file without a row** (harmless garbage) or a **row without a file** (the message was lost and is discarded on read). The node that restarts comes back as a new node and rebuilds from the cluster.

**SQLite durability:** `storage.db.synchronous` (default `NORMAL`, with WAL): since durability comes from replication, the `fsync` of every write is not paid.

**Corrupted database or full disk:** the node goes into `failed` ([07-cluster.md](07-cluster.md)). **There is no repair attempt.**

## Topic and token definitions: who decides

A topic or token definition is **small data, rarely changed and of interest to the whole cluster**. **Every node knows all of them**; they are not partitioned by the ring.

- **Definition owner.** Each definition has an **owner** on the ring: the first active node at the hash position of `topic:<name>` (or `token:<id>`). Creation, change and removal are **proposed to the owner** (`POST /cluster/definitions/propose`). It checks against its own state (name already exists, `expectedId`), assigns the **next version**, writes it to an **heir** (the next owner of the position that answers; if there are other owners and none answers, the change is **refused with `503`**) and applies it to itself. Then it sends the decided change to all the others (`POST /cluster/definitions/apply`) and answers the client after those that answer within `write.commit.timeout`; the rest receive it through the `definition` frame of the channel.
- **Version:** a node only applies a version **greater than or equal to** the one it already has.
- **Failure:** a partial change is never visible; a node only serves the new definition **at the commit**.
- **Removal:** `DELETE /topics/{topic}` **always works**, even with pending messages: each node deletes all the topic's messages it has (memory, disk and metadata).
- **Dead-letter:** has no definition of its own; it is derived from the source queue, created and deleted with it, and does not generate a row in `topics`.
- **Tokens:** the secret **never travels between nodes**: only the salt and the hash. Validation is a **local in-memory** lookup. The node **does not trust** the tokens it finds on disk (they may be out of date); they come from the cluster.
- **Consumption data** (depth, rates, histograms) **never goes to disk**: it stays in memory, per node.

## What is rebuilt at start

The **ready list**, the **invisible list**, the **per-group counts** and the **rates** are **derived** from the metadata and rebuilt when the topic is opened (restart or new owner). They are not stored. The invisible list is **complete** and derived from the `invisible_until` of the messages; that is why **the `expiry` table** that the historical documentation mentions **does not exist** in the code (pending item D82).

## Event database (`_sync/sync.db`)

It exists only during the synchronization of a node that joins: table `events` (increasing sequence number, method, path, headers, body — the call as it arrived) and `loaded` (the IDs that the load brought, for the final check). It is discarded when the table empties ([07-cluster.md](07-cluster.md), section 5). Limit: `cluster.sync.log.max-bytes`.

## Restart

| Situation | What happens |
|---|---|
| **Production** | Every restart is a **new node** (new ID). The old disk is set aside in `_lives/<old ID>` (with no manifest, in `_previous-life`) and used, in the background, to return to the cluster messages that only it had |
| **A single node** (factor 1) | The same: the node restores the previous life for itself, and the topics and tokens come back with it. To start clean, delete the volume or the directory |

## The backup database (`_backup.db`)

It only exists with the **optional S3 backup** ([33-s3-backup.md](33-s3-backup.md)). It is local like the others, and **never** enters the publishing path: the commit of a message only **inserts a row** in it.

| Table | Stores |
|---|---|
| `outbox` | The `id` of the messages that the node (owner) has not yet sent to S3. Only the `id`: the body and metadata come from where the message is. No ceiling: if the disk fills up, the node goes into `failed`, as with any lack of space |
| `saved` | Where each saved message is in the bucket (segment, offset, size): it is what lets the node **delete the body from disk** and read it back through `Range`. The replica receives the reference by notice from the primary |
| `segments` | For each segment that **this node** wrote: how many messages still live. When it reaches zero the primary deletes the segment from the bucket, without consulting S3 |
| `journal` | The messages with changed state (or removed) since the last journal |

On a production start the database is **zeroed**: the node is new, and what the previous one kept (queue, references) is not valid for the new identity (what the disk of the previous life returns comes in again through normal publishing, and the new owner queues it).

**Eviction.** With the disk above `backup.disk.evict-above` (85%), the node deletes only the **body file** (`<id>.body`) of messages that already have a reference in the bucket, down to `backup.disk.evict-to` (70%). The row in `_topic.db` stays, so the "file first, row after" order still holds. On read, the missing body becomes a `Range` on the segment. Details and the pause during drain: [33-s3-backup.md](33-s3-backup.md).

## Known limits

- **PRB-025:** `diskBytes` and `bytes` sum the size of the topic's `.body` files, **one call per message on disk**, at each read of `/metrics/topics*`.
- **PRB-019:** finding the middle message to demote and checking the ready ones when `maxDeliveries` changes scan the topic's memory.
- **D80:** the topic name becomes a folder name; Windows reserved names (`con`, `nul`, `aux`, `com1`…) are not handled yet.

## Sources and evidence

- `storage/backup` (the object store, the segment, the journal, the names and `_backup.db`); `storage/db` (`_node.db`, schema), `storage/messages` (body, topic database, write order, folder name), `storage/synclog`, `storage/state`, `storage/losses`, `storage/tokens`, `storage/versions`; `engine/messages` (overflow, `rates`, `reserve_memory`); `engine/topics`, `engine/tokens`.
- Use cases: UC-ARM (storage), UC-DEF (replicated definitions), UC-DRN (drain as save); see [26a](26a-implemented-use-cases.md).
- History: `02-arquitetura.md` (3.2 to 3.4).
