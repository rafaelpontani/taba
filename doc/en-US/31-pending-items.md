---
title: Pending items
summary: Everything that remains open in the project — design questions (D…), problems of logic, risk or limit (PRB-…) and the known limits, each with its evidence.
type: reference
status: partial
verified-on: 2026-10-18 @ b090822
sources: [crates/taba-node/src, connectors, doc/en-US/30-decisions.md]
---

# Pending items

> **Two kinds of item, two codes.** **`D…`** is a **design question** (the document does not define it, or there are two ways out), always with a proposal; **`PRB-…`** is a **problem** that the implementation found (a behavior that is incomplete, risky or has a known limit), with the **effect** and the **proposal**. The codes are **never reused**; what has already been decided or resolved is in [30-decisions.md](30-decisions.md).
>
> **Legend:** `[ ]` open, undecided; `[~]` partially resolved (the text carries **only the questions still missing**, each with its sub-code); **Depends on** = the question can only be answered after another.
>
> **Maintained by hand**, in every language folder, in the commit of the change that opens, changes or closes an item. The next free codes follow the highest already used (today **D182** and **PRB-081**).

## Known limits

Found by checking the code and running the cluster; each one has its evidence. Those that concern operation are also in [23-operations.md](23-operations.md).

| # | Type | What was seen | Effect | Proposal | Evidence |
|---|---|---|---|---|---|
| A | incomplete | **Aggregation of topic metrics across nodes does not exist.** `GET /metrics/topics*` with `scope=cluster` returns the view of the node that serves; `partial` is always `false`; the route `GET /cluster/metrics/topics` does not exist | `depth`/`published` of the "cluster" are only those of one node; on a 3-node cluster with 12 messages, each node and the `cluster` scope answered 4 | Implement the internal route and the fan-out (with `stats.topics.cache-ttl`) or document that it only holds per node | [13-observability.md](13-observability.md); `http/topic_stats/mod.rs`; run on a 3-node cluster |
| B | definition | **Four parameters are accepted but have no effect:** `cluster.sync.stream.max-line-bytes`, `cluster.sync.stream.idle-timeout`, `fifo.dedup.index.max-entries`, `stats.topics.cache-ttl` | Whoever adjusts them believes something changed; one of them links to PRB-022 | Implement, or refuse the key, or take them out of the table and of [22-configuration.md](22-configuration.md) | [22-configuration.md](22-configuration.md); script that checks the use of the `Config` fields |
| C | risk | **Losing two nodes in a row locks the cluster** (`quorum-lost`): one node is left, the exclusion of the second does not take effect, and the restarted ones do not rejoin (the register requires all in the table). There is no forced exclusion | Only recoverable by stopping everything and starting again; the resume restores what was on disk ([07-cluster.md](07-cluster.md), section 10) | The "forced exclusion" already foreseen in doc 04 (future evolution): a manual exclusion route, or a table per epoch | [07-cluster.md](07-cluster.md), [23-operations.md](23-operations.md); run with `docker compose kill` |
| E | process | **The use-case catalogues ([26a](26a-implemented-use-cases.md)/[26b](26b-planned-use-cases.md)), the decisions ([30-decisions.md](30-decisions.md)) and these pending items are maintained by hand.** The register they were generated from left the repository; `uc_coverage` reads the `### UC-…` cards of 26a and 26b | The 26a/26b scoreboard only changes when someone updates it | A generator that reads the cards and the `/// UC:` lines and regenerates both catalogues in both languages | `crates/taba-node/tests/uc_coverage.rs`; [26-use-cases.md](26-use-cases.md) |
| F | documentation | **`verificar.mjs` only knows `doc/pt-BR/`** (Portuguese metadata keys and file names); `verificar-idiomas.mjs` checks the parity of files, metadata and links between the folders | Routes, keys and codes cited only in the en-US text are not checked against the code | Run `verificar.mjs` per language and put both checkers in a PR job | `doc/pt-BR/scripts/`; [27-ci-cd.md](27-ci-cd.md) |

## Connectors (new problems and limits)

Found while writing the Java, Python and Node.js connectors ([21-connectors.md](21-connectors.md)).

| Code | Effect | Proposal |
|---|---|---|
| **PRB-079** | **The Java, Python and Node.js connectors have no integration test against a real Taba and no publishing job.** They are checked only against a local scripted HTTP server (the same level as the C# unit tests), and no registry package is published (the identifiers are D70.1) | A `scripts/integration.sh` per connector, like `connectors/csharp/scripts/integration.sh`, run before each release; the publishing jobs once D70.1 is decided |
| **PRB-080** | **The Python `asyncio` client runs the synchronous transport in worker threads** (`asyncio.to_thread`), because the standard library has no asynchronous HTTP client and the connector has no dependencies. A cancelled task closes the connection in flight, but each concurrent call still takes a thread | Measure with many concurrent long pollings; if it matters, an own HTTP/1.1 over `asyncio` streams, as the Rust connector has over `TcpStream` |
| **PRB-081** | **Node.js and Python repeat a call that failed on a pooled connection the server had closed (`ECONNRESET` before any byte of the answer), like a refused connection.** If the node had already applied the call and closed the connection before answering, a non-FIFO publication could be delivered twice. The .NET and Java HTTP clients do the same inside the library | Measure how often it happens; if it matters, send `Connection: close` on publications that carry no `deduplicationId`, or repeat only idempotent verbs |

## S3 backup (new problems and limits)

Found while implementing the backup plan ([33-s3-backup.md](33-s3-backup.md)). Each has the **effect** and the **proposal**.

| Code | Effect | Proposal |
|---|---|---|
| **PRB-053** | **The journal is not compacted.** Every `backup.meta.interval` writes one journal per topic and the bucket only deletes them together with the topic: the restore reads all of them, and the read grows with time | A periodic *snapshot* per topic (the state of all live messages) that replaces the previous journals; the restore reads the last snapshot and the journals after it |
| **PRB-054** | **A segment of a primary that stopped being one is only reclaimed by the `ttl`.** The heir does not have the old one's live count, and the references notified to the replicas do not carry it. The manager deletes a segment when **all** the messages have passed the `ttl` (default 14 days); until then the bucket keeps dead bytes | Make the manager compute the segment's life from the footers and the journals (never from memory), with a cache of the footers |
| **PRB-055** | **The restore reads each whole segment** (and all the topic's journals), in series: on a large bucket the survivor's start takes long. The search for lost messages (`recover_missing`) reads only footers and `Range`, but also lists the whole topic bucket | Restore per topic in parallel and filter by the ring already at read; restore only the messages the node is responsible for |
| **PRB-056** | **A definition removed while the manager was down may resurrect on restore:** the tombstone and the revocation leave the manager's in-memory queue if it goes down before writing them. The periodic reconciliation **only writes**, never deletes from the bucket what memory does not have (deleting would cause a general loss on a node that started empty) | Write the tombstone and the revocation to a durable `outbox` in `_backup.db` |
| **PRB-057** | **The search in the bucket after an exclusion by death costs the size of the topic's bucket** (listing, whole journals, one footer per segment) and runs on each survivor | A light index of ids per segment kept by the manager |

Also recorded as **accepted limits** (scenarios 2, 7, 8 and 10 of [33-s3-backup.md](33-s3-backup.md)): S3 down only for one live node, a message confirmed and consumed before `backup.delay` when **all** nodes go down, state lagging by the journal window (idempotent consumer), and S3 without conditional write (the restore lock is not safe). A **rejected** exclusion vote is not repeated by itself (inherited from the voting protocol; in the backup the delay until the heartbeat expires makes this rare).

## TUI server work (new problems and limits)

Found while implementing D152 to D155 ([30-decisions.md](30-decisions.md)). Each has the **effect** and the **proposal**.

| Code | Effect | Proposal |
|---|---|---|
| **PRB-058** | **A prune does not cover a node that is synchronizing, nor a message published while it runs.** A node that is syncing prunes what it has, but the blocks it copies afterwards, or the events it recorded, can bring messages back; a message published during the prune may survive. Whether the S3 backup restores a pruned message after a restart was not exercised in a test | Make a syncing node answer the prune with `503 cluster.node-syncing` (the person repeats after it is ready), or record the prune as an event of the sync log; add a prune case with the S3 backup |
| **PRB-059** | **The consolidated statistics are approximations in two fields, and the operations API's `/metrics/topics*` are still the view of one node** (finding A, in part resolved by D156: the client API's `/stats/topics…` joins the cluster). The latency percentiles are the **maximum** of the nodes' (the real percentile of the cluster needs the histograms), and `groups` and the largest group are the maximum (a lower bound; the noisy-group condition can miss a group spread over nodes); a client that reads the percentile of the cluster gets an upper bound | Send the latency histograms and the group lists between nodes; or make `/metrics/topics*` of the operations API consolidate too |
| **PRB-061** | **A node that left by drain is listed and addressable only while the receiving node remembers it** (up to 64, in memory): after the receiver restarts, `GET /nodes` no longer shows it and `/nodes/{nodeId}/drain` answers `404`, so it cannot be returned through the API | Keep the nodes that left in the node database, or derive them from the S3 stop list and heartbeats |
| **PRB-063** | **The deduplication of the FIFO at the release has the limits of the window and of the primary** ([07-cluster.md](07-cluster.md), section 10.9): (a) it uses the deduplication keys the primary still remembers, which live for `fifo.dedup.window` (300 s) from the `id`; with `cluster.restore.grace` equal to the window, a message restored after `G` may no longer find the key of the publication it repeats; (b) the provisional list is in the memory of the group's primary: if the primary changes during the restoration, the publications accepted by the old one are no longer listed and are not deduplicated; (c) the generator stays above the watermarks the node has seen, and a publication accepted before the manifests are announced may get an `id` below the watermark of a life that was not yet announced | (a) widen `fifo.dedup.window` while the cluster restores, or restore the lives before the window ends; (b) rebuild the list from the messages whose `id` is above the watermark when a new primary takes the group; (c) hold FIFO publications until the first poll answers |
| **PRB-064** | **The cold start has three limits** ([07-cluster.md](07-cluster.md), section 10.5): (a) the election of the node that starts alone uses who answers at the moment; if two nodes see each other asymmetrically (one reaches the other and not the opposite) or the lowest old ID answers late, two start alone and form separate clusters, which only the regroup of groups joins; (b) the node that starts alone moves the start date of the log of excluded messages back to the previous life's without proving that no node of the old table stayed up recording in the hole (a node that stayed up in another partition may have exclusions this cluster does not see, and the residual redelivery of section 10.10 covers them); (c) the offer of the definitions on the disk of a node that **joins** counts by version, and a tombstone only protects while it does not expire (`TOMBSTONE_TTL`): a definition the cluster deleted longer ago than that and that is still alive on a disk goes back to the cluster | (a) the tie-break of the regroup of groups; (b) check the `last_seen` of the lives of the present nodes (if all were stopped in the hole, the date may go back); (c) accept the risk, or keep the tombstones for longer than the expected absence of a node |
| **PRB-065** | **The scan of S3 has two limits** ([33-s3-backup.md](33-s3-backup.md), section "Scan at start"): (a) the journal only records the removal after the backup delay; a message acknowledged in that interval whose log was lost or trimmed (new node, log newer than the message) comes back from the scan: it is *at-least-once*, and the consumer must be idempotent; (b) the scan reads the footer of **every** segment of each queue at every start with backup, and the cost grows with the bucket (proposal: index by `id` range of the node's blocks, or bound it by the `ttl`). |
| **PRB-066** | **The probe of the old IPs has limits** ([07-cluster.md](07-cluster.md), section 10.11): (a) after the time (`cluster.restore.probe.timeout`) ends, two groups that meet later do **not** join by themselves: the operator restarts the smaller one, which enters through the normal join (proposal: keep probing at a longer interval after the time); (b) completeness is by **count** of active members, so a cluster the operator reduced on purpose shows `members-missing` until the time ends; (c) the losing node is out of service (joining) during the reunion, and client calls to it fail until it is back; (d) only what the losing group already had on disk is restored: what was only in memory is lost, as in every start ([07-cluster.md](07-cluster.md), section 10). |
| **PRB-067** | **A full restart does not carry the deduplication rows of acknowledged FIFO messages** (found when the tests moved to the one-node profile): the restoration of a life sends only the messages that were alive, so the short row that an ack leaves for the deduplication window is lost, and the same `deduplicationId` published after the restart is accepted again (`201` instead of `200`). The key of a message that was alive still deduplicates (proposal: send the short rows that have not expired together with the life, as the synchronization does). |
| **PRB-068** | **In the cluster path, a message past its `ttl` can still be acknowledged or have its invisibility changed until the sweeper removes it** (it runs every second at most): `ack` and `visibility` check the message on the owner without settling the queue first, unlike the single-node path of before. Found when the tests moved to the one-node profile; the test now waits for the sweeper (proposal: settle the queue before the register step of `ack` and `visibility`). |
| **PRB-069** | **While a node drains, a plain token cannot read `GET /metrics` on the client port.** A node in drain only validates the root token (the other tokens are data, and the node counts by IP and forwards), and `/metrics` asks for a valid token but stays out of the drain refusal, so only the root token reads the metrics of a draining node there. The operations port (no token) and the history the collectors keep are not affected | Keep it, since the operations port is the door of the collectors (D177), and document it; alternative: let `/metrics` accept any token the node still remembers while it drains |
| **PRB-071** | **The permission a route needs is only in its description.** The OpenAPI documents say `Permission: admin` in the text, but not in a field a generator or a gateway can read (no `x-required-permission`, and the security scheme has no scopes) | Add the vendor extension on each operation, with a test that checks it against `need()` in `http/auth`, so the two cannot differ |
| **PRB-072** | **Some answers are documented as a free-form object.** The S3 backup reports (`/nodes/{nodeId}/backup`, `/cluster/backup`, the flush routes) and the `signals` of a node status are `object` without fields, because the server builds them with `json!`; a generated client gets an untyped map there | Give each one a `struct` (or a documentation twin checked by `tests/openapi.rs`), as was done for the other DTOs |
| **PRB-073** | **`cargo audit` ignores `RUSTSEC-2026-0387` (`crypto-bigint` 0.5.5, timing variability).** The crate comes from `aws-sigv4` 1.6.3 (the latest), which the S3 backup uses, and no released version avoids the old `crypto-bigint`. Only the SigV4a (ECDSA) signer reaches the affected operations, and the backup signs with SigV4 (HMAC), so the node is not exposed; the exception is in `.cargo/audit.toml` | When `aws-sigv4` depends on `crypto-bigint` >= 0.7.6 (`cargo update -p aws-sigv4`), delete the line from `.cargo/audit.toml` and this item |

## Chaos run (new problems and limits)

Found by the chaos run ([37-guarantees.md](37-guarantees.md)).

| Code | Effect | Proposal |
|---|---|---|
| **PRB-077** | **An ack and a move to the dead-letter of the same message can both be decided when the register of one of them expires on the owner (`write.pending.ttl`, 30 s) before its commit arrives.** Seen only with clock jumps of 35 s in the chaos run (3 runs in 400 of four seeds, always the same shape: a message acked with `204` and then delivered again from the dead-letter queue, I3 and I5); with 8 s jumps, 1600 runs had none. The cause is **not proven**: it is the reading that fits (the expired register frees the message, the other operation registers and is decided, and both commit; the lock `by_message` that serializes them is gone). It needs a stall of more than `write.pending.ttl` in the middle of an operation, for example a long partition | Prove it with a test that expires the register between the two operations (as `tests/replication.rs` does for the ack alone, PRB-076); then decide how an expired register is told apart from a decided one (keep the operation on the owner until it is committed or rolled back, or check on commit that the message is still the one that was registered) |

**Default of `cluster.vote.min-voters` in clusters of 4 nodes or more (D182).** The default (2) lets each half of a 4-node cluster exclude the other in a 2 + 2 partition; both halves accept writes and the loser regroups when they meet (D181, no message is lost). A majority of the table as the default would avoid the split, at the price of no writes at all on a 2 + 2 partition (neither half has a majority). Proposal: keep 2 (availability) and document the majority for whoever prefers consistency; the alternative is a majority by default.

## 1. Unblocked

Can be answered **now**, in any order: they do not depend on another question or step.

### D68

- **State:** `[~]` partial · **Type:** design question

**Base versions of the connectors (the other languages): confirm the table.** The ones for **C#** (.NET Framework 4.6.1 and .NET 5) and for **Java 11, Python 3.9 and Node.js 18** are decided (D68.1, D68.2). The "base version" is the oldest version of each language that the connector agrees to run on. Instead of choosing each by eye, the proposal is a single rule, derived from what has already been decided: the base is the **oldest** that (1) speaks TLS 1.2 or higher (D69), (2) has an HTTP client with `PATCH` (D72) and, where the language has async, uses it (D71). The table that comes out of the rule is in [21-connectors.md](21-connectors.md), section 18.10.1: **PHP 5.6, Ruby 2.0, C89 with `libcurl` 7.34, C++03, Go 1.7 and Rust 1.70** for the languages still to do (the rule gave Python 2.7.9/3.4.3 and Node 0.12, and Java 11 leaves **Java 8** out only because of `PATCH`; D68.2 raised Python and Node.js). The question is: **are the rule and the table right for the languages still to do?** The same doubt as in Python and Node.js applies to PHP 5.6 and Ruby 2.0: an old base forbids the idioms the people of that language expect. Rust 1.70 is an estimate to close in the prototype.

### D70

- **State:** `[~]` partial · **Type:** design question

**Publishing the connectors.** Already decided: the place of each one (the rule "most popular and free"). Missing:
  - **D70.1.** The **organization name** and the package identifiers in each registry: Maven *groupId* (which requires a domain or a verified organization), NuGet, PyPI, Packagist, RubyGems, npm, crates.io and the Go module path (which is the repository's).
  - **D70.2.** Does **PHP** (Packagist requires `composer.json` at the root) come from a **read-only mirror** by `git subtree split`? And are the connector versions **independent** of the server's, as proposed (own SemVer, declaring the contract version)?

### D75

- **State:** `[ ]` open · **Type:** design question

**Drain requested through the API that exceeds `drain.timeout`.** Today the node stays in `draining` (`error` log, without the save mark) until someone calls `DELETE /node/drain`; the text of [07-cluster.md](07-cluster.md) (section 6.10) only says what happens with the drain by signal (exits anyway). The proposal is to keep it so and say so in 6.10; the alternative is for the node to go back to `active` by itself. Origin: the drain requested through the API was not in the text of 6.10. Linked problem, **PRB-004** (logic, `cluster/drain`): A drain requested through the API that exceeds the deadline (or has a disk error) stays in `draining` without finishing or returning. Effect: The node stays out of rotation without the save mark, waiting for an operator `DELETE`.

### D76

- **State:** `[ ]` open · **Type:** design question

**`operations.allowed-ips` and the health checks.** [22-configuration.md](22-configuration.md) says to filter `/health/*` along with the rest, but `kubelet` and the load balancer call `/health/live` and `/health/ready` from IPs that the operator rarely lists. Questions: **D76.1** Do `/health/live` and `/health/ready` stay **outside** the filter (proposal, since they only say "alive" and "ready")? **D76.2** If they stay inside, is the `403` an `ErrorResponse` (as today) or a `HealthResponse`, given that [06b](17-operations-api.md) says these two routes never return `ErrorResponse`? Linked problem, **PRB-006** (definition, `http/operations`): `allowed-ips` also filters `/health/live` and `/health/ready`, and the `403` comes out as `ErrorResponse`, against what 06b says of these routes. Effect: The orchestrator may read `403` as a sick node.

### D77

- **State:** `[ ]` open · **Type:** design question

**`Retry-After` of the drain `503`.** Today fixed at 5 s. Proposal: keep it fixed (the client only needs to wait for the load balancer to remove the node); the alternative is a parameter.

### D80

- **State:** `[ ]` open · **Type:** design question

**Topic name and Windows reserved names.** The topic name becomes a folder name ([06a](16-client-api.md)), and `con`, `nul`, `aux`, `com1`... cannot be a folder on Windows. Proposal: the folder gets the `_` prefix when the name (without uppercase) is reserved; the alternative is to forbid those topic names. Linked problem, **PRB-008** (limit, `storage/messages`): A topic name equal to a Windows reserved name (`con`, `nul`...) does not become a folder. Effect: Publishing to that topic on Windows gives a disk error (`500`).

### D84

- **State:** `[ ]` open · **Type:** design question

**Rate limit defaults.** [22-configuration.md](22-configuration.md) leaves `ratelimit.publish.rps`, `consume.rps`, `admin.rps` and `burst` as "To be defined" (only `ops.rps` has a suggestion, 2). Without a value, the node today **does not limit** the category. Proposal: keep no limit by default (whoever installs chooses), and only suggest values in the documentation; the alternative is a generous default (for example 1000 rps for publishing and consumption, 20 for administration).

### D87

- **State:** `[ ]` open · **Type:** design question

**Does the `topics.volatile.max-count` ceiling apply to `POST /topics`?** Today only `autoCreate` is blocked (it is what [22-configuration.md](22-configuration.md) says), but a volatile queue created by `POST /topics` already enters the count, so `autoCreate` may be refused because of them. Proposal: keep only `autoCreate` blocked (whoever uses `POST` is the operator, who knows what they are doing).

### D89

- **State:** `[ ]` open · **Type:** design question

**Ceiling of the FIFO deduplication map.** `fifo.dedup.index.max-entries` is "To be defined" in [22-configuration.md](22-configuration.md). Question: what is the default and what to do on reaching it? The proposal is a per-topic ceiling (for example 1,000,000 keys) and, once reached, **refuse the new publication** with `503` and `Retry-After` (instead of forgetting window keys and silently duplicating); the alternative is to discard the oldest and log `warn`. Linked problem, **PRB-022** (limit, `engine/messages`): The FIFO deduplication map keeps one entry per publication of the window and has no ceiling (`fifo.dedup.index.max-entries` is "To be defined"). Effect: With unique keys per message, memory grows with throughput × 300 s; a burst of millions fills the map.

### D90

- **State:** `[ ]` open · **Type:** design question

**Defaults of the condition thresholds.** [22-configuration.md](22-configuration.md) leaves "To be defined" `stats.topics.backlog.depth`, `backlog.age`, `stalled-after`, `idle-after`, `group-stalled-deliveries`, `stats.disk.free-min` and `stats.topics.cache-ttl`. Today, without a value, the condition **does not fire** (and the cache does not exist). Question: what are the defaults, and should the condition come on? The proposal: `backlog.depth` 10000, `backlog.age` 15 min, `stalled-after` 5 min, `idle-after` 1 h, `group-stalled-deliveries` 5 (the doc's suggestion), `stats.disk.free-min` 1 GiB and `cache-ttl` 5 s. Together: `memory-pressure` today is `diskMessages > memoryMessages` (doc 11 says ">>"); accept that definition or use a ratio (for example, double)? Linked problem, **PRB-023** (risk, `engine/messages`): A FIFO group stuck on a message that nobody confirms (a consumer that goes down at each attempt) only leaves by `maxDeliveries` or by `ttl`; without them, the group stops forever. The `group-stalled` condition exists since step 12, but only fires with `stats.topics.group-stalled-deliveries`, which has no value. Effect: The group's backlog grows without warning while the limit is not defined.

### D91

- **State:** `[ ]` open · **Type:** design question

**Topic metrics in `/metrics`.** (a) `telemetry.topics.max-topics` is "To be defined"; today it is 1000 in `per-topic` mode. (b) In `per-topic` the series carry only the tags of `telemetry.topics.group-by-tags` (the doc says "plus the tags", without saying which): check. (c) `taba.topic.condition` only comes out in `per-topic`; in `by-tags` the proposal is not to emit. Question: do you confirm these three?

### D93

- **State:** `[ ]` open · **Type:** design question

**Position, block and first node.** [Doc 05](06-ring-and-replication.md) (section 7.1) did not say how the position on the ring is computed nor what the `{id}` of a block is. The implementation used: position = the first 8 bytes of the SHA-256 of the key; virtual node `<nodeId>#<i>`; block = the arc that ends at a virtual node, with `{id}` = its position in 16 hexadecimal digits; replicas = the next physical nodes on the ring. And the node with `cluster.first-node` starts alone only when the listing has **no network response**. Question: do you confirm? The alternative for the block is a fixed number of equal ranges of the hash space (for example 4096), independent of the nodes, which would give stable blocks when the cluster changes (synchronization would work by fixed range, not by an arc that splits).

### D96

- **State:** `[ ]` open · **Type:** design question

**Who sets the deadline of `PUT visibility` and of the ack, and the forwarding deadline.** **D96.1** [Doc 06c](18-cluster-api.md) says the invisibility coordinator is always the delivery owner, so the clock that sets `invisibleUntil` is its own. On `GET` and on `take` that holds; on `PUT` and on ack, whoever coordinates is the node that received the call, with its own clock (the owner does not know who received it). Proposal: accept (the clock difference between nodes is already accepted in at-least-once, doc 14, section 13.3); the alternative is to forward the `PUT` to the primary. **D96.2** The forwarding has a fixed deadline of 60 s in the code. Proposal: a `cluster.forward.timeout` parameter with that default. Linked problem, **PRB-034** (limit, `cluster/replication/forward`): The forwarding deadline is fixed (60 s) and the `502`/`504` after the body is sent (UC-MSG-017) has no test. Effect: No way to adjust the deadline nor to prove the behavior of a mute destination.

### D99

- **State:** `[ ]` open · **Type:** design question

**The vote when the candidate announced it went into failure.** `vote-remove` answers `yes` to whoever cannot reach the candidate; but a node in `failed` is alive and the channel with it stays up, so every voter answered `no` and [doc 05](07-cluster.md) (section 7.3) was not met ("the others are free to vote its deregistration"). Implemented: with `reason: node-failed`, the voter answers `yes` if its channel received the candidate's `node-state failed`; otherwise it checks whether it can reach it. Question: should the notice be checked this way, or should the `VoteRequest` carry the proof (the state the opener saw)?

### D102

- **State:** `[ ]` open · **Type:** design question

**`quorumAvailable` and `quorum-lost`.** [Doc 11](13-observability.md) says "fewer healthy nodes than the minimum of voters" and does not say whose point of view it is nor what happens with a one-node cluster. Implemented (provisional): each node counts itself and the peers with a connected channel, and the minimum is `min(cluster.vote.min-voters, members)`, so the first node of a new cluster does not come out in `quorum-lost`. In `scope=cluster` the summary's `quorumAvailable` is the AND of the nodes that answered. Question: do you accept? Alternative: also count the nodes that did not answer the `/health/cluster` query.

### D109

- **State:** `[ ]` open · **Type:** design question

**Channel constants that are not parameters.** Fixed in the code: the notice expires in the output queue in 10 s (`NOTIFY_TTL`); 3 consecutive `not-member` answers are enough to check whether the node was excluded (`EXCLUDED_AFTER`); the channel call has a deadline of `2 × cluster.channel.keepalive.interval` (minimum 500 ms), which is also the "time without a signal" that drops the channel. Question: do they become parameters (`cluster.channel.notify-ttl` etc.) or stay fixed and documented?

### D113

- **State:** `[ ]` open · **Type:** design question

**The FIFO receipt carries the group position.** The ack and the invisibility change reach any node, which only has the `id` and the `receiptToken`, but the block of a FIFO message comes from the group (which doc 17 does not say how to find). Implemented (provisional): `receiptToken = {position}-{guid}` only on FIFO. Question: do you accept? Alternative: put the `groupId` as a parameter of the ack and of the change (changes the client API).

### D114

- **State:** `[ ]` open · **Type:** design question

**What `delivery.get.max-attempts` counts and the default.** [Doc 13](12-delivery.md) (section 12.2) says the number of failures tolerated per `GET` is limited by it, with no value. Implemented (provisional): it counts each message whose invisibility register did not close (already after the `write.retry.*` retries); the `GET` moves to the next ID of the list, without waiting, up to **3** failures; it applies on each node that delivers, so a `GET` that also asks other nodes for messages (`take`) may add up to 3 failures per node. When exhausted, UC-CNS-020 (returns what it captured) or UC-CNS-021 (`503`) applies. Question: is the default 3? Should the ceiling be for the whole `GET`, and not per node?

### D115

- **State:** `[ ]` open · **Type:** design question

**Defaults of the volatile manager and the idleness query.** [Doc 10](22-configuration.md) left `topics.volatile.idle-check.retry-interval` (suggestion 30 s) and `topics.volatile.takeover.jitter` without a value. Implemented (provisional): 30 s and 2 s. The spread is a stable delay per queue (the hash of the `id` modulo the window), not drawn at random, and the first query of a queue that the node starts to manage happens at that delay after it appears in its set. `idleSeconds` is an integer (whole seconds, rounded down, which can only postpone the deletion), and the manager also bounds the idleness by the age of the queue (`now − createdAt`), because a node with no `GET` record answers the time since it started. Question: do you accept?

### PRB-005

- **State:** problem · **Type:** definition

(definition, `http (drain)`): In drain the node refuses **every** client call, including the ack and `PUT visibility`. With no other node to forward to, the consumer cannot confirm what it already received. Effect: The delivered message becomes ready again when the invisibility expires (duplicate) if the node stays in drain for longer than the `visibilityTimeout`. The `receiptToken` only becomes valid again after the `DELETE` or the restart. Proposal: during drain, in development mode, the ack and `PUT visibility` pass (only delivery and publishing are refused). Alternative: keep refusing everything.

### PRB-015

- **State:** problem · **Type:** risk

(risk, `http/ratelimit, http/client_ip`): With `client-ip.trusted-proxies` empty (the default), any client forges `X-Forwarded-For` and chooses its own IP. Effect: Escapes the rate limit (each request with a new IP) and makes the node keep a counter per invented IP, until the pruning of the counters already full (above 4096). Doc 12 accepts the risk only for the node behind a load balancer. Proposal: the default without a load balancer is `client-ip.headers.enabled=false`. Alternative: keep and document the risk.

### PRB-017

- **State:** problem · **Type:** risk

(risk, `engine/tokens`): Validating a token with an unknown `id` answers faster than an existing `id` with a wrong secret (it does not compute the hash). Effect: In theory it allows discovering, by response time, which `id`s exist. The `id` is not a secret (it appears in logs) and the 256-bit secret cannot be guessed, so the damage is low. Proposal: accept (low damage). Alternative: compute a padding hash for the unknown `id`.

### PRB-019

- **State:** problem · **Type:** limit

(limit, `engine/messages`): Finding the middle message to demote (`nth(len/2)`) and checking the ready ones already delivered when `maxDeliveries` changes scan the topic's memory (the `maxDeliveries` check and the on-demand queries read the messages in pages; the oldest message comes from the age order). Effect: Demotion costs O(n) per message on a large topic, holding the topic lock. Proposal: swap for an ordered structure with position when measured. No design question.

### PRB-025

- **State:** problem · **Type:** limit

(limit, `engine/messages, http/metrics`): `diskBytes` (and `bytes`) sums the size of the topic's `.body` files, one call per message on disk, at each read of `/metrics/topics*` and, in the `by-tags` and `per-topic` modes, at each scrape of `/metrics`. `taba.storage.disk.usage` and `.duration` were left out for the same reason. Each read also measures all the topics listed. Effect: With many messages on disk or many topics, the read gets slow and costs disk. Proposal: store the body size in the database row (a new column) and in the index, and measure only what changed.

### PRB-026

- **State:** problem · **Type:** incomplete

(incomplete, `telemetry/process`): `process.memory.usage`, `process.thread.count`, `process.open_file_descriptor.count` and `process.cpu.utilization` only exist on Linux (they read `/proc`); on Windows and macOS there is only `process.uptime`. Effect: The node dashboard lacks memory, threads, files and CPU outside Linux. Proposal: per-platform system calls (they need `unsafe` or a dependency), or accept that only Linux has these metrics.

### PRB-039

- **State:** problem · **Type:** limit

(limit, `engine/messages/blocks`): Listing the IDs of a block reads the database rows by position range (index) but scans the in-memory map of the whole topic at each chunk, and sorts the rows of all topics together. Effect: The cost grows with the messages in memory × blocks × sources; acceptable while memory is bounded by `memory.limit`. Proposal: measure; an in-memory index by position would solve it.

### D117

- **State:** `[ ]` open · **Type:** design question

**Default fraction of trace sampling.** [Doc 10](22-configuration.md) left the `parentbased_traceidratio` fraction "to be defined". Implemented (provisional, step 12b): **0.1** (10% of the parentless traces); `OTEL_TRACES_SAMPLER_ARG` changes it. Question: do you accept 0.1? Alternative: 1.0 (everything), like the SDK default, leaving the reduction to the Collector.

### D118

- **State:** `[ ]` open · **Type:** design question

**Interval of the overflow summary.** Implemented (provisional, step 12b): an `info` log (`taba.storage.overflow`) every 60 s, only if there was overflow, with the number of messages in the window (UC-OPS-034). The interval is a constant. Question: do you accept 60 s? Does it become a parameter?

### PRB-050

- **State:** problem · **Type:** limit

(limit, `telemetry/otlp`): the OTLP export reads only the essentials of the `OTEL_*` variables: the deadline of each send is fixed (10 s), the per-signal variables (`OTEL_EXPORTER_OTLP_METRICS_ENDPOINT` etc.) do not apply to metrics (for traces and logs the SDK reads them), there is no compression, `OTEL_BSP_*` and `OTEL_BLRP_*` are the ones the SDK reads from the process environment, and the JSON log on `stdout` carries neither `trace_id` nor `span_id`. Effect: a collector that requires another deadline, compression or per-signal endpoint only works with the SDK. Proposal: read `OTEL_EXPORTER_OTLP_TIMEOUT` and `OTEL_EXPORTER_OTLP_COMPRESSION` in `config/otlp` and put the trace ids in the JSON when the span is sampled.

### D95

- **State:** `[ ]` open · **Type:** design question

**The node's own IP.** The `join` carries the node's `addresses`, and the node needs to know its own. Implemented (provisional, step 16): `cluster.advertise-addresses` (list of IPs) and, without it, the local address of a UDP socket "connected" to `cluster.entry-address` (nothing is sent); if the discovery fails, the node does not start and tells you to define the parameter. Question: do you accept? Alternative: always require the parameter.

### D119

- **State:** `[ ]` open · **Type:** design question

**The trace context between nodes.** Implemented (provisional, step 16): the HTTP `Transport` carries the `traceparent` of the current span and the cluster port opens a child span when the call carries it (only then: the heartbeat and background tasks generate no trace). The in-memory network of the tests does not carry the header. Question: do you accept? Still to decide whether `tracestate` also travels and whether the received span should always exist, even without `traceparent` (more noise, sampled).

### D120

- **State:** `[ ]` open · **Type:** design question

**HTTP transport deadlines and constants.** Fixed in the code (provisional, step 16): open the connection 5 s, received TLS handshake 10 s, graceful stop of the cluster port 10 s, idle connection kept 60 s, certificate expiry warning repeated hourly, intermediate certificates followed up to 4. The whole call follows the caller's deadline (`cluster.channel.keepalive.interval × 2`, `write.commit.timeout`...). Question: do they stay fixed and documented, or become parameters?

### PRB-051

- **State:** problem · **Type:** risk

(risk, `cluster/transport/http`, `cluster/service`): the cluster recognizes the caller by the connection's source IP, and the HTTP `Transport` does not pin the outgoing IP. On a node with several interfaces (or NAT), the system may choose an IP different from the one informed in `cluster.advertise-addresses`, and the peer answers `403` `cluster.not-member` to a member. Effect: a node that does not communicate with its peers, without a clear error. Proposal: with an explicit, local `cluster.advertise-addresses`, pin the outgoing IP (`HttpTransport::bound_to` already exists, only the tests use it); or identify the member by the certificate/`nodeId` instead of the IP.

### D121

- **State:** `[ ]` open · **Type:** design question

**The wait for the notice's `take` (`cluster.notify.wait`).** Implemented (provisional, step 17a): the topic's notice owner accumulates the ready ones for `cluster.notify.window` (100 ms), sends **one** notice with the quantity to **one** node with a listener (rotating), waits `cluster.notify.wait` for it to come fetch and, if it did not (or came and some were still left), moves on to the next node with a listener, until the messages or the nodes run out; the default wait time ended at 50 ms. The notified node counts `fetched` or `empty` according to the `take` the notice provoked. Question: does 50 ms do (the wait is that of a `take` on the same network)? Alternative: a larger value, or proportional to the channel RTT with the node.

### D103

- **State:** `[ ]` open · **Type:** design question

**Healthy blocks and `under-replicated`.** Doc 11 talks of "healthy replicas" without saying what healthy is nor the expected. Implemented (provisional): healthy is the block owner `active` in the table with the channel up (the node itself counts); the expected is the number of owners the ring gives (`min(replication.factor, members)`, so two nodes with factor 3 do not stay `under-replicated` forever); `blocksBelowReplication` includes the blocks below the quorum, and `under-replicated` appears when there are more blocks below replication than below the quorum; `syncingBlocks` counts the blocks with some `syncing` owner. Question: should the expected always be `replication.factor`? Should a node in `draining` count as healthy?

### D112

- **State:** `[ ]` open · **Type:** design question · **Depends on:** D107 (exit (b), the secondary coordinates).

**Forwarding the FIFO publication and `fifo.forward.max-attempts`.** [Doc 17](09-topics.md) (section 16.4) says to forward through the mechanism of section 6.9 and, on `421`, re-read the table. Implemented (provisional): a call of its own between nodes (`POST /cluster/topics/{topic}/fifo/publish`), because the distributor also publishes to FIFO and has no client request to forward; the client token is not checked again on the primary (whoever received the call already checked it); on `421` the node waits `write.retry.backoff` and re-reads **its own** table, without fetching the primary's; `fifo.forward.max-attempts` is 3. Question: do you accept? Alternative: the primary returns its own table on the `421`. **Depends on:** D107 (exit (b), the secondary coordinates).

### D128

- **State:** `[ ]` open · **Type:** design question

**The per-group round within the band.** Fair delivery chooses the group's band by size (power of 2); on a band tie the code chooses the oldest message among the groups of the band (and the newest on the next turn), so two small groups of similar size alternate by age, not by round (UC-GRP-007). The text of [12-delivery.md](12-delivery.md) (section 12.11) speaks of a round. Question: should the per-group round apply within the band? The proposal is to keep by age, which is fair between groups of similar size, and say so in 12.11.

## 2. Blocked by another decision or step

They are answered **after** what is in **Depends on** (or together).

### D97

- **State:** `[ ]` open · **Type:** design question · **Depends on:** D104 (the same table version).

**The member's state in the table and the epoch.** `Member.state` (`syncing`, `active`...) is a field of the member table, but the change from `syncing` to `active` is not a two-phase membership change: the node that became ready announces the state by signal (`node-state` `active`, to all, with retries, and repeated every `cluster.channel.keepalive.interval`), and each node changes the field in its own table **without raising the `version`**. Question: should the epoch go up, or should the state leave the table and live only in the heartbeat? Proposal: keep it so (the epoch keeps counting only joins and leaves, and `membership-divergent` does not fire because of the state); if the channel arrives, the heartbeat starts to carry the state. **Depends on:** D104 (the same table version).

### D104

- **State:** `[ ]` open · **Type:** design question · **Depends on:** D97 (if the epoch starts to count the state, the condition changes).

**`membership-divergent` only between nodes.** The heartbeat carries the table version, but arrives with up to `cluster.channel.keepalive.interval` of delay, and deriving the condition from it would make it blink at every membership change. Implemented: the condition only exists in `scope=cluster` (different versions among the nodes that answered); `scope=node` and `taba.cluster.condition` do not show it. Question: do you accept, or should the node compare it with the version the peers say they have, accepting the blinking? **Depends on:** D97 (if the epoch starts to count the state, the condition changes).

### D88

- **State:** `[ ]` open · **Type:** design question · **Depends on:** D92 (the group ceiling and the per-group window).

**Memory ceilings of the index.** On the fair queue the index's memory already has a ceiling: the ready list (`delivery.ready-list.size`), the invisible list (`delivery.invisible-list.size`, with the `expiry` table) and the per-group counters; the other messages are only counted, and the engine reads the metadata when it needs them. What is left: (1) `delivery.fair.max-groups` needs a default (today no ceiling; the doc 10 suggestion is 100000); (2) the FIFO queue keeps one entry per message, because each group's list is complete by design; (3) the cluster's replica table keeps the ID and age of each replica. Question: set the `max-groups` default and accept items 2 and 3 as they are, or bound them too (the FIFO queue with each group's list in the database, the replicas without an in-memory table)? Linked problem, **PRB-020** (definition, `engine/messages/index`): `delivery.fair.max-groups` has no default (doc 10 suggests 100000). Without it, a producer that invents a `groupId` per message creates a group per message. Effect: Memory and choice time grow with the number of groups in the lowest band. **Depends on:** D92 (the group ceiling and the per-group window).

### D92

- **State:** `[ ]` open · **Type:** design question · **Depends on:** D88 (`delivery.fair.max-groups`).

**Per-group rates.** `GroupStats` promises `publishRate`, `consumeRate` and `ackRate` per group, and the groups have no ceiling. Question: how to measure without a per-group window? The proposal: keep the window only for the `stats.topics.top-groups` largest groups of each topic (recomputed on reading `/groups?top=N`), and leave the rates absent on the others; the alternative is a compact per-group window (count per 10 s, 30 buckets) with the `delivery.fair.max-groups` ceiling. Linked problem, **PRB-024** (incomplete, `http/topic_stats`): `GroupStats` does not carry `publishRate`, `consumeRate` and `ackRate`: a rate window per group would cost memory proportional to the number of groups, which has no ceiling. Effect: The noisy producer can only be found by the size of the group, not by its throughput. **Depends on:** D88 (`delivery.fair.max-groups`). Step 12b did not implement it: it waits for this decision.

### D85

- **State:** `[ ]` open · **Type:** design question · **Depends on:** D84 (the burst only exists with `rps`).

**Burst (`ratelimit.burst`) without a value.** Today the rate rounded up applies, at least 1 (one second of room). Proposal: keep it and write that in [19-errors-and-limits.md](19-errors-and-limits.md); the alternative is a mandatory `ratelimit.burst` when there is `rps`. **Depends on:** D84 (the burst only exists with `rps`).

## Sources and evidence

- Each item cites its evidence: the code module, the test or the run described in [23-operations.md](23-operations.md) and [13-observability.md](13-observability.md).
- The roadmap that closes these pending items is in [32-roadmap.md](32-roadmap.md).
