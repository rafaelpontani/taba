---
title: Overview
summary: What Taba is, who it is for, what it is not, and the list of goals and requirements with the state of each one.
type: explanation
status: stable
verified-on: 2026-10-18 @ 6c5de51
sources: [README.md, Cargo.toml, crates/taba-node/src/lib.rs, docker-compose.yml, connectors/]
---

# Overview

> Read next: [02-quick-start.md](02-quick-start.md) (see it working) and [03-concepts.md](03-concepts.md) (the vocabulary).

## What it is

**Taba** is a messaging system: applications **publish** messages to **topics** and other applications **consume** them, asynchronously. It brings together in a single HTTP API two patterns that, in a cloud, are usually two services:

- **Queue** (like Amazon SQS): each message is delivered to **one** consumer, and becomes available again if the consumer does not confirm it in time.
- **Distribution** (like Amazon SNS): each publication is **copied** to every queue whose name starts with one of the prefixes of the distributor topic.

The name comes from **T**ópicos **A**ssíncronos **B**alanceados e **A**utogeridos (balanced, self-managed asynchronous topics). In Tupi, *taba* is the village: each node is a hut, the cluster is the village.

Each node is **the same program** (the `taba` binary, written in Rust), packaged in a single Docker image. A cluster is three or more identical nodes, with no coordinator, no external database and no separate broker. For testing, one node is enough (development mode).

![Context: who uses Taba](../diagrams/img/en-US/c4-level0-context.svg)

## Who it is for

| Situation | Why Taba |
|---|---|
| You want queues and pub/sub without paying per message or depending on a cloud | It runs wherever there is Docker; the cost is the cost of the machine |
| You want a messaging cluster that organizes itself | A new node joins and takes its share; a node that goes down is excluded by vote and, when it returns, is reintegrated |
| You want to integrate over HTTP, with no new protocol | The API is HTTP and JSON; the connectors are optional |

## What Taba is not

| It is not | Consequence |
|---|---|
| A protocol replacement | It does not speak AMQP, MQTT or the Kafka protocol; changing broker means changing the client |
| A streaming platform | It does not re-read history: after the ack the message is gone (in FIFO, only the body; see [09-topics.md](09-topics.md)) |
| Multi-region | A cluster lives in a single network |
| A graphical interface | There is an API, a command line and telemetry; there is no dashboard |
| Advanced authentication | There are simple tokens with permissions and prefixes; there is no OAuth/OIDC |

## Goals and requirements

The requirements have a stable code (`RF` functional, `RNF` non-functional) so they can be cited in other documents. The state is the state of the code today.

### Functional requirements

| Code | Requirement | State | Detail |
|---|---|---|---|
| RF-01 | Create, list, read, change and remove topics | ✅ | [16-client-api.md](16-client-api.md) |
| RF-02 | **Queue** topic (single delivery) and **distributor** topic (copy by prefix) | ✅ | [09-topics.md](09-topics.md) |
| RF-03 | **Volatile** queue that disappears by itself without a consumer, creatable by consumption itself | ✅ | [09-topics.md](09-topics.md) |
| RF-04 | **FIFO** queue: order per group and deduplication | ✅ | [09-topics.md](09-topics.md) |
| RF-05 | Publish, consume (with long polling) and confirm (ack) | ✅ | [12-delivery.md](12-delivery.md) |
| RF-06 | Redelivery by temporary **invisibility**, changeable after delivery | ✅ | [12-delivery.md](12-delivery.md) |
| RF-07 | Validity (`ttl`), delivery limit and **dead-letter** | ✅ | [12-delivery.md](12-delivery.md) |
| RF-08 | Message groups and **fair delivery** among producers | ✅ | [12-delivery.md](12-delivery.md) |
| RF-09 | Messages in memory, with **overflow to disk** | ✅ | [08-storage.md](08-storage.md) |
| RF-10 | **Replication** across several nodes and confirmation by **quorum** | ✅ | [10-publishing.md](10-publishing.md) |
| RF-11 | Automatic cluster formation (node join) | ✅ | [07-cluster.md](07-cluster.md) |
| RF-12 | Failure detection, exclusion **vote** and **freezing** | ✅ | [07-cluster.md](07-cluster.md) |
| RF-13 | Return of an excluded node, with recovery of lost messages | ✅ | [07-cluster.md](07-cluster.md) |
| RF-14 | **Drain**: leave the cluster without losing what is in memory | ✅ | [07-cluster.md](07-cluster.md) |
| RF-15 | **Development mode**: a single node, no cluster | ✅ | [07-cluster.md](07-cluster.md) |
| RF-16 | **Token** authentication with permissions and prefixes | ✅ | [14-security.md](14-security.md) |
| RF-17 | **Mutual TLS** between nodes, with a cluster certificate and rotation | ✅ | [14-security.md](14-security.md) |
| RF-18 | Rate limit and operations IP filter | ✅ | [14-security.md](14-security.md) |
| RF-19 | Metrics, health and conditions; OTLP and Prometheus export | ✅ | [13-observability.md](13-observability.md) |
| RF-20 | Server (`taba`) and client (`taba-cli`) command lines | ✅ | [20-cli.md](20-cli.md) |
| RF-21 | Client connectors in ten languages | 🟡 | Rust and C# ready; eight planned ([21-connectors.md](21-connectors.md)) |
| RF-22 | OpenAPI contract for the client API | ⬜ | [32-roadmap.md](32-roadmap.md) |

### Non-functional requirements

| Code | Requirement | State | How it is met |
|---|---|---|---|
| RNF-01 | **Simple API**: few HTTP routes, no new protocol | ✅ | [15-api-overview.md](15-api-overview.md) |
| RNF-02 | **Speed**: the critical path goes through memory; disk only for overflow | ✅ (no published benchmark) | [10-publishing.md](10-publishing.md) |
| RNF-03 | **Simple scalability**: a new node joins without complex manual configuration | ✅ | [07-cluster.md](07-cluster.md) |
| RNF-04 | **Simple configuration**: sensible defaults, everything by `.yml` file or `TABA_*` | ✅ | [22-configuration.md](22-configuration.md) |
| RNF-05 | **A single, light Docker image** | ✅ | [27-ci-cd.md](27-ci-cd.md) |
| RNF-06 | **No external dependencies** in the server (embedded SQLite) and in the connectors | ✅ | [05-dependencies.md](05-dependencies.md) |
| RNF-07 | **No loss of a confirmed message** when a node goes down | ✅ (with `replication.factor` ≥ 2) | [11-durability.md](11-durability.md) |
| RNF-08 | **Configurable durability**: the user chooses the replication factor and the quorum | ✅ | [11-durability.md](11-durability.md) |
| RNF-09 | **Connectors on old versions** (up to 15 years), as long as they speak TLS 1.2 | 🟡 | Only the two existing connectors ([21-connectors.md](21-connectors.md)) |
| RNF-10 | **Kubernetes as a first-class citizen** | 🟡 | Probes and stop with drain exist and there is a Helm chart ([34-helm-chart.md](34-helm-chart.md)) tested on a kind cluster; not yet validated on a production cluster ([32-roadmap.md](32-roadmap.md)) |
| RNF-11 | **Observable** by default: OpenTelemetry | ✅ | [13-observability.md](13-observability.md) |

> **Speed and cost are goals.** There is no published benchmark; the ✅ on RNF-02 says that the design puts memory on the critical path, not that there is a measurement.

## Design principles

These eight principles explain many choices in the code. Each one points to where the effect can be seen.

1. **Simplicity first.** Fewer concepts, less configuration, sensible defaults.
2. **Speed by default.** The critical path (publish, deliver) goes through memory.
3. **Configurable durability.** The write quorum (`write.quorum`) decides the commit without waiting for all the replicas ([11-durability.md](11-durability.md)).
4. **Automatic operation.** The cluster organizes itself ([07-cluster.md](07-cluster.md)).
5. **One protocol, several uses.** The same two-phase register + commit serves messages (decided by the **quorum**) and membership changes (decided by **all the nodes**).
6. **An isolated failure does not become a health failure.** Losing a message during a synchronization generates an event; only the failure of the whole node leads to exclusion.
7. **Resilience to instability.** Retries before voting and the freezing period avoid reacting to passing failures.
8. **Memory for whoever uses it most.** Memory is not divided equally among topics: the one with more reads keeps more "hot" messages.

## Comparison with alternatives

| | **Taba** | SQS + SNS | Kafka | RabbitMQ |
|---|---|---|---|---|
| Where it runs | Anywhere with Docker | Only on AWS | Where you host it, or managed | Where you host it, or managed |
| What to start | The same image, 3 nodes (1 for dev) | Nothing | Brokers and coordination | Broker (more nodes for high availability) |
| Queue and pub/sub | In the same HTTP API | Two services | Topics and consumer groups | Queues and *exchanges* |
| Billing | Only the infrastructure | Per request | Infrastructure or plan | Infrastructure or plan |
| Protocol | HTTP/JSON | HTTP/JSON | Its own binary | AMQP and others |

## State of the project

- **Server:** node and cluster implemented in the `taba-node` crate; unit tests, single-node tests and cluster tests (in-memory network), plus tests over real TCP and TLS.
- **End-to-end validation:** `docker/validate.sh` starts Compose from scratch and checks cluster formation, the `taba-cli`, nodes going down and coming back, and the absence of loss of confirmed messages.
- **Version:** `0.1.0` (`Cargo.toml`). Licenses: the server is **AGPL-3.0-only** and the connectors (`connectors/`) are **Apache-2.0**, with a CLA for contributions ([30-decisions.md](30-decisions.md#license-and-contributions-d164)).
- **Missing:** OpenAPI contract, the other eight connectors, package publishing and validation on Kubernetes ([32-roadmap.md](32-roadmap.md)).

## Sources and evidence

- Code: `crates/taba-node/src/lib.rs` (module map), `Cargo.toml`, `docker-compose.yml`, `docker/validate.sh`.
- Use cases: the catalogue is in [26a-implemented-use-cases.md](26a-implemented-use-cases.md); the areas cited per requirement appear in the detail documents.
- History: `01-visao-geral.md`, `03-escopo.md`, `07-principios-design.md`.
