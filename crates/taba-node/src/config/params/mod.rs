//! A tabela de parâmetros: chave, variável de ambiente e padrão (doc 10).
//!
//! É a fonte única das chaves aceitas. Uma chave que não está aqui é recusada no
//! arquivo `.yml` (um erro de digitação não passa em silêncio), e `Config` só lê
//! chaves daqui. Padrão `None` quer dizer "sem valor": ou é opcional, ou o valor é
//! derivado de outro parâmetro, ou ainda está **A definir** em `10-configuracao.md`.

pub struct Param {
    pub key: &'static str,
    pub default: Option<&'static str>,
}

const fn p(key: &'static str, default: Option<&'static str>) -> Param {
    Param { key, default }
}

/// A variável de ambiente de uma chave: prefixo `TABA_`, maiúsculas, `.` e `-` viram `_`.
pub fn env_name(key: &str) -> String {
    format!("TABA_{}", key.to_ascii_uppercase().replace(['.', '-'], "_"))
}

pub fn find(key: &str) -> Option<&'static Param> {
    PARAMS.iter().find(|p| p.key == key)
}

pub static PARAMS: &[Param] = &[
    // Modo de execução
    p("mode", Some("production")),
    p("development.resume-on-start", Some("true")),
    // Portas (as sugestões da doc valem como padrão: 8080 e 8081)
    p("api.port", Some("8080")),
    p("api.pagination.default-limit", Some("100")),
    p("api.pagination.max-limit", Some("1000")),
    p("cluster.port", Some("8081")),
    p("operations.port", None),
    // Segurança da porta do cluster
    p("secrets.dir", Some("/etc/taba/secrets")),
    p("cluster.tls.dir", None), // derivado: <secrets.dir>/tls
    p("cluster.tls.reload-interval", Some("60s")),
    p("cluster.tls.expiry-warning", Some("30d")),
    // Autenticação da API de cliente
    p("auth.enabled", Some("auto")),
    p("auth.root-token", None),
    p("auth.root-token-file", None), // derivado: <secrets.dir>/root-token
    p("auth.reload-interval", Some("60s")),
    // Parada e drain
    p("drain.on-stop", Some("true")),
    p("drain.timeout", Some("25s")),
    // Armazenamento e memória
    p("memory.limit", None),
    p("memory.rate.window", Some("5m")),
    p("memory.topic.reserve", Some("1000")),
    p("storage.dir", Some("/var/lib/taba")),
    p("storage.db.synchronous", Some("NORMAL")),
    // Mensagens
    p("message.max-bytes", Some("5MB")),
    p("message.hard-limit", None),
    // Escrita e durabilidade
    p("replication.factor", Some("3")),
    p("write.quorum", None), // derivado: floor(replication.factor / 2) + 1
    p("write.retry.attempts", None),
    p("write.retry.backoff", None),
    p("write.pending.ttl", None),
    p("write.commit.timeout", None),
    // Cluster
    p("ring.virtual-nodes", None),
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
    p("cluster.first-node", Some("false")),
    p("cluster.bootstrap.retry.interval", None),
    p("cluster.bootstrap.retry.max-attempts", Some("0")),
    p("cluster.message-loss.ttl", None),
    p("cluster.recovery.timeout", None),
    p("cluster.sync.stream.max-items", None),
    p("cluster.sync.stream.max-bytes", None),
    p("cluster.sync.stream.max-line-bytes", None),
    p("cluster.sync.stream.idle-timeout", None),
    p("cluster.sync.range.size", None),
    p("cluster.sync.log.max-bytes", None),
    p("cluster.sync.timeout", None),
    // Entrega de mensagens
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
    // Retenção e dead-letter
    p("retention.ttl.default", Some("14d")),
    p("retention.max-deliveries.default", Some("10")),
    p("topics.dead-letter.suffix", Some("-dead-letter")),
    p("retention.sweep.interval", Some("60s")),
    // Filas FIFO
    p("fifo.dedup.window", Some("300s")),
    p("fifo.dedup.index.max-entries", None),
    p("fifo.forward.max-attempts", None),
    // Tópicos voláteis e distribuidores
    p("topics.volatile.idle-timeout.default", Some("600s")),
    p("topics.volatile.idle-check.retry-interval", None),
    p("topics.distributor.max-targets", None),
    p("topics.volatile.max-count", None),
    p("topics.volatile.takeover.jitter", None),
    // Observabilidade (as variáveis OTEL_* não passam por aqui: são do SDK)
    p("telemetry.prometheus.enabled", Some("true")),
    p("operations.allowed-ips", None),
    p("telemetry.topics.mode", Some("off")),
    p("telemetry.topics.group-by-tags", None),
    p("telemetry.topics.max-topics", None),
    p("telemetry.logs.level", None),
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
];

#[cfg(test)]
mod tests;
