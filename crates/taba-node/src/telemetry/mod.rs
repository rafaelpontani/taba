//! Métricas, traces e logs (doc 11). O log estruturado, o registro de métricas `taba.*` com a saída
//! no texto do Prometheus, os eventos (*log records* com `event.name`), os spans do nó e a
//! exportação OTLP (métricas, traces e logs).
//!
//! O módulo não depende do resto do nó: o `engine` usa o [`histogram::Histogram`], e quem lê o
//! estado do nó para preencher o registro é a camada HTTP (`http/metrics`).

pub mod histogram;
pub mod metrics;
pub mod otlp;
pub mod process;
pub mod prometheus;
pub mod trace;

use std::collections::BTreeMap;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::{Layer, SubscriberExt};
use tracing_subscriber::registry::Registry as SpanRegistry;
use tracing_subscriber::util::SubscriberInitExt;

use crate::config::{Config, TopicsMetricsMode};
use metrics::Registry;

/// O teto de séries por tópico no modo `per-topic` enquanto `telemetry.topics.max-topics` está a
/// definir (D91).
pub const DEFAULT_MAX_TOPICS: usize = 1000;

/// Como as métricas de tópico entram em `GET /metrics` (`telemetry.topics.*`).
#[derive(Debug, Clone)]
pub struct TopicsExport {
    pub mode: TopicsMetricsMode,
    pub group_by_tags: Vec<String>,
    pub max_topics: usize,
}

/// O que o nó coleta e como expõe.
pub struct Telemetry {
    pub registry: Registry,
    /// `telemetry.prometheus.enabled`.
    pub prometheus_enabled: bool,
    /// Os atributos de recurso: `service.name`, `service.instance.id`, `service.version`, e o que
    /// veio de `OTEL_RESOURCE_ATTRIBUTES`.
    pub resource: Vec<(String, String)>,
    pub topics: TopicsExport,
    pub cpu: process::CpuMeter,
}

impl Telemetry {
    pub fn from_config(config: &Config, node_id: &str) -> Self {
        let mut resource: BTreeMap<String, String> = BTreeMap::new();
        resource.insert("service.name".into(), "taba".into());
        resource.extend(config.telemetry.resource.iter().cloned());
        // O nó define sozinho o ID da instância (novo a cada início) e a versão.
        resource.insert("service.instance.id".into(), node_id.to_owned());
        resource.insert("service.version".into(), env!("CARGO_PKG_VERSION").into());
        Self {
            registry: Registry::default(),
            prometheus_enabled: config.telemetry.prometheus_enabled,
            resource: resource.into_iter().collect(),
            topics: TopicsExport {
                mode: config.telemetry.topics_mode,
                group_by_tags: config.telemetry.topics_group_by_tags.clone(),
                max_topics: config
                    .telemetry
                    .topics_max_topics
                    .map_or(DEFAULT_MAX_TOPICS, |n| n as usize),
            },
            cpu: process::CpuMeter::default(),
        }
    }
}

/// Liga o log em JSON no nível de `telemetry.logs.level` (`info` enquanto o padrão
/// estiver a definir) e as camadas do OTLP (spans e logs), que só exportam depois de um
/// [`otlp::Pipeline::attach`]. Chamar de novo é inofensivo: vale a primeira chamada.
pub fn init_logging(level: Option<&str>) {
    let filter = EnvFilter::new(level.unwrap_or("info"));
    let hub = otlp::hub();
    let fmt = tracing_subscriber::fmt::layer()
        .json()
        .with_current_span(false)
        .with_filter(filter);
    let _ = SpanRegistry::default()
        .with(hub.trace_layer())
        .with(hub.log_layer())
        .with(fmt)
        .try_init();
}
