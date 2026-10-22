---
title: Taba documentation (en-US)
summary: Index of the English documentation, with the reading order for each profile.
type: reference
status: stable
verified-on: 2026-10-14 @ efeb47a
sources: [README.md, AGENTS.md, crates/taba-node/src/lib.rs]
---

# Taba — documentation (en-US)

Taba is an open-source messaging system, shipped as a single Docker image, that combines **queue** (each message goes to one consumer) and **distribution** (each publication is copied to several queues) in the same HTTP API. This documentation describes **how it is today**, for someone who has never seen the project.

> **How to read the status badges.** Every feature described here carries a mark: ✅ implemented (code **and** test exist), 🟡 partial (the text says what is missing), ⬜ planned (does not exist yet), ⚠️ known limit (points to a `PRB-…` pending item in [31-pending-items.md](31-pending-items.md)).

> **Where this documentation comes from.** The original design documents were written before the code, to guide it, and were removed from the tree (they live in the git history). This folder was written **after** the code, by reading it. When the two disagree, the code wins.

> **Translation of [`../pt-BR/`](../pt-BR/README.md).** This folder mirrors `doc/pt-BR/` file by file: the files keep the same number prefix (`NN-`), only the name is in English. **Every change to one language folder must be applied to all the others** (see [24-conventions.md](24-conventions.md#documentation-in-several-languages)).

## Where to start

| You are… | Read in this order |
|---|---|
| **Someone who will use** Taba in an application | [01](01-overview.md) → [02](02-quick-start.md) → [03](03-concepts.md) → [09](09-topics.md) → [12](12-delivery.md) → [16](16-client-api.md) → [21](21-connectors.md) |
| **Someone who will operate** a cluster | [01](01-overview.md) → [02](02-quick-start.md) → [03](03-concepts.md) → [07](07-cluster.md) → [22](22-configuration.md) → [23](23-operations.md) → [33](33-s3-backup.md) → [34](34-helm-chart.md) → [13](13-observability.md) → [14](14-security.md) |
| **Someone who will contribute** code | [03](03-concepts.md) → [04](04-architecture.md) → [06](06-ring-and-replication.md) → [10](10-publishing.md) → [24](24-conventions.md) → [25](25-testing.md) → [26](26-use-cases.md) |
| **An AI** (agent) that will read or change the project | [llms.txt](llms.txt) → [03](03-concepts.md) → [04](04-architecture.md) → the document of the subject → [24](24-conventions.md) |

## Index

### Part A — Understand

| File | Content |
|---|---|
| [01-overview.md](01-overview.md) | What Taba is, for whom, goals and requirements with the state of each one, what it is not |
| [02-quick-start.md](02-quick-start.md) | Start the three-node cluster, publish and consume, bring a node down |
| [03-concepts.md](03-concepts.md) | The mental model on one page: node, cluster, topic, message, block, ring, quorum |

### Part B — How it works inside

| File | Content |
|---|---|
| [04-architecture.md](04-architecture.md) | Node layers, the three ports, composition, module map |
| [05-dependencies.md](05-dependencies.md) | Libraries, tools and system dependencies, and why |
| [06-ring-and-replication.md](06-ring-and-replication.md) | Circular hash, blocks, primary/secondary/tertiary roles |
| [07-cluster.md](07-cluster.md) | Join, failure, voting, freezing, return, drain, synchronization, development mode |
| [08-storage.md](08-storage.md) | Memory, disk overflow, SQLite, restart |

### Part C — Functional specification

| File | Content |
|---|---|
| [09-topics.md](09-topics.md) | Queue, distributor, volatile queue, FIFO, dead-letter |
| [10-publishing.md](10-publishing.md) | The path of a published message |
| [11-durability.md](11-durability.md) | What is guaranteed, what is not, and in which scenario |
| [12-delivery.md](12-delivery.md) | Consumption, long polling, invisibility, ack, delivery limit, groups |
| [13-observability.md](13-observability.md) | Metrics, health, conditions, traces, logs |
| [14-security.md](14-security.md) | Tokens, cluster mutual TLS, rate limit, client IP |

### Part D — Interfaces

| File | Content |
|---|---|
| [15-api-overview.md](15-api-overview.md) | The three APIs and the common conventions |
| [16-client-api.md](16-client-api.md) | Topics, messages and tokens, route by route |
| [17-operations-api.md](17-operations-api.md) | Health, metrics and drain |
| [18-cluster-api.md](18-cluster-api.md) | Node-to-node routes and the permanent channel |
| [19-errors-and-limits.md](19-errors-and-limits.md) | Catalogue of errors and limits |
| [20-cli.md](20-cli.md) | `taba` (server) and `taba-cli` (client) |
| [20b-cli-ui.md](20b-cli-ui.md) | The full-screen interface (`taba-cli tui`): open, configure, screens and keys |
| [21-connectors.md](21-connectors.md) | Client libraries and the common contract |

### Part E — Operate

| File | Content |
|---|---|
| [22-configuration.md](22-configuration.md) | All parameters and defaults |
| [23-operations.md](23-operations.md) | Runbook: start, scale, retire, diagnose, backup, stop and start |
| [33-s3-backup.md](33-s3-backup.md) | Optional S3 backup: what goes to the bucket, restore, eviction, flush, stop |
| [34-helm-chart.md](34-helm-chart.md) | Helm chart: what it creates, every value (Ingress, MinIO/S3, Prometheus/Telegraf collector, OTLP, KEDA) |
| [35-helm-use-cases.md](35-helm-use-cases.md) | Helm chart: operating modes and the bash tests for each |

### Part F — Contribute

| File | Content |
|---|---|
| [24-conventions.md](24-conventions.md) | Code, API, folders, language |
| [25-testing.md](25-testing.md) | Test levels and how to write one |
| [36-known-issues.md](36-known-issues.md) | Tests turned off because they fail intermittently in CI |
| [26-use-cases.md](26-use-cases.md) | The `UC-…` format and the coverage rule |
| [26a-implemented-use-cases.md](26a-implemented-use-cases.md) | Cases with a test |
| [26b-planned-use-cases.md](26b-planned-use-cases.md) | Cases without a test yet |
| [27-ci-cd.md](27-ci-cd.md) | Workflows, images and releases |

### Part G — Reference and project

| File | Content |
|---|---|
| [28-glossary.md](28-glossary.md) | Project terms |
| [29-mind-map.md](29-mind-map.md) | The project on one map |
| [30-decisions.md](30-decisions.md) | Why each thing is the way it is |
| [31-pending-items.md](31-pending-items.md) | What is still open |
| [32-roadmap.md](32-roadmap.md) | What comes next |
| [37-cluster-cold-start.md](37-cluster-cold-start.md) | Proposal under discussion: resuming the cluster after a full outage (finding D) |

## Conventions of this documentation

- Text in US English. Identifiers (routes, configuration keys, fields, error codes) stay in English, in `code`.
- A name such as `05-particionamento-replicacao.md`, with no folder, is a document of the original design (written before the code) that was **removed** from the tree: it is in the git history, in commit `71183b9` and earlier. Where it and the code disagree, the code wins.
- Every file starts with metadata (`summary`, `status`, `verified-on`, `sources`) and ends with **Sources and evidence**: the paths in the code and the `UC-…` use cases that back up what was said.
- The diagrams are SVG images generated from JSON in `doc/diagrams/source/` (the index is in [24-conventions.md](24-conventions.md#diagram-index)); the SVG is the output, the JSON is the source.
- Links between files use the file name; there is no continuous section numbering across files as in the historical documentation.
