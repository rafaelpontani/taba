//! The OTLP export (doc 11, "How to attach a collector"): metrics, traces and logs pushed to the
//! collector, configured by the standard `OTEL_*` variables (`config::Otlp`). Without
//! `OTEL_EXPORTER_OTLP_ENDPOINT` none of this exists.
//!
//! - **Traces and logs:** the OpenTelemetry SDK with the OTLP exporter. Spans are born in `tracing`
//!   and the `tracing-opentelemetry` layer carries them to the SDK; events become *log records*
//!   ([`logs::LogLayer`]) with the `trace_id` and `span_id` of the current span.
//! - **Metrics:** the same registry as `GET /metrics`, read every `OTEL_METRIC_EXPORT_INTERVAL`
//!   and sent as protobuf ([`metrics`]).
//!
//! The `tracing` layers (spans and logs) enter the global subscriber only once, in
//! [`crate::telemetry::init_logging`], because `tracing-opentelemetry` only finds its own context
//! if the layer is directly on the subscriber. What changes with each node is what they feed: the
//! [`Hub`] keeps the current sampler, span processor and logger, and [`Pipeline::attach`]
//! swaps them. Without an attached pipeline the sampler discards everything and the logs do not go out.

pub mod logs;
pub mod metrics;

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock, RwLock};
use std::time::Duration;

use opentelemetry::trace::{Link, SpanKind, TraceId, TracerProvider};
use opentelemetry::{Context, KeyValue};
use opentelemetry_otlp::{
    LogExporter, Protocol, SpanExporter, WithExportConfig, WithHttpConfig, WithTonicConfig,
};
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::error::OTelSdkResult;
use opentelemetry_sdk::logs::{SdkLogger, SdkLoggerProvider};
use opentelemetry_sdk::trace::{
    BatchSpanProcessor, Sampler as SdkSampler, SamplingResult, SdkTracer, SdkTracerProvider,
    ShouldSample, Span, SpanData, SpanProcessor,
};
use tracing::level_filters::LevelFilter;
use tracing_opentelemetry::OpenTelemetryLayer;

use crate::config::{Otlp, OtlpProtocol, Sampler};

/// The instrumentation scope of the spans and logs.
pub const SCOPE: &str = "taba";

/// The current sampler: the attached pipeline's, or `AlwaysOff` without one.
#[derive(Debug, Clone)]
struct SwitchSampler(Arc<RwLock<SdkSampler>>);

impl ShouldSample for SwitchSampler {
    fn should_sample(
        &self,
        parent_context: Option<&Context>,
        trace_id: TraceId,
        name: &str,
        span_kind: &SpanKind,
        attributes: &[KeyValue],
        links: &[Link],
    ) -> SamplingResult {
        self.0
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .should_sample(parent_context, trace_id, name, span_kind, attributes, links)
    }
}

/// The current span processor: the attached pipeline's batch, or none.
#[derive(Debug, Clone)]
struct SwitchProcessor(Arc<RwLock<Option<Arc<BatchSpanProcessor>>>>);

impl SwitchProcessor {
    fn current(&self) -> Option<Arc<BatchSpanProcessor>> {
        self.0.read().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

impl SpanProcessor for SwitchProcessor {
    fn on_start(&self, span: &mut Span, cx: &Context) {
        if let Some(inner) = self.current() {
            inner.on_start(span, cx);
        }
    }

    fn on_end(&self, span: SpanData) {
        if let Some(inner) = self.current() {
            inner.on_end(span);
        }
    }

    fn force_flush(&self) -> OTelSdkResult {
        self.current().map_or(Ok(()), |inner| inner.force_flush())
    }

    fn shutdown_with_timeout(&self, timeout: Duration) -> OTelSdkResult {
        self.current()
            .map_or(Ok(()), |inner| inner.shutdown_with_timeout(timeout))
    }

    fn set_resource(&mut self, _resource: &Resource) {}
}

/// The current logger for the logs.
pub struct LogSlot {
    pub logger: SdkLogger,
    pub max_level: LevelFilter,
}

/// What the global subscriber feeds: one per process.
pub struct Hub {
    provider: SdkTracerProvider,
    sampler: Arc<RwLock<SdkSampler>>,
    processor: Arc<RwLock<Option<Arc<BatchSpanProcessor>>>>,
    logs: Arc<RwLock<Option<LogSlot>>>,
    /// A pipeline is attached: only then the spans layer sees spans (without it, a span costs nothing).
    attached: AtomicBool,
}

/// The process's `Hub`, created on first use.
pub fn hub() -> &'static Hub {
    static HUB: OnceLock<Hub> = OnceLock::new();
    HUB.get_or_init(|| {
        let sampler = Arc::new(RwLock::new(SdkSampler::AlwaysOff));
        let processor = Arc::new(RwLock::new(None));
        let provider = SdkTracerProvider::builder()
            .with_sampler(SwitchSampler(sampler.clone()))
            .with_span_processor(SwitchProcessor(processor.clone()))
            .build();
        Hub {
            provider,
            sampler,
            processor,
            logs: Arc::new(RwLock::new(None)),
            attached: AtomicBool::new(false),
        }
    })
}

impl Hub {
    /// The spans layer (`tracing` → OpenTelemetry).
    pub fn trace_layer<S>(&self) -> OpenTelemetryLayer<S, SdkTracer>
    where
        S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
    {
        tracing_opentelemetry::layer()
            .with_tracer(self.provider.tracer(SCOPE))
            .with_location(false)
            .with_threads(false)
            .with_tracked_inactivity(false)
    }

    /// `true` while a pipeline is attached.
    pub fn is_attached(&self) -> bool {
        self.attached.load(Ordering::Relaxed)
    }

    /// The logs layer (`tracing` events → *log records*).
    pub fn log_layer(&self) -> logs::LogLayer {
        logs::LogLayer {
            slot: self.logs.clone(),
        }
    }
}

/// The trace and log providers of a node.
pub struct Pipeline {
    processor: Arc<BatchSpanProcessor>,
    logger_provider: SdkLoggerProvider,
    sampler: SdkSampler,
}

fn sampler(spec: Sampler) -> SdkSampler {
    let parent = |inner: SdkSampler| SdkSampler::ParentBased(Box::new(inner));
    match spec {
        Sampler::AlwaysOn => SdkSampler::AlwaysOn,
        Sampler::AlwaysOff => SdkSampler::AlwaysOff,
        Sampler::TraceIdRatio(r) => SdkSampler::TraceIdRatioBased(r),
        Sampler::ParentBasedAlwaysOn => parent(SdkSampler::AlwaysOn),
        Sampler::ParentBasedAlwaysOff => parent(SdkSampler::AlwaysOff),
        Sampler::ParentBasedTraceIdRatio(r) => parent(SdkSampler::TraceIdRatioBased(r)),
    }
}

fn http_headers(otlp: &Otlp) -> HashMap<String, String> {
    otlp.headers.iter().cloned().collect()
}

fn grpc_metadata(otlp: &Otlp) -> tonic::metadata::MetadataMap {
    let mut map = tonic::metadata::MetadataMap::new();
    for (key, value) in &otlp.headers {
        if let (Ok(key), Ok(value)) = (
            tonic::metadata::AsciiMetadataKey::from_bytes(key.to_ascii_lowercase().as_bytes()),
            tonic::metadata::AsciiMetadataValue::try_from(value.as_str()),
        ) {
            map.insert(key, value);
        }
    }
    map
}

fn grpc_tls(otlp: &Otlp) -> Result<Option<tonic::transport::ClientTlsConfig>, String> {
    if !otlp.endpoint.starts_with("https://") {
        return Ok(None);
    }
    let mut tls = tonic::transport::ClientTlsConfig::new().with_native_roots();
    if let Some(pem) = metrics::read_certificate(otlp)? {
        tls = tls.ca_certificate(tonic::transport::Certificate::from_pem(pem));
    }
    Ok(Some(tls))
}

/// The SDK's blocking HTTP client, with the collector's CA. Created on a separate thread: the blocking
/// client can neither be born nor die inside a tokio task.
fn http_client(otlp: &Otlp) -> Result<reqwest::blocking::Client, String> {
    let certificate = metrics::read_certificate(otlp)?;
    std::thread::spawn(move || {
        let mut builder = reqwest::blocking::Client::builder().timeout(Duration::from_secs(10));
        if let Some(pem) = certificate {
            let cert = reqwest::Certificate::from_pem(&pem)
                .map_err(|e| format!("OTEL_EXPORTER_OTLP_CERTIFICATE invalid: {e}"))?;
            builder = builder.add_root_certificate(cert);
        }
        builder.build().map_err(|e| e.to_string())
    })
    .join()
    .map_err(|_| "failed to create the OTLP HTTP client".to_owned())?
}

/// The collector's TLS uses `ring`; with the dependencies' `aws-lc` also on, `rustls` does not
/// choose by itself.
pub fn install_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

impl Pipeline {
    /// Creates the trace and log exporters. Call inside a tokio runtime (the gRPC
    /// channel opens the connection on the first send). It attaches nothing to `tracing`: that is [`Self::attach`].
    pub fn start(otlp: &Otlp, resource: &[(String, String)]) -> Result<Self, String> {
        install_crypto_provider();
        let resource = Resource::builder_empty()
            .with_attributes(
                resource
                    .iter()
                    .map(|(k, v)| KeyValue::new(k.clone(), v.clone())),
            )
            .build();
        let (span_exporter, log_exporter) = match otlp.protocol {
            OtlpProtocol::HttpProtobuf => {
                let client = http_client(otlp)?;
                let spans = SpanExporter::builder()
                    .with_http()
                    .with_protocol(Protocol::HttpBinary)
                    .with_endpoint(format!("{}/v1/traces", otlp.endpoint))
                    .with_headers(http_headers(otlp))
                    .with_http_client(client.clone())
                    .build();
                let logs = LogExporter::builder()
                    .with_http()
                    .with_protocol(Protocol::HttpBinary)
                    .with_endpoint(format!("{}/v1/logs", otlp.endpoint))
                    .with_headers(http_headers(otlp))
                    .with_http_client(client)
                    .build();
                (spans, logs)
            }
            OtlpProtocol::Grpc => {
                let tls = grpc_tls(otlp)?;
                let mut spans = SpanExporter::builder()
                    .with_tonic()
                    .with_endpoint(otlp.endpoint.clone())
                    .with_metadata(grpc_metadata(otlp));
                let mut logs = LogExporter::builder()
                    .with_tonic()
                    .with_endpoint(otlp.endpoint.clone())
                    .with_metadata(grpc_metadata(otlp));
                if let Some(tls) = tls {
                    spans = spans.with_tls_config(tls.clone());
                    logs = logs.with_tls_config(tls);
                }
                (spans.build(), logs.build())
            }
        };
        let mut processor =
            BatchSpanProcessor::builder(span_exporter.map_err(|e| e.to_string())?).build();
        processor.set_resource(&resource);
        let logger_provider = SdkLoggerProvider::builder()
            .with_resource(resource)
            .with_batch_exporter(log_exporter.map_err(|e| e.to_string())?)
            .build();
        Ok(Self {
            processor: Arc::new(processor),
            logger_provider,
            sampler: sampler(otlp.sampler),
        })
    }

    /// Starts feeding the global subscriber's layers (the last call counts): spans go
    /// to this pipeline, with its sampler, and the logs from `logs_level` up too.
    pub fn attach(&self, logs_level: Option<&str>) {
        let hub = hub();
        *hub.sampler.write().unwrap_or_else(|e| e.into_inner()) = self.sampler.clone();
        *hub.processor.write().unwrap_or_else(|e| e.into_inner()) = Some(self.processor.clone());
        *hub.logs.write().unwrap_or_else(|e| e.into_inner()) = Some(LogSlot {
            logger: opentelemetry::logs::LoggerProvider::logger(&self.logger_provider, SCOPE),
            max_level: logs_level
                .and_then(|level| level.parse().ok())
                .unwrap_or(LevelFilter::INFO),
        });
        hub.attached.store(true, Ordering::Relaxed);
    }

    /// Releases the layers, if they are still this pipeline's.
    pub fn detach(&self) {
        let hub = hub();
        let mut processor = hub.processor.write().unwrap_or_else(|e| e.into_inner());
        if processor
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, &self.processor))
        {
            *processor = None;
            hub.attached.store(false, Ordering::Relaxed);
            *hub.sampler.write().unwrap_or_else(|e| e.into_inner()) = SdkSampler::AlwaysOff;
            *hub.logs.write().unwrap_or_else(|e| e.into_inner()) = None;
        }
    }

    /// Sends right away what is in the queue (for the tests and for shutdown).
    pub fn flush(&self) {
        let _ = self.processor.force_flush();
        let _ = self.logger_provider.force_flush();
    }

    /// Releases the layers, empties the queue and shuts the providers down.
    pub fn shutdown(&self) {
        self.detach();
        let _ = self.processor.shutdown();
        let _ = self.logger_provider.shutdown();
    }
}

impl std::fmt::Debug for Pipeline {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Pipeline")
    }
}
