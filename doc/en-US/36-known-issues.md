---
title: Known issues
summary: Log, with history, of the intermittent cluster tests and known issues: each with a title, status, date found, date fixed and the history of occurrences and attempts.
type: reference
status: partial
verified-on: 2026-10-26 @ 75012bc
sources: [crates/taba-node/src/cluster/channel/mod.rs, crates/taba-node/tests/channel.rs, crates/taba-node/tests/failures.rs, crates/taba-node/tests/replication.rs, crates/taba-node/tests/notify.rs, crates/taba-node/tests/common/cluster.rs, crates/taba-node/src/cluster/sync/mod.rs, crates/taba-node/src/cluster/service/mod.rs]
---

# Known issues

> Read first: [25-testing.md](25-testing.md).

Each intermittent test or known issue has its own section, **which is never deleted**: when the issue is solved it becomes `Fixed`; if it comes back, it becomes `Reopened` and the history continues in the same section. Tests marked `#[ignore]` pass on the development machine but failed intermittently on the GitHub Actions runners, which are slow and shared. To run one by hand: `cargo test -p taba-node --test <file> <name> -- --ignored`.

Possible statuses: `Open`, `Fixed`, `Reopened`. The rules for handling intermittent tests are in [AGENTS.md](../../AGENTS.md).

| Issue | Status | Found on | Fixed on |
|---|---|---|---|
| [A channel accept that locked the peers twice](#a-channel-accept-that-locked-the-peers-twice) | Fixed | 2026-10-26 | 2026-10-26 |
| [A FIFO restart test that waits 30 s in a combined run](#a-fifo-restart-test-that-waits-30-s-in-a-combined-run) | Open | 2026-10-22 | — |
| [A notice test that waits 30 s once in the full suite](#a-notice-test-that-waits-30-s-once-in-the-full-suite) | Open | 2026-10-20 | — |
| [Rejoining after a symmetric partition](#rejoining-after-a-symmetric-partition) | Fixed | 2026-10-19 | 2026-10-20 |
| [A healthy channel that does not come back](#a-healthy-channel-that-does-not-come-back) | Fixed | 2026-10-18 | 2026-10-19 |
| [Lock-order inversion when choosing replicas](#lock-order-inversion-when-choosing-replicas) | Fixed | 2026-10-18 | 2026-10-18 |

## A channel accept that locked the peers twice

**Status:** Fixed · **Found on:** 2026-10-26 · **Fixed on:** 2026-10-26

- **Where:** `src/cluster/channel/mod.rs` (`Channels::accept`).
- **Symptom:** in the CI run of the server on amd64 the last test of the `nodes_api` binary stayed at "has been running for over 60 seconds", with no output and no failure message. It was `a_node_that_does_not_answer_makes_the_consolidated_reads_partial_and_says_which` in one run and `a_prune_that_fails_on_a_node_names_it_and_repeating_it_is_safe` in another: both isolate node 3 and bring it back. It does not reproduce alone: 3 hangs in 120 runs of the binary, every one under load (several processes at once).
- **Cause:** `accept` looked the peer up with `match self.lock().get(from).cloned() { … None => { self.sync_members(); … } }`. The guard of `lock()` is a temporary in the scrutinee, and a temporary in the scrutinee of a `match` lives until the end of the whole `match`. `sync_members` (and `drop_all`, which it calls) lock the same `std::sync::Mutex` again, which is not reentrant: the thread blocks on itself forever. It only happens when a peer opens a channel that the node does not know yet (a race after the network comes back or while a node drains). On the production runtime a worker thread is lost and the lock of the peers is held for good, so every other task that touches the channels hangs: the node stays alive and stops answering the cluster. In the tests (`current_thread` runtime) nothing moves any more, **not even the timeouts**, which is why the log went silent.
- **How it was found:** with `TABA_TEST_LOG=taba_node=trace` the log of the hung run ended in a `drain started` that never finished, and not even the `drain.timeout` (25 s, which logs when it runs out) showed up: the whole runtime was stuck, not waiting. The stack of every thread, taken with `gdb` in a Linux container (recipe in [25-testing.md](25-testing.md#investigating-a-test-that-hangs-or-fails-only-sometimes)), showed `accept → sync_members → drop_all → Channels::lock` blocked.
- **Fix:** the lookup is its own statement (`let known = self.lock().get(from).cloned();`) and the guard is gone before `sync_members` runs; `receive` already did it this way. The same shape was removed from three `if let` that held a guard in the scrutinee (`allow_new_vote`, `Backup::usage`, `Sync::abandon`) and `clippy::significant_drop_in_scrutinee` is now on for the workspace (D179).
- **Test:** `tests/channel.rs::a_channel_from_a_node_the_peers_do_not_know_yet_is_refused_without_blocking_the_node` (UC-CAN-029) calls `accept` for an unknown peer on its own thread and fails after 10 s if it does not return. It fails without the fix.

**History**

| Date | Event |
|---|---|
| 2026-10-26 | Seen in CI as a test "running for over 60 seconds" (the second occurrence in another test of the same binary). Not reproduced alone or in 40 sequential runs; reproduced with 6 concurrent processes (1 hang in 48 runs). |
| 2026-10-26 | Logs on: 1 hang in 16 runs of the two tests; the last line was a drain that never ended. In a Linux container with `gdb`: 1 hang in 56 runs, and the stack gave the cause above. |
| 2026-10-26 | Fix applied, test added (fails without the fix). `cargo clippy --all-targets -- -D warnings` clean with the new lint. The two tests, repeated in the same container with 8 processes at once: see the next row. |
| 2026-10-26 | Verification: 100 rounds of 8 processes at once (800 runs of the two tests) in the container that hung before, none hung; before the fix it hung once in every 16 to 56 runs. The `channel` binary passed (16 tests). |

## A FIFO restart test that waits 30 s in a combined run

**Status:** Open · **Found on:** 2026-10-22 · **Fixed on:** —

- **Test:** `fifo.rs` › `a_restart_keeps_the_busy_groups_and_the_rows_that_deduplicate` and, with the same symptom, `messages.rs` › `stopping_saves_memory_to_disk_and_the_next_start_resumes_it` (both restart a node over the disk of the previous life).
- **Symptom:** `waited 30 s for: the folders of the previous lives to be restored` (`TestNode` waits for the restoration of the disk it takes over before it returns). Seen in 2 of 4 combined runs (the `fifo`, `messages`, `volatile`, `operations`, `topics` and `auth` binaries one after the other, and the full suite); alone it passed 8 times in a row, and 12 runs of the whole `fifo` binary and 5 more combined runs with the logs on did not fail.
- **Cause:** not identified. Two real causes of the same symptom were found and fixed on the way (a sweep that read the old disk into the state of a topic before it was set aside, and the Windows move of the topic folder racing the sweeper); this one did not reproduce afterwards.
- **Next step:** on the next occurrence keep the `INFO` log of the second start (`tracing_subscriber` in the test) to see whether the move of the folder failed (`could not set aside the previous life's disk`) or the restoration did not finish.

**History**

| Date | Event |
|---|---|
| 2026-10-22 | Found while moving the integration tests to the one-node profile: failed in two combined runs, passed alone and with the logs on in later runs. |
| 2026-10-22 | Failed in two full runs in a row (`cargo test --no-fail-fast`, `waited 30 s`, in `common/cluster.rs`); the `fifo` binary alone passed 4 times and a third full run did not fail. The cause is still not identified. |
| 2026-10-22 | Three full runs (`cargo test -p taba-node --no-fail-fast`) failed with the same `waited 30 s` and one, without the new test in `messages.rs`, passed. It failed in `messages.rs` › `stopping_saves_memory_to_disk_and_the_next_start_resumes_it` (twice) and in `fifo.rs` › `a_restart_keeps_the_busy_groups_and_the_rows_that_deduplicate`; the `messages` binary alone passed 3 times in a row and the test alone, 3. The cause is still not identified (it is not a single test). |
| 2026-10-23 | `messages.rs` › `stopping_saves_memory_to_disk_and_the_next_start_resumes_it` failed with the same `waited 30 s for: the folders of the previous lives to be restored` in 2 of 6 full runs (`cargo test -p taba-node --no-fail-fast`, the `messages` binary taking 34 s instead of 2 s), while a Docker cluster with the test data was running on the same machine; the `messages` binary alone passed 5 times in a row. It appeared while the OpenAPI and port changes were being tested, but the symptom, the test and the wait are those of the 2026-10-22 entries, and nothing of the restoration of the disk was touched. The cause is still not identified. |

## A notice test that waits 30 s once in the full suite

**Status:** Open · **Found on:** 2026-10-20 · **Fixed on:** —

- **Test:** `notify.rs` › `a_notice_is_a_hint_that_ends_fetched_or_empty`.
- **Symptom:** a `waited 30 s` panic from `eventually_async` (`tests/common/cluster.rs`) in one run of `cargo test -p taba-node`, right after the build. Alone it passed 3 times in a row with and without the change under test, and a second full run was green.
- **Cause:** not identified (the output of the failing run was not kept).
- **Next step:** on the next occurrence, keep the full output to see which condition timed out; try the test under parallel load (many test binaries running at once) before touching a timeout.

**History**

| Date | Event |
|---|---|
| 2026-10-20 | Failed once in a full local run; passed alone (3 runs) and in the next full run. |
| 2026-10-22 | Failed again in a full local run (`waited 30 s for: the old notice counted as empty`); passed 6 of 6 alone right after. The change under test does not touch the notices. Cause still not identified. |

## Rejoining after a symmetric partition

**Status:** Fixed · **Found on:** 2026-10-19 · **Fixed on:** 2026-10-20

- **Test:** `failures.rs` › `after_a_partition_where_each_side_excluded_the_other_the_losing_side_joins_again`.
- **Symptom in CI:** "waited 30 s for: the four nodes in a single table".
- **Cause (confirmed):** when the two halves met, both nodes of the losing side redid the join at the same time. One of them (node 3 in the capture) was accepted through the other while the other was still being reset, received a table with just the two of them and started to synchronize with it as source; that source then joined the winning side and no longer had the node in its table. The sources answered `cluster.not-member` and the node, in `syncing`, kept asking for 60 s (`cluster.sync.timeout`) before entering `failed`: `handle_exclusion` only acts on an `active` node, so a syncing node never found out it had been excluded. In production the same sequence would end in a node in `failed` after a minute.
- **Fix:** while the synchronization waits for a source (`NoSource`), it asks the members of the table it received; if one of them has a newer table without the node, the synchronization ends with `SyncError::Excluded` and `finish_join` forgets the table, resets the local state and the bootstrap redoes the join (`ClusterService::excluded_while_syncing`).
- **Test of the fix:** `failures.rs` › `a_syncing_node_that_the_sources_excluded_redoes_the_join_instead_of_failing` (fails without the fix, `waited 30 s for: the node to redo the join`).

**History**

| Date | Event |
|---|---|
| 2026-10-19 | Failed in both CI runs of PR #3 on the amd64 runner; passed on arm64 and on the `release` push with the same code (the PR only changed `release.yml`). Test marked `#[ignore]`. |
| 2026-10-19 | Investigation: it is not `NO_VOTE` (the test does not use it) nor the lock-order inversion. Did not reproduce locally: passed in 12 and in 30 simultaneous runs of the same binary. |
| 2026-10-20 | Reproduced locally with 40 to 60 simultaneous runs of the binary (1 failure in each of two batches). A temporary dump of each node's state and tables at the timeout showed the node stuck in `syncing` with an old table; the cause above. |
| 2026-10-20 | Fix applied and `#[ignore]` removed. Verification: 180 simultaneous runs (3 batches of 60) of the test, all passed; the new test fails without the fix; the whole `taba-node` suite passed (one unrelated one-off failure, recorded above). |

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

## Lessons learned

What the issues above taught, so the next one is found faster (the checklist for an agent is in [AGENTS.md](../../AGENTS.md)):

- **A test that goes silent is not a slow test.** "Has been running for over 60 seconds" with **no log after some point**, not even a timeout that should have logged, means the runtime is stuck, not waiting. Look for a lock taken twice or a lock-order inversion, not for a timeout to raise.
- **A `std::sync::Mutex` is not reentrant, and a guard lives longer than it looks.** A temporary in the scrutinee of a `match`, `if let`, `while let` or `for` lives until the end of the whole block, `else` and arms included (in the 2024 edition an `if let` drops it before the `else`, but a `match` still does not). Take the value out in its own `let` statement and use it afterwards.
- **It shows only under load.** These races pass alone and in sequence. Reproduce with several processes of the test binary at once, and keep the logs only of the runs that hang.
- **On the `current_thread` runtime of `#[tokio::test]` a blocked lock freezes everything**, timers included, so the test never fails by itself: the code that calls the suspect function in a test runs it on its own thread and waits with `recv_timeout`, so the test fails instead of hanging.
- **Take the stack, do not guess.** `docker/hang-hunt.sh <test-file> [filter...]` does the whole hunt (load, detailed logs only there, stack of every thread of a hung process). `TABA_TEST_LOG` narrows the place; the stack of every thread (`gdb -p <pid>`, `thread apply all bt`) names the exact line. It is cheaper than another day of reading logs.
- **Search for the whole family.** The same mistake was in `accept`, and the same shape (a guard in a scrutinee) was in three more places; the lock-order inversion of 2026-10-18 is a cousin. After a fix, run `cargo clippy -- -W clippy::significant_drop_in_scrutinee` and the checks listed in [AGENTS.md](../../AGENTS.md).
- **A fix is not done without a test that fails without it** (here, one that fails by timeout and not by hanging) and without repeating the run that used to hang.

## Sources and evidence

- The tests cited and `tests/common/cluster.rs` (`NO_VOTE`, `FAST`, `WAIT`, `TABA_TEST_LOG`).
- `src/cluster/channel/mod.rs` (`accept`, `receive`, `sync_members`, `drop_all`, `allow_new_vote`), `src/cluster/backup/evict.rs`, `src/cluster/sync/mod.rs` and `tests/channel.rs` (UC-CAN-029).
- The hang of 2026-10-26 was reproduced locally and in a Linux container; the stack, the logs and the run counts are the ones in the history table.
- The failures were seen in the repository's CI logs; the "found" dates come from the git history and the logs.
