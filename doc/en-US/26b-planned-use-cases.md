---
title: Planned use cases
summary: The documented use cases that do not yet have a test citing them — what is left to prove — by area, with the reason when the case itself gives it.
type: reference
status: partial
verified-on: 2026-10-15 @ 56121c9
sources: [crates/taba-node/tests/uc_coverage.rs]
---

# Planned use cases (without a test)

> **Maintained by hand.** A case here **does not mean the behavior does not exist**: it means that **no test cites it** today. Some features already work (and appear in other documents as ✅) without proof per case; others are ⬜ planned. The cases with a test are in [26a-implemented-use-cases.md](26a-implemented-use-cases.md). This file is the English rendering of `doc/pt-BR/26b-casos-de-uso-planejados.md`: the codes, levels and test names are the same; "Fonte histórica" is rendered as "Historical source".

**Scoreboard (2026-10-24):** **134** cases without a test, out of **862**. The cases marked "(pendente: D##)" depend on a question still open ([31-pending-items.md](31-pending-items.md)).

Level: **U** unit, **I** a real node, **C** simulated cluster, **K** connector. To close a case, cite it in a test (`/// UC: UC-XXX-NNN`) of the indicated level and regenerate ([26-use-cases.md](26-use-cases.md)).

## Where the gaps are

| Area | Subject | Without test | With test |
|---|---|---:|---:|
| `TOP` | Topics: create, list, read, change and remove | 3 | 45 |
| `MSG` | Publishing and the message body | 1 | 19 |
| `CNS` | Consumption (`GET`), long polling and `autoCreate` | 1 | 39 |
| `VIS` | Change of invisibility | 2 | 10 |
| `RET` | Expiration (`ttl`), delivery limit and dead-letter | 3 | 17 |
| `DST` | Distributor Topic | 12 | 6 |
| `GRP` | Message groups and fair delivery | 1 | 16 |
| `AUT` | Authentication and access tokens | 6 | 33 |
| `LIM` | Rate limit and client IP | 2 | 13 |
| `PAG` | Pagination | 2 | 7 |
| `CNV` | Common API conventions (format, headers, errors, status) | 6 | 11 |
| `ESC` | Write: register + commit, quorum, rollback, commit ceiling | 9 | 29 |
| `ANE` | Hash ring, blocks and roles | 3 | 8 |
| `DEF` | Topic and token definition replicated in the cluster | 10 | 21 |
| `ENT` | Delivery on the owner: ready list, `take`, long polling | 19 | 11 |
| `INV` | Invisibility on the owner and on the replicas | 15 | 16 |
| `ARM` | Storage: memory, overflow, SQLite, integrity | 7 | 22 |
| `ING` | Node join (bootstrap) | 3 | 31 |
| `FAL` | Failure detection, exclusion vote and exit by failure | 1 | 32 |
| `FRZ` | Freezing period | 0 | 13 |
| `CNC` | Concurrency between membership changes | 2 | 12 |
| `SIN` | Distributed synchronization of blocks | 0 | 29 |
| `STR` | NDJSON streams between nodes | 7 | 8 |
| `FWD` | Forwarding of client calls | 7 | 4 |
| `DRN` | Stop and drain mode | 2 | 19 |
| `DEV` | One-node profile (the former development mode) | 0 | 3 |
| `OPS` | Observability (OpenTelemetry), statistics, metrics and health | 7 | 40 |
| `CLR` | Contract of the cluster routes (DTOs and common rules) | 2 | 8 |
| `CFG` | Configuration and startup validations | 1 | 10 |

## TOP: Topics: create, list, read, change and remove

### UC-TOP-007

**Case:** Create with valid tags (up to 20; key of 1 to 64 and value of 0 to 128 characters of `A-Za-z0-9_-.:`)  
**Then:** `201` and the tags come back in the `Topic`.  
**Level:** I  
**Historical source:** 06a

---


### UC-TOP-025

**Case:** Listing and reading are answered locally, without calling other nodes (the definition is on every node).  
**Level:** C  
**Historical source:** 06a, 02 §3.3

---

### UC-TOP-033

**Case:** `PATCH` of `idleTimeout` on a volatile queue applies from the manager's next check.  
**Level:** I, C  
**Historical source:** 06a, 15 §14.3

---


## MSG: Publishing and the message body

### UC-MSG-017

**Case:** `502` or `504` on a forwarding after the body was sent, outside FIFO  
**Then:** the call is not repeated and the error rises to the client.  
**Level:** C  
**Historical source:** 06a, 04 §6.9

---


## CNS: Consumption (`GET`), long polling and `autoCreate`

### UC-CNS-039

**Case:** The first `GET` with `autoCreate` pays for creating the definition (proposed to the owner and applied on all nodes); the following ones do not.  
**Level:** C  
**Historical source:** 15 §14.4

---


## VIS: Change of invisibility

### UC-VIS-008

**Case:** `PUT` without a quorum  
**Then:** `503` `cluster.quorum-unavailable`, and the previous deadline remains in force.  
**Level:** C  
**Historical source:** 06a, 14 §13.3

---

### UC-VIS-012

**Case:** The `PUT` is decided by the same register + commit by quorum as delivery, without touching the ready list (except with `0`).  
**Level:** C  
**Historical source:** 13 §12.3, 14 §13.3

---


## RET: Expiration (`ttl`), delivery limit and dead-letter

### UC-RET-012

**Case:** The dead-letter has no definition of its own  
**Then:** there is no row in the `topics` table and the settings are those of the source queue.  
**Level:** I  
**Historical source:** 13 §12.10, 02 §3.3

---

### UC-RET-019

**Case:** On the FIFO queue, the message that reaches `maxDeliveries` goes to the dead-letter and the group continues with the next one; one expired by `ttl` at the head also releases the group (`FIF` cases).  
**Level:** I  
**Historical source:** 13 §12.10, 17 §16.6

---

### UC-RET-020

**Case:** Block synchronization discards what has already expired by `ttl` instead of copying it.  
**Level:** C  
**Historical source:** 13 §12.10, 05 §7.3

---


## DST: Distributor Topic

### UC-DST-004

**Case:** A queue that matches **two or more prefixes of the same** distributor receives **one** copy.  
**Level:** I  
**Historical source:** 15 §14.2

---

### UC-DST-005

**Case:** A queue that matches prefixes of **different distributors** receives one copy of each publication on each.  
**Level:** I  
**Historical source:** 15 §14.2

---

### UC-DST-006

**Case:** Only queues are destinations  
**Then:** a distributor never receives a copy (no loops).  
**Level:** I  
**Historical source:** 15 §14.2

---

### UC-DST-007

**Case:** The match is made at publish time  
**Then:** a queue created later only receives what is published afterwards.  
**Level:** I  
**Historical source:** 15 §14.2

---

### UC-DST-008

**Case:** Partial failure  
**Then:** one copy fails (for example, without quorum) and the others continue; the response is `200`, with `error` (`code`, `message`) on the item that failed and `id` on the others, and the event `taba.distributor.partial`. Nothing is undone.  
**Level:** C  
**Historical source:** 06a, 15 §14.2

---

### UC-DST-010

**Case:** The informed `groupId` and `deduplicationId` go to all the copies; without `deduplicationId`, each copy uses the body hash.  
**Level:** I  
**Historical source:** 15 §14.2, 17 §16.8

---

### UC-DST-012

**Case:** On a distributor the general `message.max-bytes` applies, and the copies are not checked again against each queue's `maxMessageBytes`.  
**Level:** I  
**Historical source:** 12

---

### UC-DST-013

**Case:** The `PATCH` of `prefixes` applies from the commit, only to later publications.  
**Level:** I  
**Historical source:** 06a, 15 §14.2

---

### UC-DST-014

**Case:** The distributor does not store messages  
**Then:** there is no message in memory or on disk for it, and no ready list.  
**Level:** I  
**Historical source:** 15 §14.2

---

### UC-DST-015

**Case:** There is no order between the destination queues. The test cannot require order.  
**Level:** I  
**Historical source:** 15 §14.2

---

### UC-DST-016

**Case:** `GET` and ack on a distributor  
**Then:** `409` `topic.wrong-type`.  
**Level:** I  
**Historical source:** 15 §14.1

---

### UC-DST-017

**Case:** The `publish` permission is checked only on the distributor; the destination queues are not checked again (nor the token's prefixes).  
**Level:** I  
**Historical source:** 06a §8.1.3

---


## GRP: Message groups and fair delivery

### UC-GRP-014

**Case:** On a distributor, the `groupId` is passed on to all the copies.  
**Level:** I  
**Historical source:** 13 §12.11, 15 §14.2

---


## AUT: Authentication and access tokens

### UC-AUT-004

**Case:** `auth.enabled=true` without a root token prevents the node from starting; so does a defined `auth.root-token-file` that does not exist.  
**Level:** I  
**Historical source:** 06a §8.1.3, 10

---

### UC-AUT-019

**Case:** Validation is a local in-memory lookup, with no network call  
**Then:** with the other nodes down, a valid token is still accepted.  
**Level:** C  
**Historical source:** 06a §8.1.3, 02 §3.4

---

### UC-AUT-030

**Case:** Creating, changing and revoking a token are proposed to the token's **owner** on the ring, which gives the next version, writes to the heir and applies on all nodes (`definitions/propose` and `apply`); they do not wait for all nodes  
**Then:** the owner answers after the nodes that reply within `write.commit.timeout`, and the others receive it through the channel; without the owner (down and still in the table) or with no other owner to confirm, `503` `cluster.quorum-unavailable` and nothing is applied.  
**Level:** C  
**Historical source:** 06a, 02 §3.4

---

### UC-AUT-031

**Case:** The secret never travels between nodes  
**Then:** the `TokenRecord` of `propose` and `apply` carries only the `salt` and the `secretHash`.  
**Level:** C  
**Historical source:** 06c, 02 §3.4

---

### UC-AUT-033

**Case:** A node that joins pulls the tokens (`GET /cluster/tokens`) together with the topic table; until the load finishes the node does not serve client routes, so it never validates with an incomplete table.  
**Level:** C  
**Historical source:** 02 §3.4

---

### UC-AUT-034

**Case:** A forwarded call (node in synchronization or drain) carries the `Authorization` as it came, and the **destination** checks the token again.  
**Level:** C  
**Historical source:** 06a, 04 §6.9

---


## LIM: Rate limit and client IP

### UC-LIM-008

**Case:** The counters are local to each node, with no exchange between nodes.  
**Level:** C  
**Historical source:** 12

---

### UC-LIM-014

**Case:** Node in synchronization or drain  
**Then:** counts the call per IP and forwards it; the destination does not count again. The cluster port has no rate limit.  
**Level:** C  
**Historical source:** 12, 04 §6.9

---


## PAG: Pagination

### UC-PAG-004

**Case:** Items inserted or removed before the cursor key during the walk do not shift the next page (keyset pagination, not by offset).  
**Level:** I  
**Historical source:** 06a §8.1.2

---

### UC-PAG-009

**Case:** `GET /metrics/topics` does not accept `sort` (`400`) and always paginates by name.  
**Level:** I  
**Historical source:** 06b, 11

---


## CNV: Common API conventions (format, headers, errors, status)

### UC-CNV-005

**Case:** `traceparent` in the request  
**Then:** the node's operations join the same trace.  
**Level:** I  
**Historical source:** 06d §8.3.2, 11

---

### UC-CNV-010

**Case:** `POST`, `PATCH` and `DELETE` that do not expect a body ignore whatever comes; `204` never has a body.  
**Level:** I  
**Historical source:** 06d §8.3.1

---

### UC-CNV-013

**Case:** The `/health/*` routes return `HealthResponse` (never `ErrorResponse`), and a failure **after the `200`** of an NDJSON stream uses the line `{"error":"..."}`.  
**Level:** I, C  
**Historical source:** 06d §8.3.3

---

### UC-CNV-014

**Case:** An error inside a partial result (`200` of the distributor) carries `error` with `code` and `message`, **without status**  
**Then:** the item's status is that of the `code` in the catalogue.  
**Level:** I  
**Historical source:** 06d §8.3.3

---

### UC-CNV-015

**Case:** An operation that creates or deletes (topic, token) and is left without a definitive answer (`504` or a cut connection) may already have been applied by the owner  
**Then:** on repeating, `409` `topic.already-exists` (creation) or `404` (removal) mean the operation had already been applied. A `503` `cluster.quorum-unavailable` applied nothing.  
**Level:** C  
**Historical source:** 06d §8.3.5

---

### UC-CNV-016

**Case:** `Accept: application/openmetrics-text` on `/metrics` asks for OpenMetrics; the default is Prometheus text.  
**Level:** I  
**Historical source:** 06d §8.3.2, 06b

---


## ESC: Write: register + commit, quorum, rollback, commit ceiling

### UC-ESC-018

**Case:** The rollback only applies **before** the decision  
**Then:** it is never sent after the quorum decided.  
**Level:** C  
**Historical source:** 05 §7.2

---

### UC-ESC-027

**Case:** `commit` or `rollback` of an operation the node does not know (already expired or never arrived)  
**Then:** `404` `cluster.operation-unknown`.  
**Level:** C  
**Historical source:** 06c

---

### UC-ESC-028

**Case:** `register` of an `id` that already has a pending register, or of a message that already exists with different content  
**Then:** `409` `cluster.conflict`.  
**Level:** C  
**Historical source:** 06c

---

### UC-ESC-031

**Case:** A node **in synchronization** confirms the `register` by writing it to the event database (and applies it later); for the coordinator the confirmation counts normally toward the quorum.  
**Level:** C  
**Historical source:** 06c, 05 §7.3

---

### UC-ESC-032

**Case:** A node that is not a member receives `403` `cluster.not-member` on any write route.  
**Level:** C  
**Historical source:** 06c

---

### UC-ESC-033

**Case:** Coordinator  
**Then:** `taba.write.coordinated` (`committed` and `failed`), `taba.write.duration` per phase, `taba.write.retries`, `taba.write.rollbacks` (only for quorum failure), `taba.write.replicas_confirmed` (per `stage`: `response` and `eventual`) and `taba.write.quorum_latency`.  
**Level:** C  
**Historical source:** 11

---

### UC-ESC-034

**Case:** Replica  
**Then:** `taba.write.received` per phase, `taba.write.pending`, `taba.write.pending.oldest_age` and `taba.write.pending.expired`.  
**Level:** C  
**Historical source:** 11

---

### UC-ESC-035

**Case:** The publishing trace has the root span, the register and the commit per destination node and, if applicable, the rollback; the root span ends when the quorum confirms; the `traceparent` is propagated between nodes. Failures and durability events are always kept in sampling.  
**Level:** C  
**Historical source:** 11

---

### UC-ESC-036

**Case:** The logs  
**Then:** register or commit failure per attempt (`warn`), write aborted with rollback (`error`), pending register expired (`warn`), commit ceiling exhausted on a replica (`warn`).  
**Level:** C  
**Historical source:** 11

---


## ANE: Hash ring, blocks and roles

### UC-ANE-006

**Case:** The topic is spread out  
**Then:** the messages of the same topic fall all over the ring; no node stores the whole topic or is its owner.  
**Level:** C  
**Historical source:** 05 §7.1

---

### UC-ANE-007

**Case:** The loss of a node leaves each message still on at least `replication.factor − 1` nodes.  
**Level:** C  
**Historical source:** 05 §7.1

---


### UC-ANE-012

**Case:** The ring metrics  
**Then:** `taba.ring.virtual_nodes`, and `taba.ring.blocks` per role (`primary`, `secondary`, `tertiary`) and state (`healthy`, `syncing`).  
**Level:** C  
**Historical source:** 11

---


## DEF: Topic and token definition replicated in the cluster

### UC-DEF-010

**Case:** The file stores **only the definition**  
**Then:** consumption data (depth, rates, histograms) never goes into it.  
**Level:** I  
**Historical source:** 02 §3.3

---

### UC-DEF-011

**Case:** The dead-letter has no definition row in the `topics` table  
**Then:** it is derived from the source queue, created and deleted in the same operation.  
**Level:** I  
**Historical source:** 02 §3.3

---

### UC-DEF-012

**Case:** Each token is a row of the `tokens` table of `_node.db` (with `id`, `name`, permissions, prefixes, dates, the salt and the salted hash of the secret, never the secret).  
**Level:** I  
**Historical source:** 02 §3.4

---

### UC-DEF-013

**Case:** On a start where the node starts **alone** (the first node, or the one with the lowest old ID on resume), it loads the definitions from `_node.db` (tables `topics` and `tokens`), with the versions and tombstones of `definition_versions`, preserving the `id` and creation date of each. A node that joins a cluster that is already up does not use them: it receives the cluster's and only **offers** them afterwards.  
**Level:** I, C  
**Historical source:** 04 §6.10

---

### UC-DEF-014

**Case:** The `DELETE` always works, even with undelivered or unconfirmed messages. On applying the removal (`apply` with `deleted`), each node deletes all the topic's messages it has locally (memory, `${topic}/` on disk and metadata), without checking whether they are pending, and the definition's tombstone remains.  
**Level:** C  
**Historical source:** 02 §3.3

---

### UC-DEF-017

**Case:** The load is applied straight to the final storage; everything that arrives during the load (definition changes by `apply` and the register + commit phases of messages) goes to the event database, in order of arrival, and the node confirms the registration **after writing**.  
**Level:** C  
**Historical source:** 02 §3.3

---

### UC-DEF-018

**Case:** After the load, the event database is processed on top, line by line, deleting the processed ones  
**Then:** the event wins over the load (a removal during the load deletes what the load brought). A message of a still unknown topic waits in the same database.  
**Level:** C  
**Historical source:** 02 §3.3

---

### UC-DEF-019

**Case:** Final check  
**Then:** when it finishes, the node repeats the listing and discards what no longer appears at the source (covers a removal whose commit did not reach it).  
**Level:** C  
**Historical source:** 02 §3.3

---

### UC-DEF-020

**Case:** If the source node fails in the middle, the load continues on another node, from the last `id` read.  
**Level:** C  
**Historical source:** 02 §3.3

---

### UC-DEF-021

**Case:** Until the whole synchronization finishes, the node does not serve client routes (`/health/ready` fails)  
**Then:** `GET /topics`, the prefix matching and the computation of the managing node depend on the complete table.  
**Level:** C  
**Historical source:** 02 §3.3

---


## ENT: Delivery on the owner: ready list, `take`, long polling

### UC-ENT-003

**Case:** No message is delivered twice, even with member tables diverging for moments  
**Then:** the invisibility register is exclusive (rejected if another is already pending or if the message is already invisible) and the `GET` tries another ID.  
**Level:** C  
**Historical source:** 13 §12.7, 14 §13.3

---

### UC-ENT-005

**Case:** A node that joined and is not yet healthy does **not** deliver the messages of which it is primary; the old primary removes them from the ready list at the register commit; nobody delivers them until synchronization finishes. The client calls that reach this node are forwarded, and it sends no notices or ready signals.  
**Level:** C  
**Historical source:** 13 §12.7

---

### UC-ENT-006

**Case:** Owner down and not yet deregistered  
**Then:** its messages stay undelivered until the deregistration; the other nodes of the block are backup copies and do **not** take over delivery.  
**Level:** C  
**Historical source:** 13 §12.7

---


### UC-ENT-008

**Case:** The ready list is kept per topic on each node, in memory, with `delivery.ready-list.size` IDs (default 1000).  
**Level:** U  
**Blocked:** Depends on implementation: the bounded ready list (`delivery.ready-list.size`) with cursor-based refill. Today the index is complete in memory and the parameter is read and ignored.  
**Historical source:** 13 §12.6

---

### UC-ENT-009

**Case:** A thread refills the list when the semaphore opens room (committed delivery, ID leaving), refilling the amount that opened, without querying for nothing.  
**Level:** U  
**Blocked:** Depends on implementation: the bounded ready list (`delivery.ready-list.size`) with cursor-based refill. Today the index is complete in memory and the parameter is read and ignored.  
**Historical source:** 13 §12.6

---

### UC-ENT-010

**Case:** The refill follows the accordion  
**Then:** two cursors over the node's messages (from the oldest forwards and from the newest backwards, by GUID v7, in memory and on disk), alternating the two ends.  
**Level:** U  
**Blocked:** Depends on implementation: the bounded ready list (`delivery.ready-list.size`) with cursor-based refill. Today the index is complete in memory and the parameter is read and ignored.  
**Historical source:** 13 §12.6

---

### UC-ENT-012

**Case:** A new message that arrives at the owner enters the list directly if there is room and the new-end cursor is at the end; otherwise it is found later by the thread.  
**Level:** U  
**Blocked:** Depends on implementation: the bounded ready list (`delivery.ready-list.size`) with cursor-based refill. Today the index is complete in memory and the parameter is read and ignored.  
**Historical source:** 13 §12.6

---

### UC-ENT-014

**Case:** The message whose invisibility expires returns to the list by the GUID v7 position, **not** by the scan; if the list is full, the **middle** ID leaves to make room and is refilled later (no message is lost).  
**Level:** U  
**Blocked:** Depends on implementation: the bounded ready list (`delivery.ready-list.size`) with cursor-based refill. Today the index is complete in memory and the parameter is read and ignored.  
**Historical source:** 13 §12.6, 14 §13.6

---

### UC-ENT-015

**Case:** The `GET` only queries the list, without scanning memory and disk at each call.  
**Level:** U  
**Blocked:** Depends on implementation: the bounded ready list (`delivery.ready-list.size`) with cursor-based refill. Today the index is complete in memory and the parameter is read and ignored.  
**Historical source:** 13 §12.6

---

### UC-ENT-016

**Case:** Scenario 1  
**Then:** local list with a message: the node delivers directly, without querying any other node to find the message.  
**Level:** C  
**Historical source:** 13 §12.8

---

### UC-ENT-018

**Case:** The `take` carries the **quantity still missing** to complete the client's `maxMessages` and the requested `visibilityTimeout`.  
**Level:** C  
**Historical source:** 13 §12.2 and 12.8, 06c

---

### UC-ENT-019

**Case:** The peer is drawn with a **weight** proportional to the ready messages reported in the heartbeat; without quantity information, the draw is uniform.  
**Level:** U  
**Blocked:** Depends on implementation: the `ready_peers` draw is uniform; there is no weight by the ready count reported in the heartbeat.  
**Historical source:** 13 §12.8

---

### UC-ENT-020

**Case:** The `take` answers an empty list when the "has ready messages" state was a stale hint  
**Then:** the node tries the next one, up to `delivery.take.max-attempts`; when exhausted, it moves on to long polling. The cost is bounded, never a request to all nodes.  
**Level:** C  
**Historical source:** 13 §12.8

---

### UC-ENT-021

**Case:** Whoever delivers in the `take` is always the message's owner  
**Then:** the message has a single coordinator, the clock that sets the `invisibleUntil` is the owner's.  
**Level:** C  
**Historical source:** 13 §12.8, 14 §13.3

---

### UC-ENT-026

**Case:** N listeners waiting for the same topic on a node generate **one** interest report; on receiving a notice, the node fetches at once as many messages as there are listeners.  
**Level:** C  
**Historical source:** 13 §12.8

---

### UC-ENT-027

**Case:** A lost notice only costs waiting until the end of `waitTime` (at most 20 s).  
**Level:** C  
**Historical source:** 13 §12.8

---

### UC-ENT-028

**Case:** When `waitTime` runs out the call answers empty and the listener is removed. If the client disconnects earlier, the listener is also removed.  
**Level:** C  
**Historical source:** 13 §12.8

---

### UC-ENT-029

**Case:** The message whose invisibility expires, with the ready list empty, sends the "has ready messages" signal; with a local listener, the message goes straight to it.  
**Level:** C  
**Historical source:** 14 §13.6

---

### UC-ENT-030

**Case:** Accepted trade-off  
**Then:** the primary goes down between the local commit of the invisibility and the replication to the replicas outside the quorum; the new primary inherits the `invisibleUntil` of whoever survived and the message may be delivered before the promised deadline, without breaking at-least-once.  
**Level:** C  
**Historical source:** 13 §12.2

---


## INV: Invisibility on the owner and on the replicas

### UC-INV-002

**Case:** `invisibleUntil` is computed by the **owner** (`now + visibilityTimeout`, by its clock); the replicas store the ready value and never recompute from a duration.  
**Level:** C  
**Historical source:** 14 §13.2 and 13.3

---

### UC-INV-009

**Case:** The **owner** is always the coordinator  
**Then:** on a local `GET` itself; on a `take`, the node that serves the client asks the owner, which coordinates.  
**Level:** C  
**Historical source:** 14 §13.3

---

### UC-INV-011

**Case:** The commit is decided by quorum (primary included); only then does the call answer the client. The replicas outside the quorum receive the commit in the background, within `write.commit.timeout`.  
**Level:** C  
**Historical source:** 14 §13.3

---

### UC-INV-015

**Case:** The clock difference between nodes, at an owner change, shortens or lengthens the deadline by the size of the difference  
**Then:** effect accepted in at-least-once.  
**Level:** C  
**Historical source:** 14 §13.3

---

### UC-INV-017

**Case:** The owner's thread looks at the head of the invisible list, sleeps until its deadline (with a short ceiling to react to changes and earlier entries) and, on expiry, removes the entry and **checks** the metadata  
**Then:** the message still exists, the node is still the owner and the deadline is still the entry's; otherwise it discards and moves on.  
**Level:** U  
**Blocked:** Depends on implementation: the invisible list with a ceiling (`delivery.invisible-list.size`), the `expiry` table and the reload. Today the list is an in-memory set with no ceiling, and both parameters are read and ignored.  
**Historical source:** 14 §13.6

---

### UC-INV-018

**Case:** After the check, the message enters the ready list by the GUID v7 position (near the old end); with the list full, the middle ID leaves.  
**Level:** U  
**Blocked:** Depends on implementation: the invisible list with a ceiling (`delivery.invisible-list.size`), the `expiry` table and the reload. Today the list is an in-memory set with no ceiling, and both parameters are read and ignored.  
**Historical source:** 14 §13.6

---

### UC-INV-021

**Case:** If the invisible list empties and there are still rows in the expiry table, the thread does the reload before sleeping.  
**Level:** U  
**Blocked:** Depends on implementation: the invisible list with a ceiling (`delivery.invisible-list.size`), the `expiry` table and the reload. Today the list is an in-memory set with no ceiling, and both parameters are read and ignored.  
**Historical source:** 14 §13.6

---

### UC-INV-022

**Case:** The invisible list is, per topic and only on the owner, a list of (ID, `invisibleUntil`) pairs ordered by deadline, with at most `delivery.invisible-list.size` entries; lookup by ID for change and ack.  
**Level:** U  
**Blocked:** Depends on implementation: the invisible list with a ceiling (`delivery.invisible-list.size`), the `expiry` table and the reload. Today the list is an in-memory set with no ceiling, and both parameters are read and ignored.  
**Historical source:** 14 §13.4

---

### UC-INV-023

**Case:** Inclusion rule with the list full  
**Then:** if the new deadline is **earlier** than the last one in the list, the new pair enters and the last goes to the table; otherwise the new pair goes straight to the table.  
**Level:** U  
**Blocked:** Depends on implementation: the invisible list with a ceiling (`delivery.invisible-list.size`), the `expiry` table and the reload. Today the list is an in-memory set with no ceiling, and both parameters are read and ignored.  
**Historical source:** 14 §13.5

---

### UC-INV-024

**Case:** Invariant  
**Then:** everything in the list expires before everything in the table; the thread never queries the table while the list does not empty.  
**Level:** U  
**Blocked:** Depends on implementation: the invisible list with a ceiling (`delivery.invisible-list.size`), the `expiry` table and the reload. Today the list is an in-memory set with no ceiling, and both parameters are read and ignored.  
**Historical source:** 14 §13.5

---

### UC-INV-025

**Case:** Reload  
**Then:** below `delivery.invisible.reload-threshold` (or empty), the thread reads from the table the pairs with the lowest `invisibleUntil` (an ordered query with a limit by the index), moves them to the list and deletes the rows read.  
**Level:** U, I  
**Blocked:** Depends on implementation: the invisible list with a ceiling (`delivery.invisible-list.size`), the `expiry` table and the reload. Today the list is an in-memory set with no ceiling, and both parameters are read and ignored.  
**Historical source:** 14 §13.5

---

### UC-INV-026

**Case:** No old entries  
**Then:** ack, change and new delivery delete or update the table row in the same commit that updates the list. There is no lazy removal.  
**Level:** U, I  
**Blocked:** Depends on implementation: the invisible list with a ceiling (`delivery.invisible-list.size`), the `expiry` table and the reload. Today the list is an in-memory set with no ceiling, and both parameters are read and ignored.  
**Historical source:** 14 §13.5

---

### UC-INV-027

**Case:** The write to the table is part of the commit on the owner  
**Then:** if it fails, the commit does not complete and the delivery is undone before the response to the client.  
**Level:** I  
**Historical source:** 14 §13.5

---

### UC-INV-028

**Case:** With room in the list, no write beyond the metadata.  
**Level:** U  
**Blocked:** Depends on implementation: the invisible list with a ceiling (`delivery.invisible-list.size`), the `expiry` table and the reload. Today the list is an in-memory set with no ceiling, and both parameters are read and ignored.  
**Historical source:** 14 §13.5

---

### UC-INV-031

**Case:** Restart  
**Then:** the expiry table is discarded along with the old state; the messages of the previous life come back through the restoration (`_lives`), ready, with the invisibility zeroed and a new `receiptToken`, and the lists are rebuilt when the node takes over blocks.  
**Level:** C  
**Historical source:** 14 §13.7

---


## ARM: Storage: memory, overflow, SQLite, integrity

### UC-ARM-012

**Case:** The `expiry` table stores the pairs that did not fit in the invisible list, with an index by deadline.  
**Level:** I  
**Historical source:** 02 §3.2, 14 §13.5

---

### UC-ARM-016

**Case:** The database answers  
**Then:** the oldest message of the topic (`MIN(id)`, added to the oldest in memory, kept ordered by ID), the next to expire (index by `invisibleUntil`), the messages of a block (range by ring position) and the topic counts.  
**Level:** I  
**Historical source:** 02 §3.2

---

### UC-ARM-018

**Case:** The node database, `${storage.dir}/_node.db`, is strictly local, with the tables `topics` (the topic definitions), `tokens`, `state`, `losses` (`topic`, `id`, `block`, `detected_at`) and `definition_versions` (the versions and tombstones), written through a single connection, the only writer.  
**Level:** I  
**Historical source:** 02 §3.2

---

### UC-ARM-019

**Case:** Corrupted database  
**Then:** the node goes into **failure**, signals the cluster (which is free to vote its deregistration) and does not try to repair. `/health/ready` becomes `503` with `reason: failed`.  
**Level:** C  
**Historical source:** 02 §3.2, 05 §7.3

---

### UC-ARM-020

**Case:** Full disk or I/O failure  
**Then:** the node goes into failure, as with a corrupted database; the error counts in `taba.storage.disk.errors` and generates an `error` log.  
**Level:** C  
**Historical source:** 05 §7.3, 11

---

### UC-ARM-021

**Case:** The node creates `storage.dir` (default `/var/lib/taba`, the last folder is named `taba`) at start; if it cannot create or write, it does not start.  
**Level:** I  
**Historical source:** 10

---

### UC-ARM-025

**Case:** **Consumption** data (depth, visible, invisible, rates, latencies) stays only in memory and specific to each node.  
**Level:** I  
**Historical source:** 02 §3.3

---


## ING: Node join (bootstrap)

### UC-ING-012

**Case:** At the end, the new node knows all the other members and vice versa, and actively opens the permanent channel with each node of the table.  
**Level:** C  
**Historical source:** 04 §6.2

---

### UC-ING-019

**Case:** The failure of the node's register (the node as a whole does not join) has the effect of exclusion + rebalancing; the loss of a message in synchronization only generates an alert.  
**Level:** C  
**Historical source:** 04 §6.5, 07

---


### UC-ING-026

**Case:** The member table is **not** persisted as live state  
**Then:** restarting the process loses the table and the node comes back as a new node, with a new ID; the previous life's `manifest.json` keeps a copy of it only for the restoration and the probing of the old IPs, and does not make the node come back as the old one.  
**Level:** C  
**Historical source:** 13 §12.9

---


## FAL: Failure detection, exclusion vote and exit by failure

### UC-FAL-030

**Case:** **No test yet.** An exclusion that cannot take effect within the node's turn plus the join deadline (`turn_at` + `JOIN_TIMEOUT`, 60 s) ends the vote as `failed`  
**Then:** the node stays in the table and the next channel failure opens another vote (D98). To test: keep the deregistration in permanent conflict and reduce the deadline through a test configuration. Checks the `failed` result in the vote metric, the node still in the table and the new vote on the next failure.  
**Level:** C  
**Historical source:** 04 §6.3

---


## CNC: Concurrency between membership changes


### UC-CNC-011

**Case:** **No test yet.** A join that only finds conflict (another membership change always older) repeats after a random 100 to 200 ms, but gives up at the end of 60 s (`JOIN_TIMEOUT`)  
**Then:** the coordinator returns `cluster.registration-failed` and the new node repeats the bootstrap at `cluster.bootstrap.retry.interval`. To test: keep a pending change that never ends (for example, a `register` that the oldest node never confirms) and reduce the 60 s deadline through a test configuration, so the suite does not wait real time. Checks the error at the end of the deadline, the rollback of what the coordinator registered and the node's new attempt.  
**Level:** C  
**Historical source:** 04 §6.2 and 6.8

---

### UC-CNC-012

**Case:** **No test yet.** An exclusion and a join that reach different coordinators at the same instant end with both in effect, one after the other  
**Then:** the change with the lowest `operationId` wins, and the other yields, undoes and restarts (D94). Today only the rule is tested, in `Membership` (UC-CNC-010), and the join's wait for the vote (UC-CNC-002); what is missing is to provoke the real crossing, with the `register` and the `unregister` delayed in the test network so the two phases 1 overlap. Checks that the final table has the new node and does not have the excluded one, that neither was left pending and that the displaced change restarted.  
**Level:** C  
**Historical source:** 04 §6.8

---


## STR: NDJSON streams between nodes

### UC-STR-005

**Case:** Ceiling per stream  
**Then:** the sender closes on reaching `cluster.sync.stream.max-items` or `max-bytes`, with `hasMore: true`; the receiver opens another with `from=<last>`.  
**Level:** C  
**Historical source:** 06c §8.2.1

---

### UC-STR-007

**Case:** Error after the `200`  
**Then:** the sender writes `{"error":"..."}` (which is not `ErrorResponse`) and closes, or just closes without the final line; in both cases the receiver resumes.  
**Level:** C  
**Historical source:** 06c §8.2.1

---

### UC-STR-008

**Case:** Read timeout **per line** (`cluster.sync.stream.idle-timeout`), not a ceiling for the whole stream; no *heartbeat*.  
**Level:** C  
**Historical source:** 06c §8.2.1

---

### UC-STR-010

**Case:** Backpressure  
**Then:** the receiver reads at its own pace and the sender holds no transaction or frozen list open; it reads storage in small chunks.  
**Level:** C  
**Historical source:** 06c §8.2.1

---

### UC-STR-013

**Case:** `POST /cluster/messages/recover`  
**Then:** NDJSON body of `MessageRecord` ended by `StreamEnd`; the response is `RecoverResult` (`end: true`, `count` applied); if the connection drops, the sender resends everything and those already applied are ignored.  
**Level:** C  
**Historical source:** 06c

---

### UC-STR-014

**Case:** The message streams (`ids` and `messages`) carry no tombstone  
**Then:** the removal of a message continues by the absence of the ID; what keeps a dead copy from coming back is the log of excluded messages (`/cluster/blocks/{id}/excluded`) and, for definitions, the tombstone stream (`/cluster/definitions/tombstones`), each on its own route.  
**Level:** C  
**Historical source:** 06c §8.2.1

---

### UC-STR-015

**Case:** Metrics  
**Then:** `taba.sync.streams` per `kind`, `direction`, `outcome` (`complete`, `truncated`, `error`), `taba.sync.stream.resumes`, `duration` and `bytes`.  
**Level:** C  
**Historical source:** 11

---


## FWD: Forwarding of client calls

### UC-FWD-002

**Case:** Destination in synchronization  
**Then:** a member of the table with an active channel, chosen at random (there is only one node in synchronization at a time).  
**Level:** C  
**Historical source:** 04 §6.9

---

### UC-FWD-003

**Case:** Destination in drain  
**Then:** the **freest node** (lowest load by the heartbeat, plus the calls already forwarded and in progress to that destination), with a tie drawn at random; only ready nodes outside drain enter; the choice is made **at each call**. After the channel closes, the last indicator and the last known table apply.  
**Level:** U, C  
**Blocked:** The code diverges from the card: `Forwarder::pick` uses only the node's own in-flight calls and draws among ties; the heartbeat load is not used. Decide whether the card or the code changes before testing.  
**Historical source:** 04 §6.9

---

### UC-FWD-005

**Case:** The forwarding goes to the destination's **cluster port**, through the route `/cluster/forward/{path}`; the client API does not accept an internal forwarding marker.  
**Level:** C  
**Historical source:** 04 §6.9, 06c

---

### UC-FWD-006

**Case:** With authentication on, the `Authorization` goes as it came and the **destination checks** the token (the node in synchronization does not yet have the token table).  
**Level:** C  
**Historical source:** 04 §6.9, 06c

---

### UC-FWD-007

**Case:** No loop  
**Then:** the route `/cluster/forward` is the marker; whoever receives it never forwards again, and if synchronizing it answers `503` `cluster.node-syncing`. Accepts only from members and from nodes that left by drain (an in-memory list filled at the commit of the voluntary deregistration and emptied when a new node registers with the same address).  
**Level:** C  
**Historical source:** 04 §6.9, 06c

---

### UC-FWD-008

**Case:** Destination failure **before** the body is sent (connection refused)  
**Then:** tries another node, up to a small ceiling. **After** it is sent: **does not repeat** (a repeated message `POST` would duplicate) and answers `502` `forward.bad-gateway` or `504` `forward.timeout`.  
**Level:** C  
**Historical source:** 04 §6.9

---

### UC-FWD-010

**Case:** No destination answering  
**Then:** `503` with `Retry-After` (`cluster.node-syncing` or `cluster.node-draining`).  
**Level:** C  
**Historical source:** 04 §6.9, 06c

---


## DRN: Stop and drain mode (includes drain as save)

### UC-DRN-015

**Case:** `drain.timeout` must be shorter than the orchestrator's stop deadline (`terminationGracePeriodSeconds`, 30 s by default in Kubernetes).  
**Level:** I  
**Historical source:** 04 §6.10, 10

---

### UC-DRN-028

**Case:** On a single node (one-node profile), client calls during drain answer `503` with `Retry-After` (there is no other node to forward to).  
**Level:** I  
**Historical source:** 04 §6.4 and 6.10

---


## OPS: Observability (OpenTelemetry), statistics, metrics and health

### UC-OPS-015

**Case:** The conditions appear as `0/1` gauges with the unit `{flag}` (`taba_cluster_condition{taba_condition=...}`).  
**Level:** I  
**Historical source:** 11

---

### UC-OPS-020

**Case:** In `GET /stats/topics` (client API) the filter and the `limit` are applied **after** the merge between nodes, and `sort` does not exist (`400`). Caching the result for `stats.topics.cache-ttl` (with `collectedAt`) is planned: today the parameter has no effect.  
**Level:** C  
**Historical source:** 06b, 11

---

### UC-OPS-025

**Case:** In `/stats/topics`, a node that does not answer is left out and the result is marked `partial`, with `missingNodes`  
**Then:** the sums become a floor.  
**Level:** C  
**Historical source:** 11

---


### UC-OPS-030

**Case:** Replication degradation (`under-replicated`, `syncing`) is a property of the **cluster**, not of the topic  
**Then:** it appears in the cluster condition, not in the topic one.  
**Level:** C  
**Historical source:** 11

---

### UC-OPS-036

**Case:** The node does not start if `operations.port` is in use or if `operations.allowed-ips` has an invalid entry.  
**Level:** I  
**Historical source:** 11

---

### UC-OPS-037

**Case:** The `/cluster/*` routes are not operations routes  
**Then:** they stay on the cluster port and are not affected by `operations.*`.  
**Level:** I  
**Historical source:** 11, 06 §8.0

---

### UC-OPS-042

**Case:** Each node exposes only its own series, with no `node` label  
**Then:** the `instance` label is the `service.instance.id`. The `taba.cluster.*` metrics are the node's view of the cluster, and the other `taba.*` are its work; the cluster total comes from aggregation at the consumer.  
**Level:** C  
**Historical source:** 11

---


## CLR: Contract of the cluster routes (DTOs and common rules)

### UC-CLR-003

**Case:** The response of `register`, `commit` and `rollback` is `Ack` (`operationId`, `accepted`).  
**Level:** U  
**Historical source:** 06c, 06d §8.3.7

---

### UC-CLR-008

**Case:** A node in synchronization or in drain returns `503` `cluster.node-syncing` or `cluster.node-draining` on the routes it cannot serve. A node in synchronization confirms `register` by writing to the event database (UC-ESC-031).  
**Level:** C  
**Historical source:** 06c

---


## CFG: Configuration and startup validations

### UC-CFG-006

**Case:** All nodes run the **same image**; the role of each comes only from the configuration (`cluster.first-node`, `cluster.entry-address`, `replication.factor`).  
**Level:** I  
**Historical source:** 01, 06 §8.0.1

---


## Retired cases

Codes are never reused: UC-DEF-001, UC-DEF-002, UC-DEF-003, UC-DEF-004, UC-DEF-005, UC-DEF-006, UC-DEF-007, UC-DEF-008, UC-OPS-008, UC-OPS-009, UC-OPS-010, UC-DRN-021, UC-DRN-022, UC-DRN-023, UC-DRN-024, UC-DRN-025, UC-DRN-026, UC-DRN-027, UC-DEV-001, UC-DEV-003, UC-DEV-004, UC-DEV-005, UC-DEV-010, UC-DEV-013, UC-RTN-011, UC-SEG-005, UC-ANE-011, UC-DEF-023, UC-ENT-007, UC-INV-032, UC-ING-025, UC-FRZ-008, UC-CNC-008, UC-SIN-024, UC-FWD-012, UC-DEV-006, UC-DEV-007, UC-DEV-008, UC-DEV-009, UC-CFG-007, UC-TOP-019, UC-OPS-027 (the development mode and the drain as save went away, [07-cluster.md](07-cluster.md), section 8; UC-TOP-019, because the FIFO queue accepts any `replication.factor`, [09-topics.md](09-topics.md); UC-OPS-027, because the route `GET /cluster/metrics/topics` does not exist, [18-cluster-api.md](18-cluster-api.md)).

## Sources and evidence

- Catalogue: `doc/20a`, `20b`, `20c`. Tests: the `/// UC:` lines of the code (rule of `uc_coverage.rs`).
- Maintained by hand: a case that gets its first test moves to [26a](26a-implemented-use-cases.md).
