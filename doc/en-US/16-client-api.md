---
title: Client API
summary: Route-by-route manual of the API that applications use — topics, messages (publish, consume, invisibility, ack) and tokens — with parameters, DTOs, statuses and examples.
type: reference
status: stable
verified-on: 2026-10-13 @ 71183b9
sources: [crates/taba-node/src/http/topics, crates/taba-node/src/http/messages, crates/taba-node/src/http/tokens, crates/taba-node/src/http/auth, crates/taba-node/src/core/topic, crates/taba-node/src/core/validation, crates/taba-node/src/core/content]
---

# Client API

> Common conventions (format, headers, error, pagination, status): [15-api-overview.md](15-api-overview.md). Catalogue of errors and limits: [19-errors-and-limits.md](19-errors-and-limits.md). The behavior behind each route is in [09](09-topics.md), [10](10-publishing.md) and [12](12-delivery.md).

**Port:** `api.port` (8080). **Authentication:** optional token ([14-security.md](14-security.md)). The "Permission" column only applies with authentication on.

| Verb | Route | Permission | Rate limit | Success |
|---|---|---|---|---|
| `POST` | `/topics` | `create` | admin | `201` `Topic` |
| `GET` | `/topics` | valid token | admin | `200` `Page<Topic>` |
| `GET` | `/topics/{topic}` | valid token | admin | `200` `Topic` |
| `PATCH` | `/topics/{topic}` | `admin` | admin | `200` `Topic` |
| `DELETE` | `/topics/{topic}` | `admin` | admin | `204` |
| `POST` | `/topics/{topic}/messages` | `publish` | publishing | `201` / `200` |
| `GET` | `/topics/{topic}/messages` | `consume` | consumption | `200` |
| `PUT` | `/topics/{topic}/messages/{id}/visibility` | `consume` | consumption | `200` |
| `DELETE` | `/topics/{topic}/messages/{id}` | `consume` | consumption | `204` |
| `POST` `GET` `PATCH` `DELETE` | `/tokens`, `/tokens/{id}` | root only | admin | see below |

The examples use `localhost:8080` without authentication (development mode). With authentication, add `-H "Authorization: Bearer $TOKEN"`.

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
| `422` | `topic.fifo-needs-replication` | FIFO with factor < 2 |
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
| `422` | `topic.volatile-limit`, `topic.fifo-needs-replication` | `autoCreate` limits |
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

---

## Tokens

They only exist with **authentication on** and only accept the **root token**. Without authentication or in development mode: `404 route.not-found`. Model and permissions in [14-security.md](14-security.md).

| Route | Does | Success |
|---|---|---|
| `POST /tokens` | Creates. Body `{name, permissions, topicPrefixes?, expiresAt?}`. The response carries the secret in **`token`** (`tb_<id>.<secret>`), **only here** | `201`, `Location: /tokens/{id}` |
| `GET /tokens` | Lists, paginated. `sort`: `id` or `createdAt` (not `name`); `name` filters by fragment. **Never returns the secret** | `200` |
| `GET /tokens/{id}` | Reads | `200` / `404 token.not-found` |
| `PATCH /tokens/{id}` | Only `name` and `expiresAt` (`null` removes the deadline). Permissions and prefixes are immutable | `200` |
| `DELETE /tokens/{id}` | Revokes (takes effect on all nodes at the commit) | `204` |

`Token`: `id`, `name`, `permissions`, `topicPrefixes`, `createdAt`, `expiresAt` (`TokenCreated` = `Token` + `token`).

Errors: `400` (`validation.*`: unknown permission, empty list, invalid prefix, `expiresAt` in the past), `401`/`403` (root only), `404`, `503` (register not confirmed: try again).

## Sources and evidence

- `http/topics`, `http/messages`, `http/tokens`, `http/auth`, `core/topic` (definition), `core/validation` (ranges), `core/content` (body form), `engine/topics`, `engine/messages`.
- All the examples above were run against a real node; the responses are copies.
- Use cases: UC-TOP, UC-MSG (publishing and body), UC-CNS (consumption), UC-ACK, UC-VIS, UC-AUT (tokens), UC-PAG; see [26a](26a-implemented-use-cases.md). Tests: `tests/topics.rs`, `messages.rs`, `auth.rs`, `definitions.rs`.
- History: `06a-api-cliente.md`.
