//! A exportação OTLP, lida das variáveis padrão do SDK do OpenTelemetry (`OTEL_*`), que não têm
//! chave no YAML nem o prefixo `TABA_` (doc 10, seção 8.1). Sem `OTEL_EXPORTER_OTLP_ENDPOINT` a
//! exportação fica desligada.

use std::path::PathBuf;
use std::time::Duration;

use super::source::EnvSource;
use super::{ConfigError, parse_duration};

/// O padrão do intervalo de envio das métricas, o do SDK (`OTEL_METRIC_EXPORT_INTERVAL`, 60 s).
pub const DEFAULT_METRIC_INTERVAL: Duration = Duration::from_secs(60);

/// A fração de traces mantida enquanto o doc 10 deixa o padrão "a definir" (D117, provisório).
pub const DEFAULT_SAMPLER_RATIO: f64 = 0.1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OtlpProtocol {
    Grpc,
    HttpProtobuf,
}

/// Os amostradores do SDK (`OTEL_TRACES_SAMPLER`).
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
    /// `OTEL_EXPORTER_OTLP_ENDPOINT`, sem a barra final.
    pub endpoint: String,
    pub protocol: OtlpProtocol,
    /// `OTEL_EXPORTER_OTLP_HEADERS` (`chave=valor,chave=valor`).
    pub headers: Vec<(String, String)>,
    /// `OTEL_EXPORTER_OTLP_CERTIFICATE`: a CA (PEM) que assina o certificado do coletor.
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
                    "OTEL_TRACES_SAMPLER_ARG (`{text}`) deve ser um número de 0 a 1"
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
                "OTEL_TRACES_SAMPLER (`{other}`) deve ser always_on, always_off, traceidratio, \
                 parentbased_always_on, parentbased_always_off ou parentbased_traceidratio"
            ));
            Sampler::ParentBasedTraceIdRatio(ratio)
        }
    }
}

/// Lê as variáveis `OTEL_*` da exportação. `None` quando não há endpoint.
pub fn from_env(env: &dyn EnvSource) -> Result<Option<Otlp>, ConfigError> {
    let Some(endpoint) = non_empty(env, "OTEL_EXPORTER_OTLP_ENDPOINT") else {
        return Ok(None);
    };
    let mut problems = Vec::new();
    if !(endpoint.starts_with("http://") || endpoint.starts_with("https://")) {
        problems.push(format!(
            "OTEL_EXPORTER_OTLP_ENDPOINT (`{endpoint}`) deve começar com http:// ou https://"
        ));
    }
    let protocol = match non_empty(env, "OTEL_EXPORTER_OTLP_PROTOCOL").as_deref() {
        None | Some("grpc") => OtlpProtocol::Grpc,
        Some("http/protobuf") => OtlpProtocol::HttpProtobuf,
        Some(other) => {
            problems.push(format!(
                "OTEL_EXPORTER_OTLP_PROTOCOL (`{other}`) deve ser grpc ou http/protobuf"
            ));
            OtlpProtocol::Grpc
        }
    };
    let metric_interval = match non_empty(env, "OTEL_METRIC_EXPORT_INTERVAL") {
        None => DEFAULT_METRIC_INTERVAL,
        Some(text) => match text.parse::<u64>() {
            Ok(ms) if ms > 0 => Duration::from_millis(ms),
            // Como nos outros parâmetros, `5s` também vale.
            _ => parse_duration(&text)
                .filter(|d| !d.is_zero())
                .unwrap_or_else(|| {
                    problems.push(format!(
                        "OTEL_METRIC_EXPORT_INTERVAL (`{text}`) deve ser um número de milissegundos"
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
