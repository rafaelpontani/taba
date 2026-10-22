---
title: Resuming the cluster after a full outage (proposal)
summary: Proposal under discussion for finding D — stopping the whole cluster in production loses messages and topics — with definitions that persist, old messages quarantined per life, a replicated deleted-messages log and a restoration in which each node sends to the new primary, which decides. None of this is implemented.
type: explanation
status: planned
verified-on: 2026-10-21 @ 9ae59ba
sources: [crates/taba-node/src/storage/versions/mod.rs, crates/taba-node/src/cluster/replication/definitions.rs, crates/taba-node/src/cluster/upgrade/mod.rs, crates/taba-node/src/node.rs, crates/taba-node/src/cluster/service/mod.rs, crates/taba-node/src/cluster/recovery/mod.rs, crates/taba-node/src/cluster/sync/mod.rs, crates/taba-node/src/cluster/drain/mod.rs, crates/taba-node/src/cluster/vote/mod.rs, crates/taba-node/src/cluster/replication/definitions.rs, crates/taba-node/src/cluster/backup/restore.rs, crates/taba-node/src/cluster/transport/mod.rs, crates/taba-node/src/cluster/replication/messages_api.rs, crates/taba-node/src/core/id/mod.rs, crates/taba-node/src/storage/messages/mod.rs, crates/taba-node/src/engine/messages/mod.rs, crates/taba-node/tests/recovery.rs]
---

# Resuming the cluster after a full outage (proposal)

> ⬜ **Proposal under discussion. Only steps 0 and 1 are implemented (see "Execution status"); the rest is still a proposal.** It exists to settle the design before the code. Once decided, it becomes a `D` in [30-decisions.md](30-decisions.md), leaves [31-pending-items.md](31-pending-items.md) (finding D) and this file starts describing what exists. Read first: [11-durability.md](11-durability.md), [07-cluster.md](07-cluster.md).

## Execution status

Executed in steps (section 7), all in the working tree, no commit. When everything is done, this document is rewritten as a description of what exists (an operations manual), and development mode becomes only configuration.

| Step | Status | What was left |
|---|---|---|
| 0. Version on entry and drain of the previous ones | ✅ **done** | Code in `cluster/upgrade/` (pure rule `decide`, loop `run_upgrade`, condition `upgrade-stalled`, `peer_supports`); decision **D165**; cases **UC-ING-029 to UC-ING-035** (catalogue 26a); tests in `tests/upgrade.rs`, `cluster/upgrade/tests.rs` and `cluster/health/tests.rs`; operation in [23-operations.md](23-operations.md#upgrade-the-version-rolling) |
| 1. Persist the versions and tombstones of definitions | ✅ **done** | `definition_versions` table in `_node.db` (schema 6, `storage/versions`); `Definitions` writes at each application and reads at start **only when it resumes the definitions from disk**; the `topicId` goes in the tombstone and in the tombstone stream; decision **D166**; cases **UC-DEF-039 and UC-DEF-040**; tests in `storage/versions/tests.rs`, `cluster/replication/definitions/tests.rs` and `tests/definition_versions.rs` |
| 2. Durable excluded log | ✅ **done** | **2a:** `excluded` table in each topic's database (`storage/messages/excluded`), written in the same transaction as the ack, the `prune`, the send to the dead-letter and the FIFO ack on every node that keeps a copy; limits `storage.excluded.max-size`/`max-age` and trimming every 30 s; start date in `_node.db` (`state.excluded_since`); `merged_since` and `import_excluded` for the log of past lives; decision **D167**; cases **UC-ESC-037, UC-ESC-038, UC-ARM-026 to UC-ARM-029**. **2b:** `BlockExcluded` request (`GET /cluster/blocks/{id}/excluded`) copied together with each block in the synchronization (`cluster/sync`); the new node inherits the start date from the sources (mark `excluded_inherited`); `excludedSince` in the `NodeStatus` and `summary.excludedCutoff` in `GET /health/cluster`; decision **D168**; cases **UC-SIN-031, UC-SIN-032, UC-OPS-052**; tests in `storage/messages/excluded/tests.rs` and `tests/excluded_log.rs`. NOVO-ING-05/06 stay in stage 3 |
| 3. Quarantine per life and restoration | 🟡 **in progress** (sub-stage 3a done) | **3a done:** `cluster/restore` module with the pure rules (restorable life, expected nodes, start of the restoration with `G`, strict mode and force), the life manifest (`manifest.json`) and the `cluster.restore.*` parameters; the **property test of section 4.8** over a model of these rules; decision **D169**; cases **UC-RTN-018 to UC-RTN-022 and UC-CFG-013**; tests in `cluster/restore/tests.rs` and `config/tests.rs`. **Missing:** 3b folder per ID with the manifest written in production, periodic `last_seen` and the announcement of manifests; 3c sending to the new primary and the check of the dead (local log, other nodes, S3); 3d end of the restoration (vote, held FIFO, force, `previous-life-pending`); 3e cold start (taking over the disk, coordinator, definitions); 3f S3 scan at start; 3g probing and regrouping; NOVO-ING-05/06 and NOVO-DEF-04/05 go with 3b and 3e |
| 4. Remove `mode=development` and create the one-node profile | ⬜ pending | |

**Deviations from the design in step 0** (section 4.12 below is already corrected):

- Each member's version does **not** go in the member table or in the join: `Member` and `Change` have `deny_unknown_fields`, and a new field would make the already published versions reject the table. The node reads the version in the `version` of the `NodeStatus` (`Request::Status`), which every release already publishes; a member that does not answer the route or brings no version counts as earlier.
- **One node at a time** acts: the lowest `nodeId` among those of the newest version (instead of "the node that joined"), so two nodes do not ask for different drains.
- Cases **NOVO-ING-05** (no life restorable with an earlier node present) and **NOVO-ING-06** (disk of an earlier version not migrated) were **not** done: they depend on the excluded log and the quarantine (steps 2 and 3).
- Mapping of the cases: NOVO-ING-01 → UC-ING-029; 02 → 030; 03 → 031; 04 → 032; 07 → 033; new: UC-ING-034 (one node acts) and UC-ING-035 (version comparison).
- Mapping of step 1: NOVO-DEF-01 → UC-DEF-039; NOVO-DEF-02 → UC-DEF-040; NOVO-DEF-03 was already covered by UC-DEF-026, UC-DEF-027 and UC-DEF-035 (the higher version wins, a tombstone of a higher version beats the creation), with no new case; NOVO-DEF-04 and NOVO-DEF-05 stay for step 3 (cold start).
- A **process** restart that reads the rows only happens today in development mode (`resume-on-start`); in production the node starts clean and reading at start becomes the general rule in step 3. So the proof of "survives the restart" is split: the database reopens with the rows (U) and the reading rule keeps only what describes the loaded definition (U); the cluster proof (C) covers the writing on every node.

**Values and names fixed** (they become `D` when the step that uses them is done; `G`, the strict mode, the slack and the probing are already `cluster.restore.*` parameters, D169): `G` = 5 min; strict mode in `cluster.restore.strict=false`; force = `POST /cluster/restore/force`, root token only; folder per ID (the node does not reuse the ID); the S3 sweep adds to the `restore()` of the single survivor; excluded log of 100 MB and 24 h; clock slack of 5 s; probing for 2 h every 10 s.

## 1. The problem

In production, **stopping the whole cluster loses messages and topic definitions**: `docker compose down` + `up` with the volumes kept starts a cluster with no topic at all ([11-durability.md](11-durability.md), scenario "all nodes stop at the same time, without backup"; finding D in [31-pending-items.md](31-pending-items.md)). With the S3 backup ([33-s3-backup.md](33-s3-backup.md)) the cluster restores itself, but without it there is no way out. A maintenance that stops everything, or a general outage, zeroes the cluster.

When **some** nodes fall, the cluster recovers with no problem: the replicas have the message and the new node synchronizes. The problem is only the **full outage followed by a cold start**, when no live node is left to say what exists.

**Scope.** The proposal handles the most severe case: all nodes die **without any signal** and come back together. What was **only in memory** on all the nodes is lost; writing to disk on commit is outside this proposal. Only what was already on disk is restored.

## 2. How it is today (checked in the code)

| Point | Behavior | Where |
|---|---|---|
| Identity | Every process is born with a **new ID** | `node.rs` (`NodeShared::new(id::new_at(...))`) |
| Topics and tokens | In production they are **not read from disk** at start (`load = dev && resume_on_start`); they come from the cluster through synchronization | `node.rs`, `engine/topics`, `engine/tokens` |
| Definition versions | The version map and the definition tombstones live **in memory only** (tombstones expire in 1 h) | `cluster/replication/definitions.rs` |
| Messages | The previous life's disk is set aside in `_previous-life`; the node joins **empty** and synchronizes | `cluster/service/mod.rs` (`recover_on_boot`), `storage/messages` |
| Recovery | After becoming active, the node returns **only the messages the peers list as losses** and then deletes the folder | `cluster/recovery/mod.rs` (`run_streaming`, `held_previous_life`) |
| First node | `start_alone()` releases the old disk and the sweep deletes it. Case **UC-RTN-017** and its test pin this down (D111) | `cluster/service/mod.rs`, `tests/recovery.rs` |
| Planned stop | Each node flushes memory, writes the save mark and **leaves the member table**; the disk stays intact and nobody reads it again | `cluster/drain/mod.rs` |
| S3 backup | `restore()` **skips topics that already exist** and only restores the messages of the topics it just recreated | `cluster/backup/restore.rs` |

## 3. Why the ID is new today

The new ID is a defense: a node that was down for a while and got **out of sync** must not bring back data that was already deleted or is out of date. Since the node is born empty and the cluster rebuilds everything, this class of error does not exist. The price is that **every restart resynchronizes all of the node's blocks** (the new ID changes the position on the ring) and that a full outage loses everything. Any change here must **keep the defense**.

## 4. The proposal

**Central idea: availability first, truth afterwards.** The cluster comes up and serves **new** messages right away. The **old** messages stay in quarantine on disk and are only delivered again once the block's new primary checks that they are not dead.

### 4.1 Cluster data and life data

| Data | Persists across lives? | Where |
|---|---|---|
| Topics and tokens (definitions), versions and tombstones | **Yes**, and it is loaded at start | the node's database (`_node.db`) |
| Messages | Stay **in the folder of the life** that wrote them | folder named by the old ID, in quarantine |
| The life's member table and its version (manifest) | Yes, next to the folder | the life's manifest |

Since the node already knows the topics and tokens, it **receives and delivers new messages from the first second** (ordinary queues; the FIFO waits, section 4.10).

### 4.2 Definitions (topics and tokens)

- Each definition already has a **version** and an **owner** on the ring, with a fingerprint tie-break. ✅ **Implemented (step 1, D166):** the version map and the tombstones are persisted (`definition_versions`), with the tombstone **together with the `topicId`** (a recreated topic has another `topicId`); reading at start only applies today when the node resumes the definitions from disk.
- When two states meet, the **higher version wins**, definition by definition. A global cluster version is not needed: the per-definition version is enough, and the member table version is already global and serialized.
- In a cold start, **the node with the lowest ID coordinates** the gathering of the definitions of all connected nodes (section 4.9). If the cluster is already up, the node joins clean and receives its definitions through synchronization, as today.

### 4.3 Message state: what really needs a version

- **Deletion (ack, or move to the dead-letter):** it is an **absorbing** state. There is no version after "dead". When states meet, **if any copy says dead, dead wins**.
- **Invisibility and receipt:** on resume they are **zeroed** and the `deliveryCount` keeps the highest value among the copies. That is what the loss recovery already does today (`apply_local`). The worst effect is an earlier redelivery, accepted in *at-least-once*. A per-message revision is not needed.

### 4.4 Deleted-messages log

- Every node that keeps a copy of a block, **primary or not**, records in its **local SQLite** the `id` of every message deleted from it (ack, or move to the dead-letter), with the topic (and the `topicId`) and the moment. The ack writes the record **before confirming** (today the ack deletes the file and the row, D12). So the source of truth about what died is **replicated on several nodes**.
- The log has a **size limit and a time limit**, configurable (for example 100 MB and 24 hours; the parameter names are still to be defined). The oldest records leave first.
- Each log has the **start date of continuous recording**: the instant from which it guarantees to have the deletions. It starts at the creation of the log and only advances when a record is dropped by the limit (so it is not the oldest record: a cluster with no deletions for three days covers the three days). A new node **inherits** that date from the sources when it synchronizes its blocks, so it does not pull the date to the day it joined.
- The **cluster's cutoff date** is the **most recent** of those dates among the active members (the weakest link). It is not the date of the oldest log: each node only records the blocks it keeps, and a low-traffic node, which keeps its log for a long time, did not see the deletions of the other nodes' blocks. With the weakest link's date, every deletion after it is recorded on every owner that recorded it. A node that drops records early because of size shortens the whole cluster's window; the limit should be sized with that in mind, and the cutoff date should be visible to the operator.
- The **logs of past lives** still on disk are read and **incorporated into the current log**, respecting the total time and size. They only add evidence (more dead `id`s). The recording start date only goes back if the old interval and the sources' interval meet, with no hole; if the sources already dropped the period in which the node was stopped, the date stays after the hole.
- A life can only be restored if its **last activity** (`last_seen`, in the manifest) is later than the cluster's cutoff date, with a margin for clock differences. Outside the window, the life goes back to today's path (new identity, disk discarded, only the listed losses).
- **Durability.** The record is written **in the same transaction** that deletes the message, and `storage.db.synchronous` **stays `NORMAL`** (no extra fsync on the ack). A system crash may lose the last transaction window of the log; this fits *at-least-once* (section 4.11). The cost on the ack is one more insert, and it must be measured in step 2.
- **Clocks.** Comparing `last_seen` (clock of the node that wrote the life) with the cutoff date (clocks of other nodes) uses a **configurable margin** (name and value still to be defined, on the order of seconds). If the real drift is larger than the margin, the **risk is accepted**: a life may be restored outside the window, or blocked inside it, and the consequence is the residual redelivery of section 4.11.
- **S3 is not proof of life.** With the backup active, the S3 journal serves only as **proof of death** (section 4.6). It does not confirm that a message is alive, because the journal lags by up to `backup.meta.interval` ([33-s3-backup.md](33-s3-backup.md)): a recent ack may not be in it yet. So S3 **does not widen the window** above, and does not remove the need for the clock. It is also a **source of candidates** (section 4.13), which go through the same dead-message check.
- Synchronization already carries short rows with `delete_at_ms` (`apply_tombstone`, today for the FIFO's deduplication). The idea is to **generalize** it to every queue, and the records follow the block when the owners change.

### 4.5 Quarantine per life

- Each life has **its own folder, with the old ID in the name**. Today, if the node restarts more than once before the recovery finishes, the code keeps only the first folder and deletes the new one; with a folder per ID several lives coexist.
- Each life has a **manifest**: the ID, the **member table** (with the IPs) and its **version** at the time of the stop, the instant of the **last activity** (`last_seen`, written periodically and on a clean stop) and the **high-water mark** of the highest `id` (section 4.10). On start, the node uses the IP table to find the old nodes again (section 4.9) and **announces** the manifests it keeps, so the cluster knows how many nodes were expected (section 4.8).
- The quarantine is **not deleted** by today's recovery; only at the end of the restoration (section 4.7).
- **No space ceiling of its own.** The life folders take disk until the final vote. There is no separate limit or cleanup; if the disk fills, the usual behavior applies (the node goes to `failed`, as with the backup `outbox`).

### 4.6 Restoration: each node sends, the new primary decides

- Each node reads the folder of each life in quarantine, block by block, and **sends the messages to the block's new primary** (the one in the current ring), **with the original ID**, through the `Recover` path with replication that already exists. The node **decides nothing** about the message: it only sends.
- The **new primary receives and handles** each message. Before loading it, it checks whether the `id` is dead, from the cheapest source to the most expensive:
  1. in its own **local deleted-messages log** (section 4.4);
  2. on the **other nodes**: it asks all the present nodes, in a batch per block (a list of `id`s; the answer is the subset recorded in their log). It asks **all** of them, not only the block's owners, because the new owners are not the old ones and the logs live on the nodes, not on the ring;
  3. in **S3**, if the backup is configured: the bucket's journal records each ack as a removal (D144). A message removed in the journal is dead. An unreachable S3 is missing evidence: the primary decides with the first two sources and logs a warning.
- **Topic.** Before anything else, the primary checks the topic: if it was **deleted**, or **recreated with another `topicId`** (the life's one is recorded next to the topic in the folder), the message is **discarded**, without querying the dead. This holds for the whole quarantine and is tested; today the sweep already deletes messages of unknown topics.
- If **any source** says it died, the primary discards it. Otherwise it **activates** it: it replicates to the current owners, with the quorum, and makes the message available. If it already has it (`Existing`), it skips.
- A message that is **only in the life**, within the window and with no deletion record in any source, **is activated**: it is either a confirmed message or a publication that did not reach the quorum, and delivering it is acceptable in *at-least-once*.
- The sources only **add evidence of death**. None of them widens the window in section 4.4.

### 4.7 End of the restoration

- **Durable receipt.** The new primary writes the restored message **to disk** and replicates it with the quorum before confirming. Only then does the origin node mark it as **sent**, and it is not sent again on a later start. That way a second crash does not lose a message that was saved. The **life's folder is only deleted at the end of the restoration**, after the vote below. When the node has sent all its lives **and finished the S3 scan** (section 4.13), it announces that it finished.
- **Vote.** The coordinator (the node with the lowest ID, section 4.9) opens the vote when **all the present nodes** have announced that they finished. The decision requires **all the present nodes**, because each one has disk to send; the expected nodes that have not come back yet are handled by the deadline in section 4.8. The vote changes the cluster status to **restored from disk**, a name that is not confused with the S3 restore. Only then are the **FIFO queues released** (section 4.10) and the lives' folders deleted.
- **If the vote does not close** (for example, in strict mode, while an expected node is missing), the FIFO stays undelivered and the cluster shows a visible condition (for example `previous-life-pending`). The force operation (section 4.8) changes the status.
- **A node that comes back after the release.** It joins as a new, clean node, and its lives follow the path in section 4.6. Its FIFO messages are activated in `id` order and may be delivered after newer messages of the group that were already delivered: the order of those messages is not guaranteed. It is the exception, not the rule.
- The **ordinary queues** do not wait for the vote: they are released as soon as the definitions are distributed (section 4.9), and the old messages come in meanwhile.

### 4.8 Nodes that do not come back

- The restoration of the old messages only **starts** when all the expected nodes (the members of the highest old table, from the manifests) have connected, or after a configurable deadline `G`. This does not delay the distribution of the definitions nor the release of the ordinary queues. The classic majority-of-the-table rule does not hold here, because the exclusion vote uses the **healthy** nodes as its base (`ceil(S × 51%)` of `S` healthy, [vote/mod.rs](../../crates/taba-node/src/cluster/vote/mod.rs)), and a healthy pair can exclude the majority of the table; that is why it waits for **all** of them, not for a majority.
- **After `G`**, in the default mode the restoration **goes on with a warning** and uses the present nodes. In **strict mode**, a setting, it **holds until the force operation**. The initial value of `G` is configurable (on the order of a few minutes) and is still to be defined.
- The **force** operation changes the cluster status and releases what is held, with the risk stated (messages already consumed may reappear, section 4.11). Otherwise the FIFO is never released if a node never comes back.
- The same operation serves finding C (a cluster stuck after losing two nodes).

### 4.9 Start: how the node decides

1. All nodes start **together**, each with a **new ID**, and the first thing is to try the nodes of the **previous life's IP table** (kept in the manifest, section 4.5), besides `cluster.entry-address`.
2. If **none answers**, the node **assumes it is the first** and takes what is on disk: it creates a table with just itself and loads topics, tokens, versions and tombstones. The **old messages stay in quarantine**.
3. When **several connect**, the one with the **lowest ID is the coordinator**. It gathers the definitions of **all the connected nodes** and applies the highest version of each (definition tombstones count), and distributes them to the others. Gathering only its own disk's is not enough: it may be the node that was out earlier and has the old disk. From then on, the **ordinary queues receive and deliver**; the FIFO does not yet (section 4.10).
4. A node that arrives **later**, with the cluster already up, joins through today's join, as a **new, clean node**, and receives the cluster's definitions. Its data on disk follows to the restoration (section 4.6).
5. If **two or more groups** of nodes assume the disk because they could not reach each other at first, the **tie-break** that already exists in the partition reunion applies (D101): the side with fewer members loses and, on a tie, the one with the lowest `nodeId`. The losing side redoes the join with a new ID and **reuses the data it has on disk**. Since definitions are resolved by version and messages by the dead-message check, the tie-break only decides who hydrates; it does not decide which data is the truth.
6. **Accepted window of risk:** a late node that starts alone serves with the definitions it has, and a token revoked while it was away would be accepted. The window ends as soon as any node answers, when the higher version wins, and it is bounded by the tokens' expiry.
7. The `cluster.first-node` flag is still needed only for the cluster that **never existed**, with no previous table.

### 4.10 FIFO queues during the restoration

- The FIFO is **only released** when the cluster status changes to restored from disk (section 4.7), because only then is everything that was on disk already at its primary. Until then it **receives messages and does not deliver them**.
- New messages go into a **provisional list** on the group's new primary, marked as **having no effective position** in the queue. They get a normal `id` right away, and the ID generator starts above the **high-water mark**, the highest `id` of the old life, kept in the manifest: new IDs are always greater than the old ones, and no `id` already returned to the producer changes afterwards.
- The **restored** messages reach the group's new primary through the path in section 4.6 and also stay undelivered until the release.
- **No deduplication happens during the restoration:** every publication is accepted.
- At the release, the primary **makes the provisional list effective**. It deduplicates against the restored keys and within the list itself, in `id` order, discards the duplicates, and only then releases the delivery.
- Accepted consequence: the producer of a message that is later discarded as a duplicate received "published" (`duplicate=false`) in the response. This will be the **exception**, not the rule.

### 4.11 Guarantees and limits

- The restoration keeps *at-least-once*: the messages that were on disk are restored, and the ones already consumed do **not** come back, **except in the two cases below**.
- **Residual redelivery (accepted):** an already consumed message may be delivered again when (a) a node that has deletion records is not present (after the deadline `G`, or when only old nodes that confirm each other came back), or (b) a crash makes the deleted-messages log lose its last entries (log durability, section 4.4). The consumer must be **idempotent**, as is already required for the S3 restore.
- With **all the expected nodes present**, redelivery through (a) **does not happen**; that is what the property test in section 4.8 must prove.
- What was **only in memory** on all the nodes is lost (section 1).

### 4.12 Version upgrade

The new formats (manifest, folder per ID, `_node.db` schema, deleted-messages log) and the new messages between nodes (manifest announcement, dead-message query, coordination, vote) are contracts ([24-conventions.md](24-conventions.md)). Coexistence of versions is **not handled by compatibility between them**: the new version, on entering the cluster, **retires the previous ones**.

- ✅ **Implemented (step 0, D165).** Each node publishes the program **version** in its `NodeStatus` (the member table and the join do not change). Only the node with the lowest `nodeId` among those of the newest version acts: it **drains the earlier ones, one by one**, through the drain that already exists ([cluster/drain/mod.rs](../../crates/taba-node/src/cluster/drain/mod.rs)); the next only starts when the previous left the table. Code in [cluster/upgrade/mod.rs](../../crates/taba-node/src/cluster/upgrade/mod.rs).
- The next drain **only starts** if the cluster keeps at least 3 active nodes and the node that entered has finished synchronizing ("Reduce" item of [23-operations.md](23-operations.md)). If a node does not drain in time, the process stops there and shows a **visible condition**; the operator decides (cancel the drain or stop the node).
- During coexistence, **only what both versions understand circulates**: the new messages between nodes are **not sent** to whoever did not announce support for them.
- While a node of the previous version exists, it **does not write the deleted-messages log**. By the rule in section 4.4 (the cutoff date is the weakest link's), this means **no life is restorable** until that node leaves; coexistence therefore never creates a false restoration. When the last previous node leaves, the log counts from the date on which **all** nodes record.
- The **disk** of a previous version is **not migrated**. The new node reads what both versions understand and follows today's path (previous life, only the losses that peers list). The new formats exist only on new nodes. Nodes retired by drain come back, if they do, as new, clean nodes (section 4.5).
- **Out of scope:** going back to a previous version after a node has written the new formats. A previous version that enters does **not** drain the newer ones (the rule applies only in the new → old direction).

### 4.13 S3 scan at start

With the S3 backup configured, the node's start has a **last step**:

- The node **lists what exists in S3** (segments and journal) **for the blocks it is primary of** and **indexes the live messages**: the ones in a segment with no removal in the journal. This is what allows recovering also what was lost from the disk.
- The messages found go through the **same path** as section 4.6: topic (section 4.6), dead-message check on the local log and on the other nodes, and activation with quorum. The ones the node already has (`Existing`) are skipped, as when the disk already brought them.
- The step **does not keep the cluster from becoming available**: definitions and ordinary queues do not wait for it. It only **holds delivery of the FIFO queues**: the "I finished" announcement of section 4.7 includes the scan, so the FIFO is released only after it.
- An unreachable S3 blocks nothing: the step is skipped with a **warning**, and the node goes on with the disk only.
- **In the implementation:** today `restore()` skips topics that already exist and `find_message` returns nothing both for "removed" and for "not found"; the scan has to tell the two apart and read the journal **once per topic**, with a cache.

### 4.14 Reunion of groups that assumed the disk

Today's reunion only fires for nodes that excluded each other by vote (`excluded_members`). Two groups that started without talking (section 4.9, item 5) never enter that list, so they need to look for each other another way.

- **Probing the old IPs.** While the cluster has **fewer members than the table of the previous life** (from the manifest, section 4.5), each node tries the missing IPs from time to time. Two values are configurable: the **total time** of the probing (for example 2 hours) and the **interval** between attempts (for example 10 s). The parameter names are still to be defined.
- The probing **ends** when the table is complete or when the time runs out. If it ends with nodes missing, the cluster shows a **visible condition**; from then on the missing node enters through the normal join when it returns.
- When one group reaches another, the D101 tie-break applies (fewer members loses; on a tie, the lowest `nodeId` wins).
- **The loser erases nothing.** Today the reunion, for the loser, uses `handle_exclusion`, which erases the local state. Here the loser **moves its disk to the quarantine** (folder with its ID, section 4.5), **hands over its definitions** (the highest version of each wins, tombstones count) and redoes the join with a new ID. Its messages follow the path of section 4.6, like those of any life in quarantine.
- What the losing group accepted while it was apart lives in its life's folder, so it too is restored and checked against the dead.

### 4.15 The start process, step by step

The table puts together what the code does today (production, in a cluster) and what the proposal **keeps**, **changes** or **adds**, in the order it happens.

| # | Step | Today | In the proposal |
|---|---|---|---|
| 1 | Identity | New ID on every process (`id::new_at`) | **Keeps.** The old ID becomes the **folder name** of the life (4.5) |
| 2 | Node database (`_node.db`) | Opens. Topics and tokens are **not** loaded in production (`load = dev && resume_on_start`) | **Changes.** Opens with the new schema (versions and tombstones, 4.2) and **reads the definitions** into memory, but only uses them if the node assumes the disk (step 6) or as its contribution to the coordinator's gathering |
| 3 | Messages on disk | Waits (`new_keeping_disk`). The folder is only set aside in `_previous-life` **after** the listing found the cluster; if the node starts alone, the disk is discarded | **Changes.** The disk **always** becomes a **life folder in quarantine**, with a manifest (4.5), whether the node found the cluster or not. Several lives coexist. Nothing is deleted before the final vote |
| 4 | Local S3 backup (`_backup.db`) | Reset on every production start | **Keeps** |
| 5 | Discovery | Listing (`ListNodes`) at `cluster.entry-address`. With no answer, only whoever has `cluster.first-node` starts alone, or, with S3, whoever the bucket authorizes (`startup_decision`) | **Adds** the **IP table of the previous life** (from the manifest) to the attempts. With no answer from anyone, the node **assumes it is the first** (4.9); `first-node` is left only for the cluster that never existed. With S3, the bucket's decision still holds for whoever starts alone |
| 6 | Starting alone (`start_alone`) | Table with just itself, version 1; **releases the old disk for the sweep to delete** | **Changes.** Creates the table and **assumes the disk**: loads topics, tokens, versions and tombstones (4.9). The messages stay in quarantine and come in through the restoration (4.6) |
| 7 | Several nodes connect together | Does not exist: each one enters by the join, one at a time | **Adds.** The one with the **lowest ID coordinates**: gathers the definitions of **all** connected nodes (highest version wins) and distributes them (4.9). Groups that did not reach each other are settled by 4.14 |
| 8 | Join and synchronization | `Join` (with ID, addresses, port and **program version**) to a random member; synchronization brings definitions, blocks and events; the node becomes `Active` | **Keeps.** A node that arrives later enters **clean** and receives the cluster's definitions. The **version** the join already carries is used by the drain of the previous ones (4.12) |
| 9 | Recovery of the previous life | In the background, returns **only the losses the peers list** and deletes the folder (D111) | **Changes.** Sends **all** the quarantined messages to the new primary of each block, which checks the dead (4.6); waits for the expected nodes or `G` (4.8) |
| 10 | S3 scan | Does not exist at start; only the single survivor's `restore()` | **Adds.** Last step of the start, per block the node is primary of (4.13) |
| 11 | Queues | Everything available once the node is `Active` | **Changes.** **Ordinary** queues receive and deliver as soon as the definitions are distributed; the **FIFO** only receives until the **restored from disk** status (4.7, 4.10) |
| 12 | End | The folder is deleted when the recovery ends | **Changes.** Vote of the present nodes, **restored from disk** status and only then are the folders deleted (4.7) |
| 13 | In the background | Nothing | **Adds.** Probing of the old IPs (4.14) and periodic writing of `last_seen` in the manifest (4.5) |

**What does not change:** a node entering a cluster that is already up (join, synchronization, quorum), the drain, the freezing and the exclusion vote.

### 4.16 The end of development mode: the one-node profile

Today `mode=development` is a node without a cluster, with persistence of its own (`dev && resume_on_start` in `node.rs`), no TLS, no authentication and the FIFO accepted without a replica. With the proposal, the production start already assumes the disk (section 4.15), so that mode **ceases to exist**. There is no `mode` anymore: **every node is a cluster node**, and development uses the same path with a minimal configuration.

**Production default.** All default values remain the production ones, for **at least 3 nodes**: the production `replication.factor`, cluster TLS on, authentication by the production configuration, `cluster.entry-address` required. Nothing below changes the default.

**The one-node profile** is a documented set of parameters that the developer turns on on purpose:

| Parameter | Value in the profile | What it does |
|---|---|---|
| `replication.factor` | `1` | A single node. The write quorum comes from the factor (the majority of 1 is 1; `write.quorum=1` also holds). The node starts alone, **without** `cluster.entry-address` and **without** `cluster.first-node` |
| `cluster.tls.enabled` | `false` | Turns off cluster TLS, with no secrets folder. **Only accepted with `replication.factor=1`**: with more than one node the process refuses to start |
| `auth.dev-token` | a secret of at least 32 characters | **Creates**, at start, an **`admin`** work token (not the root) with that secret, **if it does not exist yet**, for examples, Compose and tests. Turns authentication on. **Only accepted with `replication.factor=1`**; the documentation marks it **for development only** |
| `health.single-node-condition` | `true` (default) | Shows the **reduced profile** condition in `/health` when the node runs with something relaxed (factor 1, TLS off or `auth.dev-token`), listing what it is. It is informational: it does not take `/health/ready` down. `false` hides it |

Final names and values are still to be defined.

**The `dev-token`.** It is an ordinary work token, stored like any other (only the secret's hash, never in the clear), with the `admin`, `publish` and `consume` permissions. `admin` includes `manage` and `create`, and has no prefixes. The start creates it if it does not exist, and if the parameter's secret changed, updates it. Since it is not root, it **does not do what only root does**: stopping the whole cluster and creating, changing or revoking other `admin` tokens. Without `auth.root-token`, those operations are unavailable in the one-node profile.

**Rules that change in the code**

- **FIFO with factor 1.** The FIFO quorum becomes `min(2, factor)`, and the refusal of a FIFO with a factor below 2 (`fifo_needs_replication`) goes away.
- **Persistence.** The path of section 4.15 applies: with no other node, the node assumes the disk and restores the quarantine to itself. It is the primary of all blocks, and the end-of-restoration vote has a single voter.
- **Starting clean:** `development.resume-on-start=false` ceases to exist. The volume or directory is erased.

**Leaves the code:** the `Mode`, the direct loading of topics, tokens and messages, the save mark as a condition for resuming, and the "no cluster" branch (`Solo`) of the backup, because the one-node node **has** a cluster, with just itself.

**Cost.** The mode appears in `docker-compose.dev.yml`, in the Helm tests (`uc-02-desenvolvimento.sh`), in `docker/cluster.sh`, in tests (for example `Mode::Development` in the drain) and in several documents. Migrating those pieces is part of step 4.

**Example** of a node for development:

```
TABA_REPLICATION_FACTOR=1
TABA_CLUSTER_TLS_ENABLED=false
TABA_AUTH_DEV_TOKEN=any-secret-with-32-or-more-characters
```

## 5. Risks and open points

There is no design point left open. A decided point leaves this list and is described in the proposal; codes are not reused.

What is left to define is **values and names**: the deadline `G` and the strict-mode setting (section 4.8), the limits of the deleted-messages log and the clock margin (section 4.4), the time and interval of the probing (section 4.14). Before the code of step 3 comes the **property test** of the rule in section 4.8.


## 6. Alternatives considered

| Alternative | Why not |
|---|---|
| The `first-node` resumes alone from disk (like development mode) | The other nodes discard their disks and lose messages only they had; a network error on the `first-node` with live peers would make it start alone with old data and split the cluster; it requires a coordinated cold start |
| A single cluster configuration version | It does not detect what the `ack` changed: a node that was out for a week has the same version and still keeps messages already consumed |
| "The node with the highest version wins" for messages | Messages are partitioned by the ring; the node with the highest version does not have everything. The right thing for messages is the **union** of what each node has, checked against the dead-message logs |
| Releasing each block by witnesses (old owners that announce the manifest) | It forces computing who the old owners were with the factor, the quorum and the ring **of the previous life**, and proving which table is the final one. The flow in which each node sends everything to the new primary, which checks the dead messages, avoids that computation |
| A stable ID across lives | It would allow resynchronizing only the delta, but it brings back the out-of-date node that reappears. It is the larger change and is left for later |

## 7. Suggested steps

0. ✅ **Version on entry and drain of the previous ones (done)** (section 4.12): version announcement and drain, one by one, of the nodes of a previous version. It comes before step 1, because step 1 already changes the disk format.
1. ✅ **Persist definition versions and tombstones (done, D166)**, with the `topicId`, and load them at start. It does not change the API's visible behavior.
2. **A durable deleted-messages log**, with a size and time limit, on every owner of the block and propagated with it.
3. **Quarantine per life and restoration:** folder per ID, manifest, coordination by the node with the lowest ID, sending to the new primary, dead-message check (local log, other nodes and S3), the end-of-restoration vote, the S3 scan at start (section 4.13), the FIFO release and the force operation. Preceded by the property test in section 4.8.
4. **Remove `mode=development`** and create the one-node profile (section 4.16), after step 3 works.
5. **Out of scope:** writing to disk on commit, which would also preserve what was only in memory.
6. Decide later on the **stable ID** and on resynchronizing only the delta.

## 8. Questions to decide

1. What deadline and what form does the **force-release** operation have, and who may call it? (section 4.8)
2. Is "folder per ID" the right reading, or is the intent for the node to **reuse the same ID**? (sections 4.5 and 6)
3. What is the initial value of `G` and what is the strict-mode setting called? (section 4.8)
4. With S3, does today's single-survivor restore (`spawn_restore`) stay as it is, when the disk is empty, alongside the scan of section 4.13, or does the scan replace it? (sections 4.13 and 4.15, step 5)

## 9. Use cases

The cases below prove that every rule of the proposal has an observable behavior and a planned test ([26-use-cases.md](26-use-cases.md)). **They stay in this document until the implementation**: only after they are executed (with a test) are they moved to the official catalogue ([26a](26a-implemented-use-cases.md)), and each one then gets the **next free number** of its area (`UC-AREA-NNN`). Until then they carry a **provisional** code `NOVO-AREA-NN`, which does not exist in the catalogues and cannot be cited in a test. Level: **U** unit, **I** one node, **C** simulated cluster.

### 9.1 Definitions, deleted-messages log and quarantine

| Code | Case → result | Level | Section |
|---|---|---|---|
| NOVO-DEF-01 | Topics, tokens, versions and tombstones (with `topicId`) are written to `_node.db` → they survive the process restart | C | 4.2 |
| NOVO-DEF-02 | The tombstone keeps the `topicId` → it does not erase a topic recreated with the same name and another `topicId` | U | 4.2 |
| NOVO-DEF-03 | Merging two definition states → the highest version per definition wins, and a tombstone of a higher version beats the definition | U | 4.2 |
| NOVO-DEF-04 | A node that starts alone and assumes the disk → loads topics, tokens, versions and tombstones and serves without waiting for the cluster | C | 4.9 |
| NOVO-DEF-05 | A node that arrives with the cluster already up → joins clean and receives the cluster's definitions; its own disk ones do not override them | C | 4.9 |
| NOVO-ESC-01 | Ack of a message → every node holding a copy (primary or not) writes the deletion record to the local log **before** confirming | C | 4.4 |
| NOVO-ESC-02 | Sending to the dead-letter → also writes the deletion record | C | 4.4 |
| NOVO-ARM-01 | The log has a size and time limit → the oldest records leave first | U | 4.4 |
| NOVO-ARM-02 | Start date of continuous recording → starts when the log is created and only advances when a record is dropped by the limit (a cluster with no deletions for three days covers the three days) | U | 4.4 |
| NOVO-ARM-03 | Logs of past lives → are incorporated into the current one, and the start date only goes back if there is no gap between the intervals | U | 4.4 |
| NOVO-ARM-04 | The record is written in the same transaction that deletes the message, with `storage.db.synchronous=NORMAL` → there is no extra fsync on the ack | U | 4.4 |
| NOVO-SIN-01 | The block's owners change → the log follows the block, and the new node inherits the start date from the sources (it does not pull it to the join date) | C | 4.4 |
| NOVO-SIN-02 | Cluster cutoff date → it is the **most recent** among the active members (the weakest link) | C | 4.4 |
| NOVO-OPS-01 | The cluster cutoff date → is visible to the operator | I | 4.4 |
| NOVO-RTN-01 | A life whose `last_seen` is earlier than the cutoff date (with the margin) → is not restored and follows today's path (disk discarded, only the listed losses) | C | 4.4 |
| NOVO-RTN-02 | The previous life's disk → **always** becomes a folder with the old ID, whether the node found the cluster or not, and several lives coexist (a second restart does not erase the first) | C | 4.5 |
| NOVO-RTN-03 | Life manifest → carries the ID, the member table with IPs and version, the `last_seen` and the watermark; `last_seen` is written periodically and on a clean stop | U | 4.5 |
| NOVO-RTN-04 | The quarantine → is not erased by the recovery; only at the end of the restoration, after the vote | C | 4.5, 4.7 |
| NOVO-RTN-05 | The disk fills because of life folders → there is no ceiling of its own; the usual behavior applies (the node goes to `failed`) | I | 4.5 |

### 9.2 Restoring the messages

| Code | Case → result | Level | Section |
|---|---|---|---|
| NOVO-RTN-06 | Each node reads the quarantine block by block → sends the messages to the block's **new primary**, with the original ID, through the `Recover` path; the node decides nothing | C | 4.6 |
| NOVO-RTN-07 | A message of a deleted topic, or one recreated with another `topicId` → is discarded, without querying the dead | C | 4.6 |
| NOVO-RTN-08 | The `id` is in the new primary's local log → the message is discarded | C | 4.6 |
| NOVO-RTN-09 | The `id` is in the log of another present node → it is discarded; the query goes in a batch per block to **all** present nodes, not only the owners | C | 4.6 |
| NOVO-RTN-10 | The S3 journal records the `id`'s removal → the message is discarded | C | 4.6 |
| NOVO-RTN-11 | S3 unreachable → the primary decides with the first two sources and logs a warning | C | 4.6 |
| NOVO-RTN-12 | A message only in the life, inside the window and with no deletion record in any source → is activated (replicates with quorum and becomes available) | C | 4.6 |
| NOVO-RTN-13 | The primary already has the message (`Existing`) → it is skipped | C | 4.6 |
| NOVO-RTN-14 | A message's state in two copies → if either says dead, dead wins; invisibility and receipt are zeroed and `deliveryCount` keeps the highest | U | 4.3 |
| NOVO-RTN-15 | Durable receipt → the primary writes to disk and replicates with quorum **before** confirming, and only then does the origin mark it "sent"; a second crash neither loses the message nor resends it | C | 4.7 |
| NOVO-RTN-16 | All present nodes announce they finished → the coordinator (lowest ID) opens the vote, which needs all present nodes; the status becomes **restored from disk**, the FIFO is released and only then are the folders deleted | C | 4.7 |
| NOVO-RTN-17 | The vote does not close → the FIFO stays without delivery and the `previous-life-pending` condition stays visible until the force operation | C | 4.7 |
| NOVO-RTN-18 | A node that returns after the release → joins clean; its FIFO messages are activated in `id` order, and their order relative to the newer ones is not guaranteed | C | 4.7 |
| NOVO-RTN-19 | Ordinary queues during the restoration → receive and deliver as soon as the definitions are distributed; the old messages come in afterwards | C | 4.7 |
| NOVO-OPS-02 | The `previous-life-pending` condition and the restoration warning with absent nodes → appear in `/health` and in the log | I | 4.7, 4.8 |

### 9.3 Nodes that do not return and the cold start

| Code | Case → result | Level | Section |
|---|---|---|---|
| NOVO-RTN-20 | Not all the expected nodes (members of the highest old table, from the manifests) have connected → the restoration of the old messages **does not start**; definitions and ordinary queues do not wait | C | 4.8 |
| NOVO-RTN-21 | The deadline `G` passed, default mode → the restoration goes on with a warning, using the present nodes | C | 4.8 |
| NOVO-RTN-22 | `G` passed, strict mode → the restoration stays held until the force operation | C | 4.8 |
| NOVO-RTN-23 | Force operation → changes the status and releases what is held; it also serves the cluster stuck by the loss of two nodes | C | 4.8 |
| NOVO-RTN-24 | **Property test:** with all the expected nodes present → no already consumed message is delivered again | C | 4.8, 4.11 |
| NOVO-RTN-25 | Outside that (a node with records absent after `G`, or only old nodes that confirm each other) → redelivery can only happen after `G` | C | 4.8, 4.11 |
| NOVO-RTN-26 | A crash makes the log lose its last entries → redelivery is accepted (the consumer is idempotent) and the test documents it | C | 4.11 |
| NOVO-RTN-27 | What was only in memory on all the nodes → is lost | C | 4.11 |
| NOVO-RTN-28 | All nodes start together and none answers → the node assumes it is the first, loads the definitions and leaves the messages in quarantine (**replaces UC-RTN-017**) | C | 4.9 |
| NOVO-RTN-29 | Discovery → also tries the previous life's IP table, which is in the manifest | C | 4.9 |
| NOVO-RTN-30 | Several nodes connect → the lowest ID coordinates, gathers the definitions of **all** connected nodes (highest version wins) and distributes them; a coordinator with an old disk does not impose them | C | 4.9 |
| NOVO-RTN-31 | A late node starts alone → serves with the definitions it has, and the window ends when some node answers and the higher version wins | C | 4.9 |
| NOVO-RTN-32 | `cluster.first-node` → is only needed for the cluster that never existed, with no previous table | C | 4.9 |
| NOVO-RTN-33 | Two groups assumed the disk without talking → each node probes the old IPs, at the configured interval, while the cluster has fewer members than the old table, up to the configured total time | C | 4.14 |
| NOVO-RTN-34 | The probing ends → when the table is complete, or when the time runs out; if nodes are missing, a condition stays visible, and the missing node enters later through the normal join | C | 4.14 |
| NOVO-RTN-35 | When the groups reach each other → the D101 tie-break applies; the loser moves its disk to the quarantine, hands over its definitions, redoes the join with a new ID and does **not** use `handle_exclusion` | C | 4.14 |
| NOVO-RTN-36 | Messages the losing group accepted while apart → are restored and checked against the dead | C | 4.14 |

### 9.4 FIFO

| Code | Case → result | Level | Section |
|---|---|---|---|
| NOVO-FIF-01 | The FIFO during the restoration → receives messages and does not deliver, until the **restored from disk** status | C | 4.10 |
| NOVO-FIF-02 | The ID generator → starts above the watermark; new `id`s are larger than the old ones and no `id` already returned changes | U, C | 4.10 |
| NOVO-FIF-03 | New messages during the restoration → go to the primary's provisional list, with no effective position, and no deduplication happens (all are accepted) | C | 4.10 |
| NOVO-FIF-04 | The release → the primary deduplicates against the restored keys and within the list, in `id` order, drops the duplicates and only then releases delivery | C | 4.10 |
| NOVO-FIF-05 | The producer of a message later dropped as a duplicate → received `duplicate=false` (accepted exception) | C | 4.10 |
| NOVO-FIF-06 | Restored FIFO messages → reach the group's primary through the path of section 4.6 and stay without delivery until the release | C | 4.10 |

### 9.5 S3, version upgrade and the one-node profile

| Code | Case → result | Level | Section |
|---|---|---|---|
| NOVO-BKP-01 | Last step of the start with S3 → the node lists segments and journal of the blocks it is primary of and indexes the live messages (no removal in the journal) | C | 4.13 |
| NOVO-BKP-02 | The messages found in S3 → go through the same check (topic, dead on the local log and on the other nodes, quorum); the ones the node already has are skipped | C | 4.13 |
| NOVO-BKP-03 | The scan → does not keep the cluster from becoming available; it holds only the FIFO, because the "I finished" announcement includes the scan | C | 4.13 |
| NOVO-BKP-04 | S3 unreachable at start → the step is skipped with a warning and the node goes on with the disk only | C | 4.13 |
| NOVO-BKP-05 | The S3 query → tells "removed" from "not found" and reads the journal once per topic, with a cache | U | 4.13 |
| NOVO-BKP-06 | A message with no removal in the S3 journal but dead in the local log → is discarded: S3 is not proof of life and does not widen the window | C | 4.4, 4.13 |
| NOVO-ING-01 | A node of a new version enters → drains those of a previous version one by one; the next starts only when the previous reached `drained` | C | 4.12 |
| NOVO-ING-02 | The next drain would leave fewer than 3 active nodes, or the new node has not finished synchronizing → it does not start | C | 4.12 |
| NOVO-ING-03 | A node does not drain in time → the process stops there and a visible condition appears | C | 4.12 |
| NOVO-ING-04 | Coexisting versions → the new messages between nodes are not sent to whoever did not announce support | C | 4.12 |
| NOVO-ING-05 | While a node of a previous version exists → no life is restorable; after the last one leaves, the log counts from the date on which **all** record | C | 4.12 |
| NOVO-ING-06 | The disk of a previous version → is not migrated; the new node follows today's path | C | 4.12 |
| NOVO-ING-07 | A previous version that enters → does not drain the newer ones | C | 4.12 |
| NOVO-CFG-01 | No parameters → the production defaults apply: `cluster.entry-address` required, TLS on and the production `replication.factor` | U | 4.16 |
| NOVO-CFG-02 | `replication.factor=1` → the node starts alone, without `cluster.entry-address` and without `cluster.first-node`, and the write quorum is 1 | I | 4.16 |
| NOVO-CFG-03 | `cluster.tls.enabled=false` → is only accepted with `replication.factor=1` (with more than one node the process refuses to start) and needs no secrets folder | U | 4.16 |
| NOVO-CFG-04 | `auth.dev-token` → is only accepted with `replication.factor=1` and requires at least 32 characters | U | 4.16 |
| NOVO-CFG-05 | New parameters (log limits, clock margin, `G`, strict mode, probing) → invalid values stop the start | U | 4.4, 4.8, 4.14 |
| NOVO-CFG-06 | `mode` and `development.resume-on-start` → cease to exist | U | 4.16 |
| NOVO-AUT-01 | `auth.dev-token` → the start creates a work token with `admin`, `publish` and `consume`, stored only as a hash, and turns authentication on | I | 4.16 |
| NOVO-AUT-02 | The `auth.dev-token` secret changed, or the node restarted → the token is updated and not duplicated | I | 4.16 |
| NOVO-AUT-03 | The `dev-token` is not root → it does not stop the cluster and does not create, change or revoke other `admin` tokens | I | 4.16 |
| NOVO-OPS-03 | `health.single-node-condition` → `/health` lists the **reduced profile** condition with what is relaxed, without taking `/health/ready` down; `false` hides it | I | 4.16 |
| NOVO-ESC-03 | FIFO with `replication.factor=1` → is accepted, and its quorum is `min(2, factor)` | U, I | 4.16 |
| NOVO-RTN-37 | One-node profile → the restart restores topics, tokens and messages through the common path (the node is primary of all blocks and the vote has one voter) | I | 4.16 |

### 9.6 Existing cases that change

| Case today | What happens | Replaced by |
|---|---|---|
| UC-RTN-017 (the first node erases the old disk) | **Retired**: the first node now assumes the disk | NOVO-RTN-28 |
| UC-RTN-014, UC-RTN-015, UC-RTN-016 (the restart recovery returns only the losses) | **Rewritten**: the recovery now sends all the quarantined messages to the new primary, which checks the dead | NOVO-RTN-02, NOVO-RTN-06 to NOVO-RTN-13 |
| UC-RTN-011 (recovery in development mode does not apply) | **Retired**, along with the mode | NOVO-RTN-37 |
| UC-DEV-001 to UC-DEV-013 (development mode) | **Retired**, one by one: 001 and 004 by NOVO-CFG-06; 002 and 003 by NOVO-CFG-02 and NOVO-ESC-03; 005 and 011 by NOVO-OPS-03; 010 by NOVO-AUT-01 and NOVO-AUT-03; 013 by NOVO-RTN-37. Cases 006 to 009 and 012 (single-node behavior) still hold, now running on the one-node profile (level I) | NOVO-CFG-02, NOVO-AUT-01, NOVO-OPS-03, NOVO-RTN-37 |
| UC-DRN-021 to UC-DRN-027 (drain as *save*) | **Reassessed**: what was a "save" becomes the common restoration | NOVO-RTN-37 |
| UC-TOP-004, UC-TOP-019, UC-CNS-037, UC-FIF-042 (FIFO requires a factor of at least 2) | **Rewritten**: a FIFO with factor 1 is accepted | NOVO-ESC-03 |

### 9.7 Coverage: from each rule to its case

| Section of the proposal | Cases |
|---|---|
| 4.2 Definitions | NOVO-DEF-01 to NOVO-DEF-05 |
| 4.3 Message state | NOVO-RTN-14 |
| 4.4 Deleted-messages log | NOVO-ESC-01, 02; NOVO-ARM-01 to 04; NOVO-SIN-01, 02; NOVO-OPS-01; NOVO-RTN-01; NOVO-BKP-06; NOVO-CFG-05 |
| 4.5 Quarantine per life | NOVO-RTN-02 to 05 |
| 4.6 Restoration | NOVO-RTN-06 to 13 |
| 4.7 End of the restoration | NOVO-RTN-15 to 19; NOVO-OPS-02 |
| 4.8 Nodes that do not return | NOVO-RTN-20 to 25 |
| 4.9 Start | NOVO-DEF-04, 05; NOVO-RTN-28 to 32 |
| 4.10 FIFO | NOVO-FIF-01 to 06 |
| 4.11 Guarantees and limits | NOVO-RTN-24 to 27 |
| 4.12 Version upgrade | NOVO-ING-01 to 07 |
| 4.13 S3 scan | NOVO-BKP-01 to 06 |
| 4.14 Reunion | NOVO-RTN-33 to 36 |
| 4.15 Start process | NOVO-RTN-02, 28 to 30 and 37 (the table only gathers rules from the other sections) |
| 4.16 One-node profile | NOVO-CFG-01 to 06; NOVO-AUT-01 to 03; NOVO-OPS-03; NOVO-ESC-03; NOVO-RTN-37 |

## Sources and evidence

- Current state: `node.rs` (opening of topics, tokens and messages), `cluster/service/mod.rs` (`start_alone`, `recover_on_boot`, `recover_previous_life`, `attempt_join`), `cluster/recovery/mod.rs`, `storage/messages/mod.rs` (`set_aside_old_disk`), `cluster/replication/definitions.rs` (version map in memory), `cluster/drain/mod.rs` (leaving the table and the save mark), `cluster/vote/mod.rs` (the vote's base is the healthy nodes), `cluster/backup/restore.rs` (`restore`, `recover_missing`, `find_message`, the journal as a removal list), `cluster/sync/mod.rs` (`apply_tombstone` on the load), `cluster/transport/mod.rs` (`found_no_cluster`), `cluster/replication/messages_api.rs` (mandatory quorum roles) and `core/id` (`IdGenerator`, no floor today).
- The first node's behavior is pinned down by the test `tests/recovery.rs` › `the_first_node_of_a_new_cluster_wipes_the_old_disk_and_keeps_no_previous_life`.
- The S3 journal as a removal list is D144 ([30-decisions.md](30-decisions.md)).
- None of the rules above has been tested yet. The rule in section 4.8 asks for a property test before the code: with all the expected nodes present, redelivery never happens, and otherwise it only happens after `G`.
