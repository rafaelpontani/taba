---
title: Errors and limits
summary: The complete catalogue of error codes (each with its fixed HTTP status), the fixed validation limits, the configurable limits and the rate limits.
type: reference
status: stable
verified-on: 2026-10-13 @ 71183b9
sources: [crates/taba-node/src/core/error/mod.rs, crates/taba-node/src/core/validation/mod.rs, crates/taba-node/src/http/ratelimit, crates/taba-node/src/config/params/mod.rs]
---

# Errors and limits

> Error format and retry policy: [15-api-overview.md](15-api-overview.md). The configurable values: [22-configuration.md](22-configuration.md).

## Catalogue rule

Every API error is an **`ErrorCode`** of the catalogue (`core/error`), with a **fixed HTTP status**: the same code always comes with the same status. The catalogue has **50 codes**. A new code requires its line in this document **and** its case in the test table (rule of [24-conventions.md](24-conventions.md)). The codes are stable; the `message` is **not** (do not use it in logic).

## Validation — `400`

| Code | When |
|---|---|
| `validation.failed` | Generic error; the problems come in `details` |
| `validation.required` | Required field or parameter missing |
| `validation.unknown-field` | Field that the route does not define |
| `validation.out-of-range` | Number out of range (`ttl`, `maxDeliveries`, `idleTimeout`, `waitTime`, `maxMessages`, `limit`, `visibilityTimeout`) |
| `validation.invalid-name` | Topic (or token) name outside the pattern |
| `validation.reserved-name` | Name ending in `-dead-letter`, on creation and on `autoCreate` |
| `validation.immutable-field` | `PATCH` tries to change `name`, `id`, `type`, `volatile`, `fifo` (or, on a token, permissions and prefixes) |
| `validation.not-applicable` | Field that does not apply to the type (`prefixes` on a queue, `ttl` on a distributor, `deduplicationId` outside FIFO, `fifo` on a distributor) |
| `validation.invalid-body` | The body does not match the `Content-Type` (invalid JSON, text that is not UTF-8) |
| `validation.invalid-content-type` | The publication's `Content-Type` is not a valid media type |
| `validation.group-id-required` | Publishing to a FIFO queue without `groupId` |
| `validation.invalid-cursor` | Malformed `cursor`, or used with a `sort`/filters different from those that generated it |
| `validation.max-bytes-above-hard-limit` | `maxMessageBytes` above `message.hard-limit` |

## Authentication and authorization — `401`, `403`

| Code | Status | When |
|---|---|---|
| `auth.unauthorized` | 401 | No token, malformed, unknown, revoked or expired. **The response does not say which** |
| `auth.forbidden` | 403 | Valid token without the route's permission (includes `/tokens`, root only) |
| `auth.out-of-scope` | 403 | The topic (or a distributor `prefix`) is outside the token's prefixes |
| `ops.ip-not-allowed` | 403 | Operations route called from an IP outside `operations.allowed-ips` |
| `cluster.not-member` | 403 | Cluster route called by someone who is not in the member table |
| `cluster.node-frozen` | 403 | Join request from a node in freezing (carries `Retry-After`) |

## Not found — `404`

| Code | When |
|---|---|
| `topic.not-found` | The topic does not exist (and there is no `autoCreate`) |
| `message.not-found` | The message does not exist: already confirmed, expired by `ttl` or removed with the topic. **A second ack** gives this error |
| `token.not-found` | The token does not exist or was already revoked |
| `route.not-found` | Path that does not exist on that port (`/cluster/*` on the client port; `/tokens` with authentication off) |
| `cluster.operation-unknown` | Between nodes: `commit`/`rollback` of an operation the node does not know (expired) |

## Conflict — `409`

| Code | When |
|---|---|
| `topic.already-exists` | A topic with the name already exists |
| `topic.wrong-type` | `GET`/ack on a distributor; or `autoCreate` on a name that belongs to a distributor |
| `topic.read-only` | `PATCH`/`DELETE` on a dead-letter queue |
| `message.stale-receipt` | The `receiptToken` is not the one of the last delivery (the message was redelivered) |
| `drain.conflict` | `DELETE /node/drain` during deregistration, or with the process already stopping by signal |
| `cluster.conflict` | Between nodes: another pending operation on the same resource, or the message is already invisible |

## Limits and configuration — `413`, `415`, `421`, `422`, `429`

| Code | Status | When |
|---|---|---|
| `message.too-large` | 413 | Body above the topic's `maxMessageBytes` (or `message.max-bytes`) |
| `media.unsupported` | 415 | `Content-Type` ≠ `application/json` on a route with a JSON body (does not apply to publishing) |
| `fifo.misdirected` | 421 | **Between nodes:** the node is not the group's primary |
| `cluster.wrong-node` | 421 | **Between nodes, on the channel:** another process at the same address |
| `topic.fifo-needs-replication` | 422 | Creating a FIFO queue with `replication.factor` < 2 |
| `distributor.too-many-targets` | 422 | The publication would match more queues than `topics.distributor.max-targets` (no copy is made) |
| `topic.volatile-limit` | 422 | `autoCreate` hits `topics.volatile.max-count` |
| `cluster.join-unreachable` | 422 | Between nodes: the join coordinator could not connect back to the new node |
| `backup.not-configured` | 422 | `POST /node/backup/flush` or `POST /cluster/backup/flush` on a node without `backup.s3.bucket` |
| `rate-limit.exceeded` | 429 | Rate limit exceeded (carries `Retry-After`) |

## Availability and failures — `5xx`

| Code | Status | When |
|---|---|---|
| `internal.error` | 500 | Unexpected failure (a *panic* is converted to this error) |
| `forward.bad-gateway` | 502 | The forwarding failed after the body was sent |
| `forward.timeout` | 504 | The forwarding timed out after the body was sent |
| `cluster.quorum-unavailable` | 503 | Not enough nodes to close the write, delivery or ack quorum |
| `cluster.registration-failed` | 503 | A register (topic, token, node) was not confirmed by all and was canceled. **Nothing was applied** |
| `cluster.commit-timeout` | 503 | The register was confirmed, but the commit did not reach all within `write.commit.timeout`. **The operation continues in the background and will complete** |
| `cluster.node-syncing` | 503 | The node is synchronizing and found no other to forward to |
| `cluster.node-draining` | 503 | The node is in drain and found no other to forward to (in development, always) |
| `node.failed` | 503 | The node is in failure (disk full or corrupted database) and found no other to forward to |
| `fifo.primary-unavailable` | 503 | The FIFO group's primary is down, synchronizing, or exhausted `fifo.forward.max-attempts` |
| `delivery.unavailable` | 503 | The `GET` captured no message and there was a failure in the invisibility register (never returns empty in that case) |

The `503`s carry `Retry-After` (today 5 s in drain, in synchronization and on lack of quorum). The `502`/`504` were **not** repeated by the node: the client decides.

## Fixed limits (validation)

Format rules, **not configurable**, applied on creation and on change (`core/validation`):

| Item | Limit |
|---|---|
| Topic name | `^[A-Za-z0-9_-]{1,100}$`, case-sensitive. The dead-letter may have up to **112** (with `-dead-letter`) |
| Tags | up to **20** per topic; key 1 to 64, value 0 to 128, `A-Za-z0-9_-.:` |
| `prefixes` (distributor) | up to **20**, 1 to 100 characters, `A-Za-z0-9_-`; empty is refused |
| `idleTimeout` | 60 to 604,800 s |
| `ttl` | 60 to 31,536,000 s |
| `maxDeliveries` | 1 to 1000 |
| `groupId`, `deduplicationId` | 1 to 128, `A-Za-z0-9_-.:` |
| Token: `name` | 1 to 100, `A-Za-z0-9_-.:` |
| Token: `permissions` | at least one of `publish`, `consume`, `create`, `admin` |
| Token: `topicPrefixes` | up to 20, 1 to 100, `A-Za-z0-9_-` |
| Token: `expiresAt` | in the future |
| `contentType` | media type of 1 to 255 characters; default `application/json` |
| `waitTime` (long polling) | 0 to 20 s |
| `X-Request-Id` | 1 to 128, `A-Za-z0-9_-.:` |
| Pagination `limit` | 1 to `api.pagination.max-limit` (1000) |

## Configurable limits, with no ceiling by default

| Key | Effect |
|---|---|
| `message.max-bytes` (**5 MB**) | Maximum message size for every topic. Each topic can have its own `maxMessageBytes`, **even larger**, up to `message.hard-limit` (no default: no ceiling). Larger than the topic's limit: `413`. `PATCH` only applies to following publications. On a distributor the general one applies and the copies are not checked again |
| `topics.distributor.max-targets` | Destinations per distributor publication; above it, `422` and **no** copy |
| `topics.volatile.max-count` | Volatile queues in the cluster; once reached, `autoCreate` gives `422` |
| `delivery.max-messages.max` | Ceiling of `maxMessages` |
| `delivery.visibility-timeout.max` | Ceiling of `visibilityTimeout` |

## Rate limits

**Off by default** (without `ratelimit.*.rps`, the category has no limit). Token bucket, **per node**, per category and per key; details in [14-security.md](14-security.md).

| Category | Configuration key | Routes |
|---|---|---|
| Publishing | `ratelimit.publish.rps` | `POST /topics/{topic}/messages` |
| Consumption | `ratelimit.consume.rps` | `GET`, `PUT .../visibility`, `DELETE` of messages |
| Administrative | `ratelimit.admin.rps` | `/topics*`, `/tokens*` |
| Operations | `ratelimit.ops.rps` | `/health/cluster?scope=cluster`, `/metrics/topics*` |
| Burst | `ratelimit.burst` | Applies to all; absent = `ceil(rps)` |

Exempt: `/health/live`, `/health/ready`, `/metrics`, `/health/cluster?scope=node`, and the whole cluster port. Exceeded: `429 rate-limit.exceeded` with `Retry-After`.

## Sources and evidence

- `core/error/mod.rs` (the macro with all the `ErrorCode`s and statuses; **the catalogue above was checked against it**), `core/validation/mod.rs` (the ranges), `http/ratelimit`, `config/params/mod.rs`.
- Use cases: UC-CNV (conventions: errors and statuses), UC-LIM (rate limit), UC-CFG; see [26a](26a-implemented-use-cases.md). The test that checks each code and its status is in `core/error/tests.rs`.
- History: `06d-api-convencoes.md` (8.3.6), `12-limites-api.md`.
