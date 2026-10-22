---
title: Configuration
summary: How to configure the node (YAML file and TABA_* variables), the precedence, and the complete table of parameters with the effective default and what each one does.
type: reference
status: stable
verified-on: 2026-10-14 @ efeb47a
sources: [crates/taba-node/src/config/params/mod.rs, crates/taba-node/src/config/mod.rs, crates/taba-node/src/config/source/mod.rs]
---

# Configuration

> Read first: [04-architecture.md](04-architecture.md). The table of keys and defaults in the code is `config/params/mod.rs`; **a test checks that every parameter in the table is read** by `Config`. A new parameter goes in the table **and** in `Config` ([24-conventions.md](24-conventions.md)).

## How to configure

There are **two ways**, which apply to **all** parameters, and can be used together:

1. **`.yml` file**: each dot in the key becomes a nesting level. Suited to the whole configuration (versioned, mounted as a `ConfigMap` or volume).
2. **Environment variables**: each parameter has its own. Suited to what changes per environment or per node, and to secrets.

**Precedence** (highest to lowest): **environment variable** → **`.yml` file** → **default**.

**The file.** The path comes from `--config <path>` or from `TABA_CONFIG_FILE`; the default is `/etc/taba/taba.yml`. Without the file, the node uses only variables and defaults. A **given** path that does not exist **prevents startup**; the absent default does not. **A key that is not a parameter** (typo) **prevents startup**, with the name in the message; a `TABA_*` variable that is not a parameter only produces a warning in the log.

**Variable name.** Prefix `TABA_`, the key in uppercase, with `.` and `-` replaced by `_` (`topics.distributor.max-targets` → `TABA_TOPICS_DISTRIBUTOR_MAX_TARGETS`). The OpenTelemetry `OTEL_*` variables are the exception: they do not go through here ([13-observability.md](13-observability.md)).

**Values.** Lists (`operations.allowed-ips`, `client-ip.headers`, `client-ip.trusted-proxies`, `cluster.advertise-addresses`) are native YAML in the file and **comma-separated text** in the variable. **Every duration takes a unit** (`ms`, `s`, `m`, `h`, `d`; `60` alone is **refused**). Sizes use binary units (`KB`, `MB`, `GB`, `TB`; `5MB` = 5,242,880 bytes; a number alone is bytes).

```yaml
# taba.yml
mode: production
api:
  port: 8080
cluster:
  port: 8081
  entry-address: taba-lb.internal:8081
replication:
  factor: 3
topics:
  distributor:
    max-targets: 50
```

```bash
# the same keys as variables
TABA_API_PORT=8080
TABA_CLUSTER_PORT=8081
TABA_CLUSTER_ENTRY_ADDRESS=taba-lb.internal:8081
TABA_REPLICATION_FACTOR=3
TABA_TOPICS_DISTRIBUTOR_MAX_TARGETS=50
```

## Startup validations

The node **does not start** (message in Portuguese, one per problem) when:

- `storage.dir` does not end in a folder named **`taba`**;
- the three ports repeat ("use the same port; the three ports must be different"), or one is in use;
- `api.pagination.default-limit` is outside 1 up to `api.pagination.max-limit`;
- `message.max-bytes` is zero; `replication.factor` is less than 1; `write.quorum` is outside 1 up to `replication.factor`;
- in production, `cluster.entry-address` is missing (except on the first node, `cluster.first-node=true`), or it is not `host:port`;
- `cluster.vote.quorum-percent` is outside 1 to 100; `cluster.vote.min-voters`, `ring.virtual-nodes` or `write.retry.attempts` are less than 1;
- `delivery.visibility-timeout.default` or `delivery.max-messages.default` exceed their `.max`; `delivery.fair.ready-weight` is outside 0 to 1;
- `retention.ttl.default`, `retention.max-deliveries.default` or `topics.volatile.idle-timeout.default` are outside the ranges of [19-errors-and-limits.md](19-errors-and-limits.md);
- a defined `cluster.tls.dir` does not exist, or the folder exists with no valid pair;
- `auth.enabled=true` without a root token; a root token with fewer than 32 characters; a defined `auth.root-token-file` that does not exist;
- `cluster.tls.enabled=false` or `auth.dev-token` with a `replication.factor` other than 1; an `auth.dev-token` shorter than 32 characters or with a dot;
- `operations.allowed-ips` or `client-ip.trusted-proxies` have an invalid entry;
- `backup.s3.access-key` without `backup.s3.secret-key` (or the reverse); `backup.segment.max-bytes` or `backup.segment.max-messages` equal to zero; the rule `backup.disk.evict-to` < `backup.disk.evict-above` < `backup.disk.pause-at` ≤ 100 violated; `backup.dead-after` that does not exceed `backup.heartbeat.interval` (every node would look dead).


`mode` and `development.resume-on-start` **no longer exist** (the development mode went away, [37-cluster-cold-start.md](37-cluster-cold-start.md), section 4.16): a file or a variable that still sets them keeps starting, with a warning in the log, and they do nothing. To run a single node, use the one-node profile (`replication.factor=1`, and if you want `cluster.tls.enabled=false` and `auth.dev-token`).

## How to read the tables

- **Default in bold** = the effective value. **"(from the code)"** marks a default that the parameter table leaves without a value but the code applies (the original design documents called it "To be defined").
- **No value** = the feature stays off or without a ceiling (the column says which).
- ⚠️ **Accepted, but with no effect today:** `Config` reads the value and **no code uses it**. There are seven (`delivery.ready-list.size`, `delivery.invisible-list.size`, `delivery.invisible.reload-threshold`, `cluster.sync.stream.max-line-bytes`, `cluster.sync.stream.idle-timeout`, `fifo.dedup.index.max-entries`, `stats.topics.cache-ttl`): pending item recorded in [31-pending-items.md](31-pending-items.md).
- The tables were **assembled** from the code (`config/params/mod.rs`, the defaults and the reading of each field) and from the descriptions of `10-configuracao.md`; the script `doc/pt-BR/scripts/verificar.mjs` ([27-ci-cd.md](27-ci-cd.md)) checks that **every key in the code appears here** and that no documented key stopped existing.

## Run mode

| Key | Variable | Default | What it does |
|---|---|---|---|

## Ports

| Key | Variable | Default | What it does |
|---|---|---|---|
| `api.port` | `TABA_API_PORT` | **8080** | Port of the client API routes (section 8.1). Without `operations.port`, it also serves the operations routes. Answers `404` to `/cluster/*`. |
| `cluster.port` | `TABA_CLUSTER_PORT` | **8081** | Port of the `/cluster/*` routes and the WebSocket channel between nodes. It is the one the member table stores and the one `cluster.entry-address` points to. Only the other nodes should reach it; **do not publish** on the client load balancer. A single node (factor 1) opens it the same way. |
| `operations.port` | `TABA_OPERATIONS_PORT` | the client port | See the "Observability" section below. |

### Cluster port security

| Key | Variable | Default | What it does |
|---|---|---|---|
| `secrets.dir` | `TABA_SECRETS_DIR` | **/etc/taba/secrets** | Folder the operator creates once with `taba init --out <folder>` and mounts on all nodes, as a platform secret. It has `tls/` (cluster certificates) and `root-token` (root token). The two parameters below derive from it, and whoever wants other paths defines them separately. |
| `cluster.tls.dir` | `TABA_CLUSTER_TLS_DIR` | **<secrets.dir>/tls** (from the code) | Folder with the cluster certificates, in pairs `<name>.crt` (PEM) and `<name>.key`, generated by `taba init` or `taba cert generate`. The node trusts every unexpired certificate in the folder and presents the pair with the most recent start that is already valid. Mutual TLS turns on **by itself** when the folder exists, and the node does not start without at least one valid pair; if it does not exist, the port serves plain HTTP with a warning in the log. A defined value that does not exist prevents startup. |
| `cluster.tls.enabled` | `TABA_CLUSTER_TLS_ENABLED` | **true** | `false` turns the cluster TLS off (no secrets folder). **Only accepted with `replication.factor=1`**: with more than one node the process refuses to start. Part of the one-node profile, for development only (doc 37, section 4.16). |
| `cluster.tls.reload-interval` | `TABA_CLUSTER_TLS_RELOAD_INTERVAL` | **60s** | How often the folder is checked. Changes apply without restarting the node; an invalid folder keeps the previous set and logs `error`. |
| `cluster.tls.expiry-warning` | `TABA_CLUSTER_TLS_EXPIRY_WARNING` | **30d** | How far in advance the node logs `warn` for the certificate in use that is about to expire. |

## Client API authentication

| Key | Variable | Default | What it does |
|---|---|---|---|
| `auth.enabled` | `TABA_AUTH_ENABLED` | **auto** | `auto`: turns on **by itself** when a root token exists (the `root-token` of the secrets folder, `auth.root-token-file` or `auth.root-token`) and stays off without it. `false` forces off, even with the folder mounted. `true` requires the root token (the node does not start without it). When on, the client routes require `Authorization: Bearer`. |
| `auth.root-token` | `TABA_AUTH_ROOT_TOKEN` | empty | Secret, the same on all nodes, with **full access** to the client API (all permissions and the only one that manages `/tokens`). Created by `taba init` (or standalone, by `taba token generate`); at least 32 characters. The existence of a root token (this one, `auth.root-token-file` or the `root-token` of the secrets folder) is what turns authentication on in `auth.enabled=auto`. |
| `auth.root-token-file` | `TABA_AUTH_ROOT_TOKEN_FILE` | **<secrets.dir>/root-token** (from the code) | Path of a file with the root token, delivered as a platform secret. Accepts **two lines** (the current and the previous), to change the secret without restarting. Takes precedence over `auth.root-token`. |
| `auth.reload-interval` | `TABA_AUTH_RELOAD_INTERVAL` | **60s** | How often `auth.root-token-file` is re-read. |
| `auth.dev-token` | `TABA_AUTH_DEV_TOKEN` | empty | A secret of at least 32 characters (no dot). The start **creates** the work token `dev` (`admin`, `publish` and `consume`; only the hash is stored) and turns authentication on; the client uses `tb_dev.<secret>`. If the secret changed, the start updates it. **Only accepted with `replication.factor=1`**, and for development only: it is not the root token, so it does not stop the cluster or manage other `admin` tokens. |
| `health.single-node-condition` | `TABA_HEALTH_SINGLE_NODE_CONDITION` | **true** | Shows the informative `reduced-profile` condition in `/health/cluster` when the node runs with something relaxed (`replication.factor=1`, `cluster.tls.enabled=false` or `auth.dev-token`). `false` hides it. It does not bring `/health/ready` down. |

## Stop and drain mode

| Key | Variable | Default | What it does |
|---|---|---|---|
| `drain.on-stop` | `TABA_DRAIN_ON_STOP` | **true** | On receiving `SIGTERM` or `SIGINT`, the node enters drain mode (all messages go to disk, the node leaves the rotation) and only then exits. `false` exits immediately, like a crash. |
| `drain.timeout` | `TABA_DRAIN_TIMEOUT` | **25s** | Maximum time of the whole drain: emptying memory to disk and completing the voluntary deregistration. Includes waiting for forwarded calls to finish. When exhausted, the node records what is left and exits (if by signal), and the cluster falls back to the usual detection and vote. Must be **shorter** than the orchestrator's stop deadline (`terminationGracePeriodSeconds`, 30 s by default in Kubernetes). |

## Storage and memory

| Key | Variable | Default | What it does |
|---|---|---|---|
| `memory.limit` | `TABA_MEMORY_LIMIT` | no limit (nothing goes to disk by overflow) | Memory space for messages (e.g. MB/GB). On reaching the limit, the excess messages go to disk (overflow). |
| `memory.rate.window` | `TABA_MEMORY_RATE_WINDOW` | **5m** | Window of the moving average of `consumeRate` and `publishRate`, which decides the division of memory between topics. |
| `memory.topic.reserve` | `TABA_MEMORY_TOPIC_RESERVE` | **1000** | Memory guaranteed to each topic, outside the division by read rate. |
| `storage.dir` | `TABA_STORAGE_DIR` | **/var/lib/taba** | Local root of the node, where the overflow `${topic}/${id}.body` files, the node database `_node.db` (topic definitions, among other tables) and the synchronization event database (`_sync/sync.db`) live. Each node has its own; nothing is shared. The last folder must be named `taba`. If it does not exist, the node **creates it at start**; if it cannot create or write, the node does not start. |

## Pagination

| Key | Variable | Default | What it does |
|---|---|---|---|
| `api.pagination.default-limit` | `TABA_API_PAGINATION_DEFAULT_LIMIT` | **100** | Items per page when the paginated route receives no `limit`. |
| `api.pagination.max-limit` | `TABA_API_PAGINATION_MAX_LIMIT` | **1000** | Largest accepted `limit`; above it, `400` `validation.out-of-range`. The default cannot exceed it. |

## Messages

| Key | Variable | Default | What it does |
|---|---|---|---|
| `message.max-bytes` | `TABA_MESSAGE_MAX_BYTES` | **5MB** | Maximum size of a published message body, measured in the **bytes sent** (not the base64 form). Applies to every topic that does not define its own `maxMessageBytes`. Above it, `POST /topics/{topic}/messages` answers `413`. |
| `message.hard-limit` | `TABA_MESSAGE_HARD_LIMIT` | no ceiling | Absolute ceiling for the `maxMessageBytes` of any topic, against abuse by whoever configures. The general one (`message.max-bytes`) cannot exceed it either. |

## Write and durability

| Key | Variable | Default | What it does |
|---|---|---|---|
| `replication.factor` | `TABA_REPLICATION_FACTOR` | **3** | Total number of responsible nodes (primary + replicas) that must end up receiving the message — "full" durability, reached in the background when the quorum is smaller than the total. A FIFO queue requires at least **2** (section 16.2 of [09-topics.md](09-topics.md)). |
| `write.quorum` | `TABA_WRITE_QUORUM` | **floor(replication.factor / 2) + 1** (from the code) | Number of confirmations — **always including the block's primary** (never a quorum of only replicas) — enough to decide the commit and answer the client, without waiting for the other replicas. Only applies to **messages** (write, delivery/invisibility, ack); in the FIFO queue it also includes the **secondary** (section 16.5 of [09-topics.md](09-topics.md)); node registration and topic definition keep requiring all nodes, with no reduced quorum. **A single value** for write, delivery and ack. Validation: from 1 up to `replication.factor`; below the majority is accepted, but allows losing data if the primary and the confirmed replica fall together. Without enough nodes answering for the quorum, the operation fails before deciding and the API answers `503` with `Retry-After`. |
| `write.retry.attempts`<br>`write.retry.backoff` | `TABA_WRITE_RETRY_ATTEMPTS`<br>`TABA_WRITE_RETRY_BACKOFF` | **3**<br>**200ms** | Attempts (and interval/backoff) when repeating the call to the node that failed, both in register and in commit. |
| `write.pending.ttl` | `TABA_WRITE_PENDING_TTL` | **30s** | Time after which a register (phase 1) without commit is discarded by itself, if no rollback has arrived. Only applies **before** the quorum decides; after that there is no more discarding. |
| `write.commit.timeout` | `TABA_WRITE_COMMIT_TIMEOUT` | **10s** | Maximum time the coordinator insists on the commit **on replicas outside the quorum**, after already answering the client. When the ceiling is exhausted without confirmation on some replica, the coordinator **does not roll back** (the quorum, with the primary, has already made the message effective) and keeps trying in the background — the block stays temporarily below the full replication factor, but never below the quorum. |

## Cluster

| Key | Variable | Default | What it does |
|---|---|---|---|
| `ring.virtual-nodes` | `TABA_RING_VIRTUAL_NODES` | **128** | Number of positions each physical node occupies on the hash ring, to improve block distribution. |
| `cluster.channel.keepalive.interval` | `TABA_CLUSTER_CHANNEL_KEEPALIVE_INTERVAL` | **2s** (from the code) | Periodicity of the heartbeat (its own message with date and time, monitoring summary and ready messages per topic) sent on each node channel, and time without a signal to consider it down. The heartbeat carries the node state and the member table version, so it repeats the state at each interval; `2 ×` this value (at least 500 ms) is also the deadline of a channel call (D109). |
| `cluster.channel.keepalive.max-topics` | `TABA_CLUSTER_CHANNEL_KEEPALIVE_MAX_TOPICS` | **1000** (from the code) | Maximum number of topics, with ready messages, reported in each heartbeat. |
| `cluster.notify.window` | `TABA_CLUSTER_NOTIFY_WINDOW` | **100ms** | Time the topic's notice owner accumulates ready messages before sending a single notice. |
| `cluster.notify.wait` | `TABA_CLUSTER_NOTIFY_WAIT` | **50ms** (from the code) | Time the topic's notice owner waits for the notified node to come fetch the messages (`take`) before moving on to the next node with a listener. |
| `cluster.channel.reconnect.retries`<br>`cluster.channel.reconnect.backoff` | `TABA_CLUSTER_CHANNEL_RECONNECT_RETRIES`<br>`TABA_CLUSTER_CHANNEL_RECONNECT_BACKOFF` | **3** (from the code)<br>**1s** (from the code) | Attempts to reconnect (and interval/backoff) before opening the exclusion vote of a node whose channel dropped. |
| `cluster.channel.queue.max-size`<br>`cluster.channel.queue.max-age` | `TABA_CLUSTER_CHANNEL_QUEUE_MAX_SIZE`<br>`TABA_CLUSTER_CHANNEL_QUEUE_MAX_AGE` | **10000** (from the code)<br>**60s** (from the code) | Maximum size and age of the output queue for a node with a channel down; when exceeded, the node with the channel down is treated as failed and the exclusion vote is opened. |
| `cluster.vote.quorum-percent` | `TABA_CLUSTER_VOTE_QUORUM_PERCENT` | **51** | Percentage of **healthy** nodes (those the voter sees and that answer; the ones down do not vote nor enter the base) that must agree to make the node's exclusion effective: `ceil(S × percentage)`. |
| `cluster.vote.min-voters` | `TABA_CLUSTER_VOTE_MIN_VOTERS` | **2** | Minimum number of healthy nodes for a vote to count; below it, no exclusion takes effect. Prevents an isolated node from excluding all the others. |
| `cluster.freezing.duration` | `TABA_CLUSTER_FREEZING_DURATION` | **5m** (from the code) | Time during which a node deregistered for failure is prevented from registering again (anti-flapping). During freezing the join request is rejected with `Retry-After`. |
| `cluster.entry-address` | `TABA_CLUSTER_ENTRY_ADDRESS` | required with `replication.factor` above 1 (except the first node) | Address (`host:port`, port from 1 to 65535; an invalid form prevents the node from starting) of the load balancer in front of the cluster, **on the cluster port** (`cluster.port`, not the client API one), used by the node at start to list the members (`GET /cluster/nodes`) and join the cluster. Without it the node does not start, **except the first node and the one-node profile** (`replication.factor=1`), where it is not used. |
| `cluster.advertise-addresses` | `TABA_CLUSTER_ADVERTISE_ADDRESSES` | **detected (interface that goes out to the entry-address)** (from the code) | List of IPs (comma-separated) that the node puts in the `join` and that the member table stores; the other nodes recognize it by the source IP of the connections. Without a value, the node uses the local address of a UDP socket "connected" to `cluster.entry-address` (nothing is sent), that is, the interface that goes out to the cluster. Whoever has NAT, several interfaces or a name that does not resolve to the right interface defines the parameter. |
| `cluster.first-node` | `TABA_CLUSTER_FIRST_NODE` | **false** | Only on the first node of a cluster that **never existed**: if the listing finds no member, it starts the cluster by itself. A node **with a previous life** does not need it: it also looks for the cluster through the IPs of the previous table and, after `cluster.restore.cold-start.wait`, starts alone if it has the lowest old ID among those alive. On the others, a listing with no answer is an error and the node tries again. |
| `cluster.restore.grace` | `TABA_CLUSTER_RESTORE_GRACE` | **5m** | Grace period `G` that the restoration of the previous lives' messages waits for the expected nodes (the members of the biggest old table). After it, the restoration goes on with a warning and the nodes that are present (or stays held, in strict mode). Greater than zero. |
| `cluster.restore.strict` | `TABA_CLUSTER_RESTORE_STRICT` | **false** | Strict mode: after `G` the restoration stays held until the force operation, instead of going on with the nodes that are present. |
| `cluster.restore.clock-slack` | `TABA_CLUSTER_RESTORE_CLOCK_SLACK` | **5s** | Slack when comparing a life's `last_seen` (the clock of the node that wrote it) with the cluster cut-off date (the clocks of other nodes): a life is restored only if seen after the date **plus** the slack. |
| `cluster.restore.probe.timeout` | `TABA_CLUSTER_RESTORE_PROBE_TIMEOUT` | **2h** | Total time the node probes the IPs of the previous life's table that are still missing from the cluster. Not shorter than the interval. |
| `cluster.restore.probe.interval` | `TABA_CLUSTER_RESTORE_PROBE_INTERVAL` | **10s** | Interval between two probes. Greater than zero. |
| `cluster.restore.cold-start.wait` | `TABA_CLUSTER_RESTORE_COLD_START_WAIT` | **15s** | How long a node **with a previous life** looks for the cluster (through the entry address and the IPs of the previous life's table) before it may start alone. After that, the one with the lowest old ID among the nodes alive starts. A node with no previous life never starts alone (only `cluster.first-node`). |
| `cluster.bootstrap.retry.interval` | `TABA_CLUSTER_BOOTSTRAP_RETRY_INTERVAL` | **5s** | Wait between one attempt to join the cluster and the next, after a failure (listing, register, synchronization or exclusion for a register failure). |
| `cluster.bootstrap.retry.max-attempts` | `TABA_CLUSTER_BOOTSTRAP_RETRY_MAX_ATTEMPTS` | **0** | Maximum number of attempts; `0` repeats without stopping. When exhausted, the node exits with an error. |
| `cluster.message-loss.ttl` | `TABA_CLUSTER_MESSAGE_LOSS_TTL` | **14d** (from the code) | Time after which a record in the `losses` table expires; a node that returns after that can no longer recover the message. |
| `cluster.reunion.interval` | `TABA_CLUSTER_REUNION_INTERVAL` | **10 × the heartbeat (20 s), at least 1 s** (from the code) | How often the node that excluded others by vote queries their table, to find out whether the two halves of a symmetric partition met again; the losing side redoes the join (section 6.7). |
| `cluster.recovery.timeout` | `TABA_CLUSTER_RECOVERY_TIMEOUT` | **5m** (from the code) | Maximum time a returning node tries to query all the nodes of the loss list before clearing the local state and redoing the bootstrap. |
| `cluster.sync.stream.max-items` | `TABA_CLUSTER_SYNC_STREAM_MAX_ITEMS` | **500** (from the code) | Items an NDJSON stream between nodes sends before closing with `hasMore: true`. |
| `cluster.sync.stream.max-bytes` | `TABA_CLUSTER_SYNC_STREAM_MAX_BYTES` | **4MiB** (from the code) | Bytes an NDJSON stream between nodes sends before closing with `hasMore: true`. Counts only the body; the first message of a chunk always goes in. |
| `cluster.sync.stream.max-line-bytes` | `TABA_CLUSTER_SYNC_STREAM_MAX_LINE_BYTES` | — | Limit of a line (a message or topic definition), which defines the buffer of each side. The largest possible message must fit. ⚠️ **Accepted, but with no effect today** (the code reads the value and does not use it). |
| `cluster.sync.stream.idle-timeout` | `TABA_CLUSTER_SYNC_STREAM_IDLE_TIMEOUT` | — | Time without receiving a line for the receiver to consider the stream failed and resume from the last ID read. ⚠️ **Accepted, but with no effect today** (the code reads the value and does not use it). |
| `cluster.sync.range.size` | `TABA_CLUSTER_SYNC_RANGE_SIZE` | **200** (from the code) | Size of each ID range requested from a source in the content synchronization of a block; related to `cluster.sync.stream.max-items`. |
| `cluster.sync.log.max-bytes` | `TABA_CLUSTER_SYNC_LOG_MAX_BYTES` | **1GiB** (from the code) | Maximum size, on disk, of the event database (`sync.db`) of a synchronizing node. Past the ceiling, the node goes into failure and the cluster is free to vote its deregistration. |
| `cluster.sync.timeout` | `TABA_CLUSTER_SYNC_TIMEOUT` | **60s** (from the code) | Time the synchronizing node keeps retrying when no node serves a load; when exhausted, it goes into failure as with a full disk. Applies to each call to a source (network, `5xx`, `409`) and to the definition load. |

## Message delivery

| Key | Variable | Default | What it does |
|---|---|---|---|
| `delivery.visibility-timeout.default` | `TABA_DELIVERY_VISIBILITY_TIMEOUT_DEFAULT` | **120s** | Time the delivered message stays invisible to other consumers when the `GET` does not inform `visibilityTimeout`. |
| `delivery.visibility-timeout.max` | `TABA_DELIVERY_VISIBILITY_TIMEOUT_MAX` | no ceiling | Accepted ceiling of `visibilityTimeout` on the `GET` and on `PUT .../visibility`. |
| `delivery.ready-list.size` | `TABA_DELIVERY_READY_LIST_SIZE` | **1000** | Number of ready IDs the thread keeps, per topic, on each node. ⚠️ **Accepted, but with no effect today** (the code reads the value and does not use it). |
| `delivery.invisible-list.size` | `TABA_DELIVERY_INVISIBLE_LIST_SIZE` | — | Maximum number of invisible messages (ID and deadline) the owner keeps in memory, per topic; the excess goes to the expiry table of the local database. ⚠️ **Accepted, but with no effect today** (the code reads the value and does not use it). |
| `delivery.invisible.reload-threshold` | `TABA_DELIVERY_INVISIBLE_RELOAD_THRESHOLD` | — | When the in-memory invisible list falls below this size, the thread reloads it with the next to expire from the expiry table. ⚠️ **Accepted, but with no effect today** (the code reads the value and does not use it). |
| `storage.db.synchronous` | `TABA_STORAGE_DB_SYNCHRONOUS` | **NORMAL** | SQLite synchronization mode. Durability comes from replication between nodes, so `NORMAL` is enough. |
| `storage.excluded.max-size` | `TABA_STORAGE_EXCLUDED_MAX_SIZE` | **100MB** | Maximum size of the log of excluded messages (all topics added). Past it, the oldest rows go first and the start of the recording moves forward. Greater than zero. |
| `storage.excluded.max-age` | `TABA_STORAGE_EXCLUDED_MAX_AGE` | **24h** | Maximum age of a row of the log of excluded messages. Greater than zero. |
| `delivery.max-messages.default` | `TABA_DELIVERY_MAX_MESSAGES_DEFAULT` | **10** | Number of messages of the `GET` when the client does not inform `maxMessages`. |
| `delivery.max-messages.max` | `TABA_DELIVERY_MAX_MESSAGES_MAX` | no ceiling | Largest accepted `maxMessages`. Does not apply to the synchronization streams between nodes (`cluster.sync.stream.*`). |
| `delivery.get.max-attempts` | `TABA_DELIVERY_GET_MAX_ATTEMPTS` | **3** (from the code) | How many IDs from the list the `GET` tries when the invisibility register fails; when exhausted, answers `503`. |
| `delivery.fair.ready-weight` | `TABA_DELIVERY_FAIR_READY_WEIGHT` | **0.1** | How much each ready message weighs in the group size, against one in processing (`size = in processing + weight × ready`). From `0` to `1`: `0` is equivalent to SQS (only what is in processing), `1` counts the whole backlog. |
| `delivery.fair.max-groups` | `TABA_DELIVERY_FAIR_MAX_GROUPS` | no ceiling | Maximum number of message groups each owner tracks, per topic. New groups above it share a single counter (section 12.11). |
| `delivery.fair.normal-every` | `TABA_DELIVERY_FAIR_NORMAL_EVERY` | **10** | Every so many deliveries, one takes the oldest message of the list, from any group, so large groups do not wait without limit. `0` turns the guarantee off. |
| `delivery.take.max-attempts` | `TABA_DELIVERY_TAKE_MAX_ATTEMPTS` | **3** (from the code) | Ceiling of peers queried, one at a time, when the local ready list is empty (each chosen at random among those the table marks as having ready messages). When exhausted, the `GET` becomes a local listener. |

## Retention and dead-letter

| Key | Variable | Default | What it does |
|---|---|---|---|
| `retention.ttl.default` | `TABA_RETENTION_TTL_DEFAULT` | **14d** | `ttl` used by queues that do not define their own. Applies at the time of the check: changing the default affects queues without their own `ttl`. A message older than the TTL is discarded. |
| `retention.max-deliveries.default` | `TABA_RETENTION_MAX_DELIVERIES_DEFAULT` | **10** | `maxDeliveries` used by queues that do not define their own. Reached without an ack, the message goes to the dead-letter. |
| `topics.dead-letter.suffix` | `TABA_TOPICS_DEAD_LETTER_SUFFIX` | **-dead-letter** | Suffix of the dead-letter queue name of each queue. Reserved: a topic with a name ending in it cannot be created (`400`). Changing the suffix with existing queues requires migration; it is worth setting before creating topics. |
| `retention.sweep.interval` | `TABA_RETENTION_SWEEP_INTERVAL` | **60s** | How often each node sweeps its own messages and deletes those expired by `ttl`. The owner no longer delivers an expired message between one sweep and the next. |

## FIFO queues

| Key | Variable | Default | What it does |
|---|---|---|---|
| `fifo.dedup.window` | `TABA_FIFO_DEDUP_WINDOW` | **300s** | Time, from the ID's instant, in which a publication with the same `deduplicationId` (or the same body hash) in the same group is discarded as a duplicate. It is also the `deleteAt` deadline of the confirmed message. |
| `fifo.dedup.index.max-entries` | `TABA_FIFO_DEDUP_INDEX_MAX_ENTRIES` | — | Maximum number of window keys kept in memory, per topic; the excess is queried in the topic database. ⚠️ **Accepted, but with no effect today** (the code reads the value and does not use it). |
| `fifo.forward.max-attempts` | `TABA_FIFO_FORWARD_MAX_ATTEMPTS` | **3** (from the code) | How many times the node that received the publication forwards it again, with the member table updated, when the primary answers `421`. When exhausted, `503` with `Retry-After`. |

## Volatile topics and distributors

| Key | Variable | Default | What it does |
|---|---|---|---|
| `topics.volatile.idle-timeout.default` | `TABA_TOPICS_VOLATILE_IDLE_TIMEOUT_DEFAULT` | **600s** | `idleTimeout` used when the queue is created (by `POST /topics` with `volatile` or by `autoCreate`) without informing the value. |
| `topics.volatile.idle-check.retry-interval` | `TABA_TOPICS_VOLATILE_IDLE_CHECK_RETRY_INTERVAL` | **30s** (from the code) | Interval for the managing node to repeat the final check when some node did not answer. |
| `topics.distributor.max-targets` | `TABA_TOPICS_DISTRIBUTOR_MAX_TARGETS` | no ceiling | Maximum number of queues a publication to a Distributor Topic may match; above it the publication is refused entirely (`422`). |
| `topics.volatile.max-count` | `TABA_TOPICS_VOLATILE_MAX_COUNT` | no ceiling | Maximum number of volatile queues in the cluster; once reached, `autoCreate` is refused (`422`). |
| `topics.volatile.takeover.jitter` | `TABA_TOPICS_VOLATILE_TAKEOVER_JITTER` | **2s** (from the code) | Window in which the node that takes over volatile queues (node join or leave) spreads its first idleness queries. |

## Observability

| Key | Variable | Default | What it does |
|---|---|---|---|
| `telemetry.prometheus.enabled` | `TABA_TELEMETRY_PROMETHEUS_ENABLED` | **true** | Turns `GET /metrics` on/off (Prometheus format, only this node's metrics). |
| `operations.port` | `TABA_OPERATIONS_PORT` | the client port | Own port for `/metrics`, `/metrics/topics/*`, `/health/*` and `/node/drain`. Optional; if defined, these routes cease to exist on the client API port. Must be different from `api.port` and from `cluster.port`. |
| `operations.allowed-ips` | `TABA_OPERATIONS_ALLOWED_IPS` | no restriction | List of IPs/CIDR ranges with access to `/metrics`, `/metrics/topics/*`, `/health/*` and `/node/drain`; the others receive `403`. Optional. Without a port or a list, these routes are treated like any other. |
| `telemetry.topics.mode` | `TABA_TELEMETRY_TOPICS_MODE` | **off** | Applies to OTLP and `/metrics`: `off` (only `/metrics/topics` routes), `by-tags` (aggregated by tags) or `per-topic` (one series per topic, with a ceiling). |
| `telemetry.topics.group-by-tags` | `TABA_TELEMETRY_TOPICS_GROUP_BY_TAGS` | empty | Tag keys used to aggregate in `by-tags` mode. |
| `telemetry.topics.max-topics` | `TABA_TELEMETRY_TOPICS_MAX_TOPICS` | **1000** (from the code) | Maximum number of per-topic series in `per-topic` mode; the excess is aggregated in `messaging.destination.name="_other"`. |
| `telemetry.logs.level` | `TABA_TELEMETRY_LOGS_LEVEL` | **info** (from the code) | Minimum level of the structured logs: `trace`, `debug`, `info`, `warn` or `error`. |
| `telemetry.logs.format` | `TABA_TELEMETRY_LOGS_FORMAT` | **text** | Format of the logs on `stdout`: `text` (plain text, no colors), `color` (text with ANSI colors, good for the terminal and `docker compose logs`) or `json` (one JSON line per event, for log collectors). Does not change what OTLP exports. `docker-compose.yml` uses `color`. |
| `stats.topics.top-groups` | `TABA_STATS_TOPICS_TOP_GROUPS` | **10** | How many groups (`topGroups`) `GET /metrics/topics/{topic}` returns. |
| `stats.topics.cache-ttl` | `TABA_STATS_TOPICS_CACHE_TTL` | — | Validity of the aggregated result of `/metrics/topics` (avoids repeating the query to all nodes). With a single node there is no query to repeat: the parameter has no effect yet. ⚠️ **Accepted, but with no effect today** (the code reads the value and does not use it). |
| `stats.topics.backlog.depth`<br>`stats.topics.backlog.age` | `TABA_STATS_TOPICS_BACKLOG_DEPTH`<br>`TABA_STATS_TOPICS_BACKLOG_AGE` | condition off<br>condition off | `depth` and `oldestAge` above which the topic enters the `backlog` condition. Without a value, the condition does not fire. |
| `stats.topics.noisy-group-share` | `TABA_STATS_TOPICS_NOISY_GROUP_SHARE` | **50** | Share of the topic's `depth` above which the largest group fires the `noisy-group` condition. |
| `stats.topics.group-stalled-deliveries` | `TABA_STATS_TOPICS_GROUP_STALLED_DELIVERIES` | condition off | Number of deliveries of a FIFO queue group head, without an ack, above which the `group-stalled` condition fires. Without a value, the condition does not fire. |
| `stats.topics.stalled-after` | `TABA_STATS_TOPICS_STALLED_AFTER` | condition off | Time without consumption, with accumulated messages, for the `stalled` condition. Without a value, the condition does not fire. |
| `stats.topics.idle-after` | `TABA_STATS_TOPICS_IDLE_AFTER` | condition off | Time without publishing or consumption for the `idle` condition. Without a value, the condition does not fire. |
| `stats.disk.free-min` | `TABA_STATS_DISK_FREE_MIN` | condition off | Below this the cluster enters the `storage-pressure` condition. Without a value, the condition does not fire. |

## Rate limits

| Key | Variable | Default | What it does |
|---|---|---|---|
| `ratelimit.publish.rps` | `TABA_RATELIMIT_PUBLISH_RPS` | no limit | Requests per second, **per token** (or per client IP, without a valid token), for `POST /topics/{topic}/messages`. |
| `ratelimit.consume.rps` | `TABA_RATELIMIT_CONSUME_RPS` | no limit | Requests per second, per token (or per IP, without a valid token), for `GET /topics/{topic}/messages`, `PUT /topics/{topic}/messages/{id}/visibility` (invisibility) and `DELETE /topics/{topic}/messages/{id}` (ack). |
| `ratelimit.admin.rps` | `TABA_RATELIMIT_ADMIN_RPS` | no limit | Requests per second, per token (or per IP, without a valid token), for `POST/GET/PATCH/DELETE /topics` and `/topics/{topic}`, and `/tokens`; and, per IP, for `/node/drain`, `/node/backup/flush`, `/cluster/backup/flush` and `/cluster/stop`. |
| `ratelimit.ops.rps` | `TABA_RATELIMIT_OPS_RPS` | no limit | Requests per second, per IP, for `/health/cluster?scope=cluster` and `/metrics/topics`. `/health/live`, `/health/ready`, `/metrics` and `/health/cluster?scope=node` are exempt. |
| `ratelimit.burst` | `TABA_RATELIMIT_BURST` | `ceil(rps)` of each category | Size of the overshoot allowed above the rate (e.g. token bucket), to absorb short peaks without rejecting requests. Applies to each counter, token or IP. |
| `client-ip.headers.enabled` | `TABA_CLIENT_IP_HEADERS_ENABLED` | **true** | Uses the headers below to find the client IP behind a load balancer; `false` uses only the connection address. Only matters for calls counted per IP (without a valid token) and for `operations.allowed-ips`. |
| `client-ip.headers` | `TABA_CLIENT_IP_HEADERS` | **X-Forwarded-For** | List, in order, of headers consulted; the first present applies and, in it, the first address. |
| `client-ip.trusted-proxies` | `TABA_CLIENT_IP_TRUSTED_PROXIES` | empty (header accepted from any origin) | List of IPs/CIDR of the load balancers or proxies whose IP headers are accepted. Empty: accepts from any origin (the service must stay in a secure layer). Applies to the rate limit and to `operations.allowed-ips`. |


## Optional S3 backup

The feature turns on with `backup.s3.bucket`; without it none of this exists. How it works: [33-s3-backup.md](33-s3-backup.md).

| Key | Variable | Default | What it does |
|---|---|---|---|
| `backup.s3.bucket` | `TABA_BACKUP_S3_BUCKET` | no value (**feature off**) | The bucket. When defined, turns the backup on. |
| `backup.s3.region` | `TABA_BACKUP_S3_REGION` | no value | The region. Without it the SDK uses the default AWS chain (`AWS_REGION`, profile); with `backup.s3.endpoint` and no region, `us-east-1` applies. |
| `backup.s3.endpoint` | `TABA_BACKUP_S3_ENDPOINT` | no value (AWS) | The address of a compatible S3 (SeaweedFS, MinIO, Ceph), such as `http://seaweedfs:8333`. |
| `backup.s3.prefix` | `TABA_BACKUP_S3_PREFIX` | **taba** | The base prefix of all objects in the bucket. Several clusters can share a bucket with different prefixes. |
| `backup.s3.access-key` | `TABA_BACKUP_S3_ACCESS_KEY` | no value | The access key. **Optional:** without it (and without the secret one), the default AWS chain applies (IAM, profile, task role). Always goes with the secret one. |
| `backup.s3.secret-key` | `TABA_BACKUP_S3_SECRET_KEY` | no value | The secret key; never appears in a log or in `Debug`. |
| `backup.s3.path-style` | `TABA_BACKUP_S3_PATH_STYLE` | (from the code) **true** with `endpoint`, **false** without | The bucket in the path (`http://host/bucket/key`) instead of in the host name. MinIO requires it. |
| `backup.delay` | `TABA_BACKUP_DELAY` | **60s** | The message only goes to S3 after this time in the outbox: one confirmed and consumed before **never goes**. |
| `backup.meta.interval` | `TABA_BACKUP_META_INTERVAL` | **60s** | How often the mutable state (delivery, invisibility, ack) becomes a journal. It is the lag (RPO) of the state. |
| `backup.segment.max-bytes` | `TABA_BACKUP_SEGMENT_MAX_BYTES` | **8MB** | The segment closes on reaching this size (`MB` is 2²⁰ bytes, as in `message.max-bytes`). |
| `backup.segment.max-messages` | `TABA_BACKUP_SEGMENT_MAX_MESSAGES` | **5000** | The segment closes on reaching this number of messages. |
| `backup.segment.max-age` | `TABA_BACKUP_SEGMENT_MAX_AGE` | **60s** | The segment closes on reaching this age (from the moment the first message entered it). |
| `backup.heartbeat.interval` | `TABA_BACKUP_HEARTBEAT_INTERVAL` | **15s** | How often the node rewrites `nodes/<id>.json` and observes those of the others. |
| `backup.dead-after` | `TABA_BACKUP_DEAD_AFTER` | **60s** | A node is considered dead by whoever observes when its heartbeat does not change for this time. Must be **greater** than `backup.heartbeat.interval` (with room: several heartbeats). It is also the delay of the exclusion of an unreachable node. |
| `backup.drain.timeout` | `TABA_BACKUP_DRAIN_TIMEOUT` | **30s** | How long the drain, the flush and the pause for a full disk wait for the upload to S3. When exceeded, the node proceeds without S3. The orchestrator's stop deadline must be **greater** than `drain.timeout` + this value. |
| `backup.disk.evict-above` | `TABA_BACKUP_DISK_EVICT_ABOVE` | **85** | Disk use (%) above which the node deletes the body of messages already saved to S3. |
| `backup.disk.evict-to` | `TABA_BACKUP_DISK_EVICT_TO` | **70** | Eviction stops when use falls to this value (%). |
| `backup.disk.pause-at` | `TABA_BACKUP_DISK_PAUSE_AT` | **90** | During flush and drain, the move from memory to disk pauses at this use (%) until eviction frees space. |
| `backup.reconcile.interval` | `TABA_BACKUP_RECONCILE_INTERVAL` | **60s** | How often the manager rewrites the definitions that diverge from the bucket, deletes segments expired by `ttl` and the node cleans dead heartbeats. |
| `backup.restore.lock-ttl` | `TABA_BACKUP_RESTORE_LOCK_TTL` | **60s** | Validity of the restore lock: the holder renews it; stopped for this long, another node takes it. |

## Fixed limits (not configuration)

The ranges of `ttl`, `maxDeliveries`, `idleTimeout`, names, tags, prefixes and the `waitTime` of 0 to 20 s are **fixed** in the code: see [19-errors-and-limits.md](19-errors-and-limits.md).

## Sources and evidence

- `config/params/mod.rs` (all the keys and the defaults of the table), `config/mod.rs` (the typed `Config` and the validations), `config/source/mod.rs` (precedence and value conversion), `config/otlp.rs` (`OTEL_*`), `cluster/service/mod.rs` and `cluster/sync/mod.rs` (the defaults "from the code"), `http/ratelimit/mod.rs` (the default burst).
- Use cases: UC-CFG (configuration and startup validations); see [26a](26a-implemented-use-cases.md). Tests: `config/tests.rs`, `config/params/tests.rs`, `config/source/tests.rs`, `tests/startup.rs`.
- History: `10-configuracao.md`.
