---
title: Known issues
summary: The cluster tests turned off with #[ignore] because they fail intermittently on the CI runners, with the symptom, the known cause and the planned fix of each; they will be fixed in the next version.
type: reference
status: partial
verified-on: 2026-10-19 @ 879f956
sources: [crates/taba-node/tests/failures.rs, crates/taba-node/tests/replication.rs, crates/taba-node/tests/common/cluster.rs]
---

# Known issues

> Read first: [25-testing.md](25-testing.md).

In 0.1.0, three cluster tests are marked `#[ignore]`: they pass on the development machine but fail intermittently on the GitHub Actions runners, which are slow and shared. All of them will be fixed in the next version. To run them by hand: `cargo test -p taba-node --test <file> <name> -- --ignored`.

## A healthy channel that does not come back (`NO_VOTE`)

| Test | Symptom in CI |
|---|---|
| `failures.rs` › `the_messages_of_the_excluded_node_wait_and_are_delivered_by_the_new_primary` | consuming through node 1 stops halfway (e.g. 16 of 28 messages), or "waited 30 s for: the node to join the cluster" |
| `replication.rs` › `with_a_node_down_only_the_publications_it_owns_fail_and_the_replica_catches_up` | publications owned by healthy nodes fail while one node is down |

- **Cause (confirmed).** `NO_VOTE` in `tests/common/cluster.rs` prevents the exclusion vote by raising the reconnection backoff of **every** channel to 600 s, not only the downed node's. On a slow runner a **healthy** channel drops after 500 ms without a keepalive and would only come back 10 minutes later; `take` and the signals between two healthy nodes stop. A simulated 100 ms drop reproduces the failure locally.
- **Planned fix.** Turn the vote off without touching the backoff (a very high `TABA_CLUSTER_CHANNEL_RECONNECT_RETRIES` and a high `TABA_CLUSTER_CHANNEL_QUEUE_MAX_AGE`). It fixes both failures, but exposes a rare hang (≈ 1 in 25 runs) in `cluster.shutdown()` of two `replication.rs` tests after the isolated node comes back: the process stops entirely, which suggests a synchronous deadlock. It must be understood before the fix goes in.
- **Product risk found in the analysis.** A lock-order inversion: `build_batch` (`src/cluster/channel/mod.rs`) holds the peer lock and calls the signals; `ready_peers` (`src/cluster/replication/signals.rs`) holds the tables lock and calls `is_live`, which takes the peer lock. On the production multi-thread runtime this can deadlock. Fix: collect the candidates under the tables lock, release it, and only then filter with `is_live`.

## Rejoining after a symmetric partition

| Test | Symptom in CI |
|---|---|
| `failures.rs` › `after_a_partition_where_each_side_excluded_the_other_the_losing_side_joins_again` | "waited 30 s for: the four nodes in a single table" |

- **When.** It failed in both CI runs of PR #3 on the amd64 runner (2026-10-19); it passed on arm64 and on the `release` push with the same code. The PR only changed `release.yml`, so the failure does not come from it.
- **Cause.** Not investigated. The test splits the cluster 2 + 2, lets each side exclude the other, heals the network and advances the clock 301 s; the losing side (the one with the lowest `nodeId`) should redo the join and the four tables should become one again within 30 s.
- **Next step.** Find out whether it is the same kind of fragility as the tests above (channels dropping for lack of keepalive on a slow runner) or a real problem in rejoining after a partition.

## Sources and evidence

- The tests cited and `tests/common/cluster.rs` (`NO_VOTE`, `FAST`, `WAIT`).
- The failures were seen in the repository's CI logs; the `NO_VOTE` cause was reproduced locally with a simulated 100 ms drop on the channel between nodes 1 and 2.
