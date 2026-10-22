---
title: Cluster API
summary: The routes the nodes use among themselves — members and channel, definitions, messages, synchronization, recovery, observability and forwarding — with the two-phase protocol, the NDJSON streams and the DTOs.
type: reference
status: stable
verified-on: 2026-10-15 @ 56121c9
sources: [crates/taba-node/src/cluster/protocol/backup.rs, crates/taba-node/src/cluster/protocol/http.rs, crates/taba-node/src/cluster/protocol, crates/taba-node/src/http/cluster_api, crates/taba-node/src/cluster/service, crates/taba-node/src/cluster/channel]
---

# Cluster API

> ⚠️ **Internal.** No client application should call these routes, and there is **no stability guarantee** between versions: all nodes of a cluster must run the same version. Common conventions: [15-api-overview.md](15-api-overview.md). How the cluster uses these routes: [07-cluster.md](07-cluster.md), [10-publishing.md](10-publishing.md), [12-delivery.md](12-delivery.md).

**Port:** `cluster.port` (8081), **only** the other nodes. **Authentication:** mutual TLS ([14-security.md](14-security.md)); a connection without an accepted certificate fails **at the handshake**. In **development mode the port is not opened**: a production node cannot join a development node.

The protocol exists in two forms that are **the same thing**: the `Request` type (`cluster/protocol`) and the HTTP route. The real `Transport` translates them to HTTP; the in-memory network of the tests delivers the type directly.

## Common rules

**Who can call.** Whoever is in the node's **member table**. Whoever is not: `403 cluster.not-member`. The caller is recognized by the connection's **source IP** (they all use the same certificate), which must be an `address` of some member. A node that does not yet have the table answers `503 cluster.node-syncing`. **Accept a caller that is not yet a member:** `GET /cluster/nodes`, and the loss ones (`message-loss`, `messages/recover`), because whoever returns from an exclusion is no longer in the table.

**Two-phase protocol.** The `register`, `commit` and `rollback` routes:

- Each operation has an **`operationId`** (GUID v7 of the coordinator). The three phases carry the same one, and **all are idempotent by it**: repeating returns the same `Ack`, with no new effect.
- `rollback` only discards a **pending register, without commit**. A `commit` failure on a replica outside the quorum **never** triggers `rollback`: the coordinator insists until `write.commit.timeout`.
- **Messages** decide the commit by **quorum** (primary included); **nodes** require **all**. **Topic and token** do not use these phases: they have an **owner** on the ring (`definitions` routes).
- A register with no commit or rollback **expires** at `write.pending.ttl`. `commit` or `rollback` of an unknown operation: `404 cluster.operation-unknown`.
- A **synchronizing** node confirms the `register` by **writing it to the event database** and applies it later; for the coordinator, the confirmation counts normally.

**Common statuses:**

| Status | Code | When |
|---|---|---|
| `200` | — | Accepted or query served |
| `400` | `validation.failed`, `required`, `unknown-field` | Invalid body |
| `403` | `cluster.not-member` | Caller is not a member |
| `404` | `cluster.operation-unknown`, `topic.not-found`, `message.not-found` | Unknown |
| `409` | `cluster.conflict` | Another pending operation on the same resource, or the state prevents it (message already invisible, different `receiptToken`) |
| `421` | `fifo.misdirected` | Only on forwarded FIFO publication: the node is no longer the group's primary |
| `421` | `cluster.wrong-node` | Only on the channel: the node at this address is not the one the `to` looks for |
| `503` | `cluster.node-syncing`, `cluster.node-draining` | The node cannot serve now |

## Route map

Extracted from `cluster/protocol/http.rs`, the translator between `Request` and HTTP.

| Group | Routes |
|---|---|
| **Members and channel** | `GET /cluster/nodes`; `POST /cluster/nodes/join`, `register`, `unregister`, `commit`, `rollback`; `POST /cluster/vote-remove`; `GET /cluster/channel` (WebSocket) |
| **Definitions** | `POST /cluster/definitions/propose`, `apply`; `GET /cluster/topics`, `GET /cluster/tokens`, `GET /cluster/definitions/tombstones`; `GET /cluster/topics/{id}/idle` |
| **Messages** | `POST /cluster/topics/{topic}/messages/{id}/register`, `commit`, `rollback`; `…/{id}/visibility/register`, `commit`, `rollback`; `…/{id}/ack/register`, `commit`, `rollback`; `POST /cluster/topics/{topic}/messages/take`; `POST /cluster/topics/{topic}/fifo/publish`; `POST /cluster/topics/{topic}/prune` |
| **Synchronization and recovery** | `GET /cluster/blocks/{id}/ids`, `GET /cluster/blocks/{id}/excluded`, `GET /cluster/blocks/{id}/messages`, `GET /cluster/blocks/{id}/messages/{messageId}`; `GET`/`POST /cluster/message-loss`; `DELETE /cluster/message-loss/{topic}/{id}`; `POST /cluster/messages/recover`; `POST /cluster/topics/{topic}/messages/restore`; `POST /cluster/topics/{topic}/messages/dead` |
| **Observability** | `GET /cluster/status`; `GET /cluster/stats/topics`; `GET /cluster/stats/topics/{topic}/groups` |
| **S3 backup** | `POST /cluster/backup/flush`, `POST /cluster/backup/stop`, `POST /cluster/backup/saved`; `GET /cluster/backup/status`, `GET /cluster/departed` |
| **Forwarding** | `* /cluster/forward/{path}`; `* /cluster/node-call/{path}` |

> The original design documents also list `GET /cluster/metrics/topics`; **that route does not exist in the code** (the aggregation of topic metrics across nodes was not implemented: [13-observability.md](13-observability.md)).

---

## Members and channel

| Route | Does |
|---|---|
| `GET /cluster/nodes` | The **first call** of a node when starting, through the load balancer (`cluster.entry-address`). Returns the `MemberTable`. Any node answers it, even to someone who is not a member. **The only one that goes through a load balancer** |
| `POST /cluster/nodes/join` | The new node asks **one member** to join; it tests the IPs (connects back), coordinates the register + commit with **all** and only then answers with the `MemberTable`. **May take a while**: waits its turn if there is another change or vote |
| `POST /cluster/nodes/register` / `unregister` | **Phase 1** of the join and of the **deregistration** (`reason`: `vote` or `drain`) |
| `POST /cluster/nodes/commit` | **Phase 2** (makes the register **or** the deregistration effective) |
| `POST /cluster/nodes/rollback` | Aborts the pending phase 1 |
| `POST /cluster/vote-remove` | **Exclusion vote**: body `VoteRequest` (`voteId`, `candidateNodeId`, `openedBy`, `reason`: `channel-down`, `queue-overflow` or `node-failed`); response `VoteResponse` (`vote`: `yes`/`no`) |

`join`: body `JoinRequest` (`nodeId`, `addresses`, `port`, `version`). Errors: `403 cluster.node-frozen` (with `Retry-After`), `422 cluster.join-unreachable` (the member could not connect back: the node is excluded and repeats the bootstrap), `503 cluster.registration-failed` (phase 1 not confirmed by all; rollback done), `503 cluster.commit-timeout` (register confirmed and commit in the background: the node **was accepted**; query `GET /cluster/nodes` before repeating).

### The channel: `GET /cluster/channel` (WebSocket)

Opens the **permanent channel** between two nodes (`101 Switching Protocols`), **one per pair**, used in both directions, for all topics. The query carries `from` (who opens) and `to` (who must serve): a node with **another ID** at the same address answers `421 cluster.wrong-node`. **Only signals, never message content.**

Each text message is a JSON of one of two kinds:

| Kind | Fields | What it is |
|---|---|---|
| **batch** | `from`, `to`, `keepalive?`, `frames` | What a node sends: the heartbeat (when it is time) and the unconfirmed frames |
| **`confirm`** | `seqs` | The response to **each** batch, after **processing** the frames (empty if it only carried the heartbeat) |

**Heartbeat (`keepalive`):** `at`, `load` (forwarded calls in progress), `version` (of the member table), `state` (of the node), `monitoring` (**not sent yet**) and `ready` (list of `{topic, count}`, up to `cluster.channel.keepalive.max-topics`). It carries no `seq` and is not resent.

**Frames (`frames`)**, each with `seq` (increasing per channel and per direction) and `type`:

| `type` | Fields | Means |
|---|---|---|
| `listeners` | `topic`, `hasListeners` | The topic gained or lost long polling listeners on this node |
| `ready` | `topic`, `hasReady` | The node started (or stopped) having ready messages of the topic |
| `notify` | `topic`, `count` | New-message notice (a hint): come fetch with `take` |
| `node-state` | `state` | `draining`, `active` or `failed`. Whoever receives `active` marks the member as `active` in its own table, without raising the `version` |
| `frozen` | `nodeId`, `frozenAt` | A node excluded by vote is in freezing. It does not change the table or the `version` |
| `membership` | `operationId`, `version`, `change` | A membership change **already in effect** (`add`, `remove` by drain, `exclude` by vote), with the version the table now has. Whoever missed the register or the commit applies it |
| `definition` | `kind`, `key`, `version`, `deleted`, `body` | The state of a definition (topic or token), with the same rule as `apply` |

The connection is **considered down** when it closes or when nothing arrives from the peer for `2 × cluster.channel.keepalive.interval`. On opening, each side sends a **snapshot** (`listeners`, `ready`, `frozen`) and only then the changes.

---

## Definitions (topics and tokens)

Each definition has an **owner** on the ring (the first active node at the position of `topic:<name>` or `token:<id>`) and an **heir** (the first other active owner that answers).

| Route | Does |
|---|---|
| `POST /cluster/definitions/propose` | From the node that received the client **to the owner**. Body `DefinitionProposal`: `kind` (`topic`/`token`), `operation` (`create`/`update`/`delete`), `key`, `body`, `expectedId?` (used by the volatile queue manager to only delete the expected `id`) |
| `POST /cluster/definitions/apply` | From the owner to each node and to the heir. `DefinitionApply`: `kind`, `key`, `version`, `deleted`, `body`, `relay?` |
| `GET /cluster/topics`, `GET /cluster/tokens` | **Streams** of the definitions, ordered by `id`, for the bootstrap |
| `GET /cluster/definitions/tombstones` | **Stream** of the tombstones (removed definitions, the version of the removal and, for a topic, its `topicId`), so a late creation does not resurrect what was removed |
| `GET /cluster/topics/{id}/idle` | How long ago the topic's last `GET` was **on this node** (`IdleResponse`: `topicId`, `idleSeconds`, `sinceStart`). Used by the volatile queue manager |

- The owner checks (name already exists, unknown topic, `expectedId`), assigns the **next version**, writes to the **heir**, applies to itself, sends `apply` to the others and answers after those that answer within `write.commit.timeout`. The others receive it through the `definition` frame of the channel.
- **Version:** the node only applies a version **greater than or equal to** the one it has. On the **same version with different content**, the copy with the **lowest fingerprint** (SHA-256 of the normalized body) wins, on any node and in any order (D124).
- **Token:** the `TokenRecord` carries the **salt** and the **salted hash**, **never the secret**.
- Errors: `404` (`topic.not-found`, `token.not-found`), `409` (`topic.already-exists`, `cluster.conflict`), `503 cluster.quorum-unavailable` (owner down and in the table, or no other owner confirmed).

## Messages

All carry the `topic` and the `id` in the path and the `operationId` in the body. `register` goes to **all** the responsible nodes; `commit` is decided by quorum and sent to the others in the background.

| Route | `register` (body) |
|---|---|
| `…/messages/{id}/register` | `MessageRegisterRequest`: `operationId`, `coordinatorId`, `kind` (`publish` or `dead-letter`), `message` (`MessageRecord`), `sourceTopic` (`dead-letter` only). The **same pair of routes** does the move to the dead-letter (creates there and removes from the source, with the same `id`) |
| `…/messages/{id}/visibility/register` | `VisibilityRegisterRequest`: `kind` (`delivery`/`change`), `invisibleUntil` (**absolute**), `receiptToken`, `deliveryCount` (`delivery` only). The coordinator is always the **owner** |
| `…/messages/{id}/ack/register` | `AckRegisterRequest`: `receiptToken` (must match) and `deleteAt` (FIFO only) |

`commit` and `rollback` of each one receive `OperationRef` (`operationId`) and answer `Ack`. Errors: `404 message.not-found`, `409 cluster.conflict` (a pending register already exists, or the message is already invisible), `409 message.stale-receipt`.

| Route | Does |
|---|---|
| `POST /cluster/topics/{topic}/messages/take` | Asks the **owner** to deliver up to `maxMessages` ready messages (`TakeRequest`: `maxMessages`, `visibilityTimeout`). It does the invisibility register + commit and returns `TakeResponse` (`messages`: list of `MessageRecord`). **Empty** if the hint was stale, or if the node is in **drain** |
| `POST /cluster/topics/{topic}/prune` | Empties the queue **on this node** (body `{ "pruneId" }`, a GUID v7 that makes the call idempotent: the same id answers the same count and removes nothing new; the node remembers the last 64). Answers `{ "removed" }`: how many messages it **owned** and removed (the replicas are removed too, but not counted). The fan-out of `DELETE /topics/{topic}/messages` ([16-client-api.md](16-client-api.md)). Errors: `404 topic.not-found`, `409 topic.wrong-type` |
| `POST /cluster/topics/{topic}/fifo/publish` | Forwarding of the FIFO publication to the **group's primary** (body `MessageRecord`, with marker `id` `00000000-0000-7000-8000-000000000000`; whoever generates the `id` is the primary). `421 fifo.misdirected` if the node is not the primary. Also used by the copies of a distributor to a FIFO queue |

## Synchronization and recovery

All the streams follow the [NDJSON](#ndjson-streams) contract.

| Route | Does |
|---|---|
| `GET /cluster/blocks/{id}/ids` | IDs of messages the node has for the block, with the topic, ordered by ID (`BlockIdLine`: `id`, `topic`, and in FIFO `deleteAt`). **First step** of synchronization |
| `GET /cluster/blocks/{id}/excluded?from=&to=` | The node's **log of excluded messages** for the block ([37-cluster-cold-start.md](37-cluster-cold-start.md)): one line per `id` that left the topic dead (`id`, `topic`, `position`, `atMs`), ordered by ID, with **inclusive** IDs. Whoever synchronizes the block copies the log together with the messages |
| `GET /cluster/blocks/{id}/messages?from=&to=` | The block's messages in the range (IDs **inclusive**): `MessageRecord`. It is the call **balanced** across sources. In FIFO, a logically deleted message comes **without a body** and with `deleteAt` |
| `GET /cluster/blocks/{id}/messages/{messageId}` | **One** message; last resort for one that was missing in a range |
| `GET /cluster/message-loss` | The node's `losses` table (`LossLine`: `id`, `topic`); accepts a non-member |
| `POST /cluster/message-loss` | Records a loss (`LossRecord`: `topic`, `id`, `block`, `detectedAt`); response `LossAck` (`changed`) |
| `DELETE /cluster/message-loss/{topic}/{id}` | **Clears** a loss |
| `POST /cluster/messages/recover[?replicate=false][&durable=true]` | A node that **returns** sends the responsible one the messages it found in the loss list. **The body is an NDJSON stream** (`Content-Type: application/x-ndjson`) of `MessageRecord` with the final line; response `RecoverResult` (`end`, `count` applied). With `replicate=false` it only writes to the local disk; with `durable=true` whoever writes puts the message **on disk**, not only in memory (the restoration of the previous lives uses it between the owners). `415` with another `Content-Type` |
| `POST /cluster/topics/{topic}/messages/restore[?topicId=]` | The node that keeps the **folder of a previous life** sends the **new primary** of the block its messages, with the original ID ([37-cluster-cold-start.md](37-cluster-cold-start.md), section 4.6). NDJSON body like `recover`'s; `topicId` is the topic's when the folder was created. The primary checks the topic (deleted, or recreated with another `topicId`, discards), its own log of excluded messages, the other present nodes' and the S3 journal, and activates what is alive through the path of `recover` (disk and quorum). Response `RecoverResult`: `count` is what ended up **resolved** (activated, already there or discarded). Members only |
| `POST /cluster/topics/{topic}/messages/dead` | Body `{"ids":[...]}`; answers `{"dead":[...]}` with the IDs that are in this node's **log of excluded messages**. It is the batch query the restoring primary makes to the present nodes. Members only |

A malformed block `{id}`: `400 validation.failed`.

## S3 backup

They only exist with the backup configured ([33-s3-backup.md](33-s3-backup.md)); without it they answer `422 backup.not-configured`. They accept **members** only. They are the calls behind `POST /cluster/backup/flush` and `POST /cluster/stop` of the operations API ([17-operations-api.md](17-operations-api.md)).

| Route | Does |
|---|---|
| `POST /cluster/backup/flush` | The backup flush **on this** node (the three steps); answers the `FlushReport` in JSON |
| `POST /cluster/backup/stop` | The drain of **this** node for the cluster stop, with the upload to S3; answers when the node has finished: `{ "nodeId", "drained", "outboxPending" }` |
| `POST /cluster/backup/saved` | The primary tells the replica, in a batch, which messages of the topic are already in the bucket: `{ "topic", "items": [{ "id", "segment", "offset", "len" }] }` (up to 5000 items). The replica stores the reference of what it has and ignores the rest; answers `Ack` |

## Observability and forwarding

| Route | Does |
|---|---|
| `GET /cluster/status` | The local `NodeStatus` ([17-operations-api.md](17-operations-api.md)), for `GET /health/cluster?scope=cluster` to aggregate |
| `GET /cluster/stats/topics` | What **this node** sees of every topic: the numbers (counts, rates, depth, oldest message, latency percentiles, memory and disk, groups followed) and the node's memory in use, not the presentation. The client API's `/stats/topics` asks every member and joins them ([16-client-api.md](16-client-api.md)). Answers `{ "nodeId", "memoryTotalBytes", "topics": [{ "name", "kind": "queue" \| "distributor", … }] }` |
| `GET /cluster/stats/topics/{topic}/groups` | The groups of a queue as this node sees them (`groupId`, `overflow`, `visible`, `invisible`, `band`, `oldestMs`). `404 topic.not-found`, `409 topic.wrong-type` |
| `GET /cluster/backup/status` | The S3 backup state of **this** node (the same body as `GET /node/backup`). `422 backup.not-configured` |
| `GET /cluster/departed` | The nodes that left by drain, as **this** node remembers them: a list of `{ nodeId, addresses, port }` (in memory only, up to 64). `GET /nodes` unites the lists of every member, so the answer does not depend on which node received the call |
| `* /cluster/node-call/{path}` | A call of the **operations API** (`/metrics`, `/metrics/topics…`, `/node/drain`, `/node/backup`, `/node/backup/flush`) that the receiving node runs against **itself**, for a client call addressed to it (`/nodes/{nodeId}/…`, [16-client-api.md](16-client-api.md)). Accepts any verb; the response is the one the route gave, status and body as they came. The token was already checked by whoever sent it, and the call goes only to the operations routes, never to the client ones. Works even if the node is in drain (that is how a drained node is returned). Accepts only from members |
| `* /cluster/forward/{path}` | **Forwarding** of a client call from a synchronizing node, in drain or in failure. Accepts **any verb**; method, query, body and headers go as they came and the response comes back as the destination gave it. The already resolved client IP goes in the `x-taba-client-ip` header; the destination **checks the token again** and **does not count the rate limit**. Deadline of 60 s (fits a long polling). **Whoever receives never forwards again**. Accepts only from members and from nodes that left by drain |

## NDJSON streams

The routes that move volume (`/cluster/topics`, `/cluster/tokens`, `/cluster/definitions/tombstones`, `/cluster/blocks/{id}/ids`, `/cluster/blocks/{id}/excluded`, `/cluster/blocks/{id}/messages`, `/cluster/message-loss` and the body of `/cluster/messages/recover`) **do not use the client pagination**: whoever synchronizes needs to know it received **everything**.

| Rule | Effect |
|---|---|
| **Format** | `application/x-ndjson`, **one JSON per line**; one buffer per line, without building the whole response. Maximum line size is limited |
| **Order** | **Ascending ID** (GUID v7 as lowercase text) |
| **Range** | `from` and `to` (inclusive, optional); optional `limit` lowers the ceiling |
| **Mandatory final line** | `{"end":true,"count":N,"last":"<id>","hasMore":false}`. **Without it, the chunk is considered failed** (a cut between lines cannot be told apart from a normal end) |
| **Ceiling per stream** | The sender closes at `cluster.sync.stream.max-items` (default 500) or `max-bytes` (default 4 MiB), with `hasMore: true`; the receiver opens another with `from=<last>` |
| **Resume** | After a cut or `hasMore`, repeats with `from=<last ID read>`; the repeated line is ignored (idempotent by ID). No state on the server |
| **Error after the `200`** | The sender writes `{"error":"..."}` and closes, or just closes: the receiver resumes by the rule above. It is not `ErrorResponse` |
| **Backpressure** | The receiver reads at its own pace; TCP holds back the sender |

The data line: `TopicDefinition` (topics), `TokenRecord` (tokens), `{id, kind, key, version}` (tombstones), `{id, topic}` (IDs and losses) and `MessageRecord` (messages).

## Main DTOs

**`Member`:** `nodeId`, `addresses` (internal IPs), `port` (cluster), `state` (`active`, `syncing`, `draining`, `suspect`). **`MemberTable`:** `version` (epoch; different versions = `membership-divergent`), `members` and, only if there are any, `frozen` (list of `{nodeId, frozenAt}`; does not count toward equality or raise the `version`).

**`MessageRecord`** (the message as the nodes exchange it): `id`, `topic`, `body` (native JSON if `application/json`/`*/*+json`; otherwise **base64**), `contentType`, `bodyEncoding` (`base64`), `groupId`, `deduplicationId`, `deliveryCount`, `invisibleUntil`, `receiptToken`, `deadLetteredAt`, `deadLetterReason`, `deleteAt`. There is **no** "text string" form of the client API, so the stream line limit is predictable.

**`Ack`:** `operationId`, `accepted`. **`StreamEnd`:** `end`, `count`, `last`, `hasMore`. **`OperationRef`:** `operationId`.

## Sources and evidence

- `cluster/protocol/http.rs` (the route table is from this source), `cluster/protocol/{definitions,messages,recovery,signals,vote}.rs` (DTOs), `http/cluster_api`, `cluster/service` (serves the calls), `cluster/channel`, `cluster/members`, `cluster/replication`, `cluster/sync`, `cluster/recovery`.
- Use cases: UC-CLR (route contract), UC-STR (streams), UC-CAN (channel), UC-ING, UC-FAL, UC-SIN, UC-RTN, UC-FWD; see [26a](26a-implemented-use-cases.md). Tests: `tests/cluster_api.rs`, `sync.rs`, `channel.rs`, `notify.rs`, `mtls.rs`.
- History: `06c-api-cluster.md`.
