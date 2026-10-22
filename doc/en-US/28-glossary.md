---
title: Glossary
summary: The project's terms in alphabetical order, each with a short definition and the link to the document that explains it.
type: reference
status: stable
verified-on: 2026-10-14 @ efeb47a
sources: [crates/taba-node/src, doc/pt-BR]
---

# Glossary

> The terms are defined **once** in [03-concepts.md](03-concepts.md) (the mental model) and **in detail** in the document indicated. Identifiers in English (routes, keys, fields, error codes) appear in `code`.

## A

| Term | Meaning | Detail |
|---|---|---|
| **Ack** | Confirmation of consumption: `DELETE …/messages/{id}?receiptToken=`. Removes the message | [12](12-delivery.md) |
| **At-least-once** | Delivery guarantee: a message may arrive more than once, but is not lost for lack of an ack | [11](11-durability.md) |
| **`autoCreate`** | `GET` parameter that creates the queue (volatile by default) if it does not exist | [09](09-topics.md) |

## B

| Term | Meaning | Detail |
|---|---|---|
| **Backup (S3)** | The optional backup for a serious failure: message segments, state journals, definitions and heartbeats in an S3 or MinIO bucket. Never enters the publishing path | [33](33-s3-backup.md) |
| **Block** | The arc of the ring that ends at a virtual-node position; has a primary and replicas | [06](06-ring-and-replication.md) |
| **Bootstrap** | The process of a node joining the cluster | [07](07-cluster.md) |

## C

| Term | Meaning | Detail |
|---|---|---|
| **Channel (permanent)** | One WebSocket per pair of nodes, shared by all topics; carries heartbeat and notices, never messages | [07](07-cluster.md), [12](12-delivery.md) |
| **Client** | An application that uses the client API | [16](16-client-api.md) |
| **Cluster** | The set of nodes that know each other. Minimum of 3 in production | [07](07-cluster.md) |
| **Cluster certificate** | `.crt`/`.key` pair equal on all nodes, used in the mutual TLS of the cluster port | [14](14-security.md) |
| **Commit** | Phase 2 of the protocol: makes effective what the register left pending | [10](10-publishing.md) |
| **Condition** | Item of the list returned by `/health/cluster` or by the topic metrics (e.g. `under-replicated`, `backlog`) | [13](13-observability.md) |
| **Connector** | A Taba client library in a language (today Rust and C#) | [21](21-connectors.md) |
| **Coordinator** | The node that received the client's call (or that drives a membership change) | [10](10-publishing.md), [07](07-cluster.md) |
| **`contentType`** | Immutable metadata of the message: the `Content-Type` of the publication | [16](16-client-api.md) |

## D

| Term | Meaning | Detail |
|---|---|---|
| **Dead-letter** | The `{queue}-dead-letter` queue where a message goes when it reaches `maxDeliveries` without an ack | [09](09-topics.md), [12](12-delivery.md) |
| **`deduplicationId`** | Deduplication key of a publication on a FIFO queue (without it, the SHA-256 of the body) | [09](09-topics.md) |
| **Definition owner** | The node that decides and numbers the changes of a topic or token (the first active node at the position `topic:<name>`) | [08](08-storage.md) |
| **`deleteAt`** | Logical delete on the FIFO queue: the ack writes the date and deletes only the body, so deduplication holds for the window | [09](09-topics.md) |
| **Delivery counter** | `deliveryCount`: how many times the message was delivered | [12](12-delivery.md) |
| **Delivery owner** | The primary of a message's block: the only one that puts it in the ready list | [06](06-ring-and-replication.md), [12](12-delivery.md) |
| **Distributor (DT)** | A topic that does not store messages: copies each publication to the queues whose name starts with one of its prefixes | [09](09-topics.md) |
| **Drain** | Leaving the cluster without losing what is in memory: everything goes to disk and the node deregisters | [07](07-cluster.md), [17](17-operations-api.md) |
| **Drain mode** | See *Drain* | [07](07-cluster.md) |
| **Development mode** | `mode=development`: a one-node cluster, with no vote, quorum, channel or replicas | [07](07-cluster.md) |

## E

| Term | Meaning | Detail |
|---|---|---|
| **Event database** | `_sync/sync.db`: where a synchronizing node stores, in order of arrival, the events it cannot process yet | [07](07-cluster.md), [08](08-storage.md) |
| **Exclusion vote** | The healthy nodes vote to remove a node whose channel dropped | [07](07-cluster.md) |
| **`ErrorCode`** | A code of the error catalogue, with a fixed HTTP status | [19](19-errors-and-limits.md) |

## F

| Term | Meaning | Detail |
|---|---|---|
| **Fair delivery** | Preference of delivery for messages of smaller groups, by power-of-2 bands | [12](12-delivery.md) |
| **FIFO queue (QTF)** | A queue that delivers each group in order, one message at a time, and deduplicates | [09](09-topics.md) |
| **Freezing** | A period in which a node excluded for failure cannot rejoin | [07](07-cluster.md) |

## G

| Term | Meaning | Detail |
|---|---|---|
| **Group (`groupId`)** | Label of the message: in FIFO it defines the order; in the other queues, fair delivery | [09](09-topics.md), [12](12-delivery.md) |
| **GUID v7** | The format of all identifiers (message, topic, token, node): sortable by time | [15](15-api-overview.md) |

## H

| Term | Meaning | Detail |
|---|---|---|
| **Hash ring** | The circle of 64-bit positions that decides on which nodes each message lives (circular hash, *consistent hashing*) | [06](06-ring-and-replication.md) |
| **Heartbeat (backup)** | The `nodes/<id>.json` file that each node rewrites in the bucket, with an increasing `seq`; whoever observes sees the ETag change and considers the node dead when it stops for `backup.dead-after` | [33](33-s3-backup.md) |
| **Heartbeat (channel, *keepalive*)** | Periodic message of the channel (default every 2 s) with state, load and ready counts per topic | [12](12-delivery.md) |
| **Heir** | The first other active owner of a definition's position: receives the copy before the response to the client | [08](08-storage.md) |

## I

| Term | Meaning | Detail |
|---|---|---|
| **Invisibility** | Period (`invisibleUntil`) in which an already delivered message is not delivered to another consumer (default 120 s) | [12](12-delivery.md) |
| **Invisible list** | Per topic and on the owner, the invisible messages ordered by deadline | [12](12-delivery.md) |
| **`invisibleUntil`** | Absolute instant of the end of the invisibility, set by the owner's clock | [12](12-delivery.md) |

## L

| Term | Meaning | Detail |
|---|---|---|
| **Long polling** | The `GET` that waits (up to 20 s) for messages (`waitTime`) | [12](12-delivery.md) |

## M

| Term | Meaning | Detail |
|---|---|---|
| **`maxDeliveries`** | Limit of deliveries without an ack per queue (default 10); once reached, goes to the dead-letter | [12](12-delivery.md) |
| **Member table** | The list of nodes of the cluster, with a `version` | [07](07-cluster.md) |
| **Message** | Body (bytes) + metadata (`id`, `contentType`, `deliveryCount`, `invisibleUntil`, `receiptToken`…) | [03](03-concepts.md) |

## N

| Term | Meaning | Detail |
|---|---|---|
| **NDJSON** | One JSON object per line: the format of the synchronization streams between nodes | [18](18-cluster-api.md) |
| **Node** | An instance of the `taba` program; they are all equal | [04](04-architecture.md) |
| **Node database** | `_node.db`: local SQLite with the tables `topics`, `tokens`, `state`, `losses` and `definition_versions` | [08](08-storage.md) |
| **Managing node** | The primary of the position of a volatile queue's `id`: decides the deletion for idleness | [09](09-topics.md) |

## O

| Term | Meaning | Detail |
|---|---|---|
| **`operationId`** | GUID v7 of a two-phase operation; the three phases carry it and are idempotent by it | [18](18-cluster-api.md) |
| **Overflow** | Move of messages from memory to disk when `memory.limit` is reached | [08](08-storage.md) |

## P

| Term | Meaning | Detail |
|---|---|---|
| **Port (client / cluster / operations)** | The node's three ports | [04](04-architecture.md) |
| **Prefix** | Of the distributor: names of destination queues. Of the token: topics it reaches | [09](09-topics.md), [14](14-security.md) |
| **Primary / secondary / tertiary** | The nodes responsible for a block, in order along the ring; the primary is the delivery owner | [06](06-ring-and-replication.md) |

## Q

| Term | Meaning | Detail |
|---|---|---|
| **Queue (QT)** | A topic that stores messages and delivers each one to one consumer | [09](09-topics.md) |
| **Quorum (vote)** | 51% of the healthy nodes (`cluster.vote.quorum-percent`), with at least `cluster.vote.min-voters` | [07](07-cluster.md) |
| **Quorum (write)** | Confirmations to decide the commit (`write.quorum`, default majority; the primary always counts) | [10](10-publishing.md) |

## R

| Term | Meaning | Detail |
|---|---|---|
| **Rate limit** | Limit of requests per token or IP (*token bucket*, per node) | [14](14-security.md), [19](19-errors-and-limits.md) |
| **Ready list** | Per topic and on the owner, the next messages to deliver | [12](12-delivery.md) |
| **`receiptToken`** | Token generated at each delivery; required on ack and on changing the invisibility | [12](12-delivery.md) |
| **Register** | Phase 1 of the protocol: stores the operation as pending | [10](10-publishing.md) |
| **Replica** | Copy of a message on a node that is not the primary | [06](06-ring-and-replication.md) |
| **Replication factor** | How many nodes store each message (`replication.factor`, default 3) | [10](10-publishing.md) |
| **Rollback** | Discarding a pending register; only **before** the commit decision | [10](10-publishing.md) |

## S

| Term | Meaning | Detail |
|---|---|---|
| **Save (drain as)** | In development mode, drain writes everything to disk and the next start resumes | [07](07-cluster.md) |
| **Secrets folder** | Created by `taba init`: cluster certificates (`tls/`) and root token (`root-token`) | [14](14-security.md) |
| **Single writer** | Every write to a SQLite database goes through a single command queue | [04](04-architecture.md), [08](08-storage.md) |
| **Synchronization** | What a new node does when it joins: load of topics and blocks, replay of the event database, final check | [07](07-cluster.md) |

## T

| Term | Meaning | Detail |
|---|---|---|
| **`take`** | A request from one node to another (the owner) to deliver ready messages | [12](12-delivery.md) |
| **Tag** | Key/value pair of a topic, for telemetry only | [13](13-observability.md) |
| **Access token** | `tb_<id>.<secret>` credential with permissions and prefixes | [14](14-security.md) |
| **Root token** | Configuration secret with full access; the only one that manages tokens | [14](14-security.md) |
| **Topic** | A named channel: queue or distributor | [09](09-topics.md) |
| **Topic `id`** | GUID v7 generated at creation: identifies the "incarnation" (recreating the name generates another) and positions the manager | [09](09-topics.md) |
| **Topic database** | `<topic>/_topic.db`: local SQLite with the `messages` table (what is on disk) | [08](08-storage.md) |
| **`ttl`** | Message lifetime per queue (default 14 days); once expired, it is discarded (does not go to the dead-letter) | [12](12-delivery.md) |
| **Transport** | The *trait* that carries the calls between nodes (real HTTP and the test in-memory network) | [04](04-architecture.md) |

## U

| Term | Meaning | Detail |
|---|---|---|
| **UC (use case)** | `UC-<AREA>-<NNN>` code of an observable behavior, cited in the tests | [26](26-use-cases.md) |

## V

| Term | Meaning | Detail |
|---|---|---|
| **Base version** | The oldest version of a language that a connector supports | [21](21-connectors.md) |
| **Virtual node** | Additional position that a node occupies on the ring (`ring.virtual-nodes`, default 128) | [06](06-ring-and-replication.md) |
| **Volatile queue (QTV)** | A queue deleted by itself if it stays without a consumer for `idleTimeout` | [09](09-topics.md) |

## W

| Term | Meaning | Detail |
|---|---|---|
| **`waitTime`** | Seconds of long polling (0 to 20) | [16](16-client-api.md) |

## Acronyms

| Acronym | Means |
|---|---|
| **QT** / **DT** / **QTV** / **QTF** | Queue Topic / Distributor Topic / Queue Topic Volatile / Queue Topic FIFO |
| **mTLS** | Mutual TLS (both sides present a certificate) |
| **OTLP** | OpenTelemetry Protocol |
| **UCUM** | Unified Code for Units of Measure (the metric units) |
| **ADR** | *Architecture Decision Record* ([30-decisions.md](30-decisions.md)) |

## Sources and evidence

- `09-glossario.md` (the base), checked against the modules of `crates/taba-node/src` and the other documents of this folder. The definition **corrected** relative to the history: the topic definition lives in the `topics` table of `_node.db` (not in `${topic}.json`), and *freezing* is not a node state.
- Use cases: UC-ANE, UC-ESC, UC-ENT, UC-INV; see [26a](26a-implemented-use-cases.md).
