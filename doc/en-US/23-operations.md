---
title: Operations (runbook)
summary: Practical guide for whoever runs a Taba cluster — start, verify, scale, upgrade, retire nodes, diagnose each health condition, what to do on each failure and what not to do.
type: guide
status: stable
verified-on: 2026-10-14 @ efeb47a
sources: [docker-compose.yml, docker/haproxy.cfg, docker/cluster.sh, docker/validate.sh, crates/taba-node/src/cluster/health, crates/taba-node/src/cluster/drain, crates/taba-node/src/http/operations.rs]
---

# Operations (runbook)

> Read first: [07-cluster.md](07-cluster.md) and [13-observability.md](13-observability.md). The tests of this guide were **run** on a three-node cluster started by `docker compose up` (Windows, Docker 29); where something is only guidance, it says so.

## Summary for whoever is on call

| Rule | Why |
|---|---|
| **Restart and upgrade one node at a time**, waiting for `healthy` between them | Losing two nodes in quick succession **locks** the cluster (`quorum-lost`) and the way out is restarting everything, **losing messages and topics** |
| **Stop nodes with `SIGTERM`** (`docker stop`, `kubectl delete pod`), with a stop deadline **greater** than `drain.timeout` | Triggers the **drain**: without it, what is only in memory is lost and the cluster only removes it after the vote |
| **Never stop the whole cluster** in production **without the S3 backup** (or without `POST /cluster/stop`) | Every restart is a new node: with all stopped, **messages and topics are lost**. With the backup, stop with `POST /cluster/stop` ([33-s3-backup.md](33-s3-backup.md)) |
| **Protect the operations and cluster ports** on the network | They have no authentication (operations) / trust the certificate (cluster); `POST /node/drain` takes a node down |
| **Have ≥ 3 nodes** | With 2, the failure of one leaves a survivor that does not reach `cluster.vote.min-voters` |

## Start a cluster

The complete, executable recipe is in [02-quick-start.md](02-quick-start.md). In short:

1. **Create the secrets folder once** (`taba init --out <folder>`) and deliver it to **all** nodes as a platform secret ([14-security.md](14-security.md)).
2. **Define** `cluster.entry-address` (the load balancer address on the **cluster port**, TCP/L4, without terminating TLS) on all nodes, and `cluster.first-node=true` **only on the first node of a new cluster**.
3. **Start the first node** and wait for `healthy`; **then the others**. Each becomes ready (`/health/ready`) only when it **finishes synchronizing**.
4. **Configure the client load balancer** to only send traffic to nodes with `/health/ready` = `200` (operations port).

**Check:**

```bash
taba-cli health cluster -o table        # 3 active nodes, empty "conditions", worstCondition "ok"
taba-cli health ready                   # {"health":"ready"} on each node
```

Expected on a healthy cluster: `summary.members.active = N`, `quorumAvailable: true`, `syncingBlocks: 0`, `conditions: []`.

> **First node and restart:** `cluster.first-node=true` only starts the cluster by itself if **no network response** comes from the entry address. If the other nodes are up, it **joins** them. That is why it is safe to leave the key on for it forever (Compose does this).
>
> **Cold start:** if every node falls and comes back together, with its disk, **none needs `cluster.first-node`**: each one tries the entry address and the IPs of the previous life's table (in the manifest); after `cluster.restore.cold-start.wait`, the one with the **lowest old ID** among those alive starts alone, takes over the disk (topics, tokens, messages in quarantine) and the others join it. The restoration of the messages waits for the expected nodes (`cluster.restore.grace`); the FIFO is only released after it. A node that never had a disk still needs `cluster.first-node` to create the cluster.

## Scale

- **Add a node:** start another with the **same image, the same secrets folder** and the **same** `cluster.entry-address`, **without** `first-node`. It discovers the cluster, joins through register + commit on **all** nodes, synchronizes the blocks it took over and only then becomes ready. Joins are **one at a time**: if there is another change or vote in progress, the request waits.
- **Effect on the ring:** the new node takes blocks; the old ones only delete what they lost when the cluster is **healthy** ([06-ring-and-replication.md](06-ring-and-replication.md)).
- **Reduce:** use **drain** (below), never take the node down by force on purpose. After a node leaves, the redundancy of its messages stays at `replication.factor − 1` until a new node joins (accepted, D116). **Do not leave the cluster with fewer than 3 nodes** in production.
- Capacity limits: a node stores **one copy of each message of the blocks it is responsible for**; with `replication.factor=3` and 3 nodes, **every node stores everything** (each node's memory must hold the whole topic, or use `memory.limit` to go to disk).

## Upgrade the version (rolling)

**Start the new version beside the old one: the new nodes retire the old ones.** Each node announces its program version (the `version` field of its `NodeStatus`), and a node that sees members of an **earlier** version drains them, **one at a time**, with the same drain as "Retire a node on purpose": the old node writes its memory to disk and leaves the table, and the orchestrator can then stop it.

1. Add a node of the new version (Compose: scale up with the new image; Kubernetes: the rollout adds the new pod). It joins as a **new node** and synchronizes.
2. When it is `active`, the cluster drains the oldest member. The next drain only starts after the previous one left the table, and **only if at least 3 nodes stay active afterwards**: in a cluster of 3, add the new node first (4 active) so that an old one can leave.
3. Repeat until no node of an earlier version is left, and check `health cluster` (no conditions).

Only **one** node acts: the lowest `nodeId` among those of the newest version, so two nodes never ask for different drains at once. A node of an **earlier** version that joins does not drain the newer ones; the new ones retire it. A member that publishes no version (an earlier release) counts as the oldest.

If a node does not finish draining within `drain.timeout` plus 30 s, the process **stops on it** and the `upgrade-stalled` condition appears (warning): cancel the node's drain (`DELETE /node/drain`) or stop it, and the upgrade goes on.

**Restarting in place, one node at a time,** also works, waiting for `healthy` (`/health/ready`) before the next. Verified: restarting `taba-2`, `taba-3` and `taba-1` in sequence (`docker/cluster.sh restart <node>`) with 14 messages in the topic and consuming everything returned **14 of 14**.

> The versions are **not** made compatible with each other: the cluster API is internal and has no compatibility guarantee between versions ([18-cluster-api.md](18-cluster-api.md)); what a newer version adds between nodes is only sent to nodes that announced that version. `membersVersion` goes up at each membership change, which is normal in a rolling upgrade.

The `stop_grace_period` (Compose) or `terminationGracePeriodSeconds` (Kubernetes, 30 s by default) must be **greater than `drain.timeout`** (25 s). The repository's Compose uses **40 s**. Otherwise the process is killed before recording the result.

## Retire a node on purpose (drain)

```bash
# on the node to retire, through ITS operations port (not through the load balancer)
taba-cli drain start       # POST /node/drain
taba-cli drain status      # drainState: draining → drained; pendingMessages falls to 0
```

Verified: `drained` in ~4 s; `/health/ready` becomes `503`; the cluster is left with the remaining nodes active and no conditions. The node **stays up** (forwarding client calls to another node); stop the process whenever you want. To **come back**: `taba-cli drain cancel` — before leaving it cancels; after leaving it **returns as a new node** (in ~20 s the cluster went back to 3 active, no conditions).

Leaving by drain does **not** cause freezing. If the drain requested through the API exceeds `drain.timeout`, the node stays `draining` until someone calls `drain cancel` (D75).

## What each condition means, and what to do

`GET /health/cluster` returns the **list** of conditions. Combine them: the same failure usually turns on several.

| Condition | Severity | What it means | What to do |
|---|---|---|---|
| `member-unreachable` | warning | The channel with a node dropped and is reconnecting, still before the vote | **Wait ~10 s**: if the node returns, it goes away. If not, the cluster will exclude it. Check the node's machine/container (`docker compose ps`, logs) |
| `write-quorum-at-risk` | **critical** | Some block has healthy replicas below the quorum, or the **primary** is down and still in the table | **Publications to the block fail** (`503 cluster.quorum-unavailable`) until the node is excluded or returns. Common in the first seconds of a failure. **Persistent = real problem**: find the node and restart it |
| `under-replicated` | warning | Blocks with fewer copies than `replication.factor`, but still within the quorum | **Normal and transient** after a failure or a join. Goes away when the new node joins and synchronizes. Persistent with all nodes alive: look at `blocksBelowReplication` and the synchronization logs |
| `node-frozen` | warning | There are node(s) excluded by vote in freezing (default 5 min) | Expected after an exclusion. A **restarted** node (new ID) does **not** go into freezing; only the same process that returns |
| `syncing` | warning | Some node is synchronizing; delivery is suspended on its blocks | Normal when a node joins. If it **does not finish**, look at the `sync`/`bootstrap failure` logs and `cluster.sync.timeout` (60 s without a source) |
| `quorum-lost` | **critical** | Fewer healthy nodes than `cluster.vote.min-voters` (2): **no exclusion takes effect** | There is **only one** node alive. See "Stuck cluster" below |
| `membership-divergent` | **critical** | Member tables with different versions | Incomplete propagation of a change; it usually goes away by itself in seconds. Persistent: look at the `membership` logs and restart the diverging node (**one at a time**) |
| `storage-pressure` | warning | Free space in `storage.dir` below `stats.disk.free-min` (only fires if configured) | Free disk or enlarge the volume **before** it fills: **a full disk takes the node to `failed`** |
| `upgrade-stalled` | warning | The upgrade that retires earlier versions stopped on a node that did not finish draining in time | Cancel that node's drain (`DELETE /node/drain` on its operations port) or stop it; the upgrade goes on with the next. The node is the one in the `draining` state in `GET /nodes` |
| `members-missing` | warning | The probe of the old IPs ended with nodes of the previous life missing | The cluster works; the node that is missing enters through the normal join when it comes back. If it will not come back (the cluster was reduced on purpose), the condition goes away when the table completes or when the nodes restart ([07-cluster.md](07-cluster.md), section 10.11) |
| `reduced-profile` | warning | The node runs the one-node profile (development) | Expected in development. In production it shows that something relaxed was left on: check `replication.factor`, `cluster.tls.enabled` and `auth.dev-token` ([07-cluster.md](07-cluster.md), section 8) |
| `previous-life-pending` | warning | The restoration of the previous lives has not ended (the vote did not close, or the restoration waits for nodes that did not come back) | The FIFO queues receive and **do not deliver** meanwhile. `GET /health/cluster` and the node state (`GET /cluster/restore`) say which old nodes are awaited. Wait for `cluster.restore.grace`; if a node will not come back, force with `POST /cluster/restore/force` (root token, or the operations port): messages already consumed may come back ([07-cluster.md](07-cluster.md), section 10.10) |

## Failures and how to react

### A node went down (process killed or machine)

Expected, **no action**: in ~4 s `write-quorum-at-risk`, `member-unreachable`, `under-replicated` appear; in ~8 s the node is excluded (table version goes up) and only `node-frozen` remains. Nothing that was confirmed is lost (the copies are on other nodes). **Start the node again**: it joins as a new node and synchronizes. Verified with `kill` of `taba-2` followed by `start` (done, at the time, by `docker compose`; today by `docker/cluster.sh kill taba-1`).

During the ~4 s window, publications to the blocks of the downed node **may** answer `503` with `Retry-After`: the client repeats.

### Stuck cluster (`quorum-lost`)

**When it happens:** two nodes of a three-node cluster go down **in quick succession, before the first returns**. One is left; the exclusion of the second does not take effect; the restarted ones **do not rejoin** (the register requires **all** nodes of the table, and the table still lists the dead one: logs `cluster.registration-failed` and `cluster.node-syncing` in a loop).

**Symptoms:** `conditions = [write-quorum-at-risk, quorum-lost, member-unreachable, node-frozen]`; publications and consumptions on parts of the blocks fail or come back empty.

**There is no automatic recovery or forced-exclusion command.** What works: **take the whole cluster down and start it again, with the same volumes**. The cluster resumes from disk (see "All nodes went down"): topics, tokens and the messages that were on disk come back, and what was only in memory is lost. Before doing it, if the survivor still serves, **consume and save what you can**; without the volumes the cluster is born empty. Prevention: one node at a time; consider increasing the number of nodes (5) and `cluster.vote.min-voters` (a majority) on large clusters.

### All nodes went down

**With the S3 backup** ([33-s3-backup.md](33-s3-backup.md)): start the processes again (`docker compose start`). The first node that finds no one checks the bucket, waits for the heartbeat of the dead to expire (`backup.dead-after`, 60 s), takes the restore lock, restores definitions, tokens and messages, and the others join as normal nodes. Only what had not yet gone to S3 (the `backup.delay` window) and the state of the last journal window are lost.

**Without the backup, with the volumes kept:** start all the nodes again (`docker compose start`), without `first-node`. Each node sets the disk of the previous life aside in a `_lives/<old ID>` folder; after `cluster.restore.cold-start.wait` (15 s) the node with the lowest old ID among the live ones starts alone and takes over the disk (topics and tokens), the others join it, and the messages come back through the restoration of the previous lives ([07-cluster.md](07-cluster.md), section 10). Watch `GET /health/cluster`: `previous-life-pending` goes away when the restoration ends. If an old node does not come back, the restoration goes on with the present ones after `cluster.restore.grace` (5 min), or sooner, forced (`POST /cluster/restore/force`); with `cluster.restore.strict` it waits for the force. What was **only in memory** on every node is lost, and a message already consumed only reappears in the cases of section 10.10 (the consumer must be idempotent). **Without the volumes** messages and topics are lost: recreate the topics and tokens. The **root token and the certificate** are in the secrets folder and survive. Plan for this: producers with retry and topics recreatable by automation.

### Node in `failed` (disk full or corrupted database)

The node stops serving **client calls** (forwards to another node, or `503 node.failed`), keeps answering node-to-node calls and the operations routes, and **the others are free to vote its exclusion**. **Only restarting the process** takes it out of `failed` (there is no re-evaluation at runtime). Before restarting: free disk. Corrupted database: **there is no repair**; restart with a new disk if needed (the node joins as new).

### Node that does not become ready

`/health/ready` returns `503` with `reason`: `bootstrapping` (joining), `syncing` (synchronizing), `returning`, `draining`/`drained`, `failed`. If it stays in `bootstrapping`: check `cluster.entry-address` (**cluster** port, over TCP), the certificate (the same secrets folder), and the "bootstrap failure" logs (`node listing at … failed`, `cluster.registration-failed`, `cluster.node-frozen` with the remaining time).

### Publications answering `503`

| Code | Likely cause | Action |
|---|---|---|
| `cluster.quorum-unavailable` | Not enough nodes to close the quorum, or the block/group primary is out | Look at `health cluster`; repeat after `Retry-After` |
| `cluster.commit-timeout` | Register confirmed, commit slow | On topic/token the operation **will complete**; query before repeating |
| `cluster.node-syncing` / `cluster.node-draining` | The node served but found no other to forward to | Take the node out of the load balancer; repeat |
| `fifo.primary-unavailable` | The FIFO group's primary is out, synchronizing or exhausted attempts | Look at `health cluster`; repeat |

## Certificates: rotation without restarting

Procedure in [14-security.md](14-security.md). In short: generate the new pair with `--starts-in` greater than the propagation time, **add** it to the folder (the old one stays), and wait. Monitor `taba.cluster.tls.expires_in` (alert at < 30 days) and the expiry `warn`s (`cluster.tls.expiry-warning`). **Before** the certificate expires: an expired one stops being accepted and the node loses the cluster.

## Change the root token

Edit the root token file with **two lines** (the new one and the previous); each node re-reads it within `auth.reload-interval` (60 s) and accepts both; then remove the previous one. Tokens already created keep working.

## Backup and restore

Durability comes from **replication**, not from one node's disk: copying a node's `storage.dir` is **not** a restorable backup. The **optional S3 backup** ([33-s3-backup.md](33-s3-backup.md)) covers the loss of all nodes. Even with it, keep the **secrets folder** (`taba init`): the cluster certificate and the root token do **not** go to the bucket.

### Turn the backup on

```yaml
backup:
  s3:
    bucket: taba-backup
    region: sa-east-1          # credentials: the default AWS chain (IAM, profile, task role)
```

With SeaweedFS, MinIO or another compatible S3 (one that honors conditional writes; Garage does not): `backup.s3.endpoint`, `access-key` and `secret-key`. In production the bucket **must** have versioning/retention on the S3 side and write-only permission for the nodes (read is needed to restore). A ready configuration is in [`examples/backup-s3`](../../examples/backup-s3/README.md). Check with `GET /node/backup`: `s3Reachable: true` and `pending` going to zero.

### Force the backup now (flush)

Before maintenance, an infrastructure change or any risk, send everything to the bucket **without stopping anything**:

```bash
# on this node (the node's operations port, not the load balancer):
curl -s -X POST http://node:9090/node/backup/flush
# on the whole cluster (the node forwards to the others and gathers the responses):
curl -s -X POST http://node:9090/cluster/backup/flush
```

Each node does: memory → disk, disk → S3 and `outbox` → S3. `"complete": true` means **nothing was left pending**. A node with `"pending": true` was slow or down: repeat the call and check `GET /node/backup` (`pending: 0`). Without S3 configured the response is `422 backup.not-configured`. During the flush the node writes new messages to disk; with the disk above 90% the move pauses until eviction frees space (or `backup.drain.timeout`).

### Stop and start the whole cluster (with backup)

```bash
curl -s -X POST http://node:9090/cluster/stop     # drains all nodes, sends to S3 and writes the stop list
# check: "stopList": true and "warnings": []
docker compose stop                               # (or shut down the processes as your platform requires)
…
docker compose start                              # "start" is starting the processes: restore is automatic
```

If `stopList` comes `false`, S3 was unavailable: the disk drain was done, but **the backup was not sent**; do not shut down the infrastructure until you repeat the `stop` with S3 back (or accept losing the unsent window). The `stop_grace_period` (Docker) or `terminationGracePeriodSeconds` (Kubernetes) must exceed `drain.timeout` + `backup.drain.timeout` (the repository's `docker-compose.yml` uses 70 s).

### What the backup does not do

It is not a point-in-time recovery (there are no message versions or "go back to yesterday"); it does not restore the secrets folder; and the lag (RPO) is that of `backup.delay` and `backup.meta.interval`. The consumer must be **idempotent**: a restore may return as deliverable a message whose receipt or invisibility from the last window was behind.

## Monitor

- **Minimum:** `liveness` probe on `/health/live`, `readiness` on `/health/ready`; alert on `worstCondition != "ok"` for more than a few minutes; `up == 0`; `quorumAvailable == false`.
- **Prometheus:** scrape `GET /metrics` of **each** node (one target per node). Example alerts in [13-observability.md](13-observability.md).
- **Topics:** `taba-cli stats topics --condition backlog` (the `stats.topics.*` thresholds must be configured). ⚠️ The aggregation of topic metrics **across nodes** is not implemented: query each node or add in the backend.
- **Logs:** `telemetry.logs.format=json` for ingestion; `color` for the terminal. Look for `error`: message loss (`taba.message.lost`), bootstrap failure, drain that exceeded the deadline.

## Kubernetes

The repository has a **Helm chart** (`deploy/helm/taba/`, [34-helm-chart.md](34-helm-chart.md)); its operating modes and bash tests, run on a kind cluster, are in [35-helm-use-cases.md](35-helm-use-cases.md). It has not been validated on a production cluster. What the node offers to fit in (and the chart uses): `/health/live` and `/health/ready` probes on the operations port; stop by `SIGTERM` with drain (`terminationGracePeriodSeconds` > `drain.timeout`); `cluster.entry-address` pointing to an **internal** (or *headless*) `Service` that passes the cluster port **over TCP**, without terminating TLS; the secrets folder as a mounted `Secret`; `storage.dir` on a volume per pod (the last folder named `taba`). `StatefulSet` or `Deployment` both work (the node **does not depend** on a stable identity: every start is a new node). Rolling update: `maxUnavailable: 1`.

## The repository's Docker Compose: useful commands

```bash
docker compose up -d                               # starts everything (public images; TABA_VERSION chooses the tag)
docker compose -f docker-compose.yml -f docker-compose.dev.yml up --build -d   # the same, building from the repository
docker compose ps                                  # node health
docker compose run --rm taba-cli health cluster -o table
docker compose up -d --scale taba=4                # more nodes: the `taba` service has the replicas (nodes 2 onwards)
docker compose logs -f taba-0                      # colored logs (`taba` shows the replicas)
docker/cluster.sh stop taba-2  / start taba-2      # normal stop (drain) and return; taba-N is replica N (taba-0 is the seed)
docker/cluster.sh kill taba-2                      # simulates failure
docker/cluster.sh restart taba-1                   # restart (one at a time!)
docker compose down                                # ⚠️ STOPS EVERYTHING: the cluster is born empty on the next up
docker compose down -v                             # also deletes the volumes (includes the root token)
```

`docker/cluster.sh` (to `source` or as a command) starts/stops Compose, provokes failures (`taba_kill`, `taba_stop`, `taba_start`), waits for the cluster to settle and creates tokens and topics; `docker/validate.sh` runs the complete script (≈ 5 minutes: the cluster forms, `taba-cli`, nodes going down and coming back, **absence of message loss**).

> **On Windows with Git Bash**, set `MSYS_NO_PATHCONV=1` for commands that take container paths.

## Sources and evidence

- `cluster/backup` (S3 backup: [33-s3-backup.md](33-s3-backup.md)), `cluster/health/mod.rs` (the 8 conditions), `cluster/drain`, `cluster/state`, `cluster/service` (bootstrap, join, freezing, voting), `http/operations.rs`, `docker-compose.yml`, `docker/`.
- **Real runs** of this guide (3-node cluster, `docker compose`): kill of a node (conditions at 4 s and 8 s), normal stop with drain, drain through the API (`drained` in ~4 s and return as a new node), rolling restart of the three nodes (14/14 messages), loss of two nodes in a row (`quorum-lost`, rejoin impossible), `down`+`up` (cluster is born empty).
- Use cases: UC-FAL, UC-FRZ, UC-DRN, UC-RTN, UC-ING, UC-OPS; see [26a](26a-implemented-use-cases.md). Tests: `tests/failures.rs`, `freezing.rs`, `drain_cluster.rs`, `recovery.rs`.
- History: `04-cluster.md`, `11-observabilidade.md`.
