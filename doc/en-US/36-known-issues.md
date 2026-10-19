---
title: Known issues
summary: Log, with history, of the intermittent cluster tests and known issues: each with a title, status, date found, date fixed and the history of occurrences and attempts.
type: reference
status: partial
verified-on: 2026-10-19 @ 294fd03
sources: [crates/taba-node/tests/failures.rs, crates/taba-node/tests/replication.rs, crates/taba-node/tests/common/cluster.rs]
---

# Known issues

> Read first: [25-testing.md](25-testing.md).

Each intermittent test or known issue has its own section, **which is never deleted**: when the issue is solved it becomes `Fixed`; if it comes back, it becomes `Reopened` and the history continues in the same section. Tests marked `#[ignore]` pass on the development machine but failed intermittently on the GitHub Actions runners, which are slow and shared. To run one by hand: `cargo test -p taba-node --test <file> <name> -- --ignored`.

Possible statuses: `Open`, `Fixed`, `Reopened`. The rules for handling intermittent tests are in [AGENTS.md](../../AGENTS.md).

| Issue | Status | Found on | Fixed on |
|---|---|---|---|
| [Rejoining after a symmetric partition](#rejoining-after-a-symmetric-partition) | Open | 2026-10-19 | — |
| [A healthy channel that does not come back](#a-healthy-channel-that-does-not-come-back) | Fixed | 2026-10-18 | 2026-10-19 |
| [Lock-order inversion when choosing replicas](#lock-order-inversion-when-choosing-replicas) | Fixed | 2026-10-18 | 2026-10-18 |

## Rejoining after a symmetric partition

**Status:** Open · **Found on:** 2026-10-19 · **Fixed on:** —

- **Test:** `failures.rs` › `after_a_partition_where_each_side_excluded_the_other_the_losing_side_joins_again` (`#[ignore]`).
- **Symptom in CI:** "waited 30 s for: the four nodes in a single table".
- **Cause:** not identified. The test splits the cluster 2 + 2, lets each side exclude the other, heals the network and advances the clock 301 s; the losing side (the one with the lowest `nodeId`) should redo the join and the four tables should become one again within 30 s.
- **Next step:** capture the nodes' log on the amd64 runner when it fails (each node's tables and channels at the time of the timeout) to tell a fragile test (a race between `clear_calls` and the clock advance) from a real problem in rejoining.

**History**

| Date | Event |
|---|---|
| 2026-10-19 | Failed in both CI runs of PR #3 on the amd64 runner; passed on arm64 and on the `release` push with the same code (the PR only changed `release.yml`). Test marked `#[ignore]`. |
| 2026-10-19 | Investigation: it is not `NO_VOTE` (the test does not use it) nor the lock-order inversion. Did not reproduce locally: passed in 12 and in 30 simultaneous runs of the same binary. |

## A healthy channel that does not come back

**Status:** Fixed · **Found on:** 2026-10-18 · **Fixed on:** 2026-10-19

- **Tests:** `failures.rs` › `the_messages_of_the_excluded_node_wait_and_are_delivered_by_the_new_primary` and `replication.rs` › `with_a_node_down_only_the_publications_it_owns_fail_and_the_replica_catches_up`.
- **Symptom in CI:** consuming through node 1 stops halfway (e.g. 16 of 28 messages), "waited 30 s for: the node to join the cluster", or publications owned by healthy nodes failing while one node is down.
- **Cause (confirmed):** `NO_VOTE` in `tests/common/cluster.rs` prevented the exclusion vote by raising the reconnection backoff of **every** channel to 600 s. On a slow runner a healthy channel dropped after 500 ms without a keepalive and would only come back 10 minutes later; `take` and the signals between two healthy nodes stopped. A simulated 100 ms drop reproduced the failure locally.
- **Fix:** `NO_VOTE` turns the vote off with `TABA_CLUSTER_CHANNEL_RECONNECT_RETRIES=100000` and `TABA_CLUSTER_CHANNEL_QUEUE_MAX_AGE=600s`, without touching the backoff.

**History**

| Date | Event |
|---|---|
| 2026-10-18 | Failures seen in CI; both tests marked `#[ignore]`. Cause reproduced locally with a simulated 100 ms drop between nodes 1 and 2. |
| 2026-10-19 | Fix applied to `NO_VOTE` and `#[ignore]` removed from both tests. Verification: the six test files that use `NO_VOTE` passed 6 times in a row with six processes in parallel (36 runs). The rare hang in `cluster.shutdown()` (≈ 1 in 25), seen in an earlier attempt, did not appear. |

## Lock-order inversion when choosing replicas

**Status:** Fixed · **Found on:** 2026-10-18 · **Fixed on:** 2026-10-18

- **Where:** `src/cluster/channel/mod.rs` (`build_batch`) and `src/cluster/replication/signals.rs` (`ready_peers`).
- **Risk:** `build_batch` holds the peer lock and calls the signals; `ready_peers` held the tables lock and called `is_live`, which takes the peer lock. On the production multi-thread runtime this could deadlock.
- **Fix:** collect the candidates under the tables lock, release it, and only then filter with `is_live` (commit `a9ecf01`).

**History**

| Date | Event |
|---|---|
| 2026-10-18 | Found while analysing `NO_VOTE`. Fixed the same day (`a9ecf01`). |

## Sources and evidence

- The tests cited and `tests/common/cluster.rs` (`NO_VOTE`, `FAST`, `WAIT`).
- The failures were seen in the repository's CI logs; the "found" dates come from the git history and the logs.
