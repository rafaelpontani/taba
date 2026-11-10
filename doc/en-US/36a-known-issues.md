---
title: "Known issues: open"
summary: "The intermittent tests and known issues still open (or reopened): each with its status, dates, cause found so far, next step and the history of occurrences and attempts. The fixed ones are in 36f."
type: reference
status: partial
verified-on: 2026-11-10 @ d6e1d66
sources: [crates/taba-node/src/cluster/recovery/mod.rs, crates/taba-node/src/cluster/recovery/restoration.rs, crates/taba-node/src/cluster/recovery/finish.rs, crates/taba-node/tests/restore_lives.rs, crates/taba-node/tests/regroup.rs, crates/taba-node/src/engine/messages/blocks.rs, connectors/gateway-contract/drivers/go/main.go, connectors/gateway-contract/scenarios/aws/sqs-standard.scn, connectors/gateway-contract/scenarios/taba/limits-sqs.scn, crates/taba-node/tests/partition.rs, crates/taba-node/tests/batch.rs, crates/taba-node/src/cluster/replication/definitions.rs, crates/taba-node/tests/cold_start.rs, crates/taba-node/src/cluster/channel/mod.rs, crates/taba-node/tests/channel.rs, crates/taba-node/tests/failures.rs, crates/taba-node/tests/replication.rs, crates/taba-node/tests/notify.rs, crates/taba-node/tests/common/cluster.rs, crates/taba-node/src/cluster/sync/mod.rs, crates/taba-node/src/cluster/service/mod.rs]
---

# Known issues: open

> Read first: [25-testing.md](25-testing.md).

This file holds the sections of the issues that are **open** (`Open` or `Reopened`). Their index, where each file is and the pattern of a section are in [36-known-issues.md](36-known-issues.md); the fixed ones are in [36f-known-issues.md](36f-known-issues.md).

## KI-034: A restarted node in the table together with the old one in an overloaded hunt

**Status:** Open · **Found on:** 2026-11-10 · **Fixed on:** —

- **Test:** `tests/failures.rs::a_node_that_restarts_with_the_same_address_waits_for_the_vote_and_joins_as_a_new_node` (UC-CNC-002, UC-CNC-007, UC-FRZ-005, UC-CAN-014).
- **Symptom:** `the new node's request waited for the end of the vote`: a table had the old node and the new one together for a moment. Once in 80 runs of `ROUNDS=10 KEEP_GOING=1 docker/hang-hunt.sh failures`, in a round in which the full suite (`cargo test`) was also running on the same machine (the same round had other failures and hangs of overload). The same hunt repeated with the machine free (80 runs) did not fail.
- **Cause:** not identified. The reading to check: since the fix of KI-030 the test holds the `unregister` until the new node's `join` reaches a member; under that overload the held call may run out its deadline (5 s, three attempts), the exclusion fail (`failed`), the vote end without excluding, and the `join` then pass with the old node still in the table. If so, the server let a node at the same address join while the dead one was still a member, after a failed exclusion: the question is whether it must keep refusing it until a new vote excludes the old one.
- **Next step:** in the kept log (`target/hunt-failures-loaded/run_7_1.log`), the events of the cluster of this test: whether the exclusion failed before the `join`; then decide the rule above.
- **Do not:** raise the deadline of the held call.

**History**

| Date | Event |
|---|---|
| 2026-11-10 | One failure in an overloaded hunt (the full suite running at the same time); none in the clean repeat of 80 runs. |

## KI-005: A FIFO restart test that waits 30 s in a combined run

**Status:** Open · **Found on:** 2026-10-22 · **Fixed on:** —

- **Test:** `fifo.rs` › `a_restart_keeps_the_busy_groups_and_the_rows_that_deduplicate` and, with the same symptom, `messages.rs` › `stopping_saves_memory_to_disk_and_the_next_start_resumes_it` (both restart a node over the disk of the previous life).
- **Symptom:** `waited 30 s for: the folders of the previous lives to be restored` (`TestNode` waits for the restoration of the disk it takes over before it returns). Seen in 2 of 4 combined runs (the `fifo`, `messages`, `volatile`, `operations`, `topics` and `auth` binaries one after the other, and the full suite); alone it passed 8 times in a row, and 12 runs of the whole `fifo` binary and 5 more combined runs with the logs on did not fail.
- **Cause:** not confirmed. The reading that fits (found on 2026-11-10, without a failing run): `Recovery::drop_lives` tried **once** to delete the folder of each restored life. On Windows a file that another handle holds without delete sharing cannot be deleted: SQLite opens its files that way, and so may an antivirus or the indexer reading a new file. A handle still open for a moment (a connection to the life's database being closed, a scan) made `remove_dir_all` fail, the folder stayed, and the wait of `TestNode` for the folders to go ran out. It matches what was seen (only on Windows, only in combined runs, which write and scan more files, and never alone), but no failing run was kept to show the warning `could not delete the folder of a restored life`.
- **Fix (2026-11-10, in the server, not yet confirmed as the fix of this issue):** `drop_lives` tries again (5 more times, about 3 s in all) while the folder is still there, and says how many attempts it took. Windows test `tests/restore_lives.rs::the_folder_of_a_restored_life_is_deleted_even_if_a_file_in_it_is_held_for_a_moment` (UC-RTN-049): a file held as SQLite holds it makes the first removal fail, and the folder goes once the handle closes; with a single attempt it fails.
- **Next step:** on the next combined runs on Windows, look for `could not delete the folder of a restored life` or `deleted after a retry` in the log; if the failure comes back without either, the cause is another one (the restoration did not finish: `the folders of the previous lives stay`). It stays open until then.

**History**

| Date | Event |
|---|---|
| 2026-10-22 | Found while moving the integration tests to the one-node profile: failed in two combined runs, passed alone and with the logs on in later runs. |
| 2026-10-22 | Failed in two full runs in a row (`cargo test --no-fail-fast`, `waited 30 s`, in `common/cluster.rs`); the `fifo` binary alone passed 4 times and a third full run did not fail. The cause is still not identified. |
| 2026-10-22 | Three full runs (`cargo test -p taba-node --no-fail-fast`) failed with the same `waited 30 s` and one, without the new test in `messages.rs`, passed. It failed in `messages.rs` › `stopping_saves_memory_to_disk_and_the_next_start_resumes_it` (twice) and in `fifo.rs` › `a_restart_keeps_the_busy_groups_and_the_rows_that_deduplicate`; the `messages` binary alone passed 3 times in a row and the test alone, 3. The cause is still not identified (it is not a single test). |
| 2026-10-23 | `messages.rs` › `stopping_saves_memory_to_disk_and_the_next_start_resumes_it` failed with the same `waited 30 s for: the folders of the previous lives to be restored` in 2 of 6 full runs (`cargo test -p taba-node --no-fail-fast`, the `messages` binary taking 34 s instead of 2 s), while a Docker cluster with the test data was running on the same machine; the `messages` binary alone passed 5 times in a row. It appeared while the OpenAPI and port changes were being tested, but the symptom, the test and the wait are those of the 2026-10-22 entries, and nothing of the restoration of the disk was touched. The cause is still not identified. |
| 2026-11-10 | Reading of the code: a single attempt to delete the folder of a restored life, which a handle without delete sharing makes fail on Windows; the removal now tries again (Windows test). Not confirmed as the cause: no failing run was kept. |

## Lessons learned

What the issues above taught, so the next one is found faster (the checklist for an agent is in [AGENTS.md](../../AGENTS.md)):

- **A test that goes silent is not a slow test.** "Has been running for over 60 seconds" with **no log after some point**, not even a timeout that should have logged, means the runtime is stuck, not waiting. Look for a lock taken twice or a lock-order inversion, not for a timeout to raise.
- **A `std::sync::Mutex` is not reentrant, and a guard lives longer than it looks.** A temporary in the scrutinee of a `match`, `if let`, `while let` or `for` lives until the end of the whole block, `else` and arms included (in the 2024 edition an `if let` drops it before the `else`, but a `match` still does not). Take the value out in its own `let` statement and use it afterwards.
- **It shows only under load.** These races pass alone and in sequence. Reproduce with several processes of the test binary at once, and keep the logs only of the runs that hang.
- **On the `current_thread` runtime of `#[tokio::test]` a blocked lock freezes everything**, timers included, so the test never fails by itself: the code that calls the suspect function in a test runs it on its own thread and waits with `recv_timeout`, so the test fails instead of hanging.
- **Take the stack, do not guess.** `docker/hang-hunt.sh <test-file> [filter...]` does the whole hunt (load, detailed logs only there, stack of every thread of a hung process). `TABA_TEST_LOG` narrows the place; the stack of every thread (`gdb -p <pid>`, `thread apply all bt`) names the exact line. It is cheaper than another day of reading logs.
- **Search for the whole family.** The same mistake was in `accept`, and the same shape (a guard in a scrutinee) was in three more places; the lock-order inversion of 2026-10-18 is a cousin. After a fix, run `cargo clippy -- -W clippy::significant_drop_in_scrutinee` and the checks listed in [AGENTS.md](../../AGENTS.md).
- **A `waited 30 s` for a state that never arrives is a lost delivery, not a slow one.** Ask who was supposed to send it, to whom, and who was in the list of recipients at that instant. A `broadcast` (or a frame of the channel) reaches only the peers that exist **now**: a node that registers a few milliseconds later, and is still synchronizing, never gets it unless some other path (a `definition-apply` with `spread`, the heartbeat, the synchronization itself) covers it. A new way of delivering state to the cluster needs that second path, or a reason why it is not needed.
- **When the log has no line about the suspect path, add it, run again and take it out.** The failing run of the offer had no line about the offer. Temporary lines at each step (what was offered, to whom it was sent, what each node learned), marked with a word you can search (`HUNT`), and the same hunt, gave the order of the events in milliseconds; the lines were removed with `git checkout` before the fix. Keep the captured log (`target/`).
- **A test that fabricates the state of a node competes with the real node.** The heartbeat of a peer is its complete state and overwrites what the test injected; inject again until the effect is seen, or wait for the real state. A time margin of a few hundred milliseconds does not hold on a loaded runner: the hunt with all the tests of the binary at once finds it (a single test alone did not fail in 320 runs).
- **Look at which target failed before reading the server.** The count of the gateway was read in the node's code for a while; the failed job was the one of the emulators.
- **A client error is proved on the wire.** A capture in the server's network namespace says who closes and whether the response is well framed; then a wrapper of the client's connection and `httptrace` say why. A capture must start after the last restart of the container, or it sees an old namespace.
- **A window held open by a fixed delay is a guess about how long the other side takes.** Under load the other side took longer (KI-030), or a frame overtook a call (KI-031). Hold the call until the event the test is waiting for (`MemoryNetwork::hold_kind`), and the order stops depending on the machine.
- **A fix is not done without a test that fails without it** (here, one that fails by timeout and not by hanging) and without repeating the run that used to hang.
- **Look at what happened before theorizing about what could.** The pending item blamed an expired register for two years of reasoning; the dump of the failing message and a log line at each register showed that nothing had expired and that a check and a reservation were two steps. A report of a failure should say where the thing is now, on each node (`diagnosis` in `tests/chaos.rs`).
- **A retry that keeps failing is a state to inspect, not a timing to wait out.** The first note here proposed that `drain_topic` try again; the real cause would have stayed hidden. Keeping the cluster up after the failure and comparing the same message on every node (`memoryMessages`, the `excluded` table) found it in one run.

## Sources and evidence

- The tests cited and `tests/common/cluster.rs` (`NO_VOTE`, `FAST`, `WAIT`, `TABA_TEST_LOG`).
- `src/cluster/channel/mod.rs` (`accept`, `receive`, `sync_members`, `drop_all`, `allow_new_vote`), `src/cluster/backup/evict.rs`, `src/cluster/sync/mod.rs` and `tests/channel.rs` (UC-CAN-029).
- `src/cluster/replication/definitions.rs` (`offer`, `spread`, `relay`, `coordinate`), `src/cluster/service/mod.rs` (`recover_previous_life`, `finish_join`, `activate`) and `tests/cold_start.rs` (UC-RTN-058); the timeline of the lost offer is in the log kept from the hunt of 2026-10-26.
- The hang of 2026-10-26 was reproduced locally and in a Linux container; the stack, the logs and the run counts are the ones in the history table.
- The failures were seen in the repository's CI logs; the "found" dates come from the git history and the logs.
