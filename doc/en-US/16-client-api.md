---
title: Client API
summary: Route-by-route manual of the API that applications use — topics, messages (publish, consume, invisibility, ack, prune), tokens, who the token is, the nodes and the statistics, and the cluster and node operations for the root token — with parameters, DTOs, statuses and examples.
type: reference
status: stable
verified-on: 2026-10-15 @ 56121c9
sources: [crates/taba-node/src/http/topics, crates/taba-node/src/http/messages, crates/taba-node/src/http/tokens, crates/taba-node/src/http/auth, crates/taba-node/src/http/nodes, crates/taba-node/src/http/topic_stats, crates/taba-node/src/core/topic, crates/taba-node/src/core/validation, crates/taba-node/src/core/content]
---

# Client API

> Common conventions (format, headers, error, pagination, status): [15-api-overview.md](15-api-overview.md). Catalogue of errors and limits: [19-errors-and-limits.md](19-errors-and-limits.md). The behavior behind each route is in [09](09-topics.md), [10](10-publishing.md) and [12](12-delivery.md).

**Port:** `api.port` (8080). **Authentication:** optional token ([14-security.md](14-security.md)). The "Permission" column only applies with authentication on.

| Verb | Route | Permission | Rate limit | Success |
|---|---|---|---|---|
| `POST` | `/topics` | `create` | admin | `201` `Topic` |
| `GET` | `/topics` | valid token | admin | `200` `Page<Topic>` |
| `GET` | `/topics/{topic}` | valid token | admin | `200` `Topic` |
| `PATCH` | `/topics/{topic}` | `manage` | admin | `200` `Topic` |
| `DELETE` | `/topics/{topic}` | `manage` | admin | `204` |
| `POST` | `/topics/{topic}/messages` | `publish` | publishing | `201` / `200` |
| `GET` | `/topics/{topic}/messages` | `consume` | consumption | `200` |
| `PUT` | `/topics/{topic}/messages/{id}/visibility` | `consume` | consumption | `200` |
| `DELETE` | `/topics/{topic}/messages/{id}` | `consume` | consumption | `204` |
| `DELETE` | `/topics/{topic}/messages` | `manage` | admin | `200` `PruneResult` |
| `POST` `GET` `PATCH` `DELETE` | `/tokens`, `/tokens/{id}` | `admin` (an `admin` token: root only) | admin | see below |
| `GET` | `/auth/me` | valid token | exempt | `200` `Me` |
| `GET` | `/nodes` | valid token | ops | `200` `Page<Node>` |
| `GET` | `/stats/topics`, `/stats/topics/{topic}`, `/stats/topics/{topic}/groups` | valid token, **within its prefixes** | ops | `200` |
| `GET` | `/cluster/status` | `admin` | ops | `200` `ClusterStatus` |
| `GET` | `/nodes/{nodeId}/metrics`, `/nodes/{nodeId}/backup` | `admin` | ops | `200` |
| `GET` | `/cluster/backup` | `admin` | ops | `200` `ClusterBackup` |
| `GET` `POST` `DELETE` | `/nodes/{nodeId}/drain` | `admin` | admin | `200` / `202` `DrainStatus` |
| `POST` | `/nodes/{nodeId}/backup/flush`, `/cluster/backup/flush` | `admin` | admin | `200` |
| `POST` | `/cluster/stop` | root only | admin | `200` |

The examples use `localhost:8080` without authentication (a node without a root token or `auth.dev-token`). With authentication, add `-H "Authorization: Bearer $TOKEN"`.

---

## Topics

### Rules for name, tags and ranges

- **Name:** `^[A-Za-z0-9_-]{1,100}$`, case-sensitive. A name ending in `-dead-letter` is **reserved** (`400 validation.reserved-name`); an invalid name, `400 validation.invalid-name`.
- **Tags:** up to **20** per topic; key of 1 to 64 and value of 0 to 128 characters `A-Za-z0-9_-.:`. For telemetry only; they do not affect behavior.

| Field | Range |
|---|---|
| `idleTimeout` | 60 to 604,800 s (7 days) |
| `ttl` | 60 to 31,536,000 s (365 days) |
| `maxDeliveries` | 1 to 1000 |
| `maxMessageBytes` | 1 up to `message.hard-limit` |
| `prefixes` | up to 20 prefixes of 1 to 100 characters (`A-Za-z0-9_-`); empty is refused |

### `POST /topics`

Creates a topic. Generates the `id`, propagates the definition to **all nodes** (definition owner, [08-storage.md](08-storage.md)) and **only answers afterwards**. Every **queue** gets its `{name}-dead-letter` with it.

**Body `TopicCreateRequest`**

| Field | Type | Req. | Default | Description |
|---|---|---|---|---|
| `name` | text | yes | — | Name |
| `type` | text | yes | — | `queue` or `distributor` |
| `tags` | map text→text | no | `{}` | Telemetry tags |
| `prefixes` | list | `distributor` only | `[]` | Destination prefixes. Empty = the distributor copies to no one |
| `volatile` | boolean | `queue` only | `false` | Volatile queue |
| `idleTimeout` | integer (s) | volatile only | `600` | Time without a consumer until deletion |
| `fifo` | boolean | `queue` only | `false` | FIFO queue. Immutable. Requires `replication.factor` ≥ 2 |
| `maxMessageBytes` | integer | `queue` only | `null` (`message.max-bytes`, 5 MB) | Maximum per message; can exceed the general one, up to `message.hard-limit` |
| `ttl` | integer (s) | `queue` only | `null` (14 days) | Message lifetime |
| `maxDeliveries` | integer | `queue` only | `null` (10) | Deliveries without ack until the dead-letter |

A field that does not apply to the type (`prefixes` on a queue, `fifo` on a distributor): `400 validation.not-applicable`.

**Response: `201`** with `Location: /topics/{name}` and the `Topic`:

```bash
curl -s -X POST localhost:8080/topics -H 'Content-Type: application/json' \
  -d '{"name":"pedidos","type":"queue"}'
```
```json
{"createdAt":"2026-10-13T17:00:48.527Z","effective":{"maxDeliveries":10,"maxMessageBytes":5242880,"ttl":1209600},
 "fifo":false,"id":"01a13635-a80f-7985-9ffa-8d0be5eda84f","maxDeliveries":null,"maxMessageBytes":null,
 "name":"pedidos","tags":{},"ttl":null,"type":"queue","volatile":false}
```

| Status | Codes | When |
|---|---|---|
| `400` | `validation.failed`, `invalid-name`, `reserved-name`, `not-applicable`, `out-of-range`, `max-bytes-above-hard-limit`, `unknown-field` | Invalid body |
| `401`/`403` | `auth.unauthorized`, `auth.forbidden`, `auth.out-of-scope` | Without `create`, or name/`prefixes` outside the token's prefixes |
| `409` | `topic.already-exists` | Name already exists |
| `429` | `rate-limit.exceeded` | — |
| `503` | `cluster.registration-failed`, `cluster.commit-timeout`, `cluster.node-syncing` | Some node did not confirm (nothing was created), or the commit completes in the background |

### `GET /topics`

**Paginated** list, answered **locally** (the definition is replicated on every node). With authentication, only the topics within the token's prefixes. **Includes the dead-letter queues.**

| Parameter | Default | Description |
|---|---|---|
| `limit`, `cursor` | 100 / — | Pagination |
| `sort` | `name` | `name`, `id` or `createdAt`; `-` for descending |
| `name` | — | Topics whose name **contains** the text (case-sensitive) |

```bash
curl -s "localhost:8080/topics?name=pedidos&sort=-createdAt&limit=2"
# {"items":[{"id":"…","name":"pedidos","type":"queue",…},
#           {"id":"…","name":"pedidos-dead-letter","deadLetter":true,"source":"pedidos",…}],"nextCursor":"eyJr…"}
```

Errors: `400` (`out-of-range`, `invalid-cursor`, `failed`), `401`, `429`.

### `GET /topics/{topic}`

Returns the `Topic`. On the dead-letter, the values are those of the source queue, read-only, with `deadLetter: true` and `source`. Errors: `401`/`403 auth.out-of-scope`, `404 topic.not-found`, `429`.

### `PATCH /topics/{topic}`

**Partial update**: only what was sent changes. `tags` **replaces the whole map**. `null` in `maxMessageBytes`, `ttl` or `maxDeliveries` **goes back to the node default**. It also applies to already stored messages (the new `ttl` applies at the next sweep); `maxMessageBytes` only to following publications.

| Field | Applies to |
|---|---|
| `tags` | all |
| `prefixes` | `distributor` (applies from the commit, only to later publications) |
| `idleTimeout` | volatile queue (applies at the manager's next check) |
| `maxMessageBytes`, `ttl`, `maxDeliveries` | queue |

**Immutable:** `name`, `id`, `type`, `volatile`, `fifo`: sending them, **even with the current value**, is `400 validation.immutable-field`. Errors: `404`, `409 topic.read-only` (the dead-letter follows the source queue), `503`.

### `DELETE /topics/{topic}`

Removes the topic and **all its messages, even the pending ones**: it **always works**, with no check. It also removes the dead-letter. A failure at the register cancels (rollback); a failure at the commit, already decided, does not cancel: the coordinator insists. There is never a partial removal visible. `204`. On `503 cluster.commit-timeout` the removal **will complete** (a later `GET` will give `404`). `409 topic.read-only` on the dead-letter.

---

## Messages

### The message body

The body can have **any format** (JSON, text, binary). The body of the publish request is the **bytes**; the `Content-Type` says what they are. Taba stores the **bytes as they came** and the `Content-Type` as the immutable `contentType` metadata.

- **Without `Content-Type`**, `application/json` applies. Invalid value: `400 validation.invalid-content-type` (media type of 1 to 255 characters; parameters such as `charset` are stored).
- **Validation on publishing:** `application/json` must be valid JSON and `text/*` must be UTF-8, otherwise `400 validation.invalid-body`. For the other types there is no validation.
- **How the `body` appears on delivery** (only there; on disk the raw bytes stay):

| `contentType` | `body` in the response | `bodyEncoding` |
|---|---|---|
| `application/json` or `*/*+json` | The **native JSON**, exactly as sent | absent |
| `text/*` in UTF-8 (or without `charset`) | A **string** with the text | absent |
| Any other (includes `text/*` with another `charset`) | The bytes in **base64** | `base64` |

- **Size:** what counts are the **bytes sent** (not the base64), limited by the topic's `maxMessageBytes` (or `message.max-bytes`, 5 MB); above that, `413 message.too-large`. Base64 in the response takes about 33% more.

### `POST /topics/{topic}/messages`: publish

The node that receives is the **coordinator**: it generates the `id`, does the register + commit and answers when the **quorum** confirms ([10-publishing.md](10-publishing.md)). In **FIFO**, it forwards to the group's primary.

| Query | Req. | Description |
|---|---|---|
| `groupId` | FIFO only | Group (1 to 128, `A-Za-z0-9_-.:`). Outside FIFO, guides fair delivery |
| `deduplicationId` | no, **FIFO only** | Deduplication key in the group; without it, the SHA-256 of the body. Outside FIFO: `400 validation.not-applicable` |

**Response on a queue — `PublishResponse`:**

| Field | Description |
|---|---|
| `id` | The message `id` (on a duplicate, that of the **original**) |
| `duplicate` | **FIFO only.** `true` if it repeated a publication within the deduplication window (nothing was created) |

Status: **`201`** created, **`200`** duplicate.

```bash
curl -s -i -X POST localhost:8080/topics/pedidos/messages -H 'Content-Type: application/json' -d '{"pedido":9001}'
# HTTP/1.1 201 Created
# {"id":"01a13635-a913-71b2-9b55-85517752ddbc"}

curl -s -X POST "localhost:8080/topics/ordens/messages?groupId=g1&deduplicationId=d1" -d '{"n":1}'
# {"duplicate":false,"id":"01a1363a-5434-…"}   (the repetition: {"duplicate":true,"id":"01a1363a-5434-…"} with 200)
```

**Response on a distributor — `200 DistributorPublishResponse`:** `matched` (queues that matched; zero is not an error) and `deliveries`, one item per queue with `topic` and **one** of `id` (copy created) or `error` (`{code, message}`, no status):

```json
{"deliveries":[{"id":"01a1363a-4e96-…","topic":"pedidos-eu"},
               {"error":{"code":"cluster.quorum-unavailable","message":"…"},"topic":"pedidos-us"}],"matched":2}
```

| Status | Codes | When |
|---|---|---|
| `400` | `validation.group-id-required`, `invalid-body`, `invalid-content-type`, `not-applicable`, `failed` | `groupId` missing in FIFO, body that does not match the `Content-Type`, `deduplicationId` outside FIFO |
| `401`/`403` | `auth.*` | Requires `publish`. On a distributor only it is checked, not the destination queues |
| `404` | `topic.not-found` | — |
| `413` | `message.too-large` | Above the topic's limit |
| `422` | `distributor.too-many-targets` | More destinations than `topics.distributor.max-targets`: refused **entirely** |
| `429` | `rate-limit.exceeded` | — |
| `502`/`504` | `forward.bad-gateway`, `forward.timeout` | Forwarding failed **after** the body was sent. **Outside FIFO it is not repeated** (it would duplicate); in FIFO it is safe to repeat |
| `503` | `cluster.quorum-unavailable`, `fifo.primary-unavailable`, `cluster.node-syncing`, `cluster.node-draining` | The message was **not accepted** |

### `GET /topics/{topic}/messages`: consume

Delivers up to `maxMessages` **ready** messages, each **invisible** for `visibilityTimeout`. Returns what is ready, **without waiting to fill** `maxMessages` (with `waitTime > 0`, it waits only for the first). **Any** `GET` counts as a consumer for the idleness of a volatile queue. No order guarantee (except in FIFO, per group). Details in [12-delivery.md](12-delivery.md).

| Query | Default | Description |
|---|---|---|
| `maxMessages` | `10` | 1 to `delivery.max-messages.max` |
| `visibilityTimeout` | `120` | 0 to `delivery.visibility-timeout.max` (s) |
| `waitTime` | `0` | **Long polling**: 0 to **20** s |
| `autoCreate` | `false` | Creates the **queue** if it does not exist; the parameters below only apply at creation (if it exists, they are ignored) |
| `volatile` | `true` | Only with `autoCreate` |
| `idleTimeout` | `600` | Only with `autoCreate` |
| `fifo` | `false` | Only with `autoCreate` |
| `tag.<key>` | — | Only with `autoCreate` |

**Response: `200 ConsumeResponse`** `{ "messages": [ Message… ] }`. **Empty list** when nothing is ready: always `200`, **never `204`**.

`Message`:

| Field | Description |
|---|---|
| `id` | GUID v7 |
| `body` | According to the `contentType` (table above) |
| `bodyEncoding` | `base64`, if applicable |
| `contentType` | The one from publishing |
| `groupId`, `deduplicationId` | If it has them |
| `receiptToken` | **Keep it:** it is required on ack and on changing the invisibility |
| `deliveryCount` | Deliveries so far (this one included) |
| `publishedAt` | Instant of publication |
| `invisibleUntil` | End of the invisibility of this delivery |
| `deadLetteredAt`, `deadLetterReason` | Only on the dead-letter |

```bash
curl -s "localhost:8080/topics/pedidos/messages?waitTime=1"
# {"messages":[{"id":"01a13635-a913-…","body":{"pedido":9001},"contentType":"application/json",
#   "receiptToken":"01a13635-aa19-…","deliveryCount":1,"publishedAt":"2026-10-13T17:00:48.787Z",
#   "invisibleUntil":"2026-10-13T17:02:49.049Z"}]}
```

Example with text and binary (verified):

```json
{"messages":[
 {"id":"…","body":"Pedido 9001 aprovado","contentType":"text/plain; charset=utf-8","receiptToken":"…",…},
 {"id":"…","body":"iVBORw0KGgo=","contentType":"image/png","bodyEncoding":"base64","receiptToken":"…",…}]}
```

| Status | Codes | When |
|---|---|---|
| `400` | `validation.out-of-range`, `reserved-name`, `invalid-name` | Parameter out of range; `autoCreate` with a name ending in `-dead-letter` |
| `401`/`403` | `auth.*` | Requires `consume`, and `create` if `autoCreate` is going to create |
| `404` | `topic.not-found` | Does not exist and no `autoCreate` |
| `409` | `topic.wrong-type` | The topic is a distributor |
| `422` | `topic.volatile-limit` | `autoCreate` limit |
| `429` | `rate-limit.exceeded` | — |
| `503` | `delivery.unavailable`, `cluster.quorum-unavailable`, `cluster.node-syncing` | **No** message captured and there was a register failure: never answers empty in that case. If at least one was captured, `200` with the ones it got |

### `PUT /topics/{topic}/messages/{id}/visibility`

Sets the invisibility deadline of a message **already delivered** (e.g. more processing time). `PUT` because the value **replaces** and repeating gives the same result. **Without** incrementing `deliveryCount`. The value is the new time **from now**, not an increment. **`0` releases the message immediately.**

| Query | Req. | Description |
|---|---|---|
| `receiptToken` | yes | The token of the **last delivery** |

Body `VisibilityRequest`: `{ "visibilityTimeout": <s> }` (0 to `delivery.visibility-timeout.max`). **`200 VisibilityResponse`:** `{ "id", "invisibleUntil" }` (with `0`, the current instant).

```bash
curl -s -X PUT "localhost:8080/topics/dl/messages/<id>/visibility?receiptToken=<token>" \
  -H 'Content-Type: application/json' -d '{"visibilityTimeout":30}'
# {"id":"01a1363d-c5cb-…","invisibleUntil":"2026-10-13T17:10:13.601Z"}
```

Errors: `400` (`required`, `out-of-range`), `404` (`topic.not-found`, `message.not-found`: already confirmed, expired by `ttl` or the topic is gone), `409 message.stale-receipt`, `429`, `503`.

### `DELETE /topics/{topic}/messages/{id}`: confirm (ack)

`?receiptToken=<token>` (required). **`204`**. Removes the message with a decision by **quorum**; a failure before the quorum **cancels** the ack and the message remains available. If the deadline expired but the message was **not yet redelivered**, the token is still valid and the ack counts.

Errors: `400 validation.required` (without `receiptToken`), `404` (nonexistent, **including a second ack** or a message expired by `ttl`), `409` (`message.stale-receipt`: it was redelivered to another; `topic.wrong-type`: distributor), `429`, `503 cluster.quorum-unavailable` (the ack was canceled).

### `DELETE /topics/{topic}/messages`: prune (empty the queue)

Removes **every message** of the queue — visible and invisible, and the copies the replicas keep — in the **whole cluster**. Permission `manage` on the topic. The topic, its definition and its counters stay. Queues only (a distributor is `409 topic.wrong-type`).

The node that receives the call asks **every member** (itself included) to empty its own copy, all at once, under one `pruneId` (the id makes the call idempotent on each node), and waits for all of them. **It answers success only if all succeeded.** It is a rare act, so there is no retry or rollback.

```bash
curl -s -X DELETE localhost:8080/topics/orders/messages
```

**`PruneResult`**: `pruneId`, `removed` (messages removed, each counted **once**: the node that owns a message counts it, the replicas do not) and `nodes` (`nodeId` and `removed` of each).

If any node fails: `503 cluster.prune-incomplete`, naming the nodes in the message and in `details`. The queue may then be empty on some nodes only; **repeating the call is safe**. A message published while the prune runs may survive it, and a node that is still synchronizing is not covered ([31-pending-items.md](31-pending-items.md)).

Errors: `403` (`auth.forbidden` with `required: "manage"`, `auth.out-of-scope`), `404 topic.not-found`, `409 topic.wrong-type`, `429`, `503`.

---

## Tokens

They only exist with **authentication on** and only accept the **root token**. Without authentication: `404 route.not-found`. Model and permissions in [14-security.md](14-security.md).

| Route | Does | Success |
|---|---|---|
| `POST /tokens` | Creates. Body `{name, permissions, topicPrefixes?, expiresAt?}`. The response carries the secret in **`token`** (`tb_<id>.<secret>`), **only here** | `201`, `Location: /tokens/{id}` |
| `GET /tokens` | Lists, paginated. `sort`: `id` or `createdAt` (not `name`); `name` filters by fragment. **Never returns the secret** | `200` |
| `GET /tokens/{id}` | Reads | `200` / `404 token.not-found` |
| `PATCH /tokens/{id}` | Only `name` and `expiresAt` (`null` removes the deadline). Permissions and prefixes are immutable | `200` |
| `DELETE /tokens/{id}` | Revokes (takes effect on all nodes at the commit) | `204` |

`Token`: `id`, `name`, `permissions`, `topicPrefixes`, `createdAt`, `expiresAt` (`TokenCreated` = `Token` + `token`).

Errors: `400` (`validation.*`: unknown permission, empty list, invalid prefix, `expiresAt` in the past), `401`/`403` (root only), `404`, `503` (register not confirmed: try again).

## Who the token is, the nodes and the statistics

Routes for **any valid token** (with authentication off, everyone). They show what a plain token may see within its scope, without being an administrator.

| Route | Answers |
|---|---|
| `GET /auth/me` | `Me`: `authEnabled`, `root`, `id` (`null` for the root token), `name`, `permissions`, `topicPrefixes`, `expiresAt`. With authentication off the caller is `open`, with every permission. A client uses it to enable or disable what it shows **before** calling |
| `GET /nodes` | `collectedAt`, `membersVersion`, `partial`, `missingNodes` and `items`: one `Node` per member — `nodeId`, `self` (it is the node that answered), `reachable`, `state` (`active`, `syncing`, `drained`...), `drainState`, `version`, `memberState`. **No addresses.** A node that left by drain shows `state: "drained"`, **in the answer of any node**: each node only remembers the departures it saw, and one that returns from a drain starts with an empty memory, so the list unites what every member knows. A drained node that came back has a new id and is no longer listed under the old one |
| `GET /stats/topics`, `/stats/topics/{topic}`, `/stats/topics/{topic}/groups` | The same DTOs and parameters as `/metrics/topics*` of the operations API ([17-operations-api.md](17-operations-api.md)), but **of the whole cluster**, with `partial` and `missingNodes`. The list only has the topics **within the token's prefixes** (as `GET /topics` does); `/stats/topics/{topic}` outside them is `403 auth.out-of-scope`. `?scope=node` gives the view of the node that answers alone |

### Probes and metrics on this port

| Route | Token | Answers |
|---|---|---|
| `GET /health/live`, `GET /health/ready` | **none** | The same `HealthResponse` as on the operations port ([17-operations-api.md](17-operations-api.md)). They need no token because the probe of an orchestrator cannot send one |
| `GET /metrics` | **any valid token** (no particular permission; `401` without one) | The Prometheus text of **the node that answers** (`Accept: application/openmetrics-text` asks for OpenMetrics), as `GET /nodes/{nodeId}/metrics` gives for a chosen node. `404 route.not-found` with `telemetry.prometheus.enabled` off |

> **Metrics and authentication, by port.** `/metrics` has no authentication on the **operations port** (default 9090), which is the door for the collectors **inside** the infrastructure: Prometheus and Telegraf scrape `:9090/metrics` with no credentials. On the **client port** (default 8080, or the value of `api.port`) the same route asks for a token, because that port is reachable from outside. Point a collector that lives outside to the client port and give it a token (`Authorization: Bearer …`); point the ones inside to the operations port. A node in drain only validates the root token, so a plain token is refused on this route while the node drains (PRB-069); the operations port is not affected.

### Consolidated reads (`partial` and `missingNodes`)

`GET /nodes`, `GET /cluster/status`, `GET /cluster/backup` and `GET /stats/topics…` answer for the **whole cluster**. Each node says what **it** sees, on the cluster API ([18-cluster-api.md](18-cluster-api.md)); the node that receives the call asks every member at once (itself without the network) and joins the answers. Each call has a deadline of 5 s (fixed). A node that does not answer in time does **not** fail the read: the response is `200` and says it is incomplete.

```json
{ "partial": true, "missingNodes": [{ "nodeId": "01a1…", "reason": "timeout" }] }
```

`reason` is `timeout`, `unreachable` (no answer at all) or `error` (the node answered with an error). The numbers are then those of the nodes that **did** answer, and a client should show them with a warning and keep asking. A **write** that fails on some node is different: it is a `503` naming the nodes (the prune).

**How the topic statistics join** (a message is counted by the node that owns it, so the counts add up):

| Field | Join |
|---|---|
| counts and rates (`published`, `consumed`, `acked`, `visibleMessages`, `invisibleMessages`, `depth`, `bytes`, memory and disk...) | sum |
| `oldestMessageAt` | the oldest |
| `publishLatency`, `consumeLatency` (p50, p95, p99) | the **maximum** of the nodes': percentiles do not add up, and the maximum errs on the cautious side |
| `groups`, `noisyShare` | `groups` and the largest group are the maximum of the nodes' (a group can have messages on several nodes, so adding would count it twice): a **lower bound** |
| `conditions`, `worstCondition` | evaluated again on the joined numbers |
| `memoryShare` | the topic's memory over the memory of the nodes that answered |
| `/stats/topics/{topic}/groups` | each group once, with its messages from every node |

## Cluster and nodes (`admin` permission)

The operations API ([17-operations-api.md](17-operations-api.md)) has no token and is for continuous monitoring. These routes are its copies in the client API, for a client that has one URL and one token, and they need the **`admin` permission** (or the root token): the `manage` permission is per topic, and these routes show the topology and the inside of the nodes. A token without it gets `403 auth.forbidden` with `"required": "admin"` in the body, so that the client can say what is missing. Only `POST /cluster/stop` asks for the root token itself (`"required": "root"`).

**The node rule** (the same as for a message). The node that receives `/nodes/{nodeId}/…` compares the `nodeId` with its own. If it **is** that node, it answers from its own data; if it is **not**, it calls the node that has the data **once**, on the cluster port (`/cluster/node-call/{path}`, members only, [18-cluster-api.md](18-cluster-api.md)), and returns the answer as it came. It never forwards a call that was already forwarded. An unknown `nodeId` is `404 node.not-found`; a node that does not answer is `502 forward.bad-gateway` (or `504 forward.timeout`) naming it, with no retry. A node that left by drain is still addressable, to return it.

| Route | Does |
|---|---|
| `GET /cluster/status` | What `GET /health/cluster` answers: the conditions and the `NodeStatus` of every node (which now carries `runtime`: `activeRequests`, `rateLimitRejected`, `channelRttP95Seconds`), with `partial` and `missingNodes`. Accepts `scope=node`. `summary` also says whether the cluster is stopped: `servingNodes` (nodes active and out of drain), `stopped` (every node that answered is in drain), and, when `stopped`, `plannedStop` and `stoppedAt` (the S3 stop list of `POST /cluster/stop`). A node in drain answers this route (and `/nodes`) itself when no other node can take the call. |
| `GET /cluster/backup` | The S3 backup state of **every** node, each as `GET /nodes/{nodeId}/backup` gives it, with `partial` and `missingNodes` (`422 backup.not-configured` without it) |
| `GET /nodes/{nodeId}/metrics` | The node's Prometheus text (what `GET /metrics` gives) |
| `GET`, `POST`, `DELETE /nodes/{nodeId}/drain` | Status, start and return from the drain of that node: what `/node/drain` does |
| `GET /nodes/{nodeId}/backup`, `POST /nodes/{nodeId}/backup/flush` | The S3 backup of that node (`422 backup.not-configured` without it) |
| `POST /cluster/backup/flush`, `POST /cluster/stop` | The whole cluster: the same as the operations API's, here behind a token (`POST /cluster/restore/force` and `POST /cluster/stop` ask for the root token) |

Starting or killing **processes** is not offered: it is the orchestrator's job (`drain` stops the node from receiving, and the return brings it back as a new node).

## Sources and evidence

- `http/nodes`, `http/topic_stats` (`client_routes`), `http/messages/prune.rs`, `http/topics`, `http/messages`, `http/tokens`, `http/auth`, `core/topic` (definition), `core/validation` (ranges), `core/content` (body form), `engine/topics`, `engine/messages`.
- All the examples above were run against a real node; the responses are copies.
- Use cases: UC-TOP (prune: 044 to 048), UC-MSG (publishing and body), UC-CNS (consumption), UC-ACK, UC-VIS, UC-AUT (tokens; 037 to 039: `required`, `auth/me`, who sees what), UC-OPS (046 to 051: the nodes and the consolidated reads), UC-PAG; see [26a](26a-implemented-use-cases.md). Tests: `tests/topics.rs`, `messages.rs`, `auth.rs`, `definitions.rs`, `nodes_api.rs`.
- History: `06a-api-cliente.md`.
