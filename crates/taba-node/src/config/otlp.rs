//! The OTLP export, read from the OpenTelemetry SDK's standard variables (`OTEL_*`), which have
//! no key in the YAML or the `TABA_` prefix (doc 10, section 8.1). Without `OTEL_EXPORTER_OTLP_ENDPOINT` the
//! export is off.

use std::path::PathBuf;
use std::time::Duration;

use super::source::EnvSource;
use super::{ConfigError, parse_duration};

/// The default metrics send interval, the SDK's (`OTEL_METRIC_EXPORT_INTERVAL`, 60 s).
pub const DEFAULT_METRIC_INTERVAL: Duration = Duration::from_secs(60);

/// The fraction of traces kept while doc 10 leaves the default "to be defined" (D117, provisional).
pub const DEFAULT_SAMPLER_RATIO: f64 = 0.1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OtlpProtocol {
    Grpc,
    HttpProtobuf,
}

/// The SDK's samplers (`OTEL_TRACES_SAMPLER`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Sampler {
    AlwaysOn,
    AlwaysOff,
    TraceIdRatio(f64),
    ParentBasedAlwaysOn,
    ParentBasedAlwaysOff,
    ParentBasedTraceIdRatio(f64),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Otlp {
    /// `OTEL_EXPORTER_OTLP_ENDPOINT`, without the trailing slash.
    pub endpoint: String,
    pub protocol: OtlpProtocol,
    /// `OTEL_EXPORTER_OTLP_HEADERS` (`key=value,key=value`).
    pub headers: Vec<(String, String)>,
    /// `OTEL_EXPORTER_OTLP_CERTIFICATE`: the CA (PEM) that signs the collector's certificate.
    pub certificate: Option<PathBuf>,
    /// `OTEL_METRIC_EXPORT_INTERVAL` (milissegundos).
    pub metric_interval: Duration,
    pub sampler: Sampler,
}

fn non_empty(env: &dyn EnvSource, name: &str) -> Option<String> {
    env.get(name)
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty())
}

fn headers(text: &str) -> Vec<(String, String)> {
    text.split(',')
        .filter_map(|pair| pair.split_once('='))
        .filter(|(key, _)| !key.trim().is_empty())
        .map(|(key, value)| (key.trim().to_owned(), value.trim().to_owned()))
        .collect()
}

fn sampler(env: &dyn EnvSource, problems: &mut Vec<String>) -> Sampler {
    let name = non_empty(env, "OTEL_TRACES_SAMPLER")
        .unwrap_or_else(|| "parentbased_traceidratio".to_owned());
    let ratio = match non_empty(env, "OTEL_TRACES_SAMPLER_ARG") {
        None => DEFAULT_SAMPLER_RATIO,
        Some(text) => match text.parse::<f64>() {
            Ok(r) if (0.0..=1.0).contains(&r) => r,
            _ => {
                problems.push(format!(
                    "OTEL_TRACES_SAMPLER_ARG (`{text}`) must be a number from 0 to 1"
                ));
                DEFAULT_SAMPLER_RATIO
            }
        },
    };
    match name.to_ascii_lowercase().as_str() {
        "always_on" => Sampler::AlwaysOn,
        "always_off" => Sampler::AlwaysOff,
        "traceidratio" => Sampler::TraceIdRatio(ratio),
        "parentbased_always_on" => Sampler::ParentBasedAlwaysOn,
        "parentbased_always_off" => Sampler::ParentBasedAlwaysOff,
        "parentbased_traceidratio" => Sampler::ParentBasedTraceIdRatio(ratio),
        other => {
            problems.push(format!(
                "OTEL_TRACES_SAMPLER (`{other}`) must be always_on, always_off, traceidratio, \
                 parentbased_always_on, parentbased_always_off or parentbased_traceidratio"
            ));
            Sampler::ParentBasedTraceIdRatio(ratio)
        }
    }
}

/// Reads the export's `OTEL_*` variables. `None` when there is no endpoint.
pub fn from_env(env: &dyn EnvSource) -> Result<Option<Otlp>, ConfigError> {
    let Some(endpoint) = non_empty(env, "OTEL_EXPORTER_OTLP_ENDPOINT") else {
        return Ok(None);
    };
    let mut problems = Vec::new();
    if !(endpoint.starts_with("http://") || endpoint.starts_with("https://")) {
        problems.push(format!(
            "OTEL_EXPORTER_OTLP_ENDPOINT (`{endpoint}`) must start with http:// or https://"
        ));
    }
    let protocol = match non_empty(env, "OTEL_EXPORTER_OTLP_PROTOCOL").as_deref() {
        None | Some("grpc") => OtlpProtocol::Grpc,
        Some("http/protobuf") => OtlpProtocol::HttpProtobuf,
        Some(other) => {
            problems.push(format!(
                "OTEL_EXPORTER_OTLP_PROTOCOL (`{other}`) must be grpc or http/protobuf"
            ));
            OtlpProtocol::Grpc
        }
    };
    let metric_interval = match non_empty(env, "OTEL_METRIC_EXPORT_INTERVAL") {
        None => DEFAULT_METRIC_INTERVAL,
        Some(text) => match text.parse::<u64>() {
            Ok(ms) if ms > 0 => Duration::from_millis(ms),
            // As with the other parameters, `5s` also works.
            _ => parse_duration(&text)
                .filter(|d| !d.is_zero())
                .unwrap_or_else(|| {
                    problems.push(format!(
                        "OTEL_METRIC_EXPORT_INTERVAL (`{text}`) must be a number of milliseconds"
                    ));
                    DEFAULT_METRIC_INTERVAL
                }),
        },
    };
    let sampler = sampler(env, &mut problems);
    if !problems.is_empty() {
        return Err(ConfigError::Validation(problems));
    }
    Ok(Some(Otlp {
        endpoint: endpoint.trim_end_matches('/').to_owned(),
        protocol,
        headers: non_empty(env, "OTEL_EXPORTER_OTLP_HEADERS")
            .map(|h| headers(&h))
            .unwrap_or_default(),
        certificate: non_empty(env, "OTEL_EXPORTER_OTLP_CERTIFICATE").map(PathBuf::from),
        metric_interval,
        sampler,
    }))
}
