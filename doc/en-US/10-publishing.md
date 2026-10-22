---
title: Publishing messages
summary: The path of a published message — from the node that receives the call to the response to the client — with register, quorum, commit, rollback and the possible failures at each point.
type: explanation
status: stable
verified-on: 2026-10-13 @ 71183b9
sources: [crates/taba-node/src/cluster/replication/coordinator.rs, crates/taba-node/src/cluster/replication/replica.rs, crates/taba-node/src/cluster/replication/messages_api.rs, crates/taba-node/src/http/messages/mod.rs, crates/taba-node/src/engine/messages/mod.rs]
---

# Publishing messages

> Read first: [06-ring-and-replication.md](06-ring-and-replication.md). Read next: [11-durability.md](11-durability.md) (what this guarantees) and [12-delivery.md](12-delivery.md).

## Summary

A client publishes to **any node**. That node becomes the **coordinator** of the request, chooses by the message `id` the **responsible** nodes (primary and replicas) and runs a **two-phase** protocol:

1. **Register** on **all** the responsible nodes: each one stores the message as **pending**, in memory.
2. **Commit**, decided as soon as a **quorum** confirmed the register (always with the **primary**).
3. **Response** to the client, with the `id`, when the **commit is confirmed on the quorum**. The replicas outside the quorum finish in the background.

![Who takes part in a write](../diagrams/img/en-US/write-architecture.svg)

![Register and commit step by step](../diagrams/img/en-US/write-reception.svg)

## Step by step

1. **Receive.** The node checks token and permission (`publish`), rate limit, body size (`message.max-bytes`, default 5 MB, or the queue's `maxMessageBytes`) and, in FIFO, `groupId`.
2. **Generate the `id`.** The coordinator generates the GUID v7 **before** the register; it decides the block. In FIFO the one that generates it is the **group's primary** (see [09-topics.md](09-topics.md)).
3. **Find the responsible nodes.** By the hash of the `id` on the ring: primary, secondary, tertiary… (`replication.factor` nodes, default 3). Any node reaches the same result.
4. **Phase 1 — register.** The coordinator sends to **all** the responsible nodes, at the same time, each one with retries (`write.retry.attempts`, default 3, with wait `write.retry.backoff`, default 200 ms). If the coordinator itself is a responsible node, it is called **without the network**. Each node writes the message in memory as **pending** and confirms.
5. **Commit decision.** When the **quorum** confirmed — the **primary plus `write.quorum − 1` any others** (default `floor(replication.factor / 2) + 1`: 2 of 3) —, the coordinator **decides to commit** and calls the commit on the quorum nodes. The other replicas continue in parallel, without blocking.
6. **Response.** With the commit **confirmed on the quorum**, the client receives `201` with `{"id": "…"}`.
7. **Effect of the commit on the primary:** the message becomes **deliverable**: it enters the ready list of the **delivery owner** (or goes straight to the listener of a long polling) and triggers the notices to other nodes ([12-delivery.md](12-delivery.md)).

### Why the primary is always part of the quorum

Only the primary decides what is deliverable. A quorum formed by secondary and tertiary could consider the write durable **before** it is visible to any consumer, and the same truth must hold for write, delivery (invisibility) and ack. That is why `write.quorum` is **a single value** for the three operations, validated between 1 and `replication.factor`.

## Failures

| When | What happens | The client sees |
|---|---|---|
| **Before the quorum** (a responsible node does not answer) | Repeats the call to the node that failed (`write.retry.*`) | Nothing, if the quorum closes |
| **Retries exhausted without a quorum** | **Rollback**: the coordinator orders the pending register to be discarded on all that had already confirmed. A register without a commit also **expires by itself** at `write.pending.ttl` (default 30 s); the rollback only speeds up the release | `503 cluster.quorum-unavailable` with `Retry-After` |
| **Primary down**, still in the table | The write to the block fails: the secondary does **not** take over coordination | `503 cluster.quorum-unavailable` (`fifo.primary-unavailable` in FIFO). Repeat the call; after the primary's exclusion the heir takes over |
| **After the quorum is decided** (a replica outside the quorum fails) | **There is never a rollback**: the quorum may have already made the message effective. The coordinator **insists** (register + commit, idempotent) in the background | Nothing: the response already went out |
| **Ceiling exhausted** (`write.commit.timeout`, default 10 s) on a replica outside the quorum | The block stays **below the full replication factor** (never below the quorum); generates the `under-replicated` condition and a metric; the coordinator keeps trying | Nothing |
| **Commit decided but not confirmed on the quorum in time** | The decision is not undone; the client is told | `cluster.commit-timeout` with `Retry-After` |

> **Repeating a publication that gave an error may duplicate the message**, because the `id` is generated again and the first attempt may have taken effect. The **FIFO** queue is the exception: the primary deduplicates ([09-topics.md](09-topics.md)).

### Rollback and insistence: the rule

| Decision phase | Can it abort? |
|---|---|
| **Before** the quorum closes | **Yes**: nobody depends yet on what was registered |
| **After** the commit is decided | **No**: only insist until it is confirmed, within the ceiling |

It is the same rule as the cluster protocol ([07-cluster.md](07-cluster.md)). What differs is **who decides**: the quorum for messages, all the nodes for membership changes.

## Distributor topic and FIFO

- **Distributor:** the coordinator matches the prefixes and runs **this same procedure once per destination queue**, with copies of their own `id`. The copies are independent.
- **FIFO:** the node that received **forwards to the group's primary**, which generates the monotonic `id`, deduplicates and coordinates; the quorum includes **primary and secondary**.

## On a single node

In the one-node profile (factor 1) the node is the primary of everything and the quorum is 1: register and commit happen only on it, with no replicas, no repetition between nodes and no rollback for lack of quorum.

## Example

```bash
curl -s -i -X POST localhost:8080/topics/pedidos/messages \
  -H 'Content-Type: application/json' -d '{"pedido":9001}'
# HTTP/1.1 201 Created
# {"id":"01a13635-a913-71b2-9b55-85517752ddbc"}
```

The `id` is a GUID v7: it carries the instant of publication (where the message age for the `ttl` comes from) and is sortable by time.

## Parameters

| Key | Default | Effect |
|---|---|---|
| `replication.factor` | 3 | How many nodes store each message |
| `write.quorum` | `floor(factor/2)+1` | Confirmations to decide the commit (primary included) |
| `write.retry.attempts` / `write.retry.backoff` | 3 / 200 ms | Retries of each call before the quorum |
| `write.pending.ttl` | 30 s | Life of a pending register without a commit |
| `write.commit.timeout` | 10 s | Ceiling of the insistence on replicas outside the quorum |
| `message.max-bytes` | 5 MB | Maximum body (each queue can have its own `maxMessageBytes`) |

All in [22-configuration.md](22-configuration.md).

## Sources and evidence

- `cluster/replication/coordinator.rs` (`run`, `Plan`, `Decision`), `replica.rs` (the side that receives register, commit and rollback), `messages_api.rs` (the messages API in the cluster), `http/messages`, `engine/messages`.
- Use cases: UC-ESC (write: register + commit, quorum, rollback, commit ceiling), UC-MSG (publishing), UC-DEV; see [26a](26a-implemented-use-cases.md). Tests: `tests/replication.rs`, `failures.rs`, `messages.rs`.
- History: `02-arquitetura.md` (3.1), `05-particionamento-replicacao.md` (7.2).
