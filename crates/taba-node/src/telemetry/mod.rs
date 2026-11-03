//! Metrics, traces and logs (doc 11). Structured logging, the `taba.*` metrics registry with output
//! in Prometheus text, the events (*log records* with `event.name`), the node's spans and the
//! OTLP export (metrics, traces and logs).
//!
//! The module does not depend on the rest of the node: the `engine` uses [`histogram::Histogram`], and whoever reads
//! the node state to fill the registry is the HTTP layer (`http/metrics`).

pub mod histogram;
pub mod metrics;
pub mod otlp;
pub mod process;
pub mod prometheus;
pub mod trace;

use std::collections::BTreeMap;
use tracing::Metadata;
use tracing::subscriber::Interest;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::filter::{FilterExt, filter_fn};
use tracing_subscriber::layer::{Context as FilterContext, Filter, Layer, SubscriberExt};
use tracing_subscriber::registry::Registry as SpanRegistry;
use tracing_subscriber::util::SubscriberInitExt;

use crate::config::{Config, LogFormat, TopicsMetricsMode};
use metrics::Registry;

/// The ceiling of series per topic in the `per-topic` mode while `telemetry.topics.max-topics` is still
/// to be defined (D91).
pub const DEFAULT_MAX_TOPICS: usize = 1000;

/// How the topic metrics enter `GET /metrics` (`telemetry.topics.*`).
#[derive(Debug, Clone)]
pub struct TopicsExport {
    pub mode: TopicsMetricsMode,
    pub group_by_tags: Vec<String>,
    pub max_topics: usize,
}

/// What the node collects and how it exposes it.
pub struct Telemetry {
    pub registry: Registry,
    /// `telemetry.prometheus.enabled`.
    pub prometheus_enabled: bool,
    /// The resource attributes: `service.name`, `service.instance.id`, `service.version`, and what
    /// came from `OTEL_RESOURCE_ATTRIBUTES`.
    pub resource: Vec<(String, String)>,
    pub topics: TopicsExport,
    pub cpu: process::CpuMeter,
}

impl Telemetry {
    pub fn from_config(config: &Config, node_id: &str) -> Self {
        let mut resource: BTreeMap<String, String> = BTreeMap::new();
        resource.insert("service.name".into(), "taba".into());
        resource.extend(config.telemetry.resource.iter().cloned());
        // The node defines the instance ID itself (new on every start) and the version.
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

/// The spans of [`trace`] (request, call to a node, cluster operation) exist for OTLP only: the log
/// lines do not carry them, so formatting their fields on every request is not paid.
fn is_node_span(metadata: &Metadata<'_>) -> bool {
    metadata.is_span() && metadata.target() == concat!(module_path!(), "::trace")
}

/// The spans layer only sees spans while an OTLP pipeline is attached. Without it, and with the log
/// lines refusing the node spans, a span is not even created ([`tracing::Span::none`]).
struct WhileAttached;

impl<S> Filter<S> for WhileAttached {
    fn enabled(&self, _: &Metadata<'_>, _: &FilterContext<'_, S>) -> bool {
        otlp::hub().is_attached()
    }

    fn callsite_enabled(&self, _: &'static Metadata<'static>) -> Interest {
        // Decided on each span, never cached: the pipeline is attached after the start.
        Interest::sometimes()
    }
}

/// Turns on logging in the format of `telemetry.logs.format` (plain text, colored text or JSON), at the
/// level of `telemetry.logs.level` (`info` while the default is still to be defined) and the OTLP
/// layers (spans and logs), which only export after an [`otlp::Pipeline::attach`]. Calling again is
/// harmless: the first call counts.
pub fn init_logging(level: Option<&str>, format: LogFormat) {
    let filter = EnvFilter::new(level.unwrap_or("info"));
    let hub = otlp::hub();
    let fmt = match format {
        LogFormat::Json => tracing_subscriber::fmt::layer()
            .json()
            .with_current_span(false)
            .boxed(),
        LogFormat::Text => tracing_subscriber::fmt::layer().with_ansi(false).boxed(),
        LogFormat::Color => tracing_subscriber::fmt::layer().with_ansi(true).boxed(),
    }
    .with_filter(filter.and(filter_fn(|metadata| !is_node_span(metadata))));
    let _ = SpanRegistry::default()
        .with(hub.trace_layer().with_filter(WhileAttached))
        .with(hub.log_layer())
        .with(fmt)
        .try_init();
}
