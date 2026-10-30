//! The node metrics that exist (doc 11, "Node metrics"), with the name, type and unit from the
//! document's table. What does not exist yet (channel, voting, synchronization) is not here.

use super::{Def, Kind};

/// The buckets suggested by the HTTP semantic convention (5 ms to 10 s).
const HTTP_SECONDS: &[f64] = &[
    0.005, 0.01, 0.025, 0.05, 0.075, 0.1, 0.25, 0.5, 0.75, 1.0, 2.5, 5.0, 7.5, 10.0,
];
const BODY_BYTES: &[f64] = &[
    100.0,
    1_000.0,
    10_000.0,
    100_000.0,
    1_000_000.0,
    10_000_000.0,
];
const WAIT_SECONDS: &[f64] = &[
    0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0, 5.0, 10.0, 60.0, 300.0,
];
const BATCH_ITEMS: &[f64] = &[
    1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0, 250.0, 500.0, 1_000.0,
];
const BATCH_WAIT_SECONDS: &[f64] = &[
    0.000_1, 0.000_5, 0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0, 5.0,
];
const NO_BUCKETS: &[f64] = &[];

const fn def(name: &'static str, kind: Kind, unit: &'static str, help: &'static str) -> Def {
    Def {
        name,
        kind,
        unit,
        help,
        buckets: NO_BUCKETS,
    }
}

const fn histogram(
    name: &'static str,
    unit: &'static str,
    help: &'static str,
    buckets: &'static [f64],
) -> Def {
    Def {
        name,
        kind: Kind::Histogram,
        unit,
        help,
        buckets,
    }
}

// ---- Processo e host ----

pub static PROCESS_UPTIME: Def = def(
    "process.uptime",
    Kind::Gauge,
    "s",
    "Time since the process started.",
);
pub static PROCESS_CPU_UTILIZATION: Def = def(
    "process.cpu.utilization",
    Kind::Gauge,
    "1",
    "Process CPU usage, from 0 to 1 of the total cores, since the previous reading.",
);
pub static PROCESS_MEMORY_USAGE: Def = def(
    "process.memory.usage",
    Kind::UpDownCounter,
    "By",
    "Resident memory of the process.",
);
pub static PROCESS_THREAD_COUNT: Def = def(
    "process.thread.count",
    Kind::UpDownCounter,
    "{thread}",
    "Threads do sistema operacional do processo.",
);
pub static PROCESS_OPEN_FILES: Def = def(
    "process.open_file_descriptor.count",
    Kind::UpDownCounter,
    "{file}",
    "Open file descriptors.",
);
pub static NODE_CONNECTIONS: Def = def(
    "taba.node.connections",
    Kind::UpDownCounter,
    "{connection}",
    "Open HTTP connections, by port.",
);

// ---- Armazenamento ----

pub static STORAGE_MEMORY_USAGE: Def = def(
    "taba.storage.memory.usage",
    Kind::UpDownCounter,
    "By",
    "Memory taken by message bodies.",
);
pub static STORAGE_MEMORY_LIMIT: Def = def(
    "taba.storage.memory.limit",
    Kind::Gauge,
    "By",
    "Message memory limit (memory.limit).",
);
pub static STORAGE_MEMORY_MESSAGES: Def = def(
    "taba.storage.memory.messages",
    Kind::UpDownCounter,
    "{message}",
    "Messages kept in memory.",
);
pub static STORAGE_DISK_MESSAGES: Def = def(
    "taba.storage.disk.messages",
    Kind::UpDownCounter,
    "{message}",
    "Messages on disk.",
);
pub static STORAGE_DISK_FREE: Def = def(
    "taba.storage.disk.free",
    Kind::Gauge,
    "By",
    "Free space on the storage.dir volume.",
);
pub static STORAGE_OVERFLOW: Def = def(
    "taba.storage.overflow",
    Kind::Counter,
    "{message}",
    "Messages that went from memory to disk for lack of space.",
);
pub static STORAGE_READS: Def = def(
    "taba.storage.reads",
    Kind::Counter,
    "{read}",
    "Message reads, by where they were served from (memory or disk).",
);
pub static STORAGE_DISK_ERRORS: Def = def(
    "taba.storage.disk.errors",
    Kind::Counter,
    "{error}",
    "Disk input/output failures.",
);

// ---- API HTTP ----

pub static HTTP_SERVER_DURATION: Def = histogram(
    "http.server.request.duration",
    "s",
    "Latency of the requests received; the count is the total number of requests.",
    HTTP_SECONDS,
);
pub static HTTP_SERVER_ACTIVE: Def = def(
    "http.server.active_requests",
    Kind::UpDownCounter,
    "{request}",
    "Requests in progress.",
);
pub static HTTP_SERVER_REQUEST_BODY: Def = histogram(
    "http.server.request.body.size",
    "By",
    "Size of the request body (when the size is known).",
    BODY_BYTES,
);
pub static HTTP_SERVER_RESPONSE_BODY: Def = histogram(
    "http.server.response.body.size",
    "By",
    "Size of the response body (when the size is known).",
    BODY_BYTES,
);
pub static RATELIMIT_REJECTED: Def = def(
    "taba.ratelimit.rejected",
    Kind::Counter,
    "{request}",
    "Requests refused with 429.",
);
pub static AUTH_REQUESTS: Def = def(
    "taba.auth.requests",
    Kind::Counter,
    "{request}",
    "Token authentication: accepted, refused with 401 and refused with 403.",
);
pub static AUTH_TOKENS: Def = def(
    "taba.auth.tokens",
    Kind::Gauge,
    "{token}",
    "Access tokens this node knows.",
);

// ---- Entrega ----

pub static DELIVERY_READY: Def = def(
    "taba.delivery.ready",
    Kind::UpDownCounter,
    "{message}",
    "Messages ready for delivery on this node (all topics).",
);
pub static DELIVERY_FAIR_WAIT: Def = histogram(
    "taba.delivery.fair.wait",
    "s",
    "Time between publication and first delivery, by group band.",
    WAIT_SECONDS,
);
pub static DELIVERY_REDELIVERIES: Def = def(
    "taba.delivery.redeliveries",
    Kind::Counter,
    "{message}",
    "Deliveries of messages already delivered before.",
);
pub static DELIVERY_DEAD_LETTERED: Def = def(
    "taba.delivery.dead_lettered",
    Kind::Counter,
    "{message}",
    "Messages sent to the dead-letter for reaching maxDeliveries.",
);
pub static RETENTION_EXPIRED: Def = def(
    "taba.retention.expired",
    Kind::Counter,
    "{message}",
    "Messages dropped by ttl.",
);
pub static FIFO_DEDUPLICATED: Def = def(
    "taba.fifo.deduplicated",
    Kind::Counter,
    "{message}",
    "Publications to a FIFO queue dropped by deduplication.",
);
pub static FIFO_GROUPS_BUSY: Def = def(
    "taba.fifo.groups.busy",
    Kind::UpDownCounter,
    "{group}",
    "FIFO queue groups with a message in flight on this node.",
);
pub static LONGPOLL_WAITING: Def = def(
    "taba.delivery.longpoll.waiting",
    Kind::UpDownCounter,
    "{request}",
    "Long-polling calls waiting on this node.",
);
pub static LONGPOLL_OUTCOME: Def = def(
    "taba.delivery.longpoll.outcome",
    Kind::Counter,
    "{request}",
    "How the long-polling calls ended.",
);

// ---- Node state ----

pub static NODE_STATE: Def = def(
    "taba.node.state",
    Kind::Gauge,
    "{flag}",
    "1 in the node's current state.",
);
pub static NODE_DRAIN_STATE: Def = def(
    "taba.node.drain.state",
    Kind::Gauge,
    "{flag}",
    "1 in the node's drain state.",
);
pub static NODE_DRAIN_PENDING: Def = def(
    "taba.node.drain.pending",
    Kind::UpDownCounter,
    "{message}",
    "Messages still in memory (reaches 0 when the node is drained).",
);

// ---- Cluster and ring (only in mode=production) ----

pub static CLUSTER_MEMBERS: Def = def(
    "taba.cluster.members",
    Kind::Gauge,
    "{node}",
    "Members in this node's table, by state.",
);
pub static CLUSTER_MEMBERS_VERSION: Def = def(
    "taba.cluster.members.version",
    Kind::Gauge,
    "{version}",
    "Version (epoch) of the member table; divergence between nodes indicates incomplete propagation.",
);
pub static CLUSTER_MEMBERSHIP_CHANGES: Def = def(
    "taba.cluster.membership.changes",
    Kind::Counter,
    "{change}",
    "Node registrations and deregistrations, by phase.",
);
pub static CLUSTER_BOOTSTRAP_FAILURES: Def = def(
    "taba.cluster.bootstrap.failures",
    Kind::Counter,
    "{failure}",
    "Bootstrap failures of this node, by reason.",
);
pub static RING_VIRTUAL_NODES: Def = def(
    "taba.ring.virtual_nodes",
    Kind::Gauge,
    "{vnode}",
    "This node's virtual nodes on the ring (ring.virtual-nodes).",
);
pub static RING_BLOCKS: Def = def(
    "taba.ring.blocks",
    Kind::UpDownCounter,
    "{block}",
    "Blocks the node serves, by role and integrity.",
);

// ---- Writing, delivery and forwarding between nodes (only in mode=production) ----

pub static WRITE_COORDINATED: Def = def(
    "taba.write.coordinated",
    Kind::Counter,
    "{message}",
    "Message operations coordinated by this node (publish, invisibility, ack, dead-letter).",
);
pub static WRITE_RETRIES: Def = def(
    "taba.write.retries",
    Kind::Counter,
    "{retry}",
    "Retries by phase and by destination node.",
);
pub static WRITE_ROLLBACKS: Def = def(
    "taba.write.rollbacks",
    Kind::Counter,
    "{rollback}",
    "Rollbacks sent for failing to close the quorum (phase 1).",
);
pub static WRITE_COMMIT_TIMEOUTS: Def = def(
    "taba.write.commit_timeouts",
    Kind::Counter,
    "{timeout}",
    "Times write.commit.timeout ran out without a confirmed commit on a replica.",
);
pub static CLUSTER_BATCH_ITEMS: Def = histogram(
    "taba.cluster.batch.items",
    "{item}",
    "Operations in each batch sent to another node.",
    BATCH_ITEMS,
);
pub static CLUSTER_BATCH_BYTES: Def = histogram(
    "taba.cluster.batch.bytes",
    "By",
    "Size of the body of each batch sent to another node (the messages' bodies, not the envelope).",
    BODY_BYTES,
);
pub static CLUSTER_BATCH_WAIT: Def = histogram(
    "taba.cluster.batch.wait",
    "s",
    "How long an operation waited in the queue of a node before its batch left.",
    BATCH_WAIT_SECONDS,
);
pub static WRITE_RECEIVED: Def = def(
    "taba.write.received",
    Kind::Counter,
    "{message}",
    "Registros, commits e rollbacks recebidos.",
);
pub static WRITE_PENDING: Def = def(
    "taba.write.pending",
    Kind::UpDownCounter,
    "{message}",
    "Pending records (no commit).",
);
pub static WRITE_PENDING_EXPIRED: Def = def(
    "taba.write.pending.expired",
    Kind::Counter,
    "{message}",
    "Records dropped by expiry (neither commit nor rollback).",
);
pub static DELIVERY_TAKE: Def = def(
    "taba.delivery.take",
    Kind::Counter,
    "{request}",
    "Requests for delivery to another owner (take), sent and received.",
);
pub static CLUSTER_NOTIFICATIONS: Def = def(
    "taba.cluster.channel.notifications",
    Kind::Counter,
    "{message}",
    "New-message notices sent and received.",
);
pub static CLUSTER_FORWARDED: Def = def(
    "taba.cluster.forwarded",
    Kind::Counter,
    "{request}",
    "Client calls forwarded to another node (drain), by result.",
);

// ---- Channel between nodes, voting and freezing (doc 04, sections 6.3 and 6.7; doc 13, section 12.9) ----

pub static CHANNEL_CONNECTIONS: Def = def(
    "taba.cluster.channel.connections",
    Kind::UpDownCounter,
    "{connection}",
    "Channels open with other nodes (expected: nodes in the cluster − 1).",
);
pub static CHANNEL_STATE: Def = def(
    "taba.cluster.channel.state",
    Kind::Gauge,
    "{flag}",
    "1 if the channel to the peer is connected, 0 if down.",
);
pub static CHANNEL_RTT: Def = histogram(
    "taba.cluster.channel.rtt",
    "s",
    "Latency of the channel call (heartbeat and frames), by peer.",
    WAIT_SECONDS,
);
pub static CHANNEL_RECONNECTS: Def = def(
    "taba.cluster.channel.reconnects",
    Kind::Counter,
    "{attempt}",
    "Channel reconnection attempts and result.",
);
pub static CHANNEL_QUEUE_OUT: Def = def(
    "taba.cluster.channel.queue.out",
    Kind::UpDownCounter,
    "{signal}",
    "Frames in the outgoing queue to the peer.",
);
pub static CHANNEL_QUEUE_IN: Def = def(
    "taba.cluster.channel.queue.in",
    Kind::UpDownCounter,
    "{batch}",
    "Peer messages read from the channel and not yet processed (incoming queue).",
);
pub static CHANNEL_QUEUE_DROPPED: Def = def(
    "taba.cluster.channel.queue.dropped",
    Kind::Counter,
    "{signal}",
    "Frames replaced, dropped by age or by outgoing queue overflow.",
);
pub static DEFINITION_CONFLICTS: Def = def(
    "taba.cluster.definitions.conflicts",
    Kind::Counter,
    "{conflict}",
    "Topic or token definitions received with the same version and different content from what the node has.",
);
pub static CLUSTER_VOTE: Def = def(
    "taba.cluster.vote",
    Kind::Counter,
    "{vote}",
    "Exclusion votes opened or voted and the result.",
);
pub static CLUSTER_FREEZING: Def = def(
    "taba.cluster.freezing",
    Kind::Gauge,
    "{node}",
    "Nodes currently in a freezing period.",
);
pub static FIFO_FORWARD: Def = def(
    "taba.fifo.forward",
    Kind::Counter,
    "{request}",
    "FIFO publications forwarded to the group's primary.",
);
pub static CLUSTER_TLS_EXPIRES_IN: Def = def(
    "taba.cluster.tls.expires_in",
    Kind::Gauge,
    "s",
    "Time until the in-use cluster certificate expires (notAfter).",
);
pub static CLUSTER_QUORUM_AVAILABLE: Def = def(
    "taba.cluster.quorum.available",
    Kind::Gauge,
    "{flag}",
    "1 if there are enough healthy nodes to vote (cluster.vote.min-voters).",
);
pub static CLUSTER_CONDITION: Def = def(
    "taba.cluster.condition",
    Kind::Gauge,
    "{flag}",
    "1 for each condition currently in force of the cluster.",
);

// ---- Block synchronization (doc 05, section 7.3) ----

pub static SYNC_BLOCKS_ACTIVE: Def = def(
    "taba.sync.blocks.active",
    Kind::UpDownCounter,
    "{block}",
    "Blocks synchronizing right now (delivery suspended on those blocks).",
);
pub static SYNC_DURATION: Def = histogram(
    "taba.sync.duration",
    "s",
    "Time to make a block intact.",
    WAIT_SECONDS,
);
pub static SYNC_IDS_LISTED: Def = def(
    "taba.sync.ids.listed",
    Kind::Counter,
    "{id}",
    "IDs consolidated (union) in the source lists.",
);
pub static SYNC_MESSAGES_PULLED: Def = def(
    "taba.sync.messages.pulled",
    Kind::Counter,
    "{message}",
    "Messages obtained per source node.",
);
pub static SYNC_BYTES: Def = def(
    "taba.sync.bytes",
    Kind::Counter,
    "By",
    "Body volume transferred in the synchronization.",
);
pub static SYNC_LOG_EVENTS: Def = def(
    "taba.sync.log.events",
    Kind::UpDownCounter,
    "{event}",
    "Events in the event database (sync.db) still to process.",
);
pub static SYNC_LOG_BYTES: Def = def(
    "taba.sync.log.bytes",
    Kind::Gauge,
    "By",
    "Size of the event database on disk.",
);
pub static SYNC_PHASE: Def = def(
    "taba.sync.phase",
    Kind::Gauge,
    "{flag}",
    "1 in the node's current synchronization phase.",
);
pub static SYNC_FAILURES: Def = def(
    "taba.sync.failures",
    Kind::Counter,
    "{failure}",
    "Synchronization failures.",
);

// ---- Loss recovery (doc 04, section 6.6) ----

pub static RECOVER_MESSAGES: Def = def(
    "taba.recover.messages",
    Kind::Counter,
    "{message}",
    "Messages sent, applied or ignored by the recovery of a returning node.",
);
pub static CLUSTER_RECOVER: Def = def(
    "taba.cluster.recover",
    Kind::Counter,
    "{message}",
    "Messages recovered when a node returns, by direction.",
);

// ---- Topic metrics (only in the by-tags and per-topic modes) ----

pub static TOPIC_PUBLISHED: Def = def(
    "taba.topic.published",
    Kind::Counter,
    "{message}",
    "Messages published to the topic.",
);
pub static TOPIC_CONSUMED: Def = def(
    "taba.topic.consumed",
    Kind::Counter,
    "{message}",
    "Messages delivered on GET.",
);
pub static TOPIC_ACKED: Def = def(
    "taba.topic.acked",
    Kind::Counter,
    "{message}",
    "Acknowledgements (ack) of the topic.",
);
pub static TOPIC_VISIBLE: Def = def(
    "taba.topic.visible",
    Kind::UpDownCounter,
    "{message}",
    "Messages ready for delivery.",
);
pub static TOPIC_INVISIBLE: Def = def(
    "taba.topic.invisible",
    Kind::UpDownCounter,
    "{message}",
    "Messages being processed.",
);
pub static TOPIC_DEPTH: Def = def(
    "taba.topic.depth",
    Kind::UpDownCounter,
    "{message}",
    "Messages not yet acknowledged.",
);
pub static TOPIC_OLDEST_TIMESTAMP: Def = def(
    "taba.topic.oldest_timestamp",
    Kind::Gauge,
    "s",
    "Unix seconds of the oldest unacknowledged message.",
);
pub static TOPIC_MEMORY_BYTES: Def = def(
    "taba.topic.memory_bytes",
    Kind::UpDownCounter,
    "By",
    "Bytes of the topic in memory.",
);
pub static TOPIC_DISK_BYTES: Def = def(
    "taba.topic.disk_bytes",
    Kind::UpDownCounter,
    "By",
    "Bytes of the topic on disk.",
);
pub static TOPIC_CONDITION: Def = def(
    "taba.topic.condition",
    Kind::Gauge,
    "{flag}",
    "1 for each condition currently in force of the topic.",
);

/// The topic metrics, to delete them together.
pub static TOPIC_DEFS: [&Def; 10] = [
    &TOPIC_PUBLISHED,
    &TOPIC_CONSUMED,
    &TOPIC_ACKED,
    &TOPIC_VISIBLE,
    &TOPIC_INVISIBLE,
    &TOPIC_DEPTH,
    &TOPIC_OLDEST_TIMESTAMP,
    &TOPIC_MEMORY_BYTES,
    &TOPIC_DISK_BYTES,
    &TOPIC_CONDITION,
];
