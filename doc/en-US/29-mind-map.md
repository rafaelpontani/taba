---
title: Mind map
summary: All of Taba on one map — the center and the eight areas, with the decisions and parameters of each and the link to the document where it goes deeper.
type: reference
status: stable
verified-on: 2026-10-13 @ 71183b9
sources: [doc/pt-BR]
---

# Mind map

> A **text** map (nested lists). Each branch says the essentials and points to the document that explains it. The diagrams are at the end.

## The center

**Taba** = **HTTP** messaging, **one image**, with **queue + distribution**, **no coordinator**. Each **node** is equal to the others. What decides *where* a message lives is **the hash ring**; what decides *whether* an operation counts is **the quorum** (messages) or **all nodes** (membership change).

```
                    ┌────────────── 1. Topics ───────────────┐
                    │  queue · distributor · volatile · FIFO  │
   8. Operation ────┤                                        ├───── 2. Publishing
   configuration    │                                        │      register + commit · quorum
   runbook          │              T A B A                   │
                    │   node = same program, 3 ports         │
   7. Security ─────┤                                        ├───── 3. Delivery
   token · mTLS     │                                        │      invisibility · ack · long polling
                    │                                        │
   6. Observab. ────┤                                        ├───── 4. Ring and replication
   OTLP · Prometheus│                                        │      blocks · P/S/T
                    └──────── 5. Cluster and storage ────────┘
                              join · failure · drain · memory + disk
```

## 1. Topics ([09](09-topics.md))

- **Two types:** `queue` (stores and delivers to **one** consumer) · `distributor` (copies to queues whose name starts with a **prefix**)
- **Two queue attributes:**
  - **Volatile:** disappears without a consumer (`idleTimeout`, default 600 s) · creatable by `GET` (`autoCreate`) · **managing node** decides the deletion
  - **FIFO:** order **per group** · one message per group in flight · **deduplication** (300 s window) · quorum with **primary + secondary**
- **Dead-letter:** `{queue}-dead-letter`, created with the queue · receives what exceeds `maxDeliveries`
- **Fields:** `ttl` (14 d) · `maxDeliveries` (10) · `maxMessageBytes` (5 MB) · tags (telemetry)
- **Immutable:** `name`, `id`, `type`, `volatile`, `fifo`

## 2. Publishing ([10](10-publishing.md), [11](11-durability.md))

- **Any node** receives → becomes **coordinator** → computes the responsible nodes by the **`id`** (GUID v7)
- **Phase 1 (register)** to all, in memory → **quorum** (primary **always** included) → **phase 2 (commit)** → response
- **Before** the quorum: can **abort** (rollback, `write.pending.ttl`) · **after**: **never** undoes, only insists (`write.commit.timeout`)
- **Parameters:** `replication.factor` (3) · `write.quorum` (majority) · `write.retry.*`
- **Guarantee:** survives the loss of `quorum − 1` nodes · does **not** survive the stop of **all** (production)

## 3. Delivery ([12](12-delivery.md))

- **Invisibility** (`visibilityTimeout` 120 s) · `receiptToken` per delivery · ack with the token · redelivery if it expires
- **Delivery owner** = block primary · **ready list** (accordion effect) · **invisible list**
- **Long polling** (`waitTime` ≤ 20 s) · **permanent channel** (heartbeat 2 s, notices, `take`)
- **Fair delivery** across groups (power-of-2 bands, `normal-every` 10, weight 0.1)
- **Retention:** `ttl` discards (no dead-letter) · `maxDeliveries` → dead-letter

## 4. Ring and replication ([06](06-ring-and-replication.md))

- **Position** = 8 bytes of SHA-256 · **128 virtual nodes** per node · **block** = arc ending at a virtual node
- **Primary / secondary / tertiary** = distinct physical nodes along the ring
- **One ring, four keys:** message (`id`) · FIFO group (`topicId:groupId`) · volatile manager (`topicId`) · definition owner (`topic:name`)
- **Ring changes** → new node synchronizes · a node that lost the block only deletes with the cluster healthy

## 5. Cluster and storage ([07](07-cluster.md), [08](08-storage.md))

- **Join:** `cluster.entry-address` → `GET /cluster/nodes` → `join` → register + commit on **all** → **synchronizes** (topics, blocks, event database, final check) → **ready**
- **Failure:** channel drops → reconnects (3×) → **vote** (51%, ≥ 2 voters) → deregistration → **freezing** (5 min)
- **Return:** loss list → returns what it has → joins **as a new node**
- **Drain:** everything to disk → deregistration **without a vote** · `SIGTERM` triggers it
- **One-node profile:** `replication.factor=1`, no replicas · every restart is a new node
- **Storage:** **memory first**; **overflow** to disk (`memory.limit`) · `_node.db` · `<topic>/_topic.db` + `<id>.body` · `_sync/sync.db`
- **Node states:** `joining` · `syncing` · `active` · `returning` · `failed` (+ `drainState`)

## 6. Observability ([13](13-observability.md))

- **OTLP** (push: metrics, traces, logs) · **`/metrics`** Prometheus (one target per node) · **JSON** (`/health/cluster`, `/metrics/topics*`)
- **84 node metrics** (`taba.*`, `http.server.*`, `process.*`) · topic ones **opt-in** (`telemetry.topics.mode`)
- **Cluster conditions (12):** `member-unreachable` · `node-frozen` · `syncing` · `under-replicated` · `write-quorum-at-risk` · `quorum-lost` · `membership-divergent` · `storage-pressure` · `upgrade-stalled` · `members-missing` · `previous-life-pending` · `reduced-profile`
- **Health:** `/health/live` · `/health/ready` (503 in drain/synchronization)

## 7. Security ([14](14-security.md))

- **Client:** token `tb_<id>.<secret>` (salted hash) · permissions `publish`/`consume`/`create`/`manage`/`admin` · prefixes · **root** token
- **Cluster:** **mutual TLS**, single certificate in a reloaded folder, **rotation** with `--starts-in`
- **Operations:** no authentication → **network** + `operations.allowed-ips`
- **Rate limit** per token or IP (off by default) · client IP + `trusted-proxies`

## 8. Operation and interfaces ([15](15-api-overview.md)–[23](23-operations.md))

- **APIs:** client (8080) · cluster (8081) · operations (optional)
- **CLIs:** `taba` (server: `init`, `cert generate`, `token generate`) · `taba-cli` (client, profiles, TUI)
- **Connectors:** Rust ✅ · C# ✅ · 8 ⬜
- **Configuration:** YAML + `TABA_*` · `config/params`
- **Runbook:** one node at a time · drain before stopping · never stop everything

## Map of the documents by question

| Question | Document |
|---|---|
| "What is this and how do I get it up?" | [01](01-overview.md), [02](02-quick-start.md) |
| "Where does my message live?" | [06](06-ring-and-replication.md), [08](08-storage.md) |
| "Can I lose a message?" | [11](11-durability.md) |
| "How does the message reach the consumer?" | [12](12-delivery.md) |
| "A node went down: now what?" | [07](07-cluster.md), [23](23-operations.md) |
| "Which route, which error?" | [16](16-client-api.md), [19](19-errors-and-limits.md) |
| "How do I configure?" | [22](22-configuration.md) |
| "How do I monitor?" | [13](13-observability.md) |
| "How do I change the code?" | [24](24-conventions.md), [25](25-testing.md), [26](26-use-cases.md) |

![General mind map](../diagrams/img/en-US/mind-map.svg)

![1. Write and quorum](../diagrams/img/en-US/mind-map-write.svg)

![2. Delivery](../diagrams/img/en-US/mind-map-delivery.svg)

![3. Invisibility](../diagrams/img/en-US/mind-map-invisibility.svg)

![4. Topics](../diagrams/img/en-US/mind-map-topics.svg)

![5. Cluster and members](../diagrams/img/en-US/mind-map-cluster.svg)

![6. Synchronization](../diagrams/img/en-US/mind-map-sync.svg)

![7. Storage](../diagrams/img/en-US/mind-map-disk.svg)

![8. API and observability](../diagrams/img/en-US/mind-map-observability.svg)

## Sources and evidence

- `16-mapa-mental.md` (the structure in eight areas) rewritten over the documents of this folder; the numbers (84 metrics, 8 conditions, 5 states, 128 virtual nodes, 3 ports) checked in the code ([13](13-observability.md), [07](07-cluster.md), [06](06-ring-and-replication.md)).
- Use cases: all areas ([26a](26a-implemented-use-cases.md)).
