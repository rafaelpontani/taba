---
title: Use cases and traceability
summary: The format of the `UC-AREA-NNN` code, the test levels, how a test cites a case, the coverage rule that `uc_coverage` checks, the index of the 38 areas and how to read the two catalogues (implemented and planned).
type: guide
status: stable
verified-on: 2026-10-14 @ efeb47a
sources: [crates/taba-node/tests/uc_coverage.rs, doc/en-US/26a-implemented-use-cases.md, doc/en-US/26b-planned-use-cases.md]
---

# Use cases and traceability

> This document explains **the method**. The cases themselves are in two generated catalogues: [26a — implemented](26a-implemented-use-cases.md) (with a test) and [26b — planned](26b-planned-use-cases.md) (no test yet).

## What it is for

Each **observable behavior** of Taba is a **use case** with a **unique code**. The tests **cite** the code, and so two things can be proved: **every case is covered by at least one test** and **no test covers something that is not documented**. When the technical text changes, the case changes with it, in the same commit.

The cases describe **behavior**, not implementation. The "result" is what a test checks: a status, a body, a disk state, a metric, a log line, or what the other node sees.

## The code

```
UC-<AREA>-<NNN>        example: UC-MSG-003
```

- `<AREA>`: **three uppercase letters** (table below). `<NNN>`: **three digits**, sequential **within the area**.
- **Codes are never reused.** A case that ceases to exist stays `(retirado…)` in the table, **without deleting the row**. A new case gets the **next free number of the area**, even out of subject order. A case split in two becomes **two new codes** and the original is retired.

## The levels

| Acronym | Level | Where |
|---|---|---|
| **U** | Unit | `src/**/tests.rs` |
| **I** | One node (development mode, real HTTP and disk) | `tests/*.rs` with `TestNode` |
| **C** | Simulated cluster (in-memory network and manual clock) | `tests/*.rs` with `TestCluster` |
| **K** | Connector conformance | `connectors/<language>/…` |

Rule of thumb: what depends **only on the client and one node** is `I`; what depends on **more than one node, failure or time** is `C`; a pure rule is `U`. A case can ask for two levels (`U, I`) and then needs **one test at each**. Details in [25-testing.md](25-testing.md).

## How a test cites a case

A line **immediately above** the test function, with one or more codes separated by commas:

```rust
/// UC: UC-MSG-003, UC-ESC-004
#[test]
fn publishing_binary_stores_the_raw_bytes() { /* … */ }
```

- The prefix is `/// UC:` (triple slashes, including in C#) and the codes are **complete** (`UC-MSG-003`, never `MSG-003`). Attributes (`#[…]`) and other comment lines between the citation and the function are ignored.
- A test can cover several cases, and a case can have several tests. A case only counts as **covered** with a test **at the level** of the "Level" column.
- You do not cite a code that does not exist.

## What `uc_coverage` checks

`crates/taba-node/tests/uc_coverage.rs` reads the codes from the **cards** (`### UC-XXX-NNN`) of [26a](26a-implemented-use-cases.md) and [26b](26b-planned-use-cases.md) and those cited in `crates/taba-node/{src,tests}` and in `connectors/*` (`.rs` and `.cs` files). It fails when:

1. a test cites a code that **does not exist** or is `(retirado…)`;
2. a code appears **twice** in the documentation.

The rule "every documented case has a test" **only fails with `TABA_UC_STRICT=1`**; without the variable the test prints the scoreboard (`cargo test --test uc_coverage -- --nocapture`). The check **does not verify the level**: citing the code in a unit test does not prove a level `I` case; whoever reviews looks at the level on the card.

## How to maintain the catalogues

| Catalogue | What it is |
|---|---|
| [26a](26a-implemented-use-cases.md) | Cases with at least one test, by area, with the level and the tests that prove them |
| [26b](26b-planned-use-cases.md) | Cases **without a test**: what is left to prove |

Both are **maintained by hand**, in both languages: a new case goes in as a card in the format of the others (in 26a if it has a test, in 26b if not), and a case that gets its first test moves from 26b to 26a. Codes are never reused. A case in 26b **does not mean the behavior does not exist**, only that **no test cites it**. Some have "(pendente: D##)" in the text: they depend on an open question ([31-pending-items.md](31-pending-items.md)).

## The areas (38)

**Client** (the API that applications use):

| Area | Subject |
|---|---|
| `TOP` | Topics: create, list, read, change, remove |
| `MSG` | Publishing and the message body |
| `CNS` | Consumption, long polling and `autoCreate` |
| `ACK` | Confirmation of consumption |
| `VIS` | Change of invisibility |
| `RET` | Expiration (`ttl`), delivery limit and dead-letter |
| `DST` | Distributor topic |
| `VOL` | Volatile queue and managing node |
| `FIF` | FIFO queue |
| `GRP` | Message groups and fair delivery |
| `AUT` | Authentication and access tokens |
| `LIM` | Rate limit and client IP |
| `PAG` | Pagination |
| `CNV` | Common API conventions (format, headers, errors, status) |
| `CNT` | Connectors |
| `CLI` | `taba-cli` |

**Node and the message path:**

| Area | Subject |
|---|---|
| `ESC` | Write: register + commit, quorum, rollback, commit ceiling |
| `ANE` | Hash ring, blocks and roles |
| `DEF` | Topic and token definition replicated in the cluster |
| `ENT` | Delivery on the owner: ready list, `take`, long polling |
| `CAN` | Permanent channel between nodes and signal queues |
| `INV` | Invisibility on the owner and on the replicas |
| `ARM` | Storage: memory, overflow, SQLite, integrity |

**Node and cluster:**

| Area | Subject |
|---|---|
| `ING` | Node join (bootstrap) |
| `FAL` | Failure detection, voting and exit by failure |
| `FRZ` | Freezing period |
| `CNC` | Concurrency between membership changes |
| `SIN` | Distributed synchronization of blocks |
| `STR` | NDJSON streams between nodes |
| `RTN` | Return of an excluded node and recovery of losses |
| `FWD` | Forwarding of client calls |
| `DRN` | Stop and drain (includes drain as save) |
| `DEV` | Development mode |
| `SEG` | Ports, mutual TLS and cluster certificates |
| `OPS` | Observability, statistics, metrics and health |
| `CLR` | Contract of the cluster routes |
| `CFG` | Configuration and startup validations |
| `BKP` | Optional S3 backup: upload, restore, heartbeat, eviction, flush and stop |

(The number of cases per area and the coverage are in [26a](26a-implemented-use-cases.md) and [26b](26b-planned-use-cases.md).)

## Assumptions and limits of the catalogue

- Parameters with no fixed value in the original design documents ("To be defined") **have no default fixed by the tests**: the tests configure the value they need.
- Cases that depend on an open pending item (D68, D70.1, D70.2: connectors) are marked **(pendente: D##)**: the expected result may change when the question is decided.
- The names of routes, fields and codes follow [15](15-api-overview.md) to [19](19-errors-and-limits.md) and will follow the OpenAPI contract when it exists.
- The cases **do not cover performance** (no benchmark) nor the content of the OpenAPI contract (which does not exist).

## Sources and evidence

- `crates/taba-node/tests/uc_coverage.rs` (the rules and the scoreboard; **run** on 2026-10-13).
- Use cases: this document is the method; the cases are in [26a](26a-implemented-use-cases.md) and [26b](26b-planned-use-cases.md).
- History: `doc/20-casos-de-uso.md`, `doc/20a-casos-de-uso-cliente.md`, `20b`, `20c`.
