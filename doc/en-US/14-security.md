---
title: Security
summary: Who trusts whom in Taba — client API tokens, mutual TLS between nodes and certificate rotation, rate limit, client IP, operations routes without authentication and the known risks.
type: explanation
status: stable
verified-on: 2026-10-29 @ 6e8cdf6
sources: [crates/taba-node/src/secrets, crates/taba-node/src/cluster/tls, crates/taba-node/src/http/auth, crates/taba-node/src/http/ratelimit, crates/taba-node/src/http/client_ip, crates/taba-node/src/engine/tokens, crates/taba-node/src/core/token]
---

# Security

> Read first: [04-architecture.md](04-architecture.md) (the three ports).

## Trust boundaries

Taba separates the audiences **by port** (not only by path), so the network (firewall, security group, `NetworkPolicy`) can allow or block each one without looking at the request:

| Port | Audience | How it is protected |
|---|---|---|
| **Client** (`api.port`, 8080) | Applications | Optional **token** (turns on by itself with a root token) + rate limit. Use HTTPS on the load balancer |
| **Cluster** (`cluster.port`, 8081) | **Only the other nodes** | **Mutual TLS** with the cluster certificate. Never expose outside the nodes' network |
| **Operations** (`operations.port`, 9090) | Collectors and orchestrator inside the infrastructure | **No authentication**: network protection only (own port + `operations.allowed-ips`) |

![Trust zones of the three ports](../diagrams/img/en-US/security-zones.svg)

The three ports must be **different from each other**; a repeated value or a port in use prevents the node from starting. The client port answers `404` to `/cluster/*`; the cluster port answers `404` to anything that is not `/cluster/*`.

## The secrets folder and `taba init`

`taba init --out <folder>` creates, once, the **secrets folder** that is mounted on all nodes (`secrets.dir`, default `/etc/taba/secrets`):

```
secrets/
  root-token                      43 characters (256 random bits in base64): the root token
  tls/
    cluster-<YYYYMMDD>.crt  self-signed cluster certificate
    cluster-<YYYYMMDD>.key
```

- `--days` is the certificate validity (default **1825**, five years).
- `taba cert generate --days <n> [--starts-in <duration>] --out <folder>` creates **only** a new pair (for rotation). `--starts-in` (e.g. `14d`) postpones the start of validity.
- `taba token generate` prints a random 256-bit secret (a standalone root token).
- **Mounting the folder turns everything on by itself:** the cluster TLS (if `tls/` exists) and authentication (if there is a `root-token`).
- In production, **generate once** and deliver as a platform secret (Kubernetes Secret, Docker secret, volume): **not at every container start**.

> **On Windows with Git Bash**, Docker commands that take a path (`/secrets/root-token`) have the path "translated" to a Windows one. Use `MSYS_NO_PATHCONV=1` (or `docker compose exec taba-0 cat /etc/taba/secrets/root-token`).

## Token authentication (client API)

### When it turns on

| Situation | Authentication |
|---|---|
| There is a **root token** (`root-token` file in the secrets folder, `auth.root-token-file` or `auth.root-token`) | **On**: every client route requires `Authorization: Bearer <token>` |
| No root token | **Off**: no route requires anything |
| `auth.enabled=false` | Forced off, even with the folder mounted (to keep TLS without requiring a token) |
| `auth.enabled=true` **without** a root token | The node **does not start** |
| **One-node profile** with `auth.dev-token` | **On**, only with the `dev` working token (`tb_dev.<secret>`); without it and with no root token, the "There is no root token" row applies |

The root token needs **at least 32 characters**, otherwise the node does not start. The operations routes and `/cluster/*` do **not** use tokens.

### The two kinds of token

- **Root token:** comes from the configuration, the same on all nodes. **Full access**, no prefix limit, and is the **only** one that stops the cluster (`POST /cluster/stop`) and creates, changes or revokes `admin` tokens. It exists from the first instant (it does not come from the table), so there is **no "first admin" to create**. In logs and traces it appears as `token.id=root`. `auth.root-token-file` is re-read every `auth.reload-interval` (default 60 s) and accepts **two lines** (the current one and the previous one): you can change the secret without restarting any node.
- **Access token** (`tb_<id>.<secret>`): created by the root, or by a token with the `admin` permission, through `POST /tokens`. The `id` is a GUID v7; the secret has 256 bits. **The cluster stores only the salted hash** (`SHA-256(salt ‖ secret)`, random 128-bit salt per token), **never** the secret, which appears **only in the creation response**. The `id` in the prefix lets you find the token and identify it in logs and traces **without exposing the secret** (the whole token is never logged).

Every node knows all the tokens: **validating is a local in-memory lookup**, with no network. Creation, change and revocation go through the **definition owner** and spread to all ([08-storage.md](08-storage.md)); only the salt and the hash travel between nodes.

### Permissions and prefixes

| Permission | Allows |
|---|---|
| `publish` | `POST /topics/{topic}/messages` |
| `consume` | `GET`, `PUT .../visibility` and `DELETE` (ack) of messages |
| `create` | `POST /topics` and the `autoCreate` of the `GET` (which also requires `consume`). **Only creates** |
| `manage` | `PATCH` and `DELETE` on `/topics/{topic}` and emptying a queue (`DELETE /topics/{topic}/messages`). **Includes `create`, `publish` and `consume`** and respects the prefixes |
| `admin` | The tokens (`/tokens`) and the operation of the cluster and of the nodes (`/cluster/*`, `/nodes/{nodeId}/…`: status, drain, backup, metrics). **Includes `manage`, `create`, `publish` and `consume`**, takes **no prefixes** (it reaches every topic). It does **not** stop the cluster and does not create, change or revoke another `admin` token: those are the root token's alone |

Every valid token queries `GET /topics` and `GET /topics/{topic}` within its prefixes.

**`admin` was `manage`.** Until the schema 2 of the permissions, `admin` meant what `manage` means now (a manager of topics). The node renames it when it opens a database of an older schema (`_node.db`, schema 4 to 5), and a token that arrives from another node or from a backup **without** `permissionsSchema` reads its `admin` as `manage`, so no token gains the new `admin` by the rename.

**`topicPrefixes`** (up to 20, 1 to 100 characters): the token only reaches topics whose name **starts with** some prefix; empty = all. `GET /topics` returns only the topics within the prefixes; `POST /topics` only accepts a name (and, on a distributor, **each distribution `prefix`**) within them, so a restricted token does not inject messages into queues out of reach. The dead-letter is reached by the prefix of the source queue. When publishing to a distributor, the permission is checked **only on the distributor**.

### Verified example (3-node cluster)

```bash
TOK=$(docker compose exec -T taba-0 cat /etc/taba/secrets/root-token)

curl -s -X POST localhost:8080/tokens -H "Authorization: Bearer $TOK" -H 'Content-Type: application/json' \
  -d '{"name":"servico-pedidos","permissions":["publish","create"],"topicPrefixes":["pedidos"]}'
# 201 {"id":"01a13649-…","token":"tb_01a13649-….<secret>","permissions":["publish","create"],"topicPrefixes":["pedidos"],…}
```

| Call | Result |
|---|---|
| `GET /topics` without a token | `401 auth.unauthorized`, with `WWW-Authenticate: Bearer` (the response does not say *which* problem) |
| Publish to `pedidos` with the new token | `201` |
| Consume (`GET`) with the same token | `403 auth.forbidden` (missing `consume`) |
| Publish to `t2` (outside the prefix) | `403 auth.out-of-scope` |
| `GET /tokens` with the new token | `403 auth.forbidden` (root only) |

### Other rules

- **Change:** only `name` and `expiresAt` (`null` removes the deadline). **Permissions and prefixes are immutable** (`400 validation.immutable-field`): to change the reach, create another and revoke this one.
- **Revoke:** takes effect on **all** nodes at the commit; the following calls answer `401`.
- **Validity:** `expiresAt` is absolute and optional; each node ignores the expired one and deletes it in the local sweep (`retention.sweep.interval`), without propagating. **Depends on the clock** of the nodes (NTP).
- **Node in synchronization or drain:** does not have the complete table and **does not validate the token**: it forwards the `Authorization` and **the destination checks**.
- Logs: token created, changed or revoked (`info`, with the `id`); `401`/`403` (`warn`, `debug` at volume).
- **PRB-017:** validating an unknown `id` answers slightly faster than an existing `id` with a wrong secret (it does not compute the hash): in theory it allows discovering `id`s by response time. The secret remains protected.

## Mutual TLS between nodes

The cluster port uses **mutual TLS with a single cluster certificate**, read from **a folder** (`cluster.tls.dir`, default `<secrets.dir>/tls`) and **reloaded by itself** every `cluster.tls.reload-interval` (default 60 s). There is no CA or per-node certificate: trust is that of the **certificates themselves** (*pinning*).

| Rule | Effect |
|---|---|
| **Whom the node accepts** | Whoever presents a certificate **equal** to one in the folder, or **issued** by one of them (a CA put in the folder), as long as it has **not expired** (`notAfter`). The start date does **not** count for trust: a node accepts the new certificate before any node starts presenting it |
| **Which one the node presents** | The pair with the **most recent start that is already valid** (`notBefore` ≤ now) and not yet expired, when **receiving** and **opening** connections (HTTP and the channel WebSocket) |
| **Host name** | **Not verified**: the nodes connect by **IP** (the one in the member table) |
| **Without the folder** | The cluster port serves **plain HTTP** and the node logs `warn` |
| **Folder set and nonexistent, or without a valid pair** | The node **does not start** |
| **One-node profile** (`cluster.tls.enabled=false`) | The port opens over plain HTTP and the log lists what is relaxed |

**Identity of the caller:** since everyone uses the same certificate, the cluster recognizes the caller by the connection's **source IP** and checks it in the member table. That is why the node reports its IPs (`cluster.advertise-addresses`, or the interface that goes out to the `cluster.entry-address`).

### Rotation without restarting anything

1. Generate the new pair with a start in the future: `taba cert generate --days 1825 --starts-in 14d --out <folder>`.
2. Add it to the folder (e.g. a new entry in the Secret). Within `cluster.tls.reload-interval`, each node starts to **trust** it, but keeps presenting the old one.
3. When the start date arrives, **all start presenting the new one by themselves**.
4. The old one stops being accepted when it expires; it can be removed earlier.

`--starts-in` must be **longer** than the time for the new pair to reach **all** nodes (Secret propagation + `reload-interval`): a node that does not have it yet, when the date arrives, rejects whoever presents it. Connections already open (the channel) continue with the old one until they drop, which is harmless.

**Reload with an error:** unreadable file, incomplete pair or invalid certificate → the node **keeps the previous set** and logs `error`; it only changes when the whole folder is valid.

**Expiry:** an expired one stops being accepted and presented. The node logs `warn` from `cluster.tls.expiry-warning` (default **30 days**) and exposes `taba.cluster.tls.expires_in` to alert in Prometheus.

### Load balancer in front of the cluster port

Only the first bootstrap step (`GET /cluster/nodes`) goes through a load balancer (`cluster.entry-address`). That listener must **pass TLS through without opening it** (TCP/L4, like an NLB) or be a DNS name that resolves directly to the nodes (a *headless* `Service`). A load balancer that **terminates** TLS (like an ALB) loses the client certificate and breaks authentication. (In the Compose sample, HAProxy uses TCP mode for port 8081.) HTTPS of the **client** API belongs to the load balancer and is independent of this.

### Accepted limit

Whoever obtains the key of **one** node can impersonate **any** node: the same risk as a shared secret, but with encrypted traffic. Whoever wants a per-node certificate can use a CA (cert-manager, service mesh): put the CA's `.crt` in the folder to trust and the node's own pair to present.

## Rate limit

Limit of requests **per node**, **token bucket** per category and per key. **Off by default**: without `ratelimit.*.rps` the category has **no limit**. `ratelimit.burst` (allowed overshoot) is `ceil(rps)` per category if absent.

| Category | Routes | Key |
|---|---|---|
| **Publishing** (`ratelimit.publish.rps`) | `POST /topics/{topic}/messages` and `POST .../messages/batch` | `id` of the valid token (the root has the counter `root`); without a valid token, the **client IP** |
| **Consumption** (`ratelimit.consume.rps`) | `GET` of messages, `PUT .../visibility`, `DELETE` (ack), `POST .../messages/ack` and `POST .../messages/visibility` | same |
| **Administrative** (`ratelimit.admin.rps`) | `/topics*` and `/tokens*` | same |
| **Operations** (`ratelimit.ops.rps`) | `/health/cluster?scope=cluster` and `/metrics/topics` | always the **IP** |

- Exceeded: `429 rate-limit.exceeded` with `Retry-After`.
- A request **in batch** counts as **one** call, like a `GET` that delivers many messages ([D189](30-decisions.md)).
- **Exempt:** `/health/live`, `/health/ready`, `/metrics` and `/health/cluster?scope=node`: a `429` on `/health/ready` would make the load balancer remove the node by mistake.
- **Counters are local to each node**: a client spread over N nodes can reach N times the limit (accepted).
- **Order:** authentication (local) → rate limit → refusal by drain/synchronization → authorization. The `401` also counts **per IP**, so whoever tries to guess tokens is not left without a limit; a `403` counts on the token.
- All tokens have the **same** limit; per-token limits do not exist.
- A node in synchronization/drain counts **per IP** and forwards; the destination does not count again. The **cluster port has no rate limit**.
- Metric: `taba.ratelimit.rejected` (attribute `key`: `token` or `ip`).

## Client IP and trusted proxies

For calls counted **per IP** (and for `operations.allowed-ips`), the IP may come from a **header**, which matters behind a load balancer:

| Key | Default | Effect |
|---|---|---|
| `client-ip.headers.enabled` | `true` | Turns on the use of headers; when off, the connection address always applies |
| `client-ip.headers` | `X-Forwarded-For` | Headers consulted, in order; the first present applies. In a comma-separated list the **first** address applies (the original client). Absent or invalid: falls back to the connection address |
| `client-ip.trusted-proxies` | empty | List of IPs/CIDR: the headers only apply if the connection comes from one of them |

> ⚠️ **PRB-015:** with `client-ip.trusted-proxies` **empty** (the default), **any client forges** `X-Forwarded-For` and chooses its own IP: it escapes the per-IP rate limit and the `operations.allowed-ips` list. **In production, fill `trusted-proxies` with the load balancer IP, or turn off `client-ip.headers.enabled` if there is no load balancer.**

## Operations routes: network protection

`/metrics`, `/metrics/topics/*`, `/health/*` and `/node/drain` have **no authentication** on the operations port (decision D63). It is the one port the node always opens for them (`operations.port`, 9090): protect it with the **network** (security group, `NetworkPolicy`) and, in addition, `operations.allowed-ips` (403 `ops.ip-not-allowed`). Beware of `POST /node/drain`: **anyone who reaches the operations port can take a node out of the cluster**. Details in [13-observability.md](13-observability.md).

**Metrics and authentication, by port** (decision D177):

| Port | `/metrics` | `/health/live`, `/health/ready` | Rest of the operations routes |
|---|---|---|---|
| Operations (9090) | no token | no token | no token |
| Client (8080) | **any valid token** (`401` without one) | no token (probes do not send one) | do not exist (`404`) |

So a collector inside the infrastructure scrapes `:9090/metrics` with no credentials, and whoever has to read the metrics from outside uses the client port with a token. `taba-cli` never calls the operations port: it lives outside the infrastructure and reaches the cluster only through the client API. A node in drain only validates the root token, so a plain token gets `401` on the client port's `/metrics` while the node drains (PRB-069); the operations port is not affected.

**The documentation of the APIs is open.** `GET /openapi/client.json` and `/docs` on the client port, and `GET /openapi/monitoring.json` and `/docs` on the operations port, need no token even with authentication on: they describe the API, not the data. On the operations port they follow the same `operations.allowed-ips` filter as the rest of the port. The interface is embedded in the binary and loads nothing from the internet. The documents list the routes of the node, which includes whether authentication is on (the `/tokens` routes only exist with it): that is not a secret, since the routes answer `401` or `404` the same way to anyone ([13-observability.md](13-observability.md), [16-client-api.md](16-client-api.md)).

## Summary of known risks

| Code | Risk | Mitigation |
|---|---|---|
| PRB-015 | `X-Forwarded-For` forgeable with `trusted-proxies` empty | Fill the list or turn off the headers |
| PRB-017 | Different response time for an unknown token `id` | No effect on the secret |
| PRB-051 | The cluster recognizes the caller by source IP; a node with several interfaces may fail | Set `cluster.advertise-addresses` |
| D63 | Operations routes without authentication | Own port + network + `allowed-ips` |
| — | Whoever has the key of one node impersonates any node | Isolate the cluster port; CA if necessary |
| — | The token travels in clear if the load balancer does not terminate TLS | HTTPS in front of the client API |

## Sources and evidence

- `secrets/mod.rs` (`taba init`, `cert generate`, `token generate`), `cluster/tls/mod.rs` (acceptance, presentation, reload), `http/auth` (`identify`, `permit`; `authenticate`, `authorize`), `core/token` (format and hash), `engine/tokens`, `http/ratelimit`, `http/client_ip`, `cluster/service` (identity by IP).
- The "Verified example" table was run against a 3-node cluster started by `docker-compose.yml`.
- Use cases: UC-AUT (authentication and tokens), UC-LIM (rate limit and IP), UC-SEG (ports, mutual TLS and certificates); see [26a](26a-implemented-use-cases.md). Tests: `tests/auth.rs`, `ratelimit.rs`, `mtls.rs`, `secrets.rs`.
- History: `06-api.md` (8.0), `06a-api-cliente.md` (8.1.3), `12-limites-api.md`.
