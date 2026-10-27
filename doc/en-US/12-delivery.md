---
title: Message delivery
summary: How Taba delivers, confirms and redelivers messages — invisibility, ready list, delivery owner, long polling, node channel, ack, delivery limit, dead-letter and fair delivery across groups.
type: explanation
status: stable
verified-on: 2026-10-13 @ 71183b9
sources: [crates/taba-node/src/engine/messages, crates/taba-node/src/engine/messages/index, crates/taba-node/src/cluster/replication/messages_api.rs, crates/taba-node/src/cluster/replication/signals.rs, crates/taba-node/src/cluster/channel, crates/taba-node/src/http/messages]
---

# Message delivery

> Read first: [06-ring-and-replication.md](06-ring-and-replication.md) and [10-publishing.md](10-publishing.md).

## The model: temporary invisibility

Like the Amazon SQS *visibility timeout*: when a message is **delivered**, Taba makes it **invisible** for a while. If the consumer **confirms** it (ack) within that time, it is removed; if not, the deadline expires and it **becomes available again**. The semantics are **at least once** ([11-durability.md](11-durability.md)).

From the point of view of the **delivery owner**, a message is always in one of these states:

| State | How to recognize it | What controls it |
|---|---|---|
| **Ready** | `invisibleUntil` absent or in the past | Ready list |
| **Invisible** | `invisibleUntil` in the future | Invisible list (ordered by deadline) |
| **Removed** | Ack confirmed | Nothing: message and metadata deleted |

![States of a message on the delivery owner](../diagrams/img/en-US/message-states.svg)

## Metadata of the delivered message

| Field | Meaning |
|---|---|
| `deliveryCount` | How many times it was delivered (goes up at each **delivery**; does not change when invisibility is changed) |
| `invisibleUntil` | **Absolute instant**, computed by the **owner's** clock (`now + visibilityTimeout`). The replicas store the ready value and never recompute |
| `receiptToken` | Opaque token generated **at each delivery**; required for ack and change. Like the SQS `ReceiptHandle` |

**Use of the `receiptToken`:** if the deadline expired and the message was **redelivered to another consumer**, the first consumer's token became **stale**: its ack answers `409 message.stale-receipt` and nothing changes. If the deadline expired but it was **not yet redelivered**, the token is still valid and the ack counts. Changing the invisibility keeps the same token.

The metadata stays with the message, in memory or in the topic database ([08-storage.md](08-storage.md)).

## Consume (`GET`)

```bash
curl -s "localhost:8080/topics/pedidos/messages?visibilityTimeout=120&maxMessages=10&waitTime=20"
```

| Parameter | Default | Range | Effect |
|---|---|---|---|
| `visibilityTimeout` | **120** s (`delivery.visibility-timeout.default`) | up to `delivery.visibility-timeout.max` | How long the message stays invisible |
| `maxMessages` | **10** (`delivery.max-messages.default`) | up to `delivery.max-messages.max` | Maximum messages in the response |
| `waitTime` | **0** | 0 to **20** s | Long polling: waits for messages instead of answering empty |
| `autoCreate`, `volatile`, `fifo`, `idleTimeout`, `tag.*` | — | — | Creation of the queue by the `GET` ([09-topics.md](09-topics.md)) |

The response carries **from 1 to `maxMessages`** messages: it returns what is ready, without waiting to fill the quantity (with `waitTime > 0`, it waits only for the **first**). If there is nothing, `{"messages":[]}`.

### What happens inside

![Consumption: delivery and ack](../diagrams/img/en-US/consumption.svg)

1. The serving node picks an ID from the topic's **ready list**, which only holds messages of which it is the **delivery owner**.
2. Computes the nodes responsible for the ID's block.
3. **Phase 1:** registers on all that the message stays invisible until `now + visibilityTimeout`, with `deliveryCount + 1` and a new `receiptToken`.
4. **Phase 2:** when the **quorum** confirms (**primary always included**), decides the commit.
5. **Only then** is the message returned to the client. At the commit, each quorum node removes the ID from its ready list.
6. **Failure before the quorum:** repeats and, when exhausted, **rollback** of the pending one; the `GET` **moves immediately to the next ID** (the rollback finishes in another task), up to `delivery.get.max-attempts` failures (default 3). At the end it answers with what it captured; if **nothing** was captured and there were failures, it answers **`503` with `Retry-After`**, never empty (the client would read that as a queue with no messages).

The message only reaches the client after invisibility is marked on the quorum with the primary; a coordinator crash right after delivery does **not** make another node deliver it in parallel from the same block.

## Delivery owner

Each message has **a single owner**: the **primary of its block** ([06-ring-and-replication.md](06-ring-and-replication.md)). If all the replicas delivered, they would compete for the invisibility commit of the same message.

- The topic **does not belong to any node**: messages are spread over the whole ring, and the complete topic is the **union of the ready lists** of the nodes.
- **Partitioning by owner is not what guarantees correctness.** The invisibility register remains **exclusive**: a node refuses the request if another is already pending or if the message is already invisible, and the `GET` tries another ID. Even with member tables diverging for moments, **no message is delivered twice**.
- **New primary still synchronizing:** the messages of its blocks are **not delivered by anyone** until it is ready (the old primary removes them from the list at the register commit). It lasts only the synchronization time.
- **Owner down** and still in the table: its messages stay undelivered until the exclusion. The other nodes of the block are backup copies and do **not** take over delivery.
- **Order:** **no guarantee** of order (neither per topic nor per `groupId` in ordinary queues); redelivery by invisibility also shuffles. Whoever needs order uses a **FIFO queue** or orders at consumption by `id` (GUID v7).

## Ready list

Per topic and per node, the **list of ready IDs** that the node owns.

- **Order: accordion effect.** Ordered by GUID v7, delivery consumes **both ends interleaved** (oldest, newest, second oldest…), so both ends are always available.
- **In fair delivery** (groups), the order is by **band** of the group and, within the band, the accordion (see below). **In FIFO**, the list has **one entry per free group** ([09-topics.md](09-topics.md)).
- The `GET` only **queries the index**: it does not scan memory and disk at each call.
- **When memory is short**, what leaves first is what is being processed and, afterwards, the **middle** of the ready list (the last one the accordion would reach); the ends stay, being the next to deliver.
- The index is **derived** from the metadata: rebuilt when the topic is opened (restart or new owner).

> **How the code is.** The index (`engine/messages/index`) keeps the **exact counters** (ready, in processing, per group) and two **bounded** lists. The **ready list** is a window of `delivery.ready-list.size` IDs, refilled by two cursors (from the oldest forward and from the newest backward, by GUID v7, alternating the ends) only when room opens (the engine reads the metadata when a quarter of the window has opened, or when the oldest end ran out of IDs, so as not to read at every delivery); a message that returns to the list enters by the position of its GUID v7 and, with the window full, the ID at the middle leaves and is found again by the cursor. The **invisible list** holds up to `delivery.invisible-list.size` pairs (ID, deadline) by deadline; what does not fit goes to the `expiry` table of the topic database, and the list is reloaded from it below `delivery.invisible.reload-threshold`. The index **does not keep an entry per message**: only the one of whoever is in the window or in the invisible list (on the FIFO queue and in the overflow group, all of them). Of the others it only has the count. Before acting on one of them (ack, removal, new deadline, `ttl`), the engine reads its group, age and deliveries from the metadata (the topic's memory or the database) and hands them to the index; refilling the window and finding the end of a group also read the metadata, in pages (the end of a group, by the group index in the database, in `id` order). The index still does no I/O: what it does not have, the engine brings. So the index's memory on the fair queue is the window, the list and the per-group counters, and does not grow with the backlog.

## Change the invisibility

```bash
curl -s -X PUT "localhost:8080/topics/pedidos/messages/<id>/visibility?receiptToken=<token>" \
  -H 'Content-Type: application/json' -d '{"visibilityTimeout": 300}'
# 200 {"id":"…","invisibleUntil":"2026-10-13T17:10:13.601Z"}
```

- Uses the **same register + commit by quorum** as delivery, **without** incrementing `deliveryCount` and without touching the ready list.
- The value is the **new time from now** (`now + visibilityTimeout`), **not** an increment; it respects `delivery.visibility-timeout.max`.
- **`0` releases the message immediately**: the owner puts it back in the ready list as on an expiry. If `deliveryCount ≥ maxDeliveries`, it goes to the dead-letter.
- `409 message.stale-receipt` if the token is stale.

## Confirm (ack)

```bash
curl -s -i -X DELETE "localhost:8080/topics/pedidos/messages/<id>?receiptToken=<token>"
# HTTP/1.1 204 No Content
```

- Requires the `receiptToken` of the last delivery. A token that does not match → `409`, nothing changes.
- Same register + commit protocol: the quorum (primary included) confirms and the commit **removes the message and its metadata**, **with no tombstone** (the absence of the ID is enough; the final check of synchronization covers the residual window). The replicas outside the quorum remove in the background.
- **FIFO queue:** the ack writes `deleteAt` and deletes only the body, for deduplication ([09-topics.md](09-topics.md)). It is the only exception to "no tombstone".
- **Failure before the quorum:** the ack is **canceled** and the message remains available (the client repeats). There is never a partial ack visible.

## Expiry of the invisibility

A task of the owner looks at the **head** of the invisible list (the next to expire) and sleeps until the deadline (with a short ceiling, to react to changes). When it expires:

1. Removes the entry and **checks** the metadata: the message still exists, the node is still the owner and the deadline is still the entry's; otherwise it discards.
2. **Delivery limit:** if `deliveryCount ≥ maxDeliveries`, it does **not** return to the ready list: the owner sends it to the dead-letter. A message already expired by `ttl` is also discarded.
3. Otherwise, **inserts the ID in the ready list** at the GUID v7 position (a message that comes back is old and tends to stay near the old end).
4. If the list **was empty**, notifies the other nodes; if there is a local listener, the message goes **straight to it**.
5. **Nothing is written to the metadata nor sent to the replicas:** the expiry **does not propagate**. `invisibleUntil` is already an absolute instant: "in the past" **means** "visible", on any node. `deliveryCount` only goes up at the **next delivery**.

**Clock.** The deadline is decided and checked by the **owner's** clock. A clock difference between nodes, at an owner change, shortens or lengthens the deadline by the size of the difference: accepted in *at-least-once*, and a reason to keep clocks synchronized (NTP).

**Owner change and restart.** The new owner, when it finishes synchronizing, walks through the metadata of the messages of which it became primary: those with `invisibleUntil` in the future enter as invisible; the others, as ready candidates. Whoever **stops** being the owner cleans nothing: what remains fails the owner check and is discarded.

## Long polling and the node channel

With `waitTime > 0` and nothing ready, the call **waits** for new messages (up to 20 s). Since the topic is spread out, the node needs to know **where there are messages** without sweeping the cluster.

### The three scenarios

A 3-node cluster (A, B, C), topic `pedidos` with 6 messages, each node primary of two:

| Scenario | What happens |
|---|---|
| **1. Local list has a message** | The `GET` reaches A; A is the owner of m1: registers, commits on the quorum and returns. No other node is consulted. It is the common case with a full queue |
| **2. Local list empty, another node has** | A emptied and announced "no ready in `pedidos`"; B and C have the table `{A: no, B: yes, C: yes}` in memory. A draws one (weighted by the reported quantity), calls `take` on B (`POST /cluster/topics/{topic}/messages/take`); B, the owner, does the register + commit and returns the message to A, which passes it on. If B already emptied, A tries C, up to `delivery.take.max-attempts` (default 3) |
| **3. No node has** | A becomes a **local listener** and tells the others, **once**, "`pedidos` has listeners on A". When a ready message arrives on B, B sees the listener on A and sends "there are new messages here"; A calls `take` on B and delivers to the listener. With no message until `waitTime`, it answers empty |

![Scenario 1: local list](../diagrams/img/en-US/consumption-scenario1-local-list.svg)

![Scenario 2: complete with another node](../diagrams/img/en-US/consumption-scenario2-other-node.svg)

![Scenario 3: waiting](../diagrams/img/en-US/consumption-scenario3-waiting.svg)

The cost of knowing where there are messages is paid only on the **transitions** (from "has" to "has not"), with small signals:

| Alternative | Cost per `GET` with an empty local list |
|---|---|
| Read the monitoring of all nodes | Query to N nodes, stale state on arrival |
| `GET` on all, one by one | Up to N calls per `GET`; C consumers generate C × N |
| **Signals on transitions (adopted)** | Local in-memory query + up to `delivery.take.max-attempts` calls |

### The permanent channel

One **WebSocket per pair of nodes**, shared by **all** topics (never one per long polling call). Only **signals**, **never** the content of the messages (that goes through the internal routes, with register and commit). Sent only **when the state changes**:

| Signal | When |
|---|---|
| **Topics with listeners** | The topic gains its first listener on this node, or loses the last |
| **Topics with ready messages** | The node starts to have ready messages for the topic, or empties |
| **New message notice** | A new ready message arrived on the owner, **with no local listener**, and another node reported having a listener |

- The notice is a **hint**, not a guarantee: whoever receives it goes to fetch and may find nothing. A lost notice only costs waiting until the end of `waitTime`.
- **Who sends the notices:** one responsible per topic, active only while there are listeners and new messages. It **groups** those that become ready in a short window (`cluster.notify.window`, default 100 ms) into **one notice** with the quantity; notifies **one node at a time**, rotating, and waits `cluster.notify.wait` (default 50 ms) to see whether it comes to fetch; if not, moves on to the next.
- **No fan-out per call:** N listeners waiting for the same topic on a node generate **one** interest report.
- **Heartbeat:** its own message (not the WebSocket ping, to detect a hung node with the connection open), every `cluster.channel.keepalive.interval` (default 2 s), with date and time, a monitoring summary (node load), the **quantities of ready messages per topic** (up to `cluster.channel.keepalive.max-topics`, default 1000) and the node's **state**. It is **neither confirmed nor resent**; the next one replaces it. Since it carries the state, it fixes a lost `node-state`.
- **Confirmation:** every signal **except** the heartbeat carries an increasing number per channel and direction; the receiver returns the confirmation after **processing**; the sender only removes from the output queue on receiving it; on reconnection, it **resends what was left unconfirmed**. A resend already processed is recognized by the number and ignored.
- **Who opens the connection:** the node that **joins or returns** actively opens with all; a node only accepts those that are in its table. If two open at the same time, **the connection of the node with the higher ID hash stays**.
- **Initial state:** on opening the channel, each side first sends a **snapshot** (topics with listeners, with ready messages and the nodes in freezing) and only then the changes. A synchronizing node only consumes it when it becomes ready.
- **Bounded output queue:** **state** signals of the same topic **replace** each other (only the last matters); old **notices** (over 10 s) are **discarded**; `frozen` **does not expire**. Beyond `cluster.channel.queue.max-size`/`max-age`, the node on the other side is treated as **failed** and the vote opens ([07-cluster.md](07-cluster.md)).

On a **single node** (factor 1) there is no other node for the `take`: long polling only waits for messages to arrive on the node itself. **In drain**, the node answers `take` empty and the peers stop asking it.

## Expiration (`ttl`), delivery limit and dead-letter

Three retention rules apply to every queue; each has a **node default** and can be set **per queue** (`POST /topics`, `PATCH`).

| Rule | Field | Default | Range | Effect |
|---|---|---|---|---|
| Expiration | `ttl` (s) | 14 days | 60 s to 365 d | A message older than `ttl` since publication is **discarded**, delivered or not |
| Delivery limit | `maxDeliveries` | 10 | 1 to 1000 | On reaching `maxDeliveries` deliveries without an ack, it goes to the **dead-letter** |
| Dead-letter | derived from the queue | `-dead-letter` | — | Receives those that exceeded `maxDeliveries` |

The default applies **at the time of the check**: whoever did not set the field follows changes of the default; whoever set it keeps its own.

- **`ttl`:** the age comes from the ID itself; the deadline uses the queue's **current** `ttl` (a `PATCH` applies to already stored messages). It applies **even to an invisible message**. The check is **local and deterministic**, with no propagation: the owner does not deliver an expired one, and a task on each node deletes its own copies every `retention.sweep.interval` (default 60 s; the task runs at the smaller of it and 1 s), and also before each `GET`, ack and `PUT`. Once expired, it is **deleted, it does not go to the dead-letter** (metric `taba.retention.expired`).
- **`maxDeliveries`:** `maxDeliveries: 10` means **at most 10 deliveries**; the eleventh never happens. The limit is checked when the invisibility is released **and** when the message is chosen for delivery: one that already used it up is not delivered, and goes to the dead-letter at the next release even if it reached the ready list some other way (D180).
- **Dead-letter:** is born with the queue (in the same operation), is deleted with it, **has no definition of its own** (read-only, `deadLetter: true`, `source`). `PATCH` and `DELETE` on it give `409`. **There is no dead-letter of the dead-letter.** The **move** is **one register + commit operation**: creates the message in the dead-letter and removes it from the source, with the **same ID** (so, same block and same nodes). `deliveryCount` goes back to 0, `invisibleUntil` is cleared, and it gains `deadLetteredAt` and `deadLetterReason` (`max-deliveries`). Failure before the quorum: rollback, and the owner tries again on the next expiry round.
- **Reserved name:** a topic cannot be created with a name ending in the suffix (`400`). The dead-letter name may exceed 100 (up to 112 with the default suffix).

Real example (queue with `maxDeliveries: 2`, the message delivered twice without ack and returned with `visibilityTimeout: 0`):

```text
GET dl/messages  →  deliveryCount 1
GET dl/messages  →  deliveryCount 2     (after the deadline expired)
PUT .../visibility {"visibilityTimeout":0}   →  200
GET dl/messages  →  {"messages":[]}
GET dl-dead-letter/messages → {"messages":[{… "deadLetteredAt":"…","deadLetterReason":"max-deliveries"}]}
```

## Groups and fair delivery

The **noisy neighbor**: a producer that dumps thousands of messages makes the messages of whoever sends few wait behind. A message can carry a **`groupId`** (1 to 128 characters, `A-Za-z0-9_-.:`, immutable) and delivery gives **preference to the groups that have fewer messages**. Without `groupId`, the message belongs to the "empty" group and a topic without groups behaves like the simple ready list. In an **ordinary** queue the group **orders nothing**: it only serves as a key for fairness.

**Counting.** Each **owner** keeps, per topic, two counters per group: **ready** and **in processing** (invisible). The group **size** is:

```
size = in processing + weight × ready        (weight = delivery.fair.ready-weight, default 0.1)
```

The weight sits between SQS (`0`: only what is in processing, which reacts late to a burst) and the whole backlog (`1`: penalizes a large, harmless group of a slow consumer). The count is **local** (no exchange between nodes): since the ID decides the block, the messages of a group spread across the owners proportionally, so each node sees a faithful sample and fairness is **approximate in the cluster** and **exact on a single node**. The number of tracked groups has a ceiling (`delivery.fair.max-groups`); above it, new groups **share a counter**.

**Preference.** The size becomes a **band** `⌊log2(size)⌋` (size < 1 counts as 1): 1 → band 0, 2–3 → 1, 4–7 → 2… Power-of-2 bands avoid reordering the list at every message.

1. The **refill** walks the groups **from smallest to largest** and takes from each one the oldest ready messages, **one per group per round**.
2. The list is ordered by **band** and, within it, by the accordion. A new message enters at the position of its group's band.
3. **Delivery** takes from the smallest band with ready messages; if the group's band changed since the ID entered, the ID is repositioned.
4. **Guaranteed progress:** every `delivery.fair.normal-every` deliveries (default **10**), one takes the **oldest of any group**, ignoring the band. Large groups never wait without limit.

The `take` and the weighted draw between nodes **do not change**: fairness is applied by each owner, when choosing the ID. **In the FIFO queue** none of this applies (the oldest head wins). Per-group metrics: [13-observability.md](13-observability.md).

## Parameters

| Key | Default |
|---|---|
| `delivery.visibility-timeout.default` / `.max` | 120 s / no ceiling |
| `delivery.max-messages.default` / `.max` | 10 / no ceiling |
| `delivery.get.max-attempts` | 3 |
| `delivery.take.max-attempts` | 3 |
| `delivery.fair.ready-weight` / `max-groups` / `normal-every` | 0.1 / no ceiling / 10 |
| `retention.ttl.default` / `retention.max-deliveries.default` | 14 d / 10 |
| `retention.sweep.interval` | 60 s |
| `topics.dead-letter.suffix` | `-dead-letter` |
| `cluster.notify.window` / `cluster.notify.wait` | 100 ms / 50 ms |

## Known limits

- **PRB-019:** finding the middle message to demote and checking the ready ones when `maxDeliveries` changes scan the topic's memory; the `maxDeliveries` check and the on-demand queries (groups, stalled group) read the messages in pages, and the oldest message comes from the age order of the topic's memory and of the database.
- **D96:** who fixes the deadline of `PUT visibility` and of the ack, and the forwarding deadline, still has open points.
- **D109:** the channel constants (10 s of notice validity, 3 consecutive `not-member`) are in the code.
- **D113:** the FIFO receipt carries the group position (decided in the implementation).
- The FIFO queue keeps one entry per message, the cluster's replica table keeps the ID and age of each replica, and the groups have no ceiling (D88, PRB-020).
- The write to the `expiry` table undoes the delivery if it fails (single-node path); on deadline changes and on the ack the write is tried in the same step and, if it fails, the pair stays queued and is retried on the next change.

## Sources and evidence

- `engine/messages/mod.rs` (publish, consume, ack, expiry), `engine/messages/index/mod.rs` (lists, bands, accordion), `cluster/replication/messages_api.rs` and `signals.rs` (`take`, signals), `cluster/channel` (channel and output queue), `http/messages`.
- Example responses copied from a real node.
- Use cases: UC-CNS (consumption), UC-ACK, UC-VIS, UC-RET (`ttl`, deliveries and dead-letter), UC-GRP (groups), UC-ENT (delivery on the owner), UC-CAN (channel), UC-INV; see [26a](26a-implemented-use-cases.md). Tests: `tests/messages.rs`, `listeners.rs`, `channel.rs`, `notify.rs`.
- History: `13-entrega-de-mensagens.md`, `14-invisibilidade.md`.
