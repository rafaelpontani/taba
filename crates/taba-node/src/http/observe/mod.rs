//! A medição das requisições e das conexões (doc 11, "API HTTP"): `http.server.request.duration`,
//! `http.server.active_requests`, os tamanhos de corpo e `taba.node.connections`, por porta
//! (`server.port`).
//!
//! As requisições passam por uma camada do roteador (depois do roteamento, então o `http.route` é o
//! **template** da rota casada, nunca o caminho real). As conexões são contadas por um serviço
//! por conexão, que o `serve` monta no lugar do `into_make_service_with_connect_info` do axum
//! (e entrega o mesmo `ConnectInfo<SocketAddr>`).

use std::convert::Infallible;
use std::future::{Ready, ready};
use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Instant;

use axum::Router;
use axum::body::HttpBody;
use axum::extract::connect_info::ConnectInfo;
use axum::extract::{MatchedPath, Request, State};
use axum::http::header;
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::future::RouteFuture;
use axum::serve::IncomingStream;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use tower_service::Service;

use tracing::Instrument;

use crate::telemetry::metrics::{
    HTTP_SERVER_ACTIVE, HTTP_SERVER_DURATION, HTTP_SERVER_REQUEST_BODY, HTTP_SERVER_RESPONSE_BODY,
    NODE_CONNECTIONS,
};
use crate::telemetry::{Telemetry, trace};

/// Onde se mede: o registro e a porta do listener (o atributo `server.port`).
#[derive(Clone)]
pub struct Observer {
    pub telemetry: Arc<Telemetry>,
    pub port: u16,
}

/// Os verbos que viram atributo como são; os outros viram `_OTHER` (cardinalidade limitada).
fn method_label(method: &str) -> &str {
    match method {
        "GET" | "HEAD" | "POST" | "PUT" | "PATCH" | "DELETE" | "OPTIONS" | "TRACE" | "CONNECT" => {
            method
        }
        _ => "_OTHER",
    }
}

/// As rotas que contam como operação de cliente para o trace (doc 11, "Traces").
fn is_client_operation(route: &str) -> bool {
    !["/health", "/metrics", "/cluster", "/node"]
        .iter()
        .any(|prefix| route.starts_with(prefix))
}

/// Subtrai uma requisição ativa ao sair, mesmo que o futuro seja descartado no meio.
struct ActiveRequest {
    observer: Observer,
    method: String,
}

impl ActiveRequest {
    fn start(observer: &Observer, method: &str) -> Self {
        let this = Self {
            observer: observer.clone(),
            method: method.to_owned(),
        };
        this.add(1.0);
        this
    }

    fn add(&self, delta: f64) {
        self.observer.telemetry.registry.add(
            &HTTP_SERVER_ACTIVE,
            &[
                ("http.request.method", &self.method),
                ("url.scheme", "http"),
                ("server.port", &self.observer.port.to_string()),
            ],
            delta,
        );
    }
}

impl Drop for ActiveRequest {
    fn drop(&mut self) {
        self.add(-1.0);
    }
}

async fn observe(State(observer): State<Observer>, request: Request, next: Next) -> Response {
    let method = method_label(request.method().as_str()).to_owned();
    let route = request
        .extensions()
        .get::<MatchedPath>()
        .map(|m| m.as_str().to_owned());
    let request_bytes = request
        .headers()
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());
    let _active = ActiveRequest::start(&observer, &method);
    let started = Instant::now();
    // Um trace por operação de cliente; as rotas de operação e as entre nós não geram trace.
    let span = route
        .as_deref()
        .filter(|route| is_client_operation(route))
        .map(|route| trace::server_span(&method, route, observer.port, request.headers()));

    let response = match &span {
        Some(span) => next.run(request).instrument(span.clone()).await,
        None => next.run(request).await,
    };
    if let Some(span) = &span {
        trace::finish_server_span(span, response.status().as_u16());
    }

    let status = response.status().as_u16().to_string();
    let port = observer.port.to_string();
    let mut labels: Vec<(&str, &str)> = vec![
        ("http.request.method", &method),
        ("http.response.status_code", &status),
        ("url.scheme", "http"),
        ("server.port", &port),
    ];
    if let Some(route) = &route {
        labels.push(("http.route", route));
    }
    if response.status().is_server_error() {
        labels.push(("error.type", &status));
    }
    let registry = &observer.telemetry.registry;
    registry.observe(
        &HTTP_SERVER_DURATION,
        &labels,
        started.elapsed().as_secs_f64(),
    );
    if let Some(bytes) = request_bytes {
        registry.observe(&HTTP_SERVER_REQUEST_BODY, &labels, bytes as f64);
    }
    if let Some(bytes) = response.body().size_hint().exact() {
        registry.observe(&HTTP_SERVER_RESPONSE_BODY, &labels, bytes as f64);
    }
    response
}

/// Conta uma conexão aberta enquanto o serviço dela existir.
struct ConnectionGuard {
    observer: Observer,
}

impl ConnectionGuard {
    fn open(observer: Observer) -> Arc<Self> {
        observer.telemetry.registry.add(
            &NODE_CONNECTIONS,
            &[("server.port", &observer.port.to_string())],
            1.0,
        );
        Arc::new(Self { observer })
    }
}

impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        self.observer.telemetry.registry.add(
            &NODE_CONNECTIONS,
            &[("server.port", &self.observer.port.to_string())],
            -1.0,
        );
    }
}

/// O serviço de uma conexão: o roteador mais o endereço de quem conectou (`ConnectInfo`).
#[derive(Clone)]
struct Connection {
    router: Router,
    peer: SocketAddr,
    _guard: Arc<ConnectionGuard>,
}

impl Service<Request> for Connection {
    type Response = Response;
    type Error = Infallible;
    type Future = RouteFuture<Infallible>;

    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Infallible>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, mut request: Request) -> Self::Future {
        request.extensions_mut().insert(ConnectInfo(self.peer));
        self.router.call(request)
    }
}

/// Monta o serviço de cada conexão que o listener aceita.
struct MakeConnection {
    router: Router,
    observer: Observer,
}

impl Service<IncomingStream<'_, TcpListener>> for MakeConnection {
    type Response = Connection;
    type Error = Infallible;
    type Future = Ready<Result<Connection, Infallible>>;

    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Infallible>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, stream: IncomingStream<'_, TcpListener>) -> Self::Future {
        ready(Ok(Connection {
            router: self.router.clone(),
            peer: *stream.remote_addr(),
            _guard: ConnectionGuard::open(self.observer.clone()),
        }))
    }
}

/// Quanto o handshake TLS de uma conexão pode levar antes de ela ser derrubada.
const HANDSHAKE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// Quanto a parada espera as conexões abertas terminarem o que estão fazendo.
const GRACEFUL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// Como `serve`, mas com TLS mútuo (a porta do cluster, doc 06 seção 8.0.1): o handshake exige o
/// certificado do outro lado, e o que falha nele nunca chega a uma rota. Quem chama é reconhecido
/// pelo IP da conexão (`ConnectInfo`), como na porta em HTTP puro.
pub async fn serve_tls(
    listener: TcpListener,
    router: Router,
    observer: Observer,
    cancel: CancellationToken,
    config: Arc<rustls::ServerConfig>,
) -> io::Result<()> {
    use hyper_util::rt::{TokioExecutor, TokioIo};
    use hyper_util::server::conn::auto::Builder;
    use hyper_util::server::graceful::GracefulShutdown;

    let router = router.layer(middleware::from_fn_with_state(observer.clone(), observe));
    let acceptor = tokio_rustls::TlsAcceptor::from(config);
    let graceful = GracefulShutdown::new();
    loop {
        let (stream, peer) = tokio::select! {
            () = cancel.cancelled() => break,
            accepted = listener.accept() => match accepted {
                Ok(accepted) => accepted,
                Err(e) => {
                    tracing::warn!(error = %e, "falha ao aceitar uma conexão do cluster");
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                    continue;
                }
            },
        };
        let connection = Connection {
            router: router.clone(),
            peer,
            _guard: ConnectionGuard::open(observer.clone()),
        };
        let acceptor = acceptor.clone();
        let watcher = graceful.watcher();
        tokio::spawn(async move {
            let handshake = tokio::time::timeout(HANDSHAKE_TIMEOUT, acceptor.accept(stream)).await;
            let tls = match handshake {
                Ok(Ok(tls)) => tls,
                Ok(Err(e)) => {
                    tracing::debug!(%peer, error = %e, "handshake TLS do cluster recusado");
                    return;
                }
                Err(_) => {
                    tracing::debug!(%peer, "handshake TLS do cluster não terminou no prazo");
                    return;
                }
            };
            let service = hyper::service::service_fn(
                move |request: hyper::Request<hyper::body::Incoming>| {
                    let mut connection = connection.clone();
                    async move { connection.call(request.map(axum::body::Body::new)).await }
                },
            );
            let builder = Builder::new(TokioExecutor::new());
            let served = builder.serve_connection_with_upgrades(TokioIo::new(tls), service);
            if let Err(e) = watcher.watch(served).await {
                tracing::debug!(%peer, error = %e, "conexão do cluster encerrada com erro");
            }
        });
    }
    let _ = tokio::time::timeout(GRACEFUL_TIMEOUT, graceful.shutdown()).await;
    Ok(())
}

/// Atende o listener com o roteador, medindo requisições e conexões, até `cancel`. As chamadas em
/// andamento terminam antes de a função voltar.
pub async fn serve(
    listener: TcpListener,
    router: Router,
    observer: Observer,
    cancel: CancellationToken,
) -> io::Result<()> {
    let router = router.layer(middleware::from_fn_with_state(observer.clone(), observe));
    axum::serve(listener, MakeConnection { router, observer })
        .with_graceful_shutdown(cancel.cancelled_owned())
        .await
}
