//! Um coletor OTLP falso para os testes da exportação (doc 11, "Como ligar um coletor"): recebe
//! métricas, traces e logs por `http/protobuf` ou por `grpc` e guarda o que chegou.

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::post;
use opentelemetry_proto::tonic::collector::logs::v1::logs_service_server::{
    LogsService, LogsServiceServer,
};
use opentelemetry_proto::tonic::collector::logs::v1::{
    ExportLogsServiceRequest, ExportLogsServiceResponse,
};
use opentelemetry_proto::tonic::collector::metrics::v1::metrics_service_server::{
    MetricsService, MetricsServiceServer,
};
use opentelemetry_proto::tonic::collector::metrics::v1::{
    ExportMetricsServiceRequest, ExportMetricsServiceResponse,
};
use opentelemetry_proto::tonic::collector::trace::v1::trace_service_server::{
    TraceService, TraceServiceServer,
};
use opentelemetry_proto::tonic::collector::trace::v1::{
    ExportTraceServiceRequest, ExportTraceServiceResponse,
};
use opentelemetry_proto::tonic::common::v1::{AnyValue, KeyValue, any_value};
use opentelemetry_proto::tonic::logs::v1::LogRecord;
use opentelemetry_proto::tonic::metrics::v1::Metric;
use opentelemetry_proto::tonic::trace::v1::Span;
use prost::Message;
use tonic::{Request, Response, Status};

#[derive(Default)]
pub struct Captured {
    metrics: Mutex<Vec<ExportMetricsServiceRequest>>,
    traces: Mutex<Vec<ExportTraceServiceRequest>>,
    logs: Mutex<Vec<ExportLogsServiceRequest>>,
    /// Os cabeçalhos (ou metadados gRPC) de cada chamada, com o nome em minúsculas.
    headers: Mutex<Vec<BTreeMap<String, String>>>,
}

pub struct Collector {
    /// O endereço para `OTEL_EXPORTER_OTLP_ENDPOINT`.
    pub endpoint: String,
    pub captured: Arc<Captured>,
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

// ---- http/protobuf ----

async fn http_metrics(
    State(c): State<Arc<Captured>>,
    headers: HeaderMap,
    body: Bytes,
) -> StatusCode {
    record_headers(&c, &headers);
    match ExportMetricsServiceRequest::decode(body) {
        Ok(request) => {
            lock(&c.metrics).push(request);
            StatusCode::OK
        }
        Err(_) => StatusCode::BAD_REQUEST,
    }
}

async fn http_traces(
    State(c): State<Arc<Captured>>,
    headers: HeaderMap,
    body: Bytes,
) -> StatusCode {
    record_headers(&c, &headers);
    match ExportTraceServiceRequest::decode(body) {
        Ok(request) => {
            lock(&c.traces).push(request);
            StatusCode::OK
        }
        Err(_) => StatusCode::BAD_REQUEST,
    }
}

async fn http_logs(State(c): State<Arc<Captured>>, headers: HeaderMap, body: Bytes) -> StatusCode {
    record_headers(&c, &headers);
    match ExportLogsServiceRequest::decode(body) {
        Ok(request) => {
            lock(&c.logs).push(request);
            StatusCode::OK
        }
        Err(_) => StatusCode::BAD_REQUEST,
    }
}

fn record_headers(captured: &Captured, headers: &HeaderMap) {
    lock(&captured.headers).push(
        headers
            .iter()
            .filter_map(|(k, v)| Some((k.as_str().to_owned(), v.to_str().ok()?.to_owned())))
            .collect(),
    );
}

// ---- grpc ----

struct Grpc(Arc<Captured>);

fn record_metadata<T>(captured: &Captured, request: &Request<T>) {
    lock(&captured.headers).push(
        request
            .metadata()
            .iter()
            .filter_map(|entry| match entry {
                tonic::metadata::KeyAndValueRef::Ascii(k, v) => {
                    Some((k.as_str().to_owned(), v.to_str().ok()?.to_owned()))
                }
                tonic::metadata::KeyAndValueRef::Binary(..) => None,
            })
            .collect(),
    );
}

#[tonic::async_trait]
impl MetricsService for Grpc {
    async fn export(
        &self,
        request: Request<ExportMetricsServiceRequest>,
    ) -> Result<Response<ExportMetricsServiceResponse>, Status> {
        record_metadata(&self.0, &request);
        lock(&self.0.metrics).push(request.into_inner());
        Ok(Response::new(ExportMetricsServiceResponse::default()))
    }
}

#[tonic::async_trait]
impl TraceService for Grpc {
    async fn export(
        &self,
        request: Request<ExportTraceServiceRequest>,
    ) -> Result<Response<ExportTraceServiceResponse>, Status> {
        record_metadata(&self.0, &request);
        lock(&self.0.traces).push(request.into_inner());
        Ok(Response::new(ExportTraceServiceResponse::default()))
    }
}

#[tonic::async_trait]
impl LogsService for Grpc {
    async fn export(
        &self,
        request: Request<ExportLogsServiceRequest>,
    ) -> Result<Response<ExportLogsServiceResponse>, Status> {
        record_metadata(&self.0, &request);
        lock(&self.0.logs).push(request.into_inner());
        Ok(Response::new(ExportLogsServiceResponse::default()))
    }
}

impl Collector {
    /// Um coletor `http/protobuf` em `http://127.0.0.1:<porta>`.
    pub async fn http() -> Self {
        let captured = Arc::new(Captured::default());
        let router = Router::new()
            .route("/v1/metrics", post(http_metrics))
            .route("/v1/traces", post(http_traces))
            .route("/v1/logs", post(http_logs))
            .with_state(captured.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("porta livre");
        let addr = listener.local_addr().expect("endereço");
        tokio::spawn(async move {
            let _ = axum::serve(listener, router).await;
        });
        Self {
            endpoint: format!("http://{addr}"),
            captured,
        }
    }

    /// Um coletor `grpc`. Com `tls = Some((certificado, chave))` (PEM) atende em `https`.
    pub async fn grpc(tls: Option<(String, String)>) -> Self {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let captured = Arc::new(Captured::default());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("porta livre");
        let addr: SocketAddr = listener.local_addr().expect("endereço");
        let incoming = tonic::transport::server::TcpIncoming::from(listener);
        let mut server = tonic::transport::Server::builder();
        if let Some((cert, key)) = &tls {
            server = server
                .tls_config(tonic::transport::ServerTlsConfig::new().identity(
                    tonic::transport::Identity::from_pem(cert.as_bytes(), key.as_bytes()),
                ))
                .expect("TLS do coletor");
        }
        let routes = server
            .add_service(MetricsServiceServer::new(Grpc(captured.clone())))
            .add_service(TraceServiceServer::new(Grpc(captured.clone())))
            .add_service(LogsServiceServer::new(Grpc(captured.clone())));
        tokio::spawn(async move {
            let _ = routes.serve_with_incoming(incoming).await;
        });
        Self {
            endpoint: format!("{}://{addr}", if tls.is_some() { "https" } else { "http" }),
            captured,
        }
    }

    pub fn metric_requests(&self) -> Vec<ExportMetricsServiceRequest> {
        lock(&self.captured.metrics).clone()
    }

    /// Todas as métricas recebidas, a mais recente de cada nome por último.
    pub fn metrics(&self) -> Vec<Metric> {
        self.metric_requests()
            .into_iter()
            .flat_map(|r| r.resource_metrics)
            .flat_map(|r| r.scope_metrics)
            .flat_map(|s| s.metrics)
            .collect()
    }

    pub fn spans(&self) -> Vec<Span> {
        lock(&self.captured.traces)
            .iter()
            .flat_map(|r| r.resource_spans.iter())
            .flat_map(|r| r.scope_spans.iter())
            .flat_map(|s| s.spans.iter().cloned())
            .collect()
    }

    pub fn log_records(&self) -> Vec<LogRecord> {
        lock(&self.captured.logs)
            .iter()
            .flat_map(|r| r.resource_logs.iter())
            .flat_map(|r| r.scope_logs.iter())
            .flat_map(|s| s.log_records.iter().cloned())
            .collect()
    }

    /// Os atributos de recurso de cada sinal: métricas, traces e logs.
    pub fn resources(&self) -> [Vec<BTreeMap<String, String>>; 3] {
        let flatten = |attributes: &[KeyValue]| attributes_of(attributes);
        [
            self.metric_requests()
                .iter()
                .flat_map(|r| r.resource_metrics.iter())
                .filter_map(|r| r.resource.as_ref())
                .map(|r| flatten(&r.attributes))
                .collect(),
            lock(&self.captured.traces)
                .iter()
                .flat_map(|r| r.resource_spans.iter())
                .filter_map(|r| r.resource.as_ref())
                .map(|r| flatten(&r.attributes))
                .collect(),
            lock(&self.captured.logs)
                .iter()
                .flat_map(|r| r.resource_logs.iter())
                .filter_map(|r| r.resource.as_ref())
                .map(|r| flatten(&r.attributes))
                .collect(),
        ]
    }

    /// Os cabeçalhos de todas as chamadas recebidas.
    pub fn headers(&self) -> Vec<BTreeMap<String, String>> {
        lock(&self.captured.headers).clone()
    }

    /// Espera `condition` ficar verdadeira (até 20 s).
    pub async fn eventually(&self, what: &str, condition: impl Fn(&Self) -> bool) {
        for _ in 0..400 {
            if condition(self) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("o coletor não recebeu a tempo: {what}");
    }

    /// Espera um span com o nome dado (`otel.name`).
    pub async fn wait_span(&self, name: &str) -> Span {
        self.eventually(&format!("span {name}"), |c| c.span(name).is_some())
            .await;
        self.span(name).expect("span")
    }

    pub fn span(&self, name: &str) -> Option<Span> {
        self.spans().into_iter().find(|s| s.name == name)
    }
}

pub fn attributes_of(attributes: &[KeyValue]) -> BTreeMap<String, String> {
    attributes
        .iter()
        .filter_map(|kv| Some((kv.key.clone(), text_of(kv.value.as_ref()?)?)))
        .collect()
}

pub fn text_of(value: &AnyValue) -> Option<String> {
    match value.value.as_ref()? {
        any_value::Value::StringValue(s) => Some(s.clone()),
        any_value::Value::BoolValue(b) => Some(b.to_string()),
        any_value::Value::IntValue(i) => Some(i.to_string()),
        any_value::Value::DoubleValue(d) => Some(d.to_string()),
        _ => None,
    }
}

pub fn span_attributes(span: &Span) -> BTreeMap<String, String> {
    attributes_of(&span.attributes)
}

pub fn log_attributes(record: &LogRecord) -> BTreeMap<String, String> {
    attributes_of(&record.attributes)
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
