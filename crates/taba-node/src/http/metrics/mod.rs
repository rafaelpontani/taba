//! `GET /metrics` (doc 06b, doc 11): as métricas **deste nó** no texto do Prometheus. O que o
//! caminho das requisições soma (HTTP, rate limit, autenticação) já está no registro; aqui, a cada
//! leitura, entra o que o nó só sabe dizer na hora (memória, disco, prontas, estado, processo) e,
//! conforme `telemetry.topics.mode`, as métricas de tópico.

use std::collections::BTreeMap;
use std::time::Instant;

use axum::extract::State;
use axum::http::{HeaderMap, header};
use axum::response::{IntoResponse, Response};

use super::AppState;
use super::common::ApiError;
use super::operations::StatusParts;
use crate::cluster::health::{self, Condition};
use crate::cluster::members::MemberState;
use crate::cluster::state::{DrainState, NodeState};
use crate::config::TopicsMetricsMode;
use crate::core::error::TabaError;
use crate::engine::messages::stats::{self, NodeObservation, TopicSnapshot};
use crate::telemetry::metrics::*;
use crate::telemetry::{process, prometheus};

/// O atributo de faixa de `taba.delivery.fair.wait`: `0` a `14`, e `15+`.
fn band_label(band: usize) -> String {
    if band >= stats::BANDS - 1 {
        "15+".to_owned()
    } else {
        band.to_string()
    }
}

pub async fn prometheus(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let open_metrics = headers
        .get(header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.contains("application/openmetrics-text"));
    let body = tokio::task::spawn_blocking(move || {
        collect(&state);
        prometheus::render(
            &state.telemetry.registry.snapshot(),
            &state.telemetry.resource,
            open_metrics,
        )
    })
    .await
    .map_err(|_| TabaError::internal())?;
    let content_type = if open_metrics {
        prometheus::OPENMETRICS_CONTENT_TYPE
    } else {
        prometheus::TEXT_CONTENT_TYPE
    };
    Ok(([(header::CONTENT_TYPE, content_type)], body).into_response())
}

/// O envio periódico das métricas por OTLP: a cada `OTEL_METRIC_EXPORT_INTERVAL` lê o mesmo
/// registro de `GET /metrics` e o manda ao coletor; ao parar, manda uma última vez. Uma falha de
/// envio vira `warn` e a próxima rodada tenta de novo.
pub async fn run_otlp(
    state: AppState,
    mut sender: crate::telemetry::otlp::metrics::Sender,
    interval: std::time::Duration,
    cancel: tokio_util::sync::CancellationToken,
) {
    let started = std::time::SystemTime::now();
    let mut ticker = tokio::time::interval(interval);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    ticker.tick().await;
    loop {
        let stop = tokio::select! {
            () = cancel.cancelled() => true,
            _ = ticker.tick() => false,
        };
        let reading = state.clone();
        let request = tokio::task::spawn_blocking(move || {
            collect(&reading);
            crate::telemetry::otlp::metrics::build_request(
                &reading.telemetry.registry.snapshot(),
                &reading.telemetry.resource,
                started,
                std::time::SystemTime::now(),
            )
        })
        .await;
        if let Ok(request) = request
            && let Err(e) = sender.send(request).await
        {
            tracing::warn!(error = %e, "não foi possível enviar as métricas por OTLP");
        }
        if stop {
            return;
        }
    }
}

/// Preenche o registro com o estado de agora.
pub fn collect(state: &AppState) {
    let registry = &state.telemetry.registry;
    collect_process(state);
    collect_node(state);
    collect_cluster(state);
    let observation = state.messages.observe_node();
    collect_storage(state, &observation);
    registry.set(&NODE_DRAIN_PENDING, &[], observation.memory_messages as f64);
    collect_topics(state);
}

fn collect_process(state: &AppState) {
    let registry = &state.telemetry.registry;
    let uptime = state
        .clock
        .now()
        .duration_since(state.node.started_at)
        .map_or(0.0, |d| d.as_secs_f64());
    registry.set(&PROCESS_UPTIME, &[], uptime);
    let sample = process::sample();
    if let Some(bytes) = sample.memory_bytes {
        registry.set(&PROCESS_MEMORY_USAGE, &[], bytes as f64);
    }
    if let Some(threads) = sample.threads {
        registry.set(&PROCESS_THREAD_COUNT, &[], threads as f64);
    }
    if let Some(files) = sample.open_files {
        registry.set(&PROCESS_OPEN_FILES, &[], files as f64);
    }
    if let Some(cpu) = sample.cpu_seconds {
        let cores = std::thread::available_parallelism().map_or(1.0, |n| n.get() as f64);
        if let Some(value) = state.telemetry.cpu.utilization(cpu, Instant::now(), cores) {
            registry.set(&PROCESS_CPU_UTILIZATION, &[], value);
        }
    }
}

/// O estado do nó e do drain, um medidor `0/1` por estado possível.
fn collect_node(state: &AppState) {
    let registry = &state.telemetry.registry;
    let current = state.node.state();
    for node_state in [
        NodeState::Joining,
        NodeState::Active,
        NodeState::Syncing,
        NodeState::Returning,
        NodeState::Failed,
    ] {
        registry.set(
            &NODE_STATE,
            &[("taba.state", node_state.as_str())],
            f64::from(u8::from(node_state == current)),
        );
    }
    let drain = state.node.drain_state();
    for drain_state in [
        DrainState::Active,
        DrainState::Draining,
        DrainState::Drained,
    ] {
        registry.set(
            &NODE_DRAIN_STATE,
            &[("taba.state", drain_state.as_str())],
            f64::from(u8::from(drain_state == drain)),
        );
    }
    if state.auth.enabled {
        registry.set(&AUTH_TOKENS, &[], state.auth.tokens.count() as f64);
    }
}

/// A tabela de membros e o anel (só em `mode=production` com o nó no cluster): os membros por
/// estado, a época da tabela e os blocos que este nó atende, por papel (primário, secundário...).
fn collect_cluster(state: &AppState) {
    let Some(cluster) = &state.cluster else {
        return;
    };
    let registry = &state.telemetry.registry;
    if let Some(left) = state.tls.as_ref().and_then(|tls| tls.expires_in()) {
        registry.set(&CLUSTER_TLS_EXPIRES_IN, &[], left as f64);
    }
    let membership = cluster.membership();
    if let Some(table) = membership.table() {
        registry.set(&CLUSTER_MEMBERS_VERSION, &[], table.version as f64);
        for member_state in [
            MemberState::Active,
            MemberState::Syncing,
            MemberState::Draining,
            MemberState::Suspect,
        ] {
            let count = table
                .members
                .iter()
                .filter(|m| m.state == member_state)
                .count();
            registry.set(
                &CLUSTER_MEMBERS,
                &[("taba.state", member_state.as_str())],
                count as f64,
            );
        }
    }
    if let Some(replication) = cluster.replication() {
        registry.set(&WRITE_PENDING, &[], replication.replica.pending() as f64);
    }
    // Os nós em freezing e as condições do cluster que este nó enxerga (doc 11).
    if let Some(signals) = health::collect(cluster) {
        registry.set(&CLUSTER_FREEZING, &[], signals.frozen_nodes as f64);
        registry.set(
            &CLUSTER_QUORUM_AVAILABLE,
            &[],
            f64::from(u8::from(signals.quorum_available)),
        );
        let pressure = StatusParts::of(state).storage_pressure();
        let current = signals.conditions(pressure);
        for condition in Condition::ALL {
            registry.set(
                &CLUSTER_CONDITION,
                &[("taba.condition", condition.as_str())],
                f64::from(u8::from(current.contains(&condition))),
            );
        }
    }
    let rules = cluster.rules();
    registry.set(&RING_VIRTUAL_NODES, &[], f64::from(rules.virtual_nodes));
    let ring = membership.ring();
    let replicas = rules.replication_factor.max(1) as usize;
    let mut by_role = vec![0u64; replicas];
    for block in ring.blocks() {
        let owners = ring.block_owners(block, replicas).unwrap_or_default();
        if let Some(role) = owners.iter().position(|n| *n == cluster.node_id()) {
            by_role[role] += 1;
        }
    }
    for (role, count) in by_role.into_iter().enumerate() {
        registry.set(
            &RING_BLOCKS,
            &[("taba.role", &role_label(role)), ("taba.state", "healthy")],
            count as f64,
        );
    }
}

/// O papel de um nó num bloco, pela posição na ordem das réplicas.
fn role_label(role: usize) -> String {
    match role {
        0 => "primary".to_owned(),
        1 => "secondary".to_owned(),
        2 => "tertiary".to_owned(),
        n => format!("replica-{}", n + 1),
    }
}

fn collect_storage(state: &AppState, o: &NodeObservation) {
    let registry = &state.telemetry.registry;
    registry.set(&STORAGE_MEMORY_USAGE, &[], o.memory_bytes as f64);
    if let Some(limit) = o.memory_limit_bytes {
        registry.set(&STORAGE_MEMORY_LIMIT, &[], limit as f64);
    }
    registry.set(&STORAGE_MEMORY_MESSAGES, &[], o.memory_messages as f64);
    registry.set(&STORAGE_DISK_MESSAGES, &[], o.disk_messages as f64);
    if let Some(free) = state.messages.disk_free_bytes() {
        registry.set(&STORAGE_DISK_FREE, &[], free as f64);
    }
    registry.set(&STORAGE_OVERFLOW, &[], o.overflow as f64);
    registry.set(
        &STORAGE_READS,
        &[("taba.tier", "memory")],
        o.reads_memory as f64,
    );
    registry.set(
        &STORAGE_READS,
        &[("taba.tier", "disk")],
        o.reads_disk as f64,
    );
    registry.set(&STORAGE_DISK_ERRORS, &[], o.disk_errors as f64);

    registry.set(&DELIVERY_READY, &[], o.ready as f64);
    registry.set(&DELIVERY_REDELIVERIES, &[], o.redeliveries as f64);
    registry.set(&DELIVERY_DEAD_LETTERED, &[], o.dead_lettered as f64);
    registry.set(&RETENTION_EXPIRED, &[], o.expired as f64);
    registry.set(&FIFO_DEDUPLICATED, &[], o.deduplicated as f64);
    registry.set(&FIFO_GROUPS_BUSY, &[], o.busy_groups as f64);
    registry.set(&LONGPOLL_WAITING, &[], o.longpoll_waiting as f64);
    for (outcome, count) in [
        ("delivered", o.longpoll_delivered),
        ("empty", o.longpoll_empty),
        ("disconnected", o.longpoll_disconnected),
    ] {
        registry.set(
            &LONGPOLL_OUTCOME,
            &[("taba.outcome", outcome)],
            count as f64,
        );
    }
    for (band, histogram) in o.fair_wait.iter().enumerate() {
        if histogram.count() > 0 {
            registry.set_histogram(
                &DELIVERY_FAIR_WAIT,
                &[("taba.band", &band_label(band))],
                histogram.clone(),
            );
        }
    }
}

/// Os totais de uma série de tópico (um tópico, um valor de tag, ou o resto agregado em `_other`).
#[derive(Default)]
struct Totals {
    published: u64,
    consumed: u64,
    acked: u64,
    visible: u64,
    invisible: u64,
    memory_bytes: u64,
    disk_bytes: u64,
    oldest_ms: Option<u64>,
}

impl Totals {
    fn add(&mut self, s: &TopicSnapshot) {
        self.published += s.counters.published;
        self.consumed += s.counters.consumed;
        self.acked += s.counters.acked;
        self.visible += s.visible;
        self.invisible += s.invisible;
        self.memory_bytes += s.memory_bytes;
        self.disk_bytes += s.disk_bytes;
        self.oldest_ms = match (self.oldest_ms, s.oldest_ms) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
    }

    fn emit(&self, registry: &Registry, labels: &[(&str, &str)]) {
        registry.set(&TOPIC_PUBLISHED, labels, self.published as f64);
        registry.set(&TOPIC_CONSUMED, labels, self.consumed as f64);
        registry.set(&TOPIC_ACKED, labels, self.acked as f64);
        registry.set(&TOPIC_VISIBLE, labels, self.visible as f64);
        registry.set(&TOPIC_INVISIBLE, labels, self.invisible as f64);
        registry.set(&TOPIC_DEPTH, labels, (self.visible + self.invisible) as f64);
        registry.set(&TOPIC_MEMORY_BYTES, labels, self.memory_bytes as f64);
        registry.set(&TOPIC_DISK_BYTES, labels, self.disk_bytes as f64);
        // Sem mensagem não há data: a série não existe (nunca zero).
        if let Some(ms) = self.oldest_ms {
            registry.set(&TOPIC_OLDEST_TIMESTAMP, labels, ms as f64 / 1000.0);
        }
    }
}

fn tag_attribute(key: &str) -> String {
    format!("taba.topic.tag.{key}")
}

/// As métricas de tópico em `GET /metrics`, conforme `telemetry.topics.mode` (doc 11, "Exportação
/// das métricas de tópico"). O modo `off`, o padrão, não emite nada: o `/metrics` não cresce com
/// o número de tópicos.
fn collect_topics(state: &AppState) {
    let telemetry = &state.telemetry;
    let registry = &telemetry.registry;
    for def in TOPIC_DEFS {
        registry.clear(def);
    }
    if telemetry.topics.mode == TopicsMetricsMode::Off {
        return;
    }
    let now = state.clock.now_millis();
    let keys = &telemetry.topics.group_by_tags;
    // Um tópico por linha, em ordem de nome.
    let mut rows = Vec::new();
    for view in state.topics.all_views() {
        let name = &view.definition.name;
        if view.definition.queue().is_none() {
            continue;
        }
        let Ok(snapshot) = state.messages.topic_snapshot(name, &state.stats_rules) else {
            continue;
        };
        let baseline = view.definition.created_ms.max(state.messages.started_ms());
        let conditions = stats::evaluate(&snapshot, &state.stats_rules, now, baseline);
        let tags: Vec<String> = keys
            .iter()
            .map(|k| view.definition.tags.get(k).cloned().unwrap_or_default())
            .collect();
        rows.push((name.clone(), tags, snapshot, conditions));
    }
    let tag_labels = |tags: &[String]| -> Vec<(String, String)> {
        keys.iter()
            .map(|k| tag_attribute(k))
            .zip(tags.iter().cloned())
            .collect()
    };
    match telemetry.topics.mode {
        TopicsMetricsMode::Off => {}
        TopicsMetricsMode::ByTags => {
            let mut groups: BTreeMap<Vec<String>, Totals> = BTreeMap::new();
            for (_, tags, snapshot, _) in &rows {
                groups.entry(tags.clone()).or_default().add(snapshot);
            }
            for (tags, totals) in groups {
                let owned = tag_labels(&tags);
                let labels: Vec<(&str, &str)> = owned
                    .iter()
                    .map(|(k, v)| (k.as_str(), v.as_str()))
                    .collect();
                totals.emit(registry, &labels);
            }
        }
        TopicsMetricsMode::PerTopic => {
            let mut other = Totals::default();
            let mut others = 0;
            for (index, (name, tags, snapshot, conditions)) in rows.iter().enumerate() {
                if index >= telemetry.topics.max_topics {
                    other.add(snapshot);
                    others += 1;
                    continue;
                }
                let mut owned = vec![("messaging.destination.name".to_owned(), name.clone())];
                owned.extend(tag_labels(tags));
                let mut totals = Totals::default();
                totals.add(snapshot);
                let labels: Vec<(&str, &str)> = owned
                    .iter()
                    .map(|(k, v)| (k.as_str(), v.as_str()))
                    .collect();
                totals.emit(registry, &labels);
                for condition in conditions {
                    let mut with = labels.clone();
                    with.push(("taba.condition", condition.as_str()));
                    registry.set(&TOPIC_CONDITION, &with, 1.0);
                }
            }
            if others > 0 {
                other.emit(registry, &[("messaging.destination.name", "_other")]);
            }
        }
    }
}
