---
title: Guarantees and how they are checked
summary: The guarantees Taba states in a form a machine can check (I1 to I6 and the assumed risk R1), the chaos run that checks them against a recorded history, which faults it applies, what it does not cover yet and how to run or sweep it.
type: reference
status: partial
verified-on: 2026-10-27 @ ada9ff1
sources: [crates/taba-node/tests/chaos.rs, crates/taba-node/tests/common/history.rs, crates/taba-node/src/cluster/transport/memory/mod.rs, crates/taba-node/tests/common/cluster.rs, .github/workflows/chaos.yml]
---

# Guarantees and how they are checked

> Read first: [11-durability.md](11-durability.md) and [12-delivery.md](12-delivery.md).

[11-durability.md](11-durability.md) says in prose what survives each failure. A test can only prove something when it compares the result with a list of promises it can check by machine. This document is that list, and says which of them a **chaos run** already checks, with which faults, and what is still outside it.

## The guarantees

They hold for the default cluster (3 nodes, `replication.factor=3`, `write.quorum=2`) with **at most `write.quorum − 1` impaired nodes at a time**.

| Code | Guarantee | Checked by the chaos run |
|---|---|:-:|
| **I1** No loss | Every publication that received `201` is delivered at least once, unless it expires (`ttl`), is acked or goes to the dead-letter | ✅ |
| **I2** No phantom | Nothing is delivered that no publication attempt explains, and a delivered ID is the one the `201` returned | ✅ |
| **I3** Definitive ack | A message whose ack answered `204` is not handed out to a request that started after the ack | ✅ |
| **I4** FIFO | In a FIFO queue the first deliveries of a group follow the order of publication, and a publication is stored once | ✅ |
| **I5** Dead-letter | A message goes to the dead-letter only after `maxDeliveries` deliveries, is never in both places and is not delivered more than `maxDeliveries` times | ✅ |
| **I6** Declared loss | Every loss appears in `/cluster/message-loss`, never in silence: a confirmed message that is not delivered must have been declared | ✅ |
| **R1** Assumed risk | A partition with enough nodes on both sides lets each side exclude the other and go on ([07-cluster.md](07-cluster.md), "Accepted risk"): writes may happen on both sides and be duplicated or lost on the merge | ✅ measured by `tests/partition.rs`: a 2 + 2 partition of 4 nodes, writes on both halves, the reunion; every write answered `201` is delivered once (D181). The default `cluster.vote.min-voters` stays open (D182) |

Two things the guarantees do not say: delivery is **at-least-once**, so a message handed out without an ack comes back and a lost ack repeats a delivery (that is not a violation of I3: the ack did not answer `204`); and after a stop of the whole cluster the **residual redelivery** of [07-cluster.md](07-cluster.md) (section 10.10) is the documented exception to I3.

## Checking a history

Each client records what it did, with a logical instant for the start and for the end of every operation. An operation ends in one of three ways:

| Outcome | When | What the checker demands |
|---|---|---|
| `Ok` | `2xx` | I1 demands the delivery of a published message; I3 forbids a new delivery after an acked one |
| `Fail` | a definitive `4xx` | nothing |
| `Indeterminate` | timeout, closed connection, `5xx`, `408`, `429` | nothing is demanded (it may have happened), but whatever is delivered is explained by it (I2) |

A client never knows more than what the API answered, so the checker only demands what was confirmed and only forbids what no attempt can explain. The history lives in `tests/common/history.rs`; `History::check` returns one line per violation, starting with the code of the guarantee. The checker has its own tests that build a bad history for each guarantee and demand the violation: a checker that cannot fail proves nothing.

## The chaos run

`tests/chaos.rs` starts three nodes (`NO_VOTE`: no exclusion vote, so the table stays stable and I1 can be demanded), creates a plain queue and a FIFO queue with three groups, and runs together:

- 2 publishers on the plain queue and 3 on the FIFO queue (one per group, one message at a time), each going through the nodes in turn;
- a third queue with `maxDeliveries: 2` and its dead-letter queue: one publisher, and a consumer that never acks the poison messages (`seq % 3 == 0`) so they must end in the dead-letter;
- 6 consumers (2 plain, 2 FIFO, 1 for the queue with the dead-letter, 1 for the dead-letter itself) that get up to 5 messages with a 5 s invisibility, record the delivery and ack it;
- the **nemesis**, which every 100 to 300 ms draws a fault from the seed. Each fault is applied alone (the previous one is healed first):

| Fault | In `MemoryNetwork` |
|---|---|
| Isolate a node | `isolate` |
| Cut a pair, both directions | `partition` |
| Cut one direction only | `cut_one_way` |
| The call runs, the reply never gets back | `lose_replies` |
| Lose 40% of one direction | `set_link_loss` (drawn from the seed) |
| 15 ms on every call | `set_delay` |
| A node that accepts and never answers | `set_silent` |
| The clock jumps 8 s (the 5 s invisibility ends, deadlines move) | `ManualClock::advance` |

At the end the publishers stop, every fault is healed (`heal_all`), the clock advances until the invisibility of what was handed out during a fault ends, and the consumers drain the queues. Then the history is checked against what each node declared lost.

**The same seed draws the same faults in the same order.** The interleaving with the clients runs in real time and is not repeatable bit by bit, so a failing seed may need a few runs to fail again; the failure message prints the seed, the counts and the list of faults.

### What it does not cover yet

| Outside the run | Why |
|---|---|
| Abrupt death and restart of a node, drain during the run | not in the nemesis yet |
| Slow or failing disk | the storage has no hook for it yet |
| Real TCP, mutual TLS, kernel timeouts, real disk | the in-memory network does not reproduce them (planned: Docker run) |
| A stall longer than `write.pending.ttl` (30 s) in the middle of an operation, or a clock jump past it | the jumps of the run (8 s) stay below it; with 35 s jumps the run finds a message acked and then delivered again from the dead-letter ([PRB-077](31-pending-items.md)) |
| Two nodes impaired at the same time, or a partition with a majority on both sides (R1) | outside what the default cluster promises, and `NO_VOTE` hides the exclusion vote; R1 has its own test (`tests/partition.rs`, see its row) |

## How to run

```bash
cargo test --test chaos                      # the fixed seeds (1 to 4) and the tests of the checker
TABA_CHAOS_SEEDS=1..200 cargo test --test chaos a_range_of_seeds -- --nocapture
```

The second form is what `.github/workflows/chaos.yml` runs every night (outside the required checks). A seed that fails becomes a test of its own in `tests/chaos.rs`, and the bug it found follows the usual path: a `fix:` with the test, an entry in [36-known-issues.md](36-known-issues.md) and the candidate validated with `docker/validate.sh`.

## What the run found

Three bugs of the server, all fixed in the same change as the run ([D180](30-decisions.md), [36-known-issues.md](36-known-issues.md#the-chaos-run-a-delivery-past-the-limit-and-an-ack-the-owner-never-applied)):

- a message delivered **one time past `maxDeliveries`** (I5) when the commit of its previous delivery reached the owner after the invisibility had ended;
- a message left **only on its owner**, blocking its FIFO group for good (I1), when an ack that the replicas had applied was refused by the owner after the coordinator registered it again.
- the writes of the half of a 2 + 2 partition that loses the reunion, **wiped** when it joins the other half again (R1, found by the measurement of `tests/partition.rs`, D181).

## Sources and evidence

- `crates/taba-node/tests/chaos.rs` (the run, the nemesis, the tests of the checker), `tests/common/history.rs` (the history, `History::check`, the seeded generator) and `src/cluster/transport/memory/mod.rs` (`cut_one_way`, `lose_replies`, `set_link_loss`, `seed_links`, `heal_all`).
- Use cases: `UC-CHS-001` to `UC-CHS-005` in [26a](26a-implemented-use-cases.md).
- Measured when written: seeds 1 to 24 ran without a violation, with 48 to 578 confirmed publications and up to 13 redeliveries per run.
