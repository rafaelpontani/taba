//! Sending the metrics over OTLP (doc 11, "How to attach a collector"). The OpenTelemetry SDK does not
//! let an outside registry build the metric data, so the node builds the OTLP request
//! (protobuf) from the same registry that feeds `GET /metrics` and sends it over `grpc` or
//! `http/protobuf`, according to `OTEL_EXPORTER_OTLP_PROTOCOL`.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use opentelemetry_proto::tonic::collector::metrics::v1::ExportMetricsServiceRequest;
use opentelemetry_proto::tonic::collector::metrics::v1::metrics_service_client::MetricsServiceClient;
use opentelemetry_proto::tonic::common::v1::{AnyValue, InstrumentationScope, KeyValue, any_value};
use opentelemetry_proto::tonic::metrics::v1::{
    AggregationTemporality, Gauge, Histogram, HistogramDataPoint, Metric, NumberDataPoint,
    ResourceMetrics, ScopeMetrics, Sum, metric, number_data_point,
};
use opentelemetry_proto::tonic::resource::v1::Resource;
use prost::Message;
use tonic::metadata::{AsciiMetadataKey, AsciiMetadataValue};
use tonic::transport::Channel;

use crate::config::{Otlp, OtlpProtocol};
use crate::telemetry::metrics::{Kind, Snapshot, Value};

/// The name of the instrumentation scope of the node's metrics.
pub const SCOPE: &str = "taba";

fn string_attribute(key: &str, value: &str) -> KeyValue {
    KeyValue {
        key: key.to_owned(),
        value: Some(AnyValue {
            value: Some(any_value::Value::StringValue(value.to_owned())),
        }),
        ..Default::default()
    }
}

fn nanos(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_nanos()).unwrap_or(u64::MAX))
}

/// Builds the request with all the registry's series. `started` is the process start (the start
/// of the accumulated period of counters and histograms) and `now` the instant of the read.
pub fn build_request(
    snapshot: &[Snapshot],
    resource: &[(String, String)],
    started: SystemTime,
    now: SystemTime,
) -> ExportMetricsServiceRequest {
    let (start, time) = (nanos(started), nanos(now));
    let metrics = snapshot
        .iter()
        .filter(|family| !family.series.is_empty())
        .map(|family| {
            let numbers = |kind_filter: fn(&Value) -> bool| -> Vec<NumberDataPoint> {
                family
                    .series
                    .iter()
                    .filter(|(_, value)| kind_filter(value))
                    .filter_map(|(labels, value)| match value {
                        Value::Number(n) => Some(NumberDataPoint {
                            attributes: labels
                                .iter()
                                .map(|(k, v)| string_attribute(k, v))
                                .collect(),
                            start_time_unix_nano: start,
                            time_unix_nano: time,
                            value: Some(number_data_point::Value::AsDouble(*n)),
                            ..Default::default()
                        }),
                        Value::Histogram(_) => None,
                    })
                    .collect()
            };
            let data = match family.def.kind {
                Kind::Counter => metric::Data::Sum(Sum {
                    data_points: numbers(|v| matches!(v, Value::Number(_))),
                    aggregation_temporality: AggregationTemporality::Cumulative as i32,
                    is_monotonic: true,
                }),
                Kind::UpDownCounter => metric::Data::Sum(Sum {
                    data_points: numbers(|v| matches!(v, Value::Number(_))),
                    aggregation_temporality: AggregationTemporality::Cumulative as i32,
                    is_monotonic: false,
                }),
                Kind::Gauge => metric::Data::Gauge(Gauge {
                    data_points: numbers(|v| matches!(v, Value::Number(_))),
                }),
                Kind::Histogram => metric::Data::Histogram(Histogram {
                    data_points: family
                        .series
                        .iter()
                        .filter_map(|(labels, value)| match value {
                            Value::Histogram(h) => Some(HistogramDataPoint {
                                attributes: labels
                                    .iter()
                                    .map(|(k, v)| string_attribute(k, v))
                                    .collect(),
                                start_time_unix_nano: start,
                                time_unix_nano: time,
                                count: h.count(),
                                sum: Some(h.sum()),
                                bucket_counts: h.buckets().to_vec(),
                                explicit_bounds: h.bounds().to_vec(),
                                ..Default::default()
                            }),
                            Value::Number(_) => None,
                        })
                        .collect(),
                    aggregation_temporality: AggregationTemporality::Cumulative as i32,
                }),
            };
            Metric {
                name: family.def.name.to_owned(),
                description: family.def.help.to_owned(),
                unit: family.def.unit.to_owned(),
                data: Some(data),
                ..Default::default()
            }
        })
        .collect();
    ExportMetricsServiceRequest {
        resource_metrics: vec![ResourceMetrics {
            resource: Some(Resource {
                attributes: resource
                    .iter()
                    .map(|(k, v)| string_attribute(k, v))
                    .collect(),
                ..Default::default()
            }),
            scope_metrics: vec![ScopeMetrics {
                scope: Some(InstrumentationScope {
                    name: SCOPE.to_owned(),
                    version: env!("CARGO_PKG_VERSION").to_owned(),
                    ..Default::default()
                }),
                metrics,
                ..Default::default()
            }],
            ..Default::default()
        }],
    }
}

/// The deadline of a send call (`OTEL_EXPORTER_OTLP_TIMEOUT` is not read yet).
const SEND_TIMEOUT: Duration = Duration::from_secs(10);

/// Whoever delivers the request to the collector.
pub enum Sender {
    Http {
        client: reqwest::Client,
        url: String,
        headers: Vec<(String, String)>,
    },
    Grpc {
        client: MetricsServiceClient<Channel>,
        headers: Vec<(AsciiMetadataKey, AsciiMetadataValue)>,
    },
}

/// Reads the collector's CA (`OTEL_EXPORTER_OTLP_CERTIFICATE`).
pub fn read_certificate(otlp: &Otlp) -> Result<Option<Vec<u8>>, String> {
    otlp.certificate
        .as_ref()
        .map(|path| {
            std::fs::read(path).map_err(|e| {
                format!(
                    "OTEL_EXPORTER_OTLP_CERTIFICATE: could not read `{}`: {e}",
                    path.display()
                )
            })
        })
        .transpose()
}

impl Sender {
    /// Prepares the client of the chosen protocol. Inside a tokio runtime: the gRPC connection is
    /// lazy and only opens on the first send.
    pub fn new(otlp: &Otlp) -> Result<Self, String> {
        super::install_crypto_provider();
        let certificate = read_certificate(otlp)?;
        match otlp.protocol {
            OtlpProtocol::HttpProtobuf => {
                let mut builder = reqwest::Client::builder().timeout(SEND_TIMEOUT);
                if let Some(pem) = &certificate {
                    let cert = reqwest::Certificate::from_pem(pem)
                        .map_err(|e| format!("OTEL_EXPORTER_OTLP_CERTIFICATE invalid: {e}"))?;
                    builder = builder.add_root_certificate(cert);
                }
                Ok(Self::Http {
                    client: builder.build().map_err(|e| e.to_string())?,
                    url: format!("{}/v1/metrics", otlp.endpoint),
                    headers: otlp.headers.clone(),
                })
            }
            OtlpProtocol::Grpc => {
                let mut endpoint = tonic::transport::Endpoint::from_shared(otlp.endpoint.clone())
                    .map_err(|e| format!("OTEL_EXPORTER_OTLP_ENDPOINT invalid: {e}"))?
                    .timeout(SEND_TIMEOUT);
                if otlp.endpoint.starts_with("https://") {
                    let mut tls = tonic::transport::ClientTlsConfig::new().with_native_roots();
                    if let Some(pem) = &certificate {
                        tls = tls.ca_certificate(tonic::transport::Certificate::from_pem(pem));
                    }
                    endpoint = endpoint.tls_config(tls).map_err(|e| e.to_string())?;
                }
                let headers = otlp
                    .headers
                    .iter()
                    .filter_map(|(k, v)| {
                        Some((
                            AsciiMetadataKey::from_bytes(k.to_ascii_lowercase().as_bytes()).ok()?,
                            AsciiMetadataValue::try_from(v.as_str()).ok()?,
                        ))
                    })
                    .collect();
                Ok(Self::Grpc {
                    client: MetricsServiceClient::new(endpoint.connect_lazy()),
                    headers,
                })
            }
        }
    }

    pub async fn send(&mut self, request: ExportMetricsServiceRequest) -> Result<(), String> {
        match self {
            Self::Http {
                client,
                url,
                headers,
            } => {
                let mut call = client
                    .post(url.as_str())
                    .header("content-type", "application/x-protobuf")
                    .body(request.encode_to_vec());
                for (key, value) in headers.iter() {
                    call = call.header(key.as_str(), value.as_str());
                }
                let response = call.send().await.map_err(|e| e.to_string())?;
                if response.status().is_success() {
                    Ok(())
                } else {
                    Err(format!("o coletor respondeu {}", response.status()))
                }
            }
            Self::Grpc { client, headers } => {
                let mut call = tonic::Request::new(request);
                for (key, value) in headers.iter() {
                    call.metadata_mut().insert(key.clone(), value.clone());
                }
                client
                    .export(call)
                    .await
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            }
        }
    }
}

#[cfg(test)]
mod tests;
