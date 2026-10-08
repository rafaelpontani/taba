//! Métricas, traces e logs (doc 11). Hoje há o log estruturado, o registro de métricas `taba.*`
//! com a saída no texto do Prometheus e os eventos (*log records* com `event.name`); o envio por
//! OTLP e os traces entram depois (etapa 12b de `21-historico-implementacao.md`).
//!
//! O módulo não depende do resto do nó: o `engine` usa o [`histogram::Histogram`], e quem lê o
//! estado do nó para preencher o registro é a camada HTTP (`http/metrics`).

pub mod histogram;
pub mod metrics;
pub mod process;
pub mod prometheus;

use std::collections::BTreeMap;

use tracing_subscriber::EnvFilter;

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
/// estiver a definir). Chamar de novo é inofensivo: vale a primeira chamada.
pub fn init_logging(level: Option<&str>) {
    let filter = EnvFilter::new(level.unwrap_or("info"));
    let _ = tracing_subscriber::fmt()
        .json()
        .with_env_filter(filter)
        .with_current_span(false)
        .try_init();
}
