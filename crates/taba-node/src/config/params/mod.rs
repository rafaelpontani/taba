//! The parameter table: key, environment variable and default (doc 10).
//!
//! It is the single source of accepted keys. A key that is not here is refused in the
//! `.yml` file (a typo does not pass silently), and `Config` only reads
//! keys from here. A `None` default means "no value": either it is optional, or the value is
//! derived from another parameter, or it is still **To be defined** in `10-configuracao.md`.

pub struct Param {
    pub key: &'static str,
    pub default: Option<&'static str>,
}

const fn p(key: &'static str, default: Option<&'static str>) -> Param {
    Param { key, default }
}

/// Keys that no longer exist (the development mode went away, doc 07, section 8): a file or an environment that
/// still sets them keeps starting, with a warning, so an upgrade does not break a working configuration.
pub const REMOVED: &[&str] = &["mode", "development.resume-on-start"];

/// The environment variable of a key: `TABA_` prefix, uppercase, `.` and `-` become `_`.
pub fn env_name(key: &str) -> String {
    format!("TABA_{}", key.to_ascii_uppercase().replace(['.', '-'], "_"))
}

pub fn find(key: &str) -> Option<&'static Param> {
    PARAMS.iter().find(|p| p.key == key)
}

pub static PARAMS: &[Param] = &[
    // Ports (the doc's suggestions apply as defaults: 8080 and 8081)
    p("api.port", Some("8080")),
    p("api.pagination.default-limit", Some("100")),
    p("api.pagination.max-limit", Some("1000")),
    p("cluster.port", Some("8081")),
    p("operations.port", None),
    // Cluster port security
    p("secrets.dir", Some("/etc/taba/secrets")),
    p("cluster.tls.enabled", Some("true")), // `false` só com replication.factor=1
    p("cluster.tls.dir", None),             // derivado: <secrets.dir>/tls
    p("cluster.tls.reload-interval", Some("60s")),
    p("cluster.tls.expiry-warning", Some("30d")),
    // Client API authentication
    p("auth.enabled", Some("auto")),
    p("auth.root-token", None),
    p("auth.root-token-file", None), // derivado: <secrets.dir>/root-token
    p("auth.dev-token", None),       // só com replication.factor=1
    p("health.single-node-condition", Some("true")),
    p("auth.reload-interval", Some("60s")),
    // Stop and drain
    p("drain.on-stop", Some("true")),
    p("drain.timeout", Some("25s")),
    // Storage and memory
    p("memory.limit", None),
    p("memory.rate.window", Some("5m")),
    p("memory.topic.reserve", Some("1000")),
    p("storage.dir", Some("/var/lib/taba")),
    p("storage.db.synchronous", Some("NORMAL")),
    p("storage.excluded.max-size", Some("100MB")),
    p("storage.excluded.max-age", Some("24h")),
    // Messages
    p("message.max-bytes", Some("5MB")),
    p("message.hard-limit", None),
    // Writing and durability
    p("replication.factor", Some("3")),
    p("write.quorum", None), // derivado: floor(replication.factor / 2) + 1
    p("write.retry.attempts", Some("3")),
    p("write.retry.backoff", Some("200ms")),
    p("write.pending.ttl", Some("30s")),
    p("write.commit.timeout", Some("10s")),
    // Cluster
    p("ring.virtual-nodes", Some("128")),
    p("cluster.channel.keepalive.interval", None),
    p("cluster.channel.keepalive.max-topics", None),
    p("cluster.notify.window", Some("100ms")),
    p("cluster.notify.wait", None),
    p("cluster.channel.reconnect.retries", None),
    p("cluster.channel.reconnect.backoff", None),
    p("cluster.channel.queue.max-size", None),
    p("cluster.channel.queue.max-age", None),
    p("cluster.vote.quorum-percent", Some("51")),
    p("cluster.vote.min-voters", Some("2")),
    p("cluster.freezing.duration", None),
    p("cluster.entry-address", None),
    p("cluster.advertise-addresses", None),
    p("cluster.first-node", Some("false")),
    p("cluster.restore.grace", Some("5m")),
    p("cluster.restore.strict", Some("false")),
    p("cluster.restore.clock-slack", Some("5s")),
    p("cluster.restore.probe.timeout", Some("2h")),
    p("cluster.restore.probe.interval", Some("10s")),
    p("cluster.restore.cold-start.wait", Some("15s")),
    p("cluster.bootstrap.retry.interval", Some("5s")),
    p("cluster.bootstrap.retry.max-attempts", Some("0")),
    p("cluster.message-loss.ttl", None),
    p("cluster.recovery.timeout", None),
    p("cluster.reunion.interval", None),
    p("cluster.sync.stream.max-items", None),
    p("cluster.sync.stream.max-bytes", None),
    p("cluster.sync.stream.max-line-bytes", None),
    p("cluster.sync.stream.idle-timeout", None),
    p("cluster.sync.range.size", None),
    p("cluster.sync.log.max-bytes", None),
    p("cluster.sync.timeout", None),
    // Message delivery
    p("delivery.visibility-timeout.default", Some("120s")),
    p("delivery.visibility-timeout.max", None),
    p("delivery.ready-list.size", Some("1000")),
    p("delivery.invisible-list.size", None),
    p("delivery.invisible.reload-threshold", None),
    p("delivery.max-messages.default", Some("10")),
    p("delivery.max-messages.max", None),
    p("delivery.get.max-attempts", None),
    p("delivery.fair.ready-weight", Some("0.1")),
    p("delivery.fair.max-groups", None),
    p("delivery.fair.normal-every", Some("10")),
    p("delivery.take.max-attempts", None),
    // Retention and dead-letter
    p("retention.ttl.default", Some("14d")),
    p("retention.max-deliveries.default", Some("10")),
    p("topics.dead-letter.suffix", Some("-dead-letter")),
    p("retention.sweep.interval", Some("60s")),
    // Filas FIFO
    p("fifo.dedup.window", Some("300s")),
    p("fifo.dedup.index.max-entries", None),
    p("fifo.forward.max-attempts", None),
    // Volatile topics and distributors
    p("topics.volatile.idle-timeout.default", Some("600s")),
    p("topics.volatile.idle-check.retry-interval", None),
    p("topics.distributor.max-targets", None),
    p("topics.volatile.max-count", None),
    p("topics.volatile.takeover.jitter", None),
    // Observability (the OTEL_* variables do not go through here: they belong to the SDK)
    p("telemetry.prometheus.enabled", Some("true")),
    p("operations.allowed-ips", None),
    p("telemetry.topics.mode", Some("off")),
    p("telemetry.topics.group-by-tags", None),
    p("telemetry.topics.max-topics", None),
    p("telemetry.logs.level", None),
    p("telemetry.logs.format", Some("text")),
    p("stats.topics.top-groups", Some("10")),
    p("stats.topics.cache-ttl", None),
    p("stats.topics.backlog.depth", None),
    p("stats.topics.backlog.age", None),
    p("stats.topics.noisy-group-share", Some("50")),
    p("stats.topics.group-stalled-deliveries", None),
    p("stats.topics.stalled-after", None),
    p("stats.topics.idle-after", None),
    p("stats.disk.free-min", None),
    // Rate limits
    p("ratelimit.publish.rps", None),
    p("ratelimit.consume.rps", None),
    p("ratelimit.admin.rps", None),
    p("ratelimit.ops.rps", None),
    p("ratelimit.burst", None),
    p("client-ip.headers.enabled", Some("true")),
    p("client-ip.headers", Some("X-Forwarded-For")),
    p("client-ip.trusted-proxies", None),
    // Optional S3 backup (doc 23, "S3 backup"): without `backup.s3.bucket` the feature is off
    p("backup.s3.bucket", None),
    p("backup.s3.region", None),
    p("backup.s3.endpoint", None),
    p("backup.s3.prefix", Some("taba")),
    p("backup.s3.access-key", None),
    p("backup.s3.secret-key", None),
    p("backup.s3.path-style", None), // derived: `true` when there is `backup.s3.endpoint`
    p("backup.delay", Some("60s")),
    p("backup.meta.interval", Some("60s")),
    p("backup.segment.max-bytes", Some("8MB")),
    p("backup.segment.max-messages", Some("5000")),
    p("backup.segment.max-age", Some("60s")),
    p("backup.heartbeat.interval", Some("15s")),
    p("backup.dead-after", Some("60s")),
    p("backup.drain.timeout", Some("30s")),
    p("backup.disk.evict-above", Some("85")),
    p("backup.disk.evict-to", Some("70")),
    p("backup.disk.pause-at", Some("90")),
    p("backup.reconcile.interval", Some("60s")),
    p("backup.restore.lock-ttl", Some("60s")),
];

#[cfg(test)]
mod tests;
