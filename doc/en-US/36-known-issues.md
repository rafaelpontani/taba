---
title: Known issues
summary: "Log, with history, of the intermittent cluster tests and known issues: each with a title, status, date found, date fixed and the history of occurrences and attempts."
type: reference
status: partial
verified-on: 2026-11-09 @ edcf155
sources: [crates/taba-node/src/cluster/recovery/mod.rs, crates/taba-node/src/cluster/recovery/restoration.rs, crates/taba-node/src/cluster/recovery/finish.rs, crates/taba-node/tests/restore_lives.rs, crates/taba-node/tests/regroup.rs, crates/taba-node/src/engine/messages/blocks.rs, connectors/gateway-contract/drivers/go/main.go, connectors/gateway-contract/scenarios/aws/sqs-standard.scn, connectors/gateway-contract/scenarios/taba/limits-sqs.scn, crates/taba-node/tests/partition.rs, crates/taba-node/tests/batch.rs, crates/taba-node/src/cluster/replication/definitions.rs, crates/taba-node/tests/cold_start.rs, crates/taba-node/src/cluster/channel/mod.rs, crates/taba-node/tests/channel.rs, crates/taba-node/tests/failures.rs, crates/taba-node/tests/replication.rs, crates/taba-node/tests/notify.rs, crates/taba-node/tests/common/cluster.rs, crates/taba-node/src/cluster/sync/mod.rs, crates/taba-node/src/cluster/service/mod.rs]
---

# Known issues

> Read first: [25-testing.md](25-testing.md).

Each intermittent test or known issue has its own section, **which is never deleted**: when the issue is solved it becomes `Fixed`; if it comes back, it becomes `Reopened` and the history continues in the same section. Tests marked `#[ignore]` pass on the development machine but failed intermittently on the GitHub Actions runners, which are slow and shared. To run one by hand: `cargo test -p taba-node --test <file> <name> -- --ignored`.

Possible statuses: `Open`, `Fixed`, `Reopened`. The rules for handling intermittent tests are in [AGENTS.md](../../AGENTS.md).

| Issue | Status | Found on | Fixed on |
|---|---|---|---|
| [An acknowledged message brought back by an older life](#an-acknowledged-message-brought-back-by-an-older-life) | Fixed | 2026-11-09 | 2026-11-09 |
| [A cold-start vote that does not close in 30 s](#a-cold-start-vote-that-does-not-close-in-30-s) | Fixed | 2026-11-09 | 2026-11-09 |
| [A probe that does not end with the node missing in 30 s](#a-probe-that-does-not-end-with-the-node-missing-in-30-s) | Fixed | 2026-11-09 | 2026-11-09 |
| [A large ReceiveMessage of the older Go SDK that loses its connection under load](#a-large-receivemessage-of-the-older-go-sdk-that-loses-its-connection-under-load) | Fixed | 2026-11-08 | 2026-11-09 |
| [A count of an emulator right after a receive](#a-count-of-an-emulator-right-after-a-receive) | Fixed | 2026-11-08 | 2026-11-09 |
| [A message delivered twice after the reunion of a partition](#a-message-delivered-twice-after-the-reunion-of-a-partition) | Fixed | 2026-11-07 | 2026-11-09 |
| [A second channel that is not closed first](#a-second-channel-that-is-not-closed-first) | Fixed | 2026-11-05 | 2026-11-05 |
| [A node that stops answering and is not excluded in 30 s](#a-node-that-stops-answering-and-is-not-excluded-in-30-s) | Open | 2026-11-04 | — |
| [A replication test whose 14 publications missed the isolated node](#a-replication-test-whose-14-publications-missed-the-isolated-node) | Fixed | 2026-11-03 | 2026-11-03 |
| [A sync test whose second store found the database locked on arm64](#a-sync-test-whose-second-store-found-the-database-locked-on-arm64) | Fixed | 2026-11-03 | 2026-11-03 |
| [A consume with ack that fails once in docker/validate.sh after a drain](#a-consume-with-ack-that-fails-once-in-dockervalidatesh-after-a-drain) | Fixed | 2026-11-03 | 2026-11-04 |
| [An exclusion vote that fails once in the full suite](#an-exclusion-vote-that-fails-once-in-the-full-suite) | Open | 2026-10-29 | — |
| [A register that checked the message before taking it](#a-register-that-checked-the-message-before-taking-it) | Fixed | 2026-10-27 | 2026-10-29 |
| [A channel burst that does not reach the other node in the hunt](#a-channel-burst-that-does-not-reach-the-other-node-in-the-hunt) | Fixed | 2026-10-29 | 2026-11-05 |
| [A lost ready signal that the hunt does not correct in time](#a-lost-ready-signal-that-the-hunt-does-not-correct-in-time) | Open | 2026-10-29 | — |
| [The chaos run: a delivery past the limit and an ack the owner never applied](#the-chaos-run-a-delivery-past-the-limit-and-an-ack-the-owner-never-applied) | Fixed | 2026-10-27 | 2026-10-27 |
| [A restarted test node that loses its port to another test](#a-restarted-test-node-that-loses-its-port-to-another-test) | Fixed | 2026-10-26 | 2026-11-09 |
| [A node excluded while it synchronizes that cannot come back](#a-node-excluded-while-it-synchronizes-that-cannot-come-back) | Open | 2026-11-09 | — |
| [A definition offered to a node that was still joining](#a-definition-offered-to-a-node-that-was-still-joining) | Fixed | 2026-10-26 | 2026-11-09 |
| [A channel accept that locked the peers twice](#a-channel-accept-that-locked-the-peers-twice) | Fixed | 2026-10-26 | 2026-10-26 |
| [A FIFO restart test that waits 30 s in a combined run](#a-fifo-restart-test-that-waits-30-s-in-a-combined-run) | Open | 2026-10-22 | — |
| [A notice test that waits 30 s once in the full suite](#a-notice-test-that-waits-30-s-once-in-the-full-suite) | Fixed | 2026-10-20 | 2026-11-08 |
| [Rejoining after a symmetric partition](#rejoining-after-a-symmetric-partition) | Fixed | 2026-10-19 | 2026-10-20 |
| [A healthy channel that does not come back](#a-healthy-channel-that-does-not-come-back) | Fixed | 2026-10-18 | 2026-10-19 |
| [Lock-order inversion when choosing replicas](#lock-order-inversion-when-choosing-replicas) | Fixed | 2026-10-18 | 2026-10-18 |

## An acknowledged message brought back by an older life

**Status:** Fixed · **Found on:** 2026-11-09 · **Fixed on:** 2026-11-09

- **Test:** `tests/cold_start.rs::with_every_node_back_no_message_that_was_confirmed_is_delivered_again_whatever_the_falls` (UC-RTN-053).
- **Symptom:** `round 1: <id> came back` at `cold_start.rs:696`: a message that was restored and confirmed in round 0 was delivered again after the falls of round 1. Seen once in 1800 process runs of `KEEP_GOING=1 ROUNDS=180 PAR=10 docker/hang-hunt.sh cold_start`; not seen in CI.
- **Cause (confirmed, in the server):** the order of the restoration. A node that falls twice keeps two lives: the older one still holds the message (a copy whose removal did not reach it before the fall), and the newer one recorded its ack in its log of excluded messages. The restoration took the lives oldest first and **incorporated the log of each life just before sending its messages**, so the message of the older life went to the primary before the log of the newer life, which says it is dead, was in the node's log: the check of the dead (own log, the other nodes, S3) did not find it, and the message was activated. The same gap existed between nodes: the barrier of the restoration counted the lives a node announced, not whether their logs were already in its log, so a primary could ask a node that had not incorporated them yet. In the log: the restoration of round 1 with `lives=3` sent the `id` that round 0 had restored, delivered and confirmed.
- **Fix:** the node incorporates the rows of the logs of **every** life when its restoration starts, and only then tells the others `logsReady` in `GET /cluster/restore`; the barrier counts the lives of a node only after that (a node of an earlier version does not tell and counts as ready). Inside the pass, the logs of all the restorable lives (with the start of the recording) come in before the first message of any of them is sent ([07-cluster.md](07-cluster.md), sections 10.3 and 10.7).
- **Tests of the fix:** `tests/restore_lives.rs::the_log_of_a_newer_life_is_in_before_the_messages_of_an_older_one_are_sent` (fails without the fix: `node 1: an acknowledged message came back`) and `tests/restore_lives.rs::a_node_tells_its_lives_logs_are_in_only_when_its_answer_to_the_dead_has_them`.
- **Verification:** `KEEP_GOING=1 ROUNDS=180 PAR=10 docker/hang-hunt.sh cold_start` with every fix of 2026-11-09: 1800 process runs and **no** `came back` (the only 2 failures were the race of "A definition offered to a node that was still joining", fixed after it). `tests/restore_lives.rs` (12 tests) passes.

**History**

| Date | Event |
|---|---|
| 2026-11-09 | Found by the hunt that verified `PortHold` (round 17 of 180); the log showed the `id` restored again by a life of round 0. Fixed in the order of the restoration, with the two tests above. |
| 2026-11-09 | Verified: 1800 process runs of the same hunt with the fix, no `came back`. |

## A cold-start vote that does not close in 30 s

**Status:** Fixed · **Found on:** 2026-11-09 · **Fixed on:** 2026-11-09

- **Test:** `tests/cold_start.rs::with_every_node_back_no_message_that_was_confirmed_is_delivered_again_whatever_the_falls` (UC-RTN-053), in `come_back_all`.
- **Symptom:** `waited 30 s for: the vote closed on every node` (`common/cluster.rs:89`). Seen twice in 1800 process runs of `KEEP_GOING=1 ROUNDS=180 PAR=10 docker/hang-hunt.sh cold_start` (rounds 3 and 16); not seen in CI.
- **Cause (in the test):** the same as the section below. The nodes fell one after the other, and under load the ones still up had time to vote the first fallen out and rewrite the manifest of their life with a table of two. On the return the disks disagreed on the table: one node started alone with its old group of one, the others formed another group, and only the probe of the old IPs (every 10 s) brought them together through the regroup (doc 07, section 10.11), later than the 30 s of the wait. Both failing logs have the line `this side is the smaller and regroups` and one more node that `starts alone` with `alive=1` than the logs of the other failures.
- **Fix:** `fall_all` of `tests/cold_start.rs` cuts the network between the nodes (`isolate`) before the first fall and gives it back (`restore`) after the last one: with no quorum nobody is voted out, and every disk keeps the table of three. Every test of the file that drops the whole cluster uses it, and `regroup.rs::two_groups` does the same.
- **Verification:** `KEEP_GOING=1 ROUNDS=180 PAR=10 docker/hang-hunt.sh cold_start` with `fall_all`: 1800 process runs, **no** `the vote closed on every node` and no `the probe ended with a node missing` (before: 2 + 2 in 1800); `KEEP_GOING=1 ROUNDS=60 PAR=10 docker/hang-hunt.sh regroup`: 600 runs, no failure.

**History**

| Date | Event |
|---|---|
| 2026-11-09 | Seen twice in the hunt that verified `PortHold`; recorded with the cause open. |
| 2026-11-09 | Cause found in the logs (a manifest with a table of two) and fixed in the test (`fall_all`); verified in 1800 runs of `cold_start` and 600 of `regroup` with no failure. |

## A probe that does not end with the node missing in 30 s

**Status:** Fixed · **Found on:** 2026-11-09 · **Fixed on:** 2026-11-09

- **Test:** `tests/cold_start.rs::the_node_that_was_missing_when_the_probe_ended_joins_later_by_the_normal_join_and_the_condition_goes_away` (UC-RTN-057).
- **Symptom:** `waited 30 s for: the probe ended with a node missing` (`common/cluster.rs:89`). Seen twice in 1800 process runs of `KEEP_GOING=1 ROUNDS=180 PAR=10 docker/hang-hunt.sh cold_start` (rounds 28 and 161); not seen in CI.
- **Cause (in the test):** in both failing logs the probe **did** end on both nodes, with `expected=2`: the table of the previous life had two members, not three, so two active members are not "missing" anyone and `members_missing` stays `false`. The nodes fell one after the other (`while … fall`), and under load the two still up voted the first fallen out and wrote the manifest with the table of two before they fell themselves.
- **Fix:** the falls with the network cut, in `fall_all` (section above).
- **Verification:** `KEEP_GOING=1 ROUNDS=180 PAR=10 docker/hang-hunt.sh cold_start` with `fall_all`: 1800 process runs, **no** `the vote closed on every node` and no `the probe ended with a node missing` (before: 2 + 2 in 1800); `KEEP_GOING=1 ROUNDS=60 PAR=10 docker/hang-hunt.sh regroup`: 600 runs, no failure.

**History**

| Date | Event |
|---|---|
| 2026-11-09 | Seen twice in the hunt that verified `PortHold`; recorded with the cause open. |
| 2026-11-09 | Cause found in the logs (a manifest with a table of two) and fixed in the test (`fall_all`); verified in 1800 runs of `cold_start` and 600 of `regroup` with no failure. |

## A large ReceiveMessage of the older Go SDK that loses its connection under load

**Status:** Fixed · **Found on:** 2026-11-08 · **Fixed on:** 2026-11-09

- **Test:** the gateway contract tests (`connectors/gateway-contract/run.sh`) on a Taba node, driver `go-old` (aws-sdk-go-v2 with `service/sqs` v1.26.0 and smithy-go v1.16.0): the `receive` of a large message (200 KB).
- **Symptom:** `operation error SQS: ReceiveMessage, https response error StatusCode: 200 … deserialization failed, failed to decode response body, read tcp …: use of closed network connection`, and the next line fails because it has no receipt handle. Locally, under CPU load, about 1 run in 8; never on the emulators.
- **Cause (confirmed, in the client):** net/http writes the request body straight to the socket, the node answers at once and `Do` returns with the headers. smithy-go then closes the request body (`transport/http/client.go`). After writing the body, net/http reads it once more to check that nothing is left (`transferWriter.writeBody`, `doBodyCopy(io.Discard, body)`); that read goes through the `WriteTo` of smithy-go's `safeWriteToReadCloser`, which answers `io.EOF` after the close instead of `(0, nil)`. When the machine is loaded and that read comes after the close, `persistConn.writeLoop` takes the `EOF` as a write error and closes the connection in the middle of the response. The same code is in smithy-go v1.28.8, but the current SDK did not fail in our runs. The node answers correctly: `Content-Length` right, no `Connection: close`, no FIN or RST from its side.
- **Fix:** test only. The Go driver gives net/http the request body without its `WriteTo` (`plainBodyTransport` in `drivers/go/main.go`): a plain `Read` after the close is `(0, io.EOF)`, which `io.Copy` takes as the end of the body.
- **How it was found:** a `tcpdump` in the node's network namespace (`-i eth0`, started after the `docker restart` of `gw_target_taba_up`; `-i any` captured nothing) showed the RST coming from the client in the middle of a well-framed response. A `net.Conn` wrapper in the driver showed `persistConn.writeLoop` closing the connection, and `httptrace.WroteRequest` gave the error: `EOF`. Turning keep-alive off did not help (2 failures in 19 runs); hiding `WriteTo` did.
- **Verification:** with the fix, 40 of 40 runs of the 200 KB scenario passed under the same load (12 containers using the CPU); before it, between 1 in 20 and 7 in 20 failed. The current and the older SDK pass every scenario.

**History**

| Date | Event |
|---|---|
| 2026-11-08 | Failed in the CI of the release chain of v0.3.10 (gateway contract tests on a Taba node); passed when the job ran again. |
| 2026-11-09 | Reproduced locally with a 200 KB message and 10 to 12 containers using the CPU (between 1 in 20 and 7 in 20 runs); cause found with a capture and the client's traces, fixed in the driver. |

## A count of an emulator right after a receive

**Status:** Fixed · **Found on:** 2026-11-08 · **Fixed on:** 2026-11-09

- **Test:** the gateway contract tests on the emulators (`run.sh --target emulator`, the job "Scenarios on the emulators"), driver `rust-current`, `aws/sqs-standard.scn` line 115.
- **Symptom:** `getQueueAttributes :: attr_ApproximateNumberOfMessages=2 but was '0'` after a `sendBatch` of three messages and a `receive` of one, on ElasticMQ. The same line passed on the Taba node in the same run.
- **Cause:** the line asked for an exact count right after a receive, but SQS calls these counts approximate (they can take a while to settle), and the `sendBatch` before it did not check that the three entries were accepted. Which of the two the emulator did is not known (one occurrence, not reproduced). It is not the node: Taba answers the counts from the index of the queue at that instant (`topic_snapshot`).
- **Fix:** test only. `aws/sqs-standard.scn` checks `sendBatch … => ok=3 failed=0` and only that the two counts are there (`>=0`); the exact counts went to `taba/limits-sqs.scn`, which only runs on Taba.
- **Verification:** the whole contract passed on the Taba node (the 12 SDK variants, 286 of 286 lines each, the native consumer and the cross tests) and on the emulators (173 of 173 SQS lines and 50 of 50 SNS lines for each variant).

**History**

| Date | Event |
|---|---|
| 2026-11-08 | Failed once in the CI of the pull request of the Pulumi provider, on the emulators; passed when the job ran again. |
| 2026-11-09 | Not reproduced locally. The log of the failed job shows that the target was ElasticMQ, not the node; the line was split as above. |

## A message delivered twice after the reunion of a partition

**Status:** Fixed · **Found on:** 2026-11-07 · **Fixed on:** 2026-11-09

- **Test:** `tests/partition.rs::the_half_that_loses_the_reunion_keeps_what_it_accepted_while_apart` (UC-FAL-026, the measurement of R1 in [37-guarantees.md](37-guarantees.md)).
- **Symptom:** in the arm64 server job of the `release.yml` run of the #63 merge (run 42409549368), the test ended with `delivered twice: [(18, 2)]` at `partition.rs:147`: no message lost, but the one with `seq` 18 (published on node 3, in the half with 3 and 4) was delivered twice. The same commit passed on amd64, in the pull request CI and in `v0.3.8-rc.1`, which has the same server code.
- **Cause (confirmed, in the server):** a check of the dead apart from the store. Both nodes of the losing half keep a copy of each message they accepted (the replication factor), so both restore their life and send the same message, with the same `id`, to the new primary. The primary checks the dead of each batch (`accept_restore`: its own log, the log of the other nodes, S3) and only then activates it. Both batches passed the check while the message was still alive; the copy of the first was activated, delivered and acknowledged (the `id` went to the log of excluded messages); then the batch of the second reached the store, which only asked whether the node had the message (`holds`), and stored it again. In the log of a failing run: two `lost message recovered` lines for the same `id` 19 ms apart, and the two deliveries with `deliveryCount=1` on two different nodes, each ack answered `204`. It was not the *at-least-once* of a failed ack, which would have come back with `deliveryCount=2`.
- **Fix:** the store of a recovered or restored message reads the log of excluded messages again **under the same hold of the topic** (`Messages::apply_recovered`, used by `Recovery::apply_local`; a new answer, `Loaded::Dead`, which counts as resolved). An ack takes the message out and records its exclusion under that same hold, so once the message is not there the log already says whether it died.
- **Test of the fix:** `src/engine/messages/tests/lives.rs::a_copy_of_a_message_acknowledged_after_the_check_of_the_dead_is_not_stored_again` (fails without the fix: `left: Stored, right: Dead`).
- **How it was found:** `docker/hang-hunt.sh partition` (8 processes, 40 rounds) reproduced it in 4 of 320 processes; a temporary `HUNT` line in the test's `drain` (node, `seq`, `id`, `deliveryCount`, status of the ack) showed the same `id` delivered by two nodes and both acks accepted.
- **Verification:** with the fix, the same hunt ran 120 rounds of 8 processes (960 runs) without a problem; before it, 4 of 320 failed.

**History**

| Date | Event |
|---|---|
| 2026-11-07 | One failure on arm64 in the `release.yml` run of the #63 merge, which only changed connectors and documentation; the failed jobs were run again. |
| 2026-11-07 | `docker/hang-hunt.sh partition` (8 processes at once) reproduced it in round 12: `delivered twice: [(17, 2)]`. The log is kept in `target/hang-hunt/run_12_2.log`, not read yet. |
| 2026-11-09 | The hunt (40 rounds of 8 processes) failed in 4 processes with the same signature; the cause was found in the log with a temporary line in the test and fixed in the store of the restoration. |

## A batch test that counted the calls before the background commit

**Status:** Fixed · **Found on:** 2026-11-07 · **Fixed on:** 2026-11-07

- **Test:** `tests/batch.rs::a_batch_of_publications_goes_through_each_phase_in_one_call_per_node_and_group` (and, with the same pattern, `operations_waiting_for_the_same_node_travel_in_fewer_calls_than_operations` and `with_max_items_1_every_operation_goes_in_a_call_of_its_own`).
- **Symptom:** in the arm64 server job of pull request #62, `batch.rs:436` failed with `362`: the test expected at least 400 register and commit calls between nodes for 100 publications. The other 9 tests of the file passed.
- **Cause:** the test, not the batch. The coordinator answers when the quorum confirms the commit (the primary and one more node); the third node gets its commit in the background ([doc 10](10-publishing.md)). The test counted the calls of the in-memory network right after the answer, so on a slow runner some commits had not been sent yet.
- **Fix:** the three tests wait (`eventually`, up to 30 s) until the expected number of calls has gone through before comparing it with the number of batches. What they prove stays the same. The server code did not change.

**History**

| Date | Event |
|---|---|
| 2026-11-07 | One failure on arm64 in #62. Tests fixed; 0 failures in 40 runs of the whole file (10 rounds of 4 processes) on Windows. |

## A second channel that is not closed first

**Status:** Fixed · **Found on:** 2026-11-05 · **Fixed on:** 2026-11-05

- **Test:** `tests/channel.rs::two_connections_at_once_end_with_a_single_channel_per_pair`.
- **Symptom:** in the arm64 server job of the pull request "Release v0.3.6" (#52), `channel.rs:599` failed with `assertion failed: closed.is_none()`: the first thing the test read from the extra connection was a message, not the close. The other 15 tests of the file passed.
- **Cause:** the test, not the channel. `Channels::install` only refuses the extra connection when the node has, at that instant, a link opened by itself; with a link opened by the other side, or with no link, it keeps the new one and talks on it until node 1 reopens its own, and then the extra connection closes. That is the designed behaviour (the same side reopening replaces the old link), but the test demanded that the first thing read was the close. The log of the captured failure shows the link of the pair going down 3 ms before the assertion, which does not happen in the path the test assumed. Which link node 2 held is not logged, so that detail comes from the code.
- **Fix:** the test reads until the connection closes, with a limit of 30 s. What it proves stays the same: the connection of the lowest hash closes and one channel remains. The server code did not change.

**History**

| Date | Event |
|---|---|
| 2026-11-05 | One failure on arm64 in #52. Not reproduced in the hunt: 0 failures in 480 runs of the test alone (60 rounds of 8 processes) and 0 in 320 runs of the whole file (40 rounds of 8 processes). |
| 2026-11-05 | Reproduced once in another 320 runs of the whole file, with the log kept. Test fixed; the same hunt then: 0 failures in 320 runs. |

## A node that stops answering and is not excluded in 30 s

**Status:** Open · **Found on:** 2026-11-04 · **Fixed on:** —

- **Test:** `tests/failures.rs::a_node_that_stops_answering_with_the_connection_open_is_detected_and_excluded`.
- **Symptom:** in the amd64 server job of the back-merge `main → release` after `v0.3.5` (#50), the test ended with `waited 30 s`. Running the job again passed.
- **Cause:** not identified. The log of the failing job was not studied.
- **Next step:** `docker/hang-hunt.sh failures a_node_that_stops_answering` until it keeps a failing run, and read which node did not take part in the vote.
- **Do not:** raise the 30 s of `eventually`.

**History**

| Date | Event |
|---|---|
| 2026-11-04 | One failure on amd64 in #50; running the job again passed. |

## A replication test whose 14 publications missed the isolated node

**Status:** Fixed · **Found on:** 2026-11-03 · **Fixed on:** 2026-11-03

- **Test:** `tests/replication.rs::with_a_node_down_only_the_publications_it_owns_fail_and_the_replica_catches_up`.
- **Symptom:** in the checks of the back-merge `main → release` after `v0.3.3` (#43), the amd64 server job failed at `replication.rs:393` with `ok 14 failed 0`: none of the 14 publications was refused.
- **Cause:** the test isolates node 3 and expects some publications to fail, the ones whose primary is node 3; the primary comes from the hash of the random `id` of each message, so with 14 fixed publications none falls on node 3 with probability (2/3)^14, about once in 300 runs. Not related to a change of code.
- **Fix:** the test publishes until it has seen both outcomes (at least one accepted and one refused), at least 14 and at most 60 times; the chance of missing node 3 in 60 is about 10^-11.
- **Verification:** the test passed 5 of 5 and the file of 21 tests passed.

**History**

| Date | Event |
|---|---|
| 2026-11-03 | Failed once on amd64 in #43 (`ok 14 failed 0`); the test publishes until it sees both outcomes. |

## A sync test whose second store found the database locked on arm64

**Status:** Fixed · **Found on:** 2026-11-03 · **Fixed on:** 2026-11-03

- **Test:** `tests/sync.rs::a_message_that_is_listed_but_has_no_content_anywhere_is_a_loss_and_the_sync_goes_on` (UC-SIN-025 to 028).
- **Symptom:** in the `release.yml` run on `main` for `v0.3.3` (merge of #38), the arm64 server job failed at `sync.rs:322`: `evict_body(...).unwrap()` on `SqliteFailure(DatabaseBusy, "database is locked")`; the release stopped before publishing. The same code passed on amd64, in the candidate run and in the checks of the pull request.
- **Cause:** the test opens a second `MessageStore` over the database of a running node to delete a body, and the node may be writing the same database (its writer, the trim of the log of excluded messages): the write of the test gets `SQLITE_BUSY`. A node has only one store per database, so the product is not affected; the slower arm64 runner made the window larger.
- **Fix:** the test tries the delete again while the database is locked (`eventually`), and fails on any other error.
- **Verification:** the test passed 5 of 5 alone and the file of 12 tests 3 of 3.

**History**

| Date | Event |
|---|---|
| 2026-11-03 | Failed once on arm64 in the release of `main`; the test retries while the database is locked. |

## A consume with ack that fails once in docker/validate.sh after a drain

**Status:** Fixed · **Found on:** 2026-11-03 · **Fixed on:** 2026-11-04

- **Test:** `docker/validate.sh`, step "No acknowledged message was lost": `drain_topic validation-crash` calls `taba-cli consume --max 10 --wait 3 --ack` in a loop right after `taba-1` left by drain and came back.
- **Symptom:** one `taba-cli consume` exited with code 1 and the script stopped (`Failed (code 1)`) without saying why: `taba_cli` threw away the stderr of `docker compose run`, and the error of `taba-cli` with it. It was the first run after the machine restarted, with Docker Desktop just started: the cluster took 402 s to become healthy (24 s in the next run). The next run, on the same code, passed every step (50 messages published, all received and acknowledged).
- **Cause:** a message already acknowledged came back to life from the S3 backup. When a member leaves the cluster, each node runs `recover_missing` on its next heartbeat round and fetches from the bucket what it should hold and does not. To know what is dead it read only the journal in the bucket, which arrives late (`backup.meta.interval`), and not the node's own log of excluded messages. A message that had reached a segment (the drain of the node that left sends everything) and was acknowledged in the seconds before that round came back, on that node only. It was delivered again, and its ack could no longer close the quorum: the primary accepted and the two other nodes, which no longer had it, answered `message.not-found`, so the client got `503 cluster.quorum-unavailable`. No message was lost; acknowledged messages were delivered again. Only clusters with `backup.s3.bucket` are affected. The fault is in the node, not in the host: the first failure looked related to a slow start only because the timing of the heartbeat round decides whether the acks come first.
- **How it was found:** with the cluster kept up after a failure (`TABA_KEEP=1`), the log of the failing run has `messages that only existed on dead nodes came back from S3 recovered=5` less than a second after the acks; `memoryMessages` of the three nodes was `1 6 1` where a healthy topic has the same number on all; and 5 of the messages still delivered were in the `excluded` table of all three nodes.
- **Fix:** `recover_missing` also skips what the node's log of excluded messages has (`src/cluster/backup/restore.rs`). The ack records the exclusion before it takes the message out of the index, under the same lock, so a message the node no longer holds is already in the log. Test: `tests/backup.rs::a_message_acked_before_the_journal_reaches_the_bucket_does_not_come_back_from_s3`, which fails without the fix (the recovery returns 1). UC-BKP-046.
- **Verification:** `docker/validate.sh` failed 4 of 4 runs on this host before the fix (one of them on `a3bdab9`, before the SQS and SNS gateway) and passed 3 of 3 with it.

**History**

| Date | Event |
|---|---|
| 2026-11-03 | One failure in the first run after a restart of the machine (cluster healthy in 402 s); the next run passed. `taba_cli` now shows the error. |
| 2026-11-04 | Four failures in a row with `cluster.quorum-unavailable` in the same step, including on the commit before the gateway; `v0.3.5` was promoted with the failure recorded in its pull request. |
| 2026-11-04 | Cause found and fixed: `recover_missing` ignored the log of excluded messages. New test; `docker/validate.sh` passed 3 of 3. |

## An exclusion vote that fails once in the full suite

**Status:** Open · **Found on:** 2026-10-29 · **Fixed on:** —

- **Test:** `tests/failures.rs::only_the_lowest_node_id_among_the_healthy_coordinates_the_exclusion` (UC-FAL on the exclusion), the assertion on the coordinators of the `unregister` calls (line 1087).
- **Symptom:** it failed once in a full `cargo test` in a Linux container (22 tests of the file in parallel with the other binaries' load); alone it passed 3 of 3, and a second full run passed.
- **Cause:** not identified. It does not touch the register of a message (found while validating [a register that checked the message before taking it](#a-register-that-checked-the-message-before-taking-it)). The reading to check is a second healthy node that also registers the unregistration before it sees the first one finished.
- **Next step:** `docker/hang-hunt.sh failures only_the_lowest_node_id` and read which node was the extra coordinator.

**History**

| Date | Event |
|---|---|
| 2026-10-29 | One failure in the full suite, 3 passes alone, 1 full pass after. |

## A register that checked the message before taking it

**Status:** Fixed · **Found on:** 2026-10-27 · **Fixed on:** 2026-10-29

- **Where:** `src/cluster/replication/replica.rs` (`Replica::register`, `reserve`). Found by `tests/chaos.rs` ([37-guarantees.md](37-guarantees.md)), seeds 2, 3 and 4, [PRB-077](30-decisions.md#resolved-problems-prb).
- **Symptom:** in about 8 % of the processes of the hunt (`docker/hang-hunt.sh chaos`, 8 at once; 13 in 160), "I5: seq N was acked on its queue and also reached the dead-letter" and, in some, "I3: seq N was acked (204) and delivered again".
- **How it was found:** the failure report of the chaos run now has a dump of the message that broke I3 or I5 (where it is, on each node and in each queue: `diagnosis`), and the hunt was run with a line at each register, commit, rollback and expiry of the replica and at the start and end of each ack and move to the dead-letter (taken out afterwards). The first reading (an expired register) was wrong: **no register expired** in any of the failing runs. In the log of one, the move to the dead-letter registered and committed on the three nodes (+80 ms), and only after that the register of the ack arrived on them, 120 ms after it had been sent, and was accepted.
- **Cause:** `ack_step`, `register_write` and `visibility_step` checked the message (`check_visibility`, `check_publish`...) in a blocking thread and then called `register`. The check and the reservation of the message were two steps with no lock between them, and under load (8 processes at once) a whole operation fitted in the gap: the ack checked a message that was in the queue, the move to the dead-letter registered and committed, and the ack registered, committed (it applies to a message that is gone, with no error) and was confirmed to the client with `204`.
- **Fix:** `register` reserves the message first (`by_message`, `409 cluster.conflict` for whoever arrives in the meantime) and runs the check with the reservation held; a failed check, or a call dropped in the middle of it, releases the reservation (`Reservation`, on `Drop`). The register repeated after the decision and the replay of events check nothing and are not affected.
- **Test:** `replica::tests::a_register_holds_the_message_while_its_check_runs` (a slow check; the second operation must get the conflict) and `a_register_whose_check_fails_releases_the_message`. Verification: the same hunt, 20 rounds of 8 processes: **0 failures in 160** (before the fix, 13 in 160); full suite, clippy and fmt.
- **Do not:** raise `write.pending.ttl` or the waits of the run: nothing expired.

**History**

| Date | Event |
|---|---|
| 2026-10-27 | Seen with clock jumps of 35 s (3 runs in 400): thought to be an expired register (the first form of PRB-077). |
| 2026-10-29 | Seen without clock jumps in the hunt (3/96, 5/96, 3/96 on the code before the batches and with them): the batches did not change the rate. |
| 2026-10-29 | The dump and the lines at each register showed that no register expired: the check ran before the reservation. Fixed by reserving first. 0 failures in 160 processes. |

## A channel burst that does not reach the other node in the hunt

**Status:** Fixed · **Found on:** 2026-10-29 · **Fixed on:** 2026-11-05

- **Where:** `tests/channel.rs::each_peer_has_an_out_queue_and_an_in_queue_that_drain` (and, once each, `every_node_has_one_channel_to_each_other_node_and_measures_it` and `two_connections_at_once_end_with_a_single_channel_per_pair`).
- **Symptom:** in `docker/hang-hunt.sh channel` (8 processes at once) it ends with `waited 30 s for: the burst to reach node 2`, in about 1 process in 14.
- **Cause:** the test, not the channel. The burst signals `ready` for invented topics (`t0` to `t299`). The next heartbeat (200 ms in the tests) lists the topics that are really ready, and `Signals::apply_keepalive` rightly removes the others from the table of node 2, so `t299` is there only until that heartbeat. On a loaded runner the check, every 10 ms, can miss that window, and then it waits 30 s for a state that will not come back. It was not a consequence of the batches: the same hunt failed at the same rate on the code before them.
- **Fix:** the test starts with `QUIET` (heartbeat of 30 s). In `every_node_has_one_channel_to_each_other_node_and_measures_it`, a separate cause: `queue_out` was read once and could catch a signal on its way (`left: 1`); it now waits for the queue to empty. The other test once named here is [A second channel that is not closed first](#a-second-channel-that-is-not-closed-first).
- **Verification:** see the history. The server code did not change.

**History**

| Date | Event |
|---|---|
| 2026-10-29 | Found in the hunt that validated the `take` and the FIFO forward in the batch; the same rate on the earlier commit. |
| 2026-11-05 | The hunt of the whole file (40 rounds of 8 processes, on `4892068`) had a problem in 15 rounds: 18 failures of `each_peer_has_an_out_queue_and_an_in_queue_that_drain` and 1 of `every_node_has_one_channel_to_each_other_node_and_measures_it`. |
| 2026-11-05 | `each_peer_has_an_out_queue_and_an_in_queue_that_drain` failed on arm64 in #52, the first time in CI. With `QUIET`, the same hunt had 0 failures of it in 320 runs (it had 18) and 2 of `every_node_has_one_channel_to_each_other_node_and_measures_it`, fixed next. With both fixes, see [A second channel that is not closed first](#a-second-channel-that-is-not-closed-first). |

## A lost ready signal that the hunt does not correct in time

**Status:** Open · **Found on:** 2026-10-29 · **Fixed on:** —

- **Where:** `tests/replication.rs::a_lost_ready_signal_is_corrected_by_the_periodic_repeat`.
- **Symptom:** in `docker/hang-hunt.sh replication` (8 processes at once, 15 threads each) it fails in about 3 of 5 processes of each round, in three ways: the node 2 consumes only part of the messages in the 10 s of the wait (`left: 4, right: 9`), or a publication is answered `503 cluster.commit-timeout` or `403 cluster.not-member` (`replication.rs:40`). It passes alone and in the normal run of the suite.
- **Cause:** not identified. It is **not a consequence of the batches**: the same hunt on the code before them (`ab14fa5`) failed in 19 of 32 processes, on the first batch (`078c11f`) in 13 of 16 and on the code with `take` and the FIFO forward in the batch in 15 of 16; the other 19 tests of the file almost never failed (a few single failures, one test each, in the same rounds). The test loses every `channel` frame, publishes and then waits 10 s (200 × 50 ms of real time) for the repeat of the state; the manual `Clock` does not move, and the heartbeat that repeats the state runs in real time, so the reading that fits (not proven) is that under that load the margin is what is short.
- **Next step:** read the log of a failing run (`target/hang-hunt-replication/run_*.log`) to see whether the repeat comes late or does not come, and compare the `503` with the quorum of the publication; until then the test is not a signal of a regression under the hunt.
- **Do not:** raise the 10 s of the wait without knowing which of the two it is.

**History**

| Date | Event |
|---|---|
| 2026-10-29 | Found in the hunt that validated the `take` and the FIFO forward in the batch. The same hunt on the two earlier commits gave the same rate, so it was already there. |

## The chaos run: a delivery past the limit and an ack the owner never applied

**Status:** Fixed · **Found on:** 2026-10-27 · **Fixed on:** 2026-10-27

- **Where:** `src/engine/messages/mod.rs` (`take`, `take_candidates`), `src/cluster/replication/coordinator.rs` (`commit_until_done`) and `src/cluster/protocol/mod.rs` (`Request::decided`). Found by `tests/chaos.rs` ([37-guarantees.md](37-guarantees.md)).
- **Symptom 1 (I5):** with `maxDeliveries: 2` a message was delivered with `deliveryCount: 3`, in most runs of seed 3. Cause: a delivery commit that reached the owner after its own invisibility had ended put the message in the ready list without the release that checks the limit. Fix: `take_candidates` does not deliver a message that used up its deliveries: it hides it for an instant and the next release sends it to the dead-letter (the first form of the fix, an immediate move, broke `a_shorter_ttl_expires_what_is_already_stored_even_in_flight`, which expects the `ttl` to win, and was changed). Test: `a_commit_that_arrives_after_its_invisibility_does_not_deliver_past_the_limit` (fails without the fix).
- **Symptom 2 (I1):** in about 1 run in 100 a confirmed message was never delivered: it was left only on its owner (the replicas had applied its ack), its delivery needs the replicas, and so its FIFO group stayed blocked for good. Cause: the commit of the ack to the owner arrived after the owner's register had expired; the coordinator registered it again and gave up on the first refusal, either because the receipt had expired or because another operation held the message (`cluster.conflict`). Fix: the register sent again after the decision checks no receipt (`Request::decided`) and, on a conflict, keeps trying until the other operation ends. Tests: `the_register_repeated_after_the_decision_checks_no_receipt` and `an_ack_the_replicas_applied_reaches_the_owner_even_if_another_operation_took_the_message` (fails without the fix).
- **How it was found:** the first suspect was the checker itself: the trail of a message (`History::trail`) showed that the `maxDeliveries: 2` of one queue had been applied to the others (a false positive, fixed in the checker). Then the failure report was extended with where the message is on each node, and it showed the owner holding a message the replicas no longer had. The baseline without the delivery fix also failed (1 run in 120), so the second cause was independent.
- **Not fixed:** an ack and a move to the dead-letter both decided when a register expires ([PRB-085](31-pending-items.md), not seen); the failures once attributed to it were [a register that checked the message before taking it](#a-register-that-checked-the-message-before-taking-it).
- **Do not:** raise the 30 s of `eventually` or let the run stop at the first delivery: the group blocked for good is exactly what the wait is there to show.

**History**

| Date | Event |
|---|---|
| 2026-10-27 | The run (I5 added) failed on seed 3 with `delivered 3 times, over maxDeliveries 2`; the first cause was the checker, which applied the limit to the queues without it. |
| 2026-10-27 | After the checker was fixed, 1 run in 5 still failed (a real delivery past the limit); the cluster test of the late commit reproduced it and the check at `take` fixed it. |
| 2026-10-27 | A second failure, I1 (a message never delivered), 1 run in 100; the same rate without the first fix, so independent. A group blocked by a message only on its owner. |
| 2026-10-27 | Fix of the register sent again (no receipt check): still 5 failures in 300 runs of the four seeds. The cluster test with a delayed commit and another operation on the message reproduced the rest (conflict at the new register); the coordinator now keeps trying. |
| 2026-10-27 | With both fixes and 35 s clock jumps: no I1 in 400 runs, 3 failures of another shape (I3 and I5, [PRB-077](31-pending-items.md)). With 8 s jumps (below `write.pending.ttl`): **0 failures in 400 runs of the four seeds (1600 runs)**; before the fixes, about 1 in 100 failed. |
| 2026-10-27 | The check at `take` made `a_shorter_ttl_expires_what_is_already_stored_even_in_flight` fail (the `ttl` must win over the dead-letter); the final form hides the message and leaves the move to the release. Final code: the whole suite passes (47 binaries) and **0 failures in 250 runs of the four seeds (1000 runs)**. |

## A restarted test node that loses its port to another test

**Status:** Fixed · **Found on:** 2026-10-26 · **Fixed on:** 2026-11-09

- **Where:** `tests/common/cluster.rs` (`respawn_node`) and the tests that bring a node back at the address of the one that fell (`tests/cold_start.rs`: `fall`, `come_back`).
- **Symptom:** `the process restarts: Bind { name: "do cluster", addr: 127.0.0.1:<port>, source: … AddrInUse … }` after the 30 s of `respawn_node`. Seen 3 times in about 1800 process runs of `docker/hang-hunt.sh cold_start …` (10 processes at once), in `when_several_nodes_connect_the_highest_version…` and in `a_node_that_joins_offers_the_definitions_of_its_disk…`; not seen in CI so far.
- **Cause (checked in the log of the failing run):** `fall()` shuts the node down and frees its cluster port; the test starts the node again on the same port later. In between, the system hands the freed (ephemeral) port to another listener: in the log the port was the cluster port of the fallen node and, two seconds later, the **client** port of a node of **another test of the same process**. `respawn_node` retries for 30 s, but that node keeps the port until its own test ends, so the retry cannot win. The tests of a binary run in parallel threads, so the same can happen in CI.
- **Fix:** `PortHold` in `tests/common/cluster.rs`: a placeholder listener bound right after the shutdown (in `fall` of `cold_start.rs` and `regroup.rs`) and dropped just before the new bind (`come_back`).
- **Verification:** `KEEP_GOING=1 ROUNDS=180 PAR=10 docker/hang-hunt.sh cold_start` (1800 process runs) and `KEEP_GOING=1 ROUNDS=150 PAR=10 docker/hang-hunt.sh regroup` (1500): **no** `AddrInUse`, against 3 in about 1800 before. The hunt takes one test file at a time (the next arguments filter test names). The 7 other failures of the `cold_start` hunt are other issues, in their own sections.
- **Do not:** raise the 30 s of `respawn_node`; the port does not come back while the other test lives.

**History**

| Date | Event |
|---|---|
| 2026-10-26 | Seen once in the hunt that validated the fix of the lost offer (round 29, 10 processes at once). The log shows the port reused by another node of the same process. Not fixed: unrelated to that change. |
| 2026-10-26 | Two more in the 1500-run validation (rounds 19 and 141), one of them in `a_node_that_joins_offers…`: the same `AddrInUse`. |
| 2026-10-28 | **First time in CI**: the `Release` run of the push to `main` (merge of PR #34, amd64) failed in `with_every_node_back_no_message_that_was_confirmed_is_delivered_again_whatever_the_falls` (`cold_start.rs`, `AddrInUse` on the cluster port). `PortHold` added; `cold_start` (13) and `regroup` (3) pass locally. |
| 2026-11-09 | Verified: 1800 runs of `cold_start` and 1500 of `regroup` under load without `AddrInUse`. `Fixed`. |

## A node excluded while it synchronizes that cannot come back

**Status:** Open · **Found on:** 2026-11-09 · **Fixed on:** —

- **Test:** `tests/cold_start.rs::an_offered_definition_goes_to_every_member_of_the_table_and_not_only_to_the_peers_the_channel_knows` (UC-RTN-058), in `TestCluster::start_with(3, …)`, before the test does anything.
- **Symptom:** `waited 30 s for: the node to join the cluster` (`common/cluster.rs:89`). Seen once in 300 process runs of `KEEP_GOING=1 ROUNDS=30 PAR=10 docker/hang-hunt.sh cold_start`; not seen in CI.
- **Cause (found in the log):** the third node registered (`new node registered` at 09.78) and was synchronizing; under load its channel with the others dropped and came back three times (`reestablished` at 14.96, 15.07, 16.00), and at 16.57 the other two opened the vote with `reason="channel-down"` and excluded it (`yes=2 healthy=2`). The node saw it (`the sources excluded this node while it synchronized: redoing the join`), but its ID was now frozen (D106: an excluded node does not come back for `left_secs=300`), and every new join was refused with `ingresso rejeitado (freezing)` (332 times) until the test gave up. The tests' keepalive (`200ms`, `FAST`) makes the drop more likely, but the path exists in production: a node excluded in the middle of its synchronization is out of the cluster for 5 minutes ([PRB-111](31-pending-items.md#intermittent-tests-new-problems-and-limits)).
- **Next step:** decide PRB-111 (the vote of a node that is still synchronizing, or the freezing of a node that never became active) and then a test that drops the channel of a synchronizing node.

**History**

| Date | Event |
|---|---|
| 2026-11-09 | Seen once in the hunt that verified the second fix of the section below; cause read in the log and recorded with PRB-111. |

## A definition offered to a node that was still joining

**Status:** Fixed · **Found on:** 2026-10-26 · **Fixed on:** 2026-11-09

- **Where:** `src/cluster/replication/definitions.rs` (`Definitions::offer`), tested in `tests/cold_start.rs`.
- **Symptom:** in CI, `cold_start` › `when_several_nodes_connect_the_highest_version_of_each_definition_wins_even_over_the_disk_of_the_coordinator` failed with `waited 30 s for: the highest version in every node` (on the PR run of 2026-10-26), and `a_node_that_joins_offers_the_definitions_of_its_disk_and_the_highest_version_wins` failed with `waited 30 s for: the offered queue in the cluster` (arm64, the push to `release`, the same day). Both only under load. Locally: 2 failures in about 290 runs of the two tests, with 10 processes at once.
- **Cause:** a node that joins offers what its disk held (`offer`). The winner was applied on the node and sent only as a frame of the channel (`relay`), and a frame only reaches the peers that this node knows at that instant. In the captured run the third node became active and offered `only-3` at 46.965; the second node was registered in the table at 46.972, seven milliseconds later, and was not a peer yet, so the frame did not go to it. Its synchronization had listed the definitions before the source applied `only-3` (at 46.986), and the channel frame was never sent to it: it ended up active with `shared` but without `only-3`, for good. A change decided by an owner does not have the gap: `coordinate` also sends a `definition-apply` to every member of the table (`spread`), and a synchronizing node records that request with the events and applies it at the end. `offer` was the only path that delivered a new definition only through the channel.
- **How it was found:** `docker/hang-hunt.sh cold_start …` reproduced it in the container; the log had no line about the offer, so temporary lines (`HUNT`, since removed) were added at the `offer`, `learn`, `relay` and `broadcast` and the failing run was captured again. Comparing, node by node, what was offered, to whom it was sent and what each node learned showed the `broadcast` with one peer, seven milliseconds before the second node appeared.
- **Fix:** after applying what wins, `offer` releases the key's lock and sends a `definition-apply` to every member of the table (`spread`, all the definitions at once), besides the `relay`. The lock is released first: with it held, two nodes offering the same definition would wait for each other until `write.commit.timeout`.
- **Test:** `tests/cold_start.rs::an_offered_definition_goes_to_every_member_of_the_table_and_not_only_to_the_peers_the_channel_knows` (UC-RTN-058) offers a definition from node 2 and requires a `definition-apply` to nodes 1 and 3; it fails without the fix.
- **Second cause (2026-11-09):** `spread` read the table **once**, before sending. A node whose register committed a few milliseconds after that reading was not in the list, and its synchronization pulled the definitions from a member that was still applying the change (the `definition-apply` of the spread had not finished there): the node ended up active without the definition, and the frame of `relay` did not reach it either. In the captured run of `when_several_nodes_connect…` the third node spread `shared` (version 6) and `only-3` to node 1 alone at 46.077–46.089; node 2 was registered at 46.112 and loaded `shared` at version 1 from the listing of node 1, which only finished applying at 46.22; at 52.24 node 2 offered its version 4, the others refused it, and node 2 kept it, without `only-3`. The run of `a_node_that_joins_offers…` (the arm64 failure of 2026-10-26, not reproduced then) was the same: the spread of `offered` went to one member, and the third node registered 60 ms later.
- **Second fix:** when the wait of `spread` ends, the table is read again and the members that came in meanwhile also get the `definition-apply`. At that point every member of the first reading answered (it already applied), and a node that is not in the table yet has not finished its register, so it pulls the definitions only later, from members that already have the change. The owner's `coordinate` uses the same `spread` and has the same correction.
- **Test of the second fix:** `tests/cold_start.rs::a_node_that_registers_while_an_offer_is_spreading_also_receives_it` (UC-RTN-058): the `definition-apply` to node 1 is held back (the call gives up after 5 s), a new node that was cut from the network registers while node 2 waits and stops before it pulls the definitions (`pause_sync` in `topics`), and node 2 must send the change to it too; it fails without the fix (`node 2 tells the new node, which came in during the spread`). Under load the register alone may take more than those 5 s; then the attempt tests nothing, the test waits for that node to finish its join (one node synchronizes at a time) and tries again with another node, up to 5 times.
- **Verification of the second fix:** `KEEP_GOING=1 ROUNDS=120 PAR=10 docker/hang-hunt.sh cold_start`: 1200 process runs with **no** failure of `when_several_nodes_connect…` or of `a_node_that_joins_offers…` (before: 2 in 1800). The final version of the new test, in `KEEP_GOING=1 ROUNDS=30 PAR=10 docker/hang-hunt.sh cold_start`: 300 runs, no failure of it.

**History**

| Date | Event |
|---|---|
| 2026-10-26 | First failure in the CI of the PR (`when_several_nodes_connect…`); passed locally in 3 + 40 + 9 runs. |
| 2026-10-26 | Second failure, in the push to `release` on arm64 (`a_node_that_joins_offers…`), another test of the same binary and the same area. |
| 2026-10-26 | `docker/hang-hunt.sh cold_start` (10 processes at once): failure in round 13 and in round 16 of another run, both in `when_several_nodes_connect…`. With the temporary log, the cause above. |
| 2026-10-26 | Fix applied and test added (fails without the fix). The same hunt with the fix, 1500 process runs: see the next row. |
| 2026-10-26 | Verification: `KEEP_GOING=1 docker/hang-hunt.sh cold_start …`, 150 rounds of 10 processes at once (1500 runs of the offer tests): **no** `waited 30 s` for a definition, against 2 in about 290 runs before the fix. The only 2 problems were `AddrInUse` when restarting a node (see "A restarted test node that loses its port to another test"), unrelated. |
| 2026-11-09 | **Reopened**: `waited 30 s for: the highest version in every node` in `when_several_nodes_connect…`, twice in 1800 process runs of `KEEP_GOING=1 ROUNDS=180 PAR=10 docker/hang-hunt.sh cold_start` (rounds 45 and 54). The log at the default level does not say which node misses which definition; the next step is the hunt of this test alone with temporary lines at `offer`, `learn` and `spread`. |
| 2026-11-09 | The same hunt with temporary `HUNT` lines at `offer`, `apply_with` and `spread` (since removed): 2 failures in 1800 runs, one of each test of the area (rounds 15 and 148), both the second cause above. Fixed in `spread` with the test above. |
| 2026-11-09 | Verified: 1200 runs with no failure of the two tests of the area. The first version of the new test depended on the whole join fitting in the 5 s of the call and failed in the container under load; rewritten (the pause in `topics` and the new attempt) and run 300 times with no failure. The only failure of those 300 was another one: [a node excluded while it synchronizes that cannot come back](#a-node-excluded-while-it-synchronizes-that-cannot-come-back). |

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

**Status:** Fixed · **Found on:** 2026-10-20 · **Fixed on:** 2026-11-08

- **Test:** `notify.rs` › `a_notice_is_a_hint_that_ends_fetched_or_empty` (and, under load, the two other tests of the file).
- **Symptom:** a `waited 30 s` panic from `eventually_async` (`tests/common/cluster.rs`) in one run of `cargo test -p taba-node`, right after the build. Alone it passed 3 times in a row with and without the change under test, and a second full run was green.
- **Cause:** the test fabricates a node's state (a `Ready` signal of node 1) and then waits for the `GET` of node 2 to ask node 1 for a `take`. But a peer's heartbeat is the **complete** state of that peer (`apply_heartbeat`): the real heartbeat of node 1, with no ready messages, erased the injected entry, `ready_peers` came back empty, no `take` was made and the "empty" notice was never counted. Under load (the full suite, a slow runner) the heartbeat landed between the injection and the `take`. The two other tests of the file were short on time margin: in the second the notice passed to the next listener (`sent` 2 instead of 1) when its wait of 700 ms was overrun by a starved `sleep(250)`, and in the first the window of 1500 ms closed before the 24 publications ended. The server was right in all three.
- **Fix:** tests only. The first injects the old hint again until the notice is counted (it is still the same old hint); the second waits 2500 ms for a listener; the third has a window of 4000 ms and waits for a notice with more than one message instead of reading only the last quantity.
- **How it was found:** `docker/hang-hunt.sh notify` with the three tests together (12 processes at once, `KEEP_GOING=1`) reproduced it: 20 of 30 rounds failed (the test alone did not fail in 320 runs). Temporary `HUNT` lines in `receive`, `ready_peers` and `take_remote` showed `ready-received … has_ready=true` and, 4 ms later, `ready_peers raw n=0`.
- **Verification:** after the fix, 480 runs (40 rounds of 12 processes) without a problem.

**History**

| Date | Event |
|---|---|
| 2026-10-20 | Failed once in a full local run; passed alone (3 runs) and in the next full run. |
| 2026-10-22 | Failed again in a full local run (`waited 30 s for: the old notice counted as empty`); passed 6 of 6 alone right after. The change under test does not touch the notices. Cause still not identified. |
| 2026-11-08 | Failed in the CI of the pull request of the Pulumi provider (`Server (build and tests, amd64)`). Reproduced with the hunt (20 of 30 rounds), cause found in the log and fixed in the tests; 480 runs without a problem. |

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
- **A `waited 30 s` for a state that never arrives is a lost delivery, not a slow one.** Ask who was supposed to send it, to whom, and who was in the list of recipients at that instant. A `broadcast` (or a frame of the channel) reaches only the peers that exist **now**: a node that registers a few milliseconds later, and is still synchronizing, never gets it unless some other path (a `definition-apply` with `spread`, the heartbeat, the synchronization itself) covers it. A new way of delivering state to the cluster needs that second path, or a reason why it is not needed.
- **When the log has no line about the suspect path, add it, run again and take it out.** The failing run of the offer had no line about the offer. Temporary lines at each step (what was offered, to whom it was sent, what each node learned), marked with a word you can search (`HUNT`), and the same hunt, gave the order of the events in milliseconds; the lines were removed with `git checkout` before the fix. Keep the captured log (`target/`).
- **A test that fabricates the state of a node competes with the real node.** The heartbeat of a peer is its complete state and overwrites what the test injected; inject again until the effect is seen, or wait for the real state. A time margin of a few hundred milliseconds does not hold on a loaded runner: the hunt with all the tests of the binary at once finds it (a single test alone did not fail in 320 runs).
- **Look at which target failed before reading the server.** The count of the gateway was read in the node's code for a while; the failed job was the one of the emulators.
- **A client error is proved on the wire.** A capture in the server's network namespace says who closes and whether the response is well framed; then a wrapper of the client's connection and `httptrace` say why. A capture must start after the last restart of the container, or it sees an old namespace.
- **A fix is not done without a test that fails without it** (here, one that fails by timeout and not by hanging) and without repeating the run that used to hang.
- **Look at what happened before theorizing about what could.** The pending item blamed an expired register for two years of reasoning; the dump of the failing message and a log line at each register showed that nothing had expired and that a check and a reservation were two steps. A report of a failure should say where the thing is now, on each node (`diagnosis` in `tests/chaos.rs`).
- **A retry that keeps failing is a state to inspect, not a timing to wait out.** The first note here proposed that `drain_topic` try again; the real cause would have stayed hidden. Keeping the cluster up after the failure and comparing the same message on every node (`memoryMessages`, the `excluded` table) found it in one run.

## Sources and evidence

- The tests cited and `tests/common/cluster.rs` (`NO_VOTE`, `FAST`, `WAIT`, `TABA_TEST_LOG`).
- `src/cluster/channel/mod.rs` (`accept`, `receive`, `sync_members`, `drop_all`, `allow_new_vote`), `src/cluster/backup/evict.rs`, `src/cluster/sync/mod.rs` and `tests/channel.rs` (UC-CAN-029).
- `src/cluster/replication/definitions.rs` (`offer`, `spread`, `relay`, `coordinate`), `src/cluster/service/mod.rs` (`recover_previous_life`, `finish_join`, `activate`) and `tests/cold_start.rs` (UC-RTN-058); the timeline of the lost offer is in the log kept from the hunt of 2026-10-26.
- The hang of 2026-10-26 was reproduced locally and in a Linux container; the stack, the logs and the run counts are the ones in the history table.
- The failures were seen in the repository's CI logs; the "found" dates come from the git history and the logs.
