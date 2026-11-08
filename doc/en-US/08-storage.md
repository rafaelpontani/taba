---
title: Storage
summary: Where each piece of node data lives — memory first, overflow to disk, one SQLite database per topic, the node database, the event database — and what happens on a restart.
type: explanation
status: stable
verified-on: 2026-11-07 @ d79a140
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
    _topic.db              topic database (SQLite, WAL), only with what is on disk: rows and bodies
```

Everything is **strictly local**: none of these files is copied between nodes. What travels are the messages and metadata through the register + commit protocol.

![Disk map of a node](../diagrams/img/en-US/disk-storage.svg)

## Memory and overflow

- **Limit:** `memory.limit` (no default). **Without it, nothing goes to disk by overflow.**
- On reaching the limit, the excess messages are written to disk: the **row and the body** go to the topic database in one transaction. When reading a message that is not in memory, it is fetched from disk.
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
| `definition_versions` | `(kind, key)` | The **version** of each topic and token the node applied and the **tombstone** of each removal, with the topic's `topicId` and the instant it was applied. One row per definition, written together with the definition; a tombstone leaves after 1 hour ([07-cluster.md](07-cluster.md), section 10) |

The schema version is written to the database (`SCHEMA_VERSION`); a database of a **newer** version than the node's is not touched.

## The topic database (`<topic>/_topic.db`)

Table `messages`, **one row per message on disk**: `id`, `content_type`, `group_id`, `published_at`, `delivery_count`, `invisible_until`, `receipt_token`, `age_base` (where the `ttl` counts from: publication, or entry into the dead-letter), `dead_lettered_at`, `dead_letter_reason`, `position` (the position on the ring) and, in FIFO, `deduplication_id` and `delete_at`. Indexes by `invisible_until`, `age_base`, `group_id` (the end of a group is read from it, in `id` order), `position`, and the partial ones `(group_id, deduplication_id)` (only the rows with `deduplication_id`) and `delete_at` (only the short row of the FIFO ack), so the other rows pay no entry in them. A database from an earlier version has these two whole: they become partial when it opens (D192).

**Body.** The body is the bytes as they came. Up to `storage.body.inline-max` (default 256 bytes) it lives in the row itself (`body` column); a larger one goes to the `bodies` table (`id`, `body`), so the state updates of the row (delivery, deadline) do not rewrite it. The row and the body are written and deleted **in the same transaction**. A topic folder from an earlier version may still have `<id>.body` files: they are read when the row has no body and deleted with the message, but nothing writes a file any more.

It answers: the oldest message, the next ones to expire, the messages of a block (range by position, for the synchronization streams and cleanup) and the counts.

**`topic_meta`.** Each topic database keeps the `topicId` the folder was created for (key `topic_id`); the restoration of a previous life discards the messages of a topic recreated with another `topicId` ([07-cluster.md](07-cluster.md), section 10.6). A folder from before the record does not have it, and the topic counts as the same.

**Log of excluded messages.** The `excluded` table (`id`, `position`, `at_ms`) keeps the `id` of every message that **left the topic dead** (ack, `prune`, sent to the dead-letter; the `ttl` is not included), with the ring position (the block is found by it). The record is written **in the same transaction** that deletes the message, with no extra fsync (`storage.db.synchronous` stays as configured). The log has an age limit (`storage.excluded.max-age`, 24 h) and a size limit (`storage.excluded.max-size`, 100 MB, adding all topics); the oldest rows go first, and the **start of the continuous recording** (key `excluded_since` of the `state` table in `_node.db`) moves forward to the instant of the newest discarded row ([07-cluster.md](07-cluster.md), section 10).

**Single writer, group commit (D192).** Each topic database has **one write connection**. The frequent writes (publish, delivery, ack, the `expiry` pairs) go into the topic's **write queue**; the first caller that waits becomes the **leader**: it takes everything in the queue, writes it in **one transaction** and wakes the others. Whoever arrives while it writes goes in the next round, with what gathered meanwhile. On the batch routes ([10-publishing.md](10-publishing.md)) the engine puts the writes in the queue holding the topic, **releases the topic** and only then waits for the commit: the other calls of the topic enter the same commit, and the answer to the client still only goes after it. The rare writes (FIFO short row, dead-letter, eviction, cleanups) take the write connection after the queue is empty.

**Reads.** Reads use **read-only connections** (WAL mode), in parallel with the writer. A read by `id` (metadata, body, the message's `expiry` pair) sees what is still in the queue. The pages of messages and the earliest `expiry` pairs (what the ready window and the invisible list read while holding the topic) **do not wait for the queue** (D193): with the queue stopped for an instant, the read pins its snapshot of the database and copies what is pending in its range, then applies that copy over the rows (a removed message leaves, a changed one is filtered by its new values, a new one comes in); the page still comes back full only when there may be more after it. The copy is only of what the page can return (D194): the first `limit` IDs of the queue in the direction of the page; when the queue has more beyond them, the page stops at the last one copied and goes on from there with another read, so its cost follows the page and not the size of the queue. The other reads by range (counts, ages, block listings) wait for the queue to empty first.

**Delivery without waiting for the commit (D193).** On the batch routes of the node alone (D191) the delivery puts its write (counter, deadline, `receiptToken`) in the queue and answers without waiting for the commit; a background thread (`taba-flush`) waits for it and logs a failure. Publishing and the ack still answer only after their commit. The writes of the `expiry` table made while choosing what to deliver, and the ones made by the window's refill after a batch, go into the same deferred scope and are waited for after the topic is released.

**Checkpoint.** The copy of the WAL to the database file runs in the **background** (`PASSIVE`, with its own connection), asked by the writer every 32 MB written; the inline checkpoint of SQLite stays only as a safety net, at about 400 MB of WAL.

**Write order** (so a crash does not corrupt): the row and its body go in and out in **one transaction**, so a crash leaves both or neither. With a legacy `<id>.body` file, the file is deleted after the row. The node that restarts comes back as a new node and rebuilds from the cluster.

**SQLite durability:** `storage.db.synchronous` (default `NORMAL`, with WAL): since durability comes from replication, the `fsync` of every write is not paid.

**Corrupted database or full disk:** the node goes into `failed` ([07-cluster.md](07-cluster.md)). **There is no repair attempt.**

## Topic and token definitions: who decides

A topic or token definition is **small data, rarely changed and of interest to the whole cluster**. **Every node knows all of them**; they are not partitioned by the ring.

- **Definition owner.** Each definition has an **owner** on the ring: the first active node at the hash position of `topic:<name>` (or `token:<id>`). Creation, change and removal are **proposed to the owner** (`POST /cluster/definitions/propose`). It checks against its own state (name already exists, `expectedId`), assigns the **next version**, writes it to an **heir** (the next owner of the position that answers; if there are other owners and none answers, the change is **refused with `503`**) and applies it to itself. Then it sends the decided change to all the others (`POST /cluster/definitions/apply`) and answers the client after those that answer within `write.commit.timeout`; the rest receive it through the `definition` frame of the channel.
- **Create or update (`PUT /topics/{topic}`):** the proposal (`upsert`) carries the declared definition and the owner decides, from its own table and under the lock of the name, whether to create, change, recreate or do nothing, so the choice and the change are one step. Nothing to change gives no new version ([16-client-api.md](16-client-api.md#create-or-update-a-topic-put-topicstopic)).
- **Version:** a node only applies a version **greater than or equal to** the one it already has.
- **Failure:** a partial change is never visible; a node only serves the new definition **at the commit**.
- **Removal:** `DELETE /topics/{topic}` **always works**, even with pending messages: each node deletes all the topic's messages it has (memory, disk and metadata). With `?ifEmpty=true` it only removes a queue that holds no message on any node, and only the `id` it counted.
- **Dead-letter:** has no definition of its own; it is derived from the source queue, created and deleted with it, and does not generate a row in `topics`.
- **Tokens:** the secret **never travels between nodes**: only the salt and the hash. Validation is a **local in-memory** lookup. The node **does not trust** the tokens it finds on disk (they may be out of date); they come from the cluster.
- **Consumption data** (depth, rates, histograms) **never goes to disk**: it stays in memory, per node.

## What is rebuilt at start

The **ready list**, the **invisible list**, the **per-group counts** and the **rates** are **derived** from the metadata and rebuilt when the topic is opened (restart or new owner). They are not stored. The invisible list has a ceiling (`delivery.invisible-list.size`); the pairs that do not fit stay in the **`expiry`** table of the topic database (`invisible_until`, `id`, keyed by the deadline, with an index by `id` to find the pair of a message), which is also derived: it is emptied and rebuilt from the metadata when the topic is opened.

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

**Eviction.** With the disk above `backup.disk.evict-above` (85%), the node deletes only the **body** (the `body` column of the row, the `bodies` row or the legacy `<id>.body` file) of messages that already have a reference in the bucket, down to `backup.disk.evict-to` (70%). The row in `_topic.db` stays. On read, the missing body becomes a `Range` on the segment. Details and the pause during drain: [33-s3-backup.md](33-s3-backup.md).

## Known limits

- **PRB-025:** `diskBytes` and `bytes` sum the lengths of the topic's bodies with one query per topic, which reads every row of it, at each read of `/metrics/topics*`.
- **PRB-019:** finding the middle message to demote and checking the ready ones when `maxDeliveries` changes scan the topic's memory; the `maxDeliveries` check and the on-demand queries read the messages in pages.
- **D80:** the topic name becomes a folder name; Windows reserved names (`con`, `nul`, `aux`, `com1`…) are not handled yet.

## Sources and evidence

- `storage/backup` (the object store, the segment, the journal, the names and `_backup.db`); `storage/db` (`_node.db`, schema), `storage/messages` (body, topic database, write order, folder name), `storage/messages/writer` (write queue, group commit, read connections, checkpoint), `storage/synclog`, `storage/state`, `storage/losses`, `storage/tokens`, `storage/versions`; `engine/messages` (overflow, `rates`, `reserve_memory`); `engine/topics`, `engine/tokens`.
- Use cases: UC-ARM (storage), UC-DEF (replicated definitions), UC-DRN (drain as save); see [26a](26a-implemented-use-cases.md).
- History: `02-arquitetura.md` (3.2 to 3.4).
