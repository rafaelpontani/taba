---
title: Concepts
summary: The mental model of Taba on one page — node, cluster, topic, message, block, ring, quorum, invisibility, ack — each term defined once, with a link to the detail.
type: explanation
status: stable
verified-on: 2026-10-13 @ 71183b9
sources: [crates/taba-node/src/core, crates/taba-node/src/engine, crates/taba-node/src/cluster]
---

# Concepts

This is the minimum vocabulary to read the rest of the documentation. Each term is defined here **once**; the other files only use it. The complete list, in alphabetical order, is in [28-glossary.md](28-glossary.md).

## In one sentence

Applications **publish messages** to **topics**; Taba stores them on several **nodes** of a **cluster** (spreading them over a **hash ring**), delivers each one to **one consumer** and deletes it when the consumer **confirms** it; if the consumer does not confirm in time, the message comes back.

## Who is who

| Term | What it is |
|---|---|
| **Node** | One instance of the `taba` program. They are all equal; there is no master. Each node has an **ID** (GUID v7) generated at every start. |
| **Cluster** | The set of nodes that know each other. Minimum in production: **3**. With **1** node, only in the **one-node profile**. |
| **Member table** | The list of nodes in the cluster (IDs and addresses), with a **version** that goes up at every change. Every node has a copy. |
| **Client** | Any application that talks to the client API (producer or consumer). |
| **Operator** | Whoever runs the cluster. Talks to the operations API and to the command line. |

## What is stored

| Term | What it is |
|---|---|
| **Topic** | A named channel. It has a **type**: **queue** (`queue`) or **distributor** (`distributor`). It has an `id` (GUID v7) that identifies the "incarnation": recreating the same name generates another `id`. |
| **Queue** | A topic that stores messages and delivers them, one to each consumer. It can be **volatile** (disappears by itself if it is left without a consumer) and/or **FIFO** (order per group). |
| **Distributor** | A topic that **does not store** messages: it copies each publication to the queues whose name starts with one of its **prefixes**. |
| **Message** | A body (bytes, normally JSON) plus metadata: `id` (GUID v7, sortable by time), `contentType`, `publishedAt` and, when delivered, `deliveryCount`, `invisibleUntil` and `receiptToken`. |
| **Dead-letter** | The `{queue}-dead-letter` queue, created with each queue, where a message goes when it reaches the delivery limit without an ack. |
| **Token** | A client API credential, with permissions and topic prefixes. The **root token** has full access. |

## How a message is spread

| Term | What it is |
|---|---|
| **Hash ring** | A circle of 64-bit positions. Each node occupies several positions (**virtual nodes**, default 128). The position of a message comes from the hash of its `id`. |
| **Block** | The arc of the ring that ends at a virtual-node position. Every message whose position falls in the arc belongs to the block. |
| **Primary / secondary / tertiary** | The nodes responsible for a block, in order along the ring, always **different physical nodes**. The **primary** is the **delivery owner**: only it puts the message in the ready list. |
| **Replication factor** | How many nodes store each message (`replication.factor`, default **3**). |
| **Write quorum** | How many confirmations are needed to consider a publication done (`write.quorum`, default majority: 2 of 3). The primary always counts. |
| **Register + commit** | The two-phase protocol of the write: the coordinator sends the message to all the responsible nodes (**register**, stored as pending) and, once the quorum is reached, sends **commit**. |
| **Coordinator** | The node that received the client's call. Any node can be the coordinator. |

## How a message is delivered

| Term | What it is |
|---|---|
| **Long polling** | The `GET` that waits (up to 20 s) for messages instead of answering empty. |
| **Ready list** | Per topic, on the owner: the IDs of the next messages to deliver. |
| **Invisibility** | After delivery, the message stays **invisible** until `invisibleUntil` (default 120 s after delivery). If there is no ack by then, it returns to the ready list with `deliveryCount` +1. |
| **Ack** | `DELETE` of the message with the received `receiptToken`: confirms consumption and deletes the message. |
| **Group (`groupId`)** | A label on the message. In **FIFO** it defines the order (one message per group in flight). In the other queues it serves **fair delivery**. |

## How the cluster sustains itself

| Term | What it is |
|---|---|
| **Permanent channel** | A WebSocket connection between each pair of nodes, which carries the **heartbeat** and delivery notices. It does not carry messages. |
| **Exclusion vote** | When the channel with a node drops and does not come back, the healthy nodes vote (51%) to remove it. |
| **Freezing** | A period in which a node excluded for failure cannot return, to avoid instability. |
| **Synchronization** | What a new node does when it joins: it pulls topics and blocks from the nodes that already have them, storing in an **event database** whatever arrives in the meantime. Only then does it become **ready**. |
| **Drain** | Leaving the cluster without losing what is only in memory: everything goes to disk and the node deregisters itself. |
| **One-node profile** | `replication.factor=1`: a one-node cluster that starts alone and is the primary of everything; the path is the production one, with quorum 1. |

## The three ports

| Port | Default | Who uses it | What it serves |
|---|---|---|---|
| **Client** (`api.port`) | 8080 | Applications | Topics, messages, tokens |
| **Cluster** (`cluster.port`) | 8081 | Other nodes | `/cluster/*` routes and the permanent channel; mutual TLS |
| **Operations** (`operations.port`) | no default (without the key, the operations routes are on the client port) | Operator, load balancer, Prometheus | `/health/*`, `/metrics*`, drain |

Details in [15-api-overview.md](15-api-overview.md).

## A dependency map of the concepts

![How cluster, node, ring, block, topic and message connect](../diagrams/img/en-US/entities.svg)

## Where to go next

| I want to understand… | Read |
|---|---|
| How the node is built inside | [04-architecture.md](04-architecture.md) |
| The ring, the blocks and the roles | [06-ring-and-replication.md](06-ring-and-replication.md) |
| How the cluster joins, fails and returns | [07-cluster.md](07-cluster.md) |
| The topic types | [09-topics.md](09-topics.md) |
| Publishing and delivering | [10-publishing.md](10-publishing.md), [12-delivery.md](12-delivery.md) |

## Sources and evidence

- `crates/taba-node/src/core/ring` (position and blocks), `core/topic`, `core/id` (GUID v7), `cluster/members`, `cluster/state`, `engine/messages`.
- Use cases: UC-ANE (ring), UC-ESC (write), UC-ENT (delivery), UC-INV (invisibility); see [26a](26a-implemented-use-cases.md).
- History: `09-glossario.md`, `16-mapa-mental.md`.
