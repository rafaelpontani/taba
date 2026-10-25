---
title: Roadmap
summary: What is already ready (version 0.1) and what comes next, in milestones — versioning of the client contract, other connectors, publishing, Kubernetes, operation — with the pending items each milestone closes.
type: reference
status: partial
verified-on: 2026-10-14 @ efeb47a
sources: [connectors/README.md, doc/en-US/30-decisions.md, doc/en-US/31-pending-items.md]
---

# Roadmap

> This is the **plan of next steps**, with only what is **open** (what is done is summarized in "Where we are"). The questions and problems cited are in [31-pending-items.md](31-pending-items.md); what has already been decided, in [30-decisions.md](30-decisions.md). Project rule: when closing a pending item, delete the entry and the reference here.

## Where we are: version 0.1

**0.1 closes** when a person starts a three-node cluster with one command, uses the API through `taba-cli` in a terminal and through a C# console application. The first five steps closed it (a sixth, the OpenAPI contract, came after); all are **done**:

| Step | State |
|---|---|
| 1 — `taba-cli`, the first connector (commands, profiles, TUI, Docker image, binaries for 6 platforms) | ✅ |
| 2 — Docker and Compose (`Dockerfile`, three-node `docker-compose.yml`, sample HAProxy, `taba init`) | ✅ |
| 3 — C# connector (nine operations, `subscribe`, dependency injection, unit and integration tests) | ✅ |
| 4 — C# console example (`examples/csharp-console/`) | ✅ |
| 5 — End-to-end validation (`docker/validate.sh`: cluster, `taba-cli`, nodes going down and coming back, no message loss) | ✅ |
| 6 — OpenAPI contract (client and monitoring APIs), generated from the routes, served by each node with an interface at `/docs`, and published in `openapi/` | ✅ |

The whole **server** is also ready (node, cluster, FIFO, volatile, distributor, tokens, mTLS, observability) and the **CI/CD** with automatic versioning ([27-ci-cd.md](27-ci-cd.md)).

> ⚠️ **Caveats of 0.1** (found while documenting, see [31-pending-items.md](31-pending-items.md) "Findings"): stopping the **whole** cluster loses messages and topics; losing **two nodes in a row** locks the cluster; `scope=cluster` of the topic metrics does not aggregate across nodes; seven configuration parameters have no effect.

## Milestones

The order is a **proposal**; milestones 1 and 2 do not depend on each other.

### Milestone 1 — Client API contract

| Item | Detail | Closes |
|---|---|---|
| **Versioning policy in the paths** ⬜ | The OpenAPI documents exist ([15](15-api-overview.md)); what is left is deciding the policy of the prefix (today there is no `/v1`) **before** the other connectors fix the paths | RF-22 |
| **Conformance suite** ⬜ | `connectors/contrato/`: fixtures (request/response pairs), the JSON reader test documents, the scenarios and a one-node `docker-compose` of Taba (one-node profile) | UC-CNT (level `K`) |

### Milestone 2 — The other connectors

In phases ([21-connectors.md](21-connectors.md)); phases 2, 3 and 4 **do not depend on each other**, only on 1:

| Phase | Languages | Note |
|---|---|---|
| 1 — Reference | **Python** (the most readable) and **Java** (the oldest base) | Stress the design at both ends |
| 2 — C core | **C** (`libtaba`) and **C++** (C++03 header over `libtaba`) | |
| 3 — Platforms | **Node.js** and **Go** | |
| 4 — Remaining | **PHP** and **Ruby**; in **Rust**, the `async` form | |
| 5 — Integration | The scenarios pass on **all** against the real Taba; usage examples fill the README table | Depends on milestone 1 |

Decisions that must be closed: **D68** (base versions), **D70.1** and **D70.2** (organization and package names; PHP mirror; independent versions).

### Milestone 3 — Publishing

| Item | Detail |
|---|---|
| Packages 🟡 | nuget.org (`Taba.*`) and crates.io (`taba-client`, `taba-cli`) automated in `release.yml` by Trusted Publishing, turned on by the `PUBLISH_CRATES` and `NUGET_USER` variables ([27-ci-cd.md](27-ci-cd.md#publishing-and-metadata)); the registries of the other languages are missing. **They cannot be deleted** |
| Images ✅ | Docker Hub and GHCR, with the same digest on every tag ([27-ci-cd.md](27-ci-cd.md#docker-tags-and-floating-tags)) |
| Public repository ⬜ | Today private; the definitive publication is the owner's decision |

### Milestone 4 — Operation in production

| Item | Detail | Closes |
|---|---|---|
| **Kubernetes** 🟡 | Helm chart done and tested on kind ([34-helm-chart.md](34-helm-chart.md)); missing: validation on a real cluster (probes, stop with drain, internal `Service` of the cluster port) and publishing the chart | RNF-10 |
| **Forced exclusion** ⬜ | A manual exclusion route to unlock a cluster in `quorum-lost` (today the way out is restarting everything; the cluster resume restores what was on disk); see "Future evolution" of the cluster protocol | Finding C |
| **Stable ID between lives** ⬜ | Reuse the ID of a node that comes back to resynchronize only the delta, without bringing back the out-of-date node (D176) | Cluster resume |
| **Write to disk at commit** ⬜ | Write to disk on confirmation, to also preserve what was only in memory in a total fall (D176) | Cluster resume |
| **Backup and restore** 🟡 | ✅ Done: optional S3 backup (segments, journal, definitions, heartbeat, restore, eviction, flush, cluster stop; [33-s3-backup.md](33-s3-backup.md)). ⬜ Missing: compact the journal (PRB-053), recover earlier the segments of a primary that stopped being one (PRB-054), restore without reading whole segments (PRB-055), delete from the bucket definitions removed with the manager down (PRB-056), validate against a real MinIO/AWS (today's test uses a fake S3 over HTTP) and measure publishing latency with and without S3 | Finding J |
| **Aggregation of topic metrics across nodes** ⬜ | The internal route and the `fan-out` (with `stats.topics.cache-ttl`) | Finding A |
| **Benchmarks** ⬜ | The speed and cost goals have no published measurement | RNF-02 |

### Milestone 5 — Close the calibration pending items

The "unblocked" pending items of [31-pending-items.md](31-pending-items.md) are, for the most part, **calibration** or small decisions:

| Group | Items |
|---|---|
| Configuration defaults | **D84** (rate limit), **D90** (topic condition thresholds), **D91** (topic metrics in `/metrics`), **D89** (FIFO deduplication ceiling), **D87** (volatile queue ceiling), **D85** |
| Delivery index memory | **D88**, **PRB-018**: the lists have a ceiling, but the index's message map keeps everything in memory |
| Drain | **D75** (API drain that exceeds the deadline), **D77**, **PRB-005** (refuses the ack in drain) |
| Security | **D76** (IP filter vs. health checks), **PRB-015** (`X-Forwarded-For` forgeable with an empty list), **PRB-017** |
| Cluster | **D93**, **D99**, **D102**, **D104**, **D109**, **D120**, **PRB-051** (outgoing IP) |
| Observability | **D117**, **D118**, **PRB-026** (process metrics only on Linux), **PRB-050** (OTLP limits), **PRB-025**, **PRB-039** |
| Portability | **D80** (Windows reserved names), **PRB-008** |

### Milestone 6 — Documentation and repository

| Item | Detail |
|---|---|
| **Root language** ✅ | `README.md` and `AGENTS.md` in **en-US**, pointing to `doc/README.md` (which lists the documentation of each language) |
| **`uc_coverage` reading the new catalogues** ✅ | It reads the cards of `26a`/`26b`; a generator that maintains them is still missing ([31-pending-items.md](31-pending-items.md), item E) |
| **Diagrams of this documentation** ✅ | The 31 spots are drawn (index in [24-conventions.md](24-conventions.md#diagram-index)); the source is the Archify JSON |
| **Automatic verification** ⬜ | `node doc/pt-BR/scripts/verificar.mjs` and `verificar-idiomas.mjs` in a PR job ([27-ci-cd.md](27-ci-cd.md)) |
| **Integration job in CI** ⬜ | `integration.sh` and `validate.sh` (today only by hand) |
| **Translations** 🟡 | ✅ `doc/en-US/` mirrors `doc/pt-BR/`. ⬜ Other language folders next to them; every change goes to all ([24-conventions.md](24-conventions.md#documentation-in-several-languages)) |

## Out of scope (for now)

Administration graphical interface; advanced authentication (OAuth/OIDC, per-operation policy); AMQP, MQTT and Kafka protocols; stream processing and reprocessing of history; **multi-region**. Items out of scope may be reassessed.

## Sources and evidence

- `doc/08c-proximos-passos.md` (the steps of 0.1 and the "later"), `doc/08b-pendencias.md` (the pending items per milestone), `connectors/README.md` (the state of the connectors), `21-pipeline-ci-cd.md` and `.github/` (what is missing in CI).
- The ✅/⬜ states were checked against the repository (existence of folders, workflows, manifests) and against real runs described in [23-operations.md](23-operations.md).
