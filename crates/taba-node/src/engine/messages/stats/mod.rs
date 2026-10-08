//! O que as mensagens contam sobre si mesmas (doc 11): os contadores por tópico e do nó, os
//! retratos que `GET /metrics/topics*` e `GET /metrics` leem e as condições do tópico. É só dado:
//! quem conta é o `Messages`, e quem lê é a camada HTTP, sem que o `engine` conheça formato de
//! saída nenhum.

use std::sync::Mutex;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};

use crate::config::Config;
use crate::telemetry::histogram::{Histogram, LATENCY_BOUNDS};

/// Os baldes do tempo de espera da primeira entrega (`taba.delivery.fair.wait`), em segundos.
pub const WAIT_BOUNDS: &[f64] = &[
    0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0, 5.0, 10.0, 60.0, 300.0,
];

/// As faixas do atributo `band`: de `0` a `14`, e `15+`.
pub const BANDS: usize = 16;

/// O que um tópico de fila conta desde que o nó abriu o tópico.
#[derive(Debug, Clone)]
pub struct TopicCounters {
    pub published: u64,
    pub published_bytes: u64,
    pub publish_errors: u64,
    pub consumed: u64,
    pub acked: u64,
    /// Só na FIFO: publicações descartadas por duplicidade.
    pub deduplicated: u64,
    pub redelivered: u64,
    pub dead_lettered: u64,
    pub expired: u64,
    pub last_published_ms: Option<u64>,
    pub last_consumed_ms: Option<u64>,
    pub publish_latency: Histogram,
    pub consume_latency: Histogram,
}

impl TopicCounters {
    pub fn new() -> Self {
        Self {
            published: 0,
            published_bytes: 0,
            publish_errors: 0,
            consumed: 0,
            acked: 0,
            deduplicated: 0,
            redelivered: 0,
            dead_lettered: 0,
            expired: 0,
            last_published_ms: None,
            last_consumed_ms: None,
            publish_latency: Histogram::new(LATENCY_BOUNDS),
            consume_latency: Histogram::new(LATENCY_BOUNDS),
        }
    }
}

impl Default for TopicCounters {
    fn default() -> Self {
        Self::new()
    }
}

/// O que o distribuidor conta (doc 15, seção 14.7).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DistributorCounters {
    pub received: u64,
    pub copies: u64,
    pub copy_errors: u64,
}

/// Os contadores do nó todo. Atômicos: quem conta não pega trava nenhuma.
#[derive(Debug)]
pub struct NodeCounters {
    pub overflow: AtomicU64,
    pub reads_memory: AtomicU64,
    pub reads_disk: AtomicU64,
    pub disk_errors: AtomicU64,
    pub redeliveries: AtomicU64,
    pub dead_lettered: AtomicU64,
    pub expired: AtomicU64,
    pub deduplicated: AtomicU64,
    pub longpoll_waiting: AtomicI64,
    pub longpoll_delivered: AtomicU64,
    pub longpoll_empty: AtomicU64,
    pub longpoll_disconnected: AtomicU64,
    fair_wait: Mutex<Vec<Histogram>>,
}

impl NodeCounters {
    pub fn new() -> Self {
        Self {
            overflow: AtomicU64::new(0),
            reads_memory: AtomicU64::new(0),
            reads_disk: AtomicU64::new(0),
            disk_errors: AtomicU64::new(0),
            redeliveries: AtomicU64::new(0),
            dead_lettered: AtomicU64::new(0),
            expired: AtomicU64::new(0),
            deduplicated: AtomicU64::new(0),
            longpoll_waiting: AtomicI64::new(0),
            longpoll_delivered: AtomicU64::new(0),
            longpoll_empty: AtomicU64::new(0),
            longpoll_disconnected: AtomicU64::new(0),
            fair_wait: Mutex::new((0..BANDS).map(|_| Histogram::new(WAIT_BOUNDS)).collect()),
        }
    }

    pub fn add(counter: &AtomicU64, n: u64) {
        counter.fetch_add(n, Ordering::Relaxed);
    }

    /// A espera até a primeira entrega de uma mensagem cujo grupo estava na faixa `band`.
    pub fn observe_wait(&self, band: u32, seconds: f64) {
        let index = (band as usize).min(BANDS - 1);
        self.fair_wait.lock().unwrap_or_else(|e| e.into_inner())[index].record(seconds);
    }

    pub fn fair_wait(&self) -> Vec<Histogram> {
        self.fair_wait
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}

impl Default for NodeCounters {
    fn default() -> Self {
        Self::new()
    }
}

/// O que `Messages::observe_node` entrega a `GET /metrics`.
#[derive(Debug, Clone)]
pub struct NodeObservation {
    pub memory_bytes: u64,
    pub memory_limit_bytes: Option<u64>,
    pub memory_messages: u64,
    pub disk_messages: u64,
    pub ready: u64,
    pub busy_groups: u64,
    pub overflow: u64,
    pub reads_memory: u64,
    pub reads_disk: u64,
    pub disk_errors: u64,
    pub redeliveries: u64,
    pub dead_lettered: u64,
    pub expired: u64,
    pub deduplicated: u64,
    pub longpoll_waiting: i64,
    pub longpoll_delivered: u64,
    pub longpoll_empty: u64,
    pub longpoll_disconnected: u64,
    pub fair_wait: Vec<Histogram>,
}

/// p50, p95 e p99 de uma latência, em segundos. `None`: ainda sem observação.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Percentiles {
    pub p50: Option<f64>,
    pub p95: Option<f64>,
    pub p99: Option<f64>,
}

impl Percentiles {
    pub fn of(histogram: &Histogram) -> Self {
        Self {
            p50: histogram.percentile(0.50),
            p95: histogram.percentile(0.95),
            p99: histogram.percentile(0.99),
        }
    }
}

/// Um grupo de mensagens (doc 11, "Métricas por grupo").
#[derive(Debug, Clone, PartialEq)]
pub struct GroupSnapshot {
    /// `None` no contador compartilhado dos grupos acima de `delivery.fair.max-groups`. A mensagem
    /// sem `groupId` é o grupo de nome vazio.
    pub group_id: Option<String>,
    pub overflow: bool,
    pub visible: u64,
    pub invisible: u64,
    pub band: u32,
    /// O instante da mensagem mais antiga do grupo.
    pub oldest_ms: Option<u64>,
}

impl GroupSnapshot {
    pub fn messages(&self) -> u64 {
        self.visible + self.invisible
    }
}

/// O retrato de uma fila num instante (doc 11, "Métricas por tópico").
#[derive(Debug, Clone)]
pub struct TopicSnapshot {
    pub counters: TopicCounters,
    pub publish_rate: f64,
    pub consume_rate: f64,
    pub ack_rate: f64,
    pub visible: u64,
    pub invisible: u64,
    /// A mensagem mais antiga ainda não confirmada.
    pub oldest_ms: Option<u64>,
    pub memory_messages: u64,
    pub disk_messages: u64,
    pub memory_bytes: u64,
    pub disk_bytes: u64,
    /// Os grupos acompanhados, e quantas mensagens o maior tem.
    pub groups: u64,
    pub largest_group: u64,
    /// Só na FIFO: grupos com mensagem em voo.
    pub busy_groups: u64,
    /// Só na FIFO, e só com `stats.topics.group-stalled-deliveries`: o grupo cuja cabeça passou
    /// do limite de entregas.
    pub stalled_group: Option<String>,
    pub publish_latency: Percentiles,
    pub consume_latency: Percentiles,
}

impl TopicSnapshot {
    pub fn depth(&self) -> u64 {
        self.visible + self.invisible
    }

    /// A parcela do `depth` que o maior grupo ocupa, de 0 a 1.
    pub fn noisy_share(&self) -> f64 {
        match self.depth() {
            0 => 0.0,
            depth => self.largest_group as f64 / depth as f64,
        }
    }

    pub fn bytes(&self) -> u64 {
        self.memory_bytes + self.disk_bytes
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Info,
    Warning,
}

impl Severity {
    pub const fn as_str(self) -> &'static str {
        match self {
            Severity::Info => "info",
            Severity::Warning => "warning",
        }
    }
}

/// As condições do tópico, em ordem de severidade e depois de nome.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Condition {
    Backlog,
    Stalled,
    GroupStalled,
    NoisyGroup,
    MemoryPressure,
    Idle,
}

impl Condition {
    pub const ALL: [Condition; 6] = [
        Condition::Backlog,
        Condition::Stalled,
        Condition::GroupStalled,
        Condition::NoisyGroup,
        Condition::MemoryPressure,
        Condition::Idle,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Condition::Backlog => "backlog",
            Condition::Stalled => "stalled",
            Condition::GroupStalled => "group-stalled",
            Condition::NoisyGroup => "noisy-group",
            Condition::MemoryPressure => "memory-pressure",
            Condition::Idle => "idle",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.as_str() == text)
    }

    pub const fn severity(self) -> Severity {
        match self {
            Condition::Backlog | Condition::Stalled | Condition::GroupStalled => Severity::Warning,
            Condition::NoisyGroup | Condition::MemoryPressure | Condition::Idle => Severity::Info,
        }
    }
}

/// Os limites das condições e o tamanho do detalhe (`stats.topics.*`). Um limite que não foi
/// definido desliga a condição que depende dele (D90).
#[derive(Debug, Clone)]
pub struct StatsRules {
    pub top_groups: usize,
    pub backlog_depth: Option<u64>,
    pub backlog_age_ms: Option<u64>,
    pub noisy_group_share: u32,
    pub group_stalled_deliveries: Option<u32>,
    pub stalled_after_ms: Option<u64>,
    pub idle_after_ms: Option<u64>,
    /// `stats.disk.free-min`: abaixo disto de espaço livre, a condição `storage-pressure` do nó.
    pub disk_free_min: Option<u64>,
}

fn millis(d: std::time::Duration) -> u64 {
    u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
}

impl StatsRules {
    pub fn from_config(config: &Config) -> Self {
        let s = &config.stats;
        Self {
            top_groups: s.topics_top_groups as usize,
            backlog_depth: s.topics_backlog_depth,
            backlog_age_ms: s.topics_backlog_age.map(millis),
            noisy_group_share: s.topics_noisy_group_share,
            group_stalled_deliveries: s.topics_group_stalled_deliveries,
            stalled_after_ms: s.topics_stalled_after.map(millis),
            idle_after_ms: s.topics_idle_after.map(millis),
            disk_free_min: s.disk_free_min,
        }
    }
}

/// As condições vigentes do tópico, as mais graves primeiro. `baseline_ms` é o início da conta
/// para um tópico que ainda não teve atividade: o maior entre a criação e a partida do nó.
pub fn evaluate(
    snapshot: &TopicSnapshot,
    rules: &StatsRules,
    now_ms: u64,
    baseline_ms: u64,
) -> Vec<Condition> {
    let depth = snapshot.depth();
    let last_consumed = snapshot
        .counters
        .last_consumed_ms
        .unwrap_or(0)
        .max(baseline_ms);
    let last_activity = last_consumed.max(snapshot.counters.last_published_ms.unwrap_or(0));
    let too_old = match (rules.backlog_age_ms, snapshot.oldest_ms) {
        (Some(limit), Some(at)) => now_ms.saturating_sub(at) > limit,
        _ => false,
    };
    let active = |condition: Condition| match condition {
        Condition::Backlog => rules.backlog_depth.is_some_and(|limit| depth > limit) || too_old,
        Condition::Stalled => {
            depth > 0
                && rules
                    .stalled_after_ms
                    .is_some_and(|after| now_ms.saturating_sub(last_consumed) > after)
        }
        Condition::GroupStalled => snapshot.stalled_group.is_some(),
        Condition::NoisyGroup => {
            snapshot.groups >= 2
                && depth > 0
                && snapshot.largest_group * 100 > depth * u64::from(rules.noisy_group_share)
        }
        Condition::MemoryPressure => snapshot.disk_messages > snapshot.memory_messages,
        Condition::Idle => rules
            .idle_after_ms
            .is_some_and(|after| now_ms.saturating_sub(last_activity) > after),
    };
    let mut out: Vec<Condition> = Condition::ALL.into_iter().filter(|c| active(*c)).collect();
    out.sort_by_key(|c| std::cmp::Reverse(c.severity() == Severity::Warning));
    out
}

#[cfg(test)]
mod tests;
