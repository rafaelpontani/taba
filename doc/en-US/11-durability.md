---
title: Durability
summary: What Taba guarantees about confirmed messages, in which failures they survive, in which they are lost, and how to adjust the balance between latency and guarantee.
type: explanation
status: stable
verified-on: 2026-10-14 @ efeb47a
sources: [crates/taba-node/src/cluster/replication, crates/taba-node/src/cluster/recovery, crates/taba-node/src/cluster/sync, crates/taba-node/src/cluster/drain, crates/taba-node/src/storage, crates/taba-node/src/engine/messages]
---

# Durability

> Read first: [10-publishing.md](10-publishing.md) and [08-storage.md](08-storage.md).

This document answers: **"if I received the `201` (or the `204` of the ack), in which situations is the message not lost?"** The short answer: it survives the failure of **any node** (and of more than one, depending on the replication factor), but **not** a simultaneous stop of all nodes **in production**, because durability comes from **in-memory replication**, not from disk. **With the optional S3 backup** ([33-s3-backup.md](33-s3-backup.md)) the cluster restores what was in the bucket, with a lag (RPO) of `backup.delay` for messages and `backup.meta.interval` for state.

## The model

| Question | Answer |
|---|---|
| Where is the confirmed message? | In memory (and on disk, if there was overflow) of at least **`write.quorum`** nodes responsible for the block, **primary included** |
| When does the client receive the confirmation? | When the **commit is confirmed on the quorum** |
| Does the disk guarantee the message? | **Not by itself.** Disk only comes in through overflow (memory full) and drain. Durability comes from the **copies** |
| How many copies exist in the end? | `replication.factor` (default 3); those outside the quorum complete in the background |
| What does the user adjust? | `replication.factor` and `write.quorum`: more confirmed copies, more latency |

This is principle 3 of [01-overview.md](01-overview.md): **configurable durability**.

## Scenarios

Consider the default: 3 nodes, `replication.factor=3`, `write.quorum=2`.

| Scenario | Does the confirmed message survive? | Why |
|---|---|---|
| A node goes down (process or machine) | **Yes** | There are ≥ 2 confirmed copies; at least 1 is left after the failure. The cluster excludes the node by vote and the remaining copies keep serving |
| A node goes down **right after** confirming, before the 3rd replica completes | **Yes** | The quorum (2) already had the message; the 2nd copy survives |
| Two nodes go down **at the same time** | **May be lost** | If the two were the quorum ones, only the 3rd copy (if it completed) remains. Messages whose 3rd replica was still pending may be lost |
| A node is **restarted** (process restarts) | **Yes**, the copies on the **other** nodes | The node comes back as a **new node** and synchronizes. The disk of the previous life still returns to the cluster what only it had |
| A node leaves by **drain** | **Yes** | The node writes everything to disk and deregisters; its copies already existed on the other nodes |
| The three nodes are restarted **one at a time** (`docker compose restart`) | **Yes** | Verified: with 14 messages in the topic, restarting `taba-2`, `taba-3` and `taba-1` in sequence (each until `healthy`) and consuming everything returned **14 of 14** |
| **All nodes** stop at the same time (production), **with the S3 backup** | **Yes, with a lag:** definitions and tokens come back; messages come back **minus** the confirmed and expired ones (journal) and **plus** those that were in the bucket. It loses what was still waiting in the `outbox` (up to `backup.delay`) and the state of the last window. *At-least-once*: the consumer must be idempotent | **Planned** stop (`POST /cluster/stop`): no lag, the drain sends everything to S3. Crash: the last upload. Details and scenarios: [33-s3-backup.md](33-s3-backup.md) |
| **All nodes** stop at the same time (production), **without backup** | **No: loses the messages and also the topic definitions** | In production every restart is a new node and the old disk only serves to return "losses" listed by the other nodes. With the whole cluster stopped there is no one to list. Verified: `docker compose down` + `up` (volumes kept) started a cluster **with no topic at all**. ⚠️ Design limit |
| Network partition | **Depends on the side** | Each side may exclude the other and continue ([07-cluster.md](07-cluster.md), "Accepted risk"). Messages with a copy on both sides survive; writes on the minority side fail for lack of quorum |
| Development mode, 1 node | **Only with drain** | No replicas: drain writes everything and the next start resumes. A crash without drain loses what was only in memory |
| **Primary** of a block down | The message is **not lost**, but writes to the block **fail** | The secondary does not coordinate; when the primary is excluded, the heir takes over |

> **Two nodes at the same time** also depends on `replication.factor`: with 5 copies and quorum 3, it survives two simultaneous failures. As a rule: the cluster tolerates `write.quorum − 1` simultaneous node losses **without losing what was confirmed**.

**In summary** (3 nodes, `replication.factor=3`, `write.quorum=2`):

| Nodes lost at the same time | The confirmed message | Why |
|:-:|---|---|
| 0 | ✅ survives | There are three copies, or at least two confirmed |
| 1 | ✅ survives | At least one of the quorum copies is left |
| 2 | ⚠️ may be lost | If the two were the quorum ones, only the third copy remains, and only if it had already completed |
| 3 (total stop) | ❌ loses everything | In production every restart is a new node; nobody lists the losses |

## What is *never* lost without notice: losses in synchronization

When a new node synchronizes a block and a **listed** message does not come from any source, the node does **not pretend it never existed**:

1. Tries another source, the single-ID route and re-lists; if the ID disappeared from the lists, it was an **ack** (silently discarded).
2. If it remains **listed and without content**, it becomes a **declared loss**: `error` log (topic, ID, block, nodes queried) and a record in the `losses` table of the responsible nodes.
3. When an excluded node **returns**, it queries these lists and **returns** the messages it still has, idempotent by ID ([07-cluster.md](07-cluster.md)).

A loss does **not** compromise the node's health nor stop the synchronization.

## Delivery guarantees

- **At least once** (*at-least-once*): a message may be delivered more than once (ack that does not arrive, expired deadline), but is **not lost for lack of an ack**. Whoever consumes must be **idempotent**.
- **No global order:** there is no order per topic. The **FIFO queue** gives order **per group** ([09-topics.md](09-topics.md)).
- **No publication duplicates:** only in the FIFO queue, within the deduplication window. In the others, repeating a publication that failed may duplicate.
- **A message stays at most `ttl`:** 14 days by default; once expired, it is discarded (and does **not** go to the dead-letter).

### Accepted trade-off in invisibility

If the primary goes down **exactly** between the local commit and the replication to the replicas outside the quorum, and is excluded before it finishes, the new primary inherits the `invisibleUntil` of whoever survived, which may be out of date: the message may become deliverable again **before** the deadline promised to the first consumer. It does not break *at-least-once*; it only shortens, in this rare case, the minimum processing time.

## Retention

| Rule | Default | Effect |
|---|---|---|
| `ttl` | 14 days (`retention.ttl.default`; range 60 s to 365 d per queue) | A message older than `ttl` is discarded wherever it is, **also** if invisible |
| `maxDeliveries` | 10 (`retention.max-deliveries.default`; range 1 to 1000) | On reaching it without an ack, it goes to the dead-letter |
| Dead-letter | `{queue}-dead-letter` | Holds what failed in consumption, also subject to `ttl` |

The age comes **from the `id` itself** (GUID v7 carries the instant of publication; in the dead-letter, the instant of entry into it). The check is **local and deterministic**, with no register + commit round: each node deletes its own copies every `retention.sweep.interval` (default 60 s).

## How to adjust

| I want to… | Do |
|---|---|
| Tolerate more simultaneous failures | Increase `replication.factor` and `write.quorum` (publishing latency goes up) |
| Less publishing latency | Reduce `write.quorum` (the guarantee drops); it must be ≥ 1 and ≤ `replication.factor` |
| Lose nothing on a **planned** stop of a node | Keep `drain.on-stop` on (default) and `stop_grace_period` greater than `drain.timeout` |
| Survive a failure of the whole cluster | **There is no way today:** use more than one cluster, or a producer that repeats publications. ⚠️ see [32-roadmap.md](32-roadmap.md) |
| Less memory | Set `memory.limit`: the excess goes to disk |

## Known limits

- **Total stop** in production loses what was only in memory (above), **unless** the S3 backup is on ([33-s3-backup.md](33-s3-backup.md)): then it loses only the window not yet sent.
- **PRB-005:** during drain the node refuses the ack; with no other node to forward to, the consumer cannot confirm.
- **D116 (decided):** the redundancy of the messages of a node that leaves stays at `replication.factor − 1` until a new node joins.
- **D114:** what `delivery.get.max-attempts` counts is not yet decided.

## Sources and evidence

- `cluster/replication/coordinator.rs` (quorum, rollback, insistence), `cluster/recovery` (losses and return), `cluster/sync` (declared loss), `cluster/drain`, `storage/*`, `engine/messages` (`ttl`, dead-letter).
- Use cases: UC-ESC, UC-FAL, UC-RTN (return and recovery), UC-SIN, UC-DRN, UC-RET (`ttl`, deliveries, dead-letter); see [26a](26a-implemented-use-cases.md). Tests: `tests/failures.rs`, `recovery.rs`, `drain_cluster.rs`, `sync.rs`, `replication.rs`; `docker/validate.sh` checks the absence of loss when a node goes down and comes back.
- History: `05-particionamento-replicacao.md` (7.2, 7.3), `13-entrega-de-mensagens.md` (12.10).
