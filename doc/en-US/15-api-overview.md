---
title: The three APIs — overview and conventions
summary: The three HTTP APIs of the node (client, operations, cluster), who uses each one, and the common conventions — format, headers, error, pagination, status and retry.
type: reference
status: stable
verified-on: 2026-10-13 @ 71183b9
sources: [crates/taba-node/src/http/mod.rs, crates/taba-node/src/http/common, crates/taba-node/src/core/error, crates/taba-node/src/core/pagination, crates/taba-node/src/core/time, crates/taba-node/src/core/id]
---

# The three APIs: overview and conventions

> Read first: [04-architecture.md](04-architecture.md) (the three ports). Then: [16-client-api.md](16-client-api.md), [17-operations-api.md](17-operations-api.md), [18-cluster-api.md](18-cluster-api.md) and the error catalogue in [19-errors-and-limits.md](19-errors-and-limits.md).

> **State of the contract:** there is **no OpenAPI contract yet** nor a version prefix in the paths (`/topics`, not `/v1/topics`). This document and the next three are today the specification. `taba-cli version` shows a "contract version" (1). ⬜ OpenAPI contract: [32-roadmap.md](32-roadmap.md).

## Who uses what

| API | Port (default) | Who uses it | Authentication | Document |
|---|---|---|---|---|
| **Client** | `api.port` (8080) | Applications (producers and consumers) | Optional token | [16](16-client-api.md) |
| **Operations** | `operations.port` (or the client port) | Operator, Prometheus, orchestrator | None (network) | [17](17-operations-api.md) |
| **Cluster** | `cluster.port` (8081) | **Only the other nodes** | Mutual TLS | [18](18-cluster-api.md) |

Each API has **its own audience** and the network restricts each port. The client port answers `404 route.not-found` to `/cluster/*`; the cluster port, to anything that is not `/cluster/*`.

![The node's three APIs and their routes](../diagrams/img/en-US/node-ports.svg)

## Route summary

**Client** ([16](16-client-api.md)):

| Verb | Route | Token permission | Success |
|---|---|---|---|
| `POST` | `/topics` | `create` | `201` |
| `GET` | `/topics` | any valid token | `200` (paginated) |
| `GET` | `/topics/{topic}` | any valid token | `200` |
| `PATCH` | `/topics/{topic}` | `manage` | `200` |
| `DELETE` | `/topics/{topic}` | `manage` | `204` |
| `POST` | `/topics/{topic}/messages` | `publish` | `201` (`200` on FIFO duplicate and on distributor) |
| `GET` | `/topics/{topic}/messages` | `consume` (+ `create` on `autoCreate`) | `200` |
| `PUT` | `/topics/{topic}/messages/{id}/visibility` | `consume` | `200` |
| `DELETE` | `/topics/{topic}/messages/{id}` | `consume` | `204` |
| `POST`, `GET`, `PATCH`, `DELETE` | `/tokens`, `/tokens/{id}` | root token only | `201`, `200`, `200`, `204` |

**Operations** ([17](17-operations-api.md)): `GET /health/live`, `GET /health/ready`, `GET /health/cluster`, `GET /metrics`, `GET /metrics/topics`, `GET /metrics/topics/{topic}`, `GET /metrics/topics/{topic}/groups`, `POST`/`GET`/`DELETE /node/drain`.

**Cluster** ([18](18-cluster-api.md)): `/cluster/*`, in six groups.

## Data format

| Item | Rule |
|---|---|
| Format | JSON in UTF-8 (`Content-Type: application/json`), except: `/metrics` (Prometheus text), the cluster streams (`application/x-ndjson`) and the message publication **body** (any media type) |
| Field names | `camelCase`. Enumerations in lowercase, with a hyphen when compound (`queue`, `max-deliveries`) |
| Dates | RFC 3339 in UTC with milliseconds: `2026-10-13T17:00:48.527Z`. Date fields end in `At` (`createdAt`) or are an absolute deadline with its own name (`invisibleUntil`, `expiresAt`) |
| Durations | **Integer of seconds**, no unit in the value (`ttl`, `visibilityTimeout`, `idleTimeout`, `waitTime`) |
| Sizes | Integers, in bytes |
| Identifiers | Lowercase **GUID v7**, canonical hyphenated form. Applies to topic, message, token and node. Sortable by time |
| Topic name | `^[A-Za-z0-9_-]{1,100}$`, **case-sensitive** |
| Absent × `null` | **Absent** = does not apply to the resource (e.g. `prefixes` on a queue). **`null`** = applies and has no value of its own (e.g. `ttl: null` = "uses the node default") |
| Unknown field in the request | `400 validation.unknown-field` (a typo does not pass silently). **In the response, the client must ignore fields it does not know** |
| Required field missing | `400 validation.required`, with the field in `details` |
| Empty body | `POST`, `PATCH` and `DELETE` that do not expect a body ignore it; `204` never has a body |

## Headers

**In the request:**

| Header | Description |
|---|---|
| `Authorization: Bearer <token>` | Client API with authentication on |
| `Content-Type` | `application/json` on routes with a JSON body (another value: `415 media.unsupported`). **Exception:** message publishing accepts any media type, which becomes the message's `contentType` |
| `Accept` | `application/json` (default). On `/metrics`, `application/openmetrics-text` asks for OpenMetrics |
| `traceparent` | W3C trace context; the node's operations join the same trace |
| `X-Request-Id` | Optional (1 to 128 characters `A-Za-z0-9_-.:`); without it, the node generates a GUID v7 |

**In the response:**

| Header | When |
|---|---|
| `X-Request-Id` | **Always** (the received or the generated one); it is the `requestId` of the error body and links the call to logs and traces |
| `Retry-After` | `429`, `503` and `403` for freezing: seconds until it is worth trying again. **It is the only way** of reporting the time |
| `WWW-Authenticate: Bearer` | `401` |
| `Location` | `201` |

## Errors

Every error response (`4xx`, `5xx`) of a JSON route has the `ErrorResponse` format. The **status goes only in the HTTP status line**, the **wait time only in `Retry-After`**.

```json
{
  "code": "topic.not-found",
  "message": "O tópico não existe.",
  "requestId": "01a13635-c00c-7a83-a4ad-5a194859cf74"
}
```

| Field | Description |
|---|---|
| `code` | **Stable** code `group.reason` (catalogue in [19](19-errors-and-limits.md)). It is what the client program must test |
| `message` | Text for a person, in Portuguese (the server's current language); **may change** and must not enter logic |
| `requestId` | The same as `X-Request-Id` |
| `details` | Optional, on validation errors: list of `{ field, code, message }` (e.g. `field: "query.limit"`) |

Validation example (`limit=2000`):

```json
{"code":"validation.out-of-range","message":"limit deve estar entre 1 e 1000.","requestId":"…",
 "details":[{"field":"query.limit","code":"validation.out-of-range","message":"Entre 1 e 1000."}]}
```

**Exceptions:** the health checks (`/health/live`, `/health/ready`) return their own `HealthResponse`; a failure **after the `200`** in an NDJSON stream uses the line `{"error":"..."}`; in a partial `200` result (publishing to a distributor), the item that failed carries `error: { code, message }` **with no status**.

## Pagination

`GET /topics`, `GET /tokens`, `GET /metrics/topics` and `GET /metrics/topics/{topic}/groups` (without `top`) answer with:

```json
{ "items": [ … ], "nextCursor": "eyJrIjoi…" }
```

| Parameter | Description |
|---|---|
| `limit` | Items per page: default **100** (`api.pagination.default-limit`), maximum **1000** (`api.pagination.max-limit`) |
| `cursor` | The previous `nextCursor`, **opaque** (the client never builds it) |
| `sort` | Only where the route accepts it (`GET /topics`, `GET /tokens`), only with immutable fields; prefix `-` for descending. Without `sort`, the order is by name |

- `nextCursor` **absent** = last page. **There is no `page`, `offset` or `total`.**
- **Keyset** pagination: the order is total and stable (every `sort` ends in a unique, immutable field), the cursor stores the last key read plus the `sort` and the filters, and **does not expire** (there is no state on the server). A cursor with a `sort` or filters different from the current one: `400 validation.invalid-cursor`.
- The guarantee is **weak consistency**: whoever walks through everything receives every item that existed during the walk, with no repetition; items created or removed during it may or may not appear.

## Status codes

| Status | Meaning | Retry? |
|---|---|---|
| `200` | It worked and there is a result | — |
| `201` | Resource created (with `Location`) | — |
| `202` | Accepted and **continues in the background** (drain, return from drain) | — |
| `204` | It worked, no body (deletion, ack) | — |
| `400` | Invalid parameter, field or body | **No**, without fixing |
| `401` | Token missing, invalid, revoked or expired | No |
| `403` | Without the permission, topic outside the prefixes, IP outside the list | No |
| `404` | Topic, message or token does not exist, or the route does not exist on that port | No |
| `409` | The current state prevents it (name already exists, stale receipt, wrong type, read-only dead-letter) | No |
| `413` | Body larger than the topic's limit | No |
| `415` | Wrong `Content-Type` on a route with a JSON body | No |
| `421` | **Only between nodes** (FIFO publication on the wrong node; wrong node at the address) | — |
| `422` | The request is valid, but the cluster does not serve it in this configuration (FIFO with factor < 2, destination ceiling, volatile queue ceiling) | No |
| `429` | Rate limit | **Yes**, after `Retry-After` |
| `500` | Unexpected node failure | — |
| `502`, `504` | The **forwarding** to another node failed after the body was sent; **it was not repeated** | The client decides |
| `503` | The cluster could not close the operation **now** (no quorum, synchronizing, in drain) | **Yes**, after `Retry-After` |

> **Golden rule:** `5xx` and `429` can be repeated after `Retry-After`; `4xx` (except `429`) cannot. On a `503` or `504` of an operation that **creates or deletes** (topic, token), the result may already have been applied on part of the cluster: query the resource before repeating, or treat `409 topic.already-exists` and `404` on the repetition as success. Repeating a **publication** (outside the FIFO queue) **may duplicate** the message.

## What is left out

There is no versioning in the paths, nor documented CORS or compression. **The node only terminates TLS on the cluster port**: HTTPS of the **client** API (and of the operations one) belongs to the load balancer.

## Sources and evidence

- `http/mod.rs` (the three ports and layers), `http/common` (`X-Request-Id`, `ErrorResponse`, `404`, `500`), `core/error` (catalogue and fixed statuses), `core/pagination` (opaque cursor), `core/id` (GUID v7), `core/time` (RFC 3339).
- Example responses copied from a real node.
- Use cases: UC-CNV (conventions), UC-PAG (pagination), UC-TOP; see [26a](26a-implemented-use-cases.md). Tests: `tests/topics.rs`, `messages.rs`.
- History: `06-api.md`, `06d-api-convencoes.md`, `06a-api-cliente.md` (8.1.2).
