//! Measuring requests and connections (doc 11, "HTTP API"): `http.server.request.duration`,
//! `http.server.active_requests`, the body sizes and `taba.node.connections`, per port
//! (`server.port`).
//!
//! Requests go through a router layer (after routing, so `http.route` is the matched route's
//! **template**, never the real path). Connections are counted by a per-connection
//! service, which `serve` assembles in place of axum's `into_make_service_with_connect_info` (and
//! hands over the same `ConnectInfo<SocketAddr>`).

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

/// Where it is measured: the registry and the listener's port (the `server.port` attribute).
#[derive(Clone)]
pub struct Observer {
    pub telemetry: Arc<Telemetry>,
    pub port: u16,
    /// `port` as text, written once (the attribute goes on every request).
    port_label: Arc<str>,
}

impl Observer {
    pub fn new(telemetry: Arc<Telemetry>, port: u16) -> Self {
        Self {
            telemetry,
            port,
            port_label: port.to_string().into(),
        }
    }
}

/// The verbs that become an attribute as they are; the others become `_OTHER` (bounded cardinality).
fn method_label(method: &axum::http::Method) -> &'static str {
    use axum::http::Method;
    match *method {
        Method::GET => "GET",
        Method::HEAD => "HEAD",
        Method::POST => "POST",
        Method::PUT => "PUT",
        Method::PATCH => "PATCH",
        Method::DELETE => "DELETE",
        Method::OPTIONS => "OPTIONS",
        Method::TRACE => "TRACE",
        Method::CONNECT => "CONNECT",
        _ => "_OTHER",
    }
}

/// The routes that count as a client operation for the trace (doc 11, "Traces").
fn is_client_operation(route: &str) -> bool {
    !["/health", "/metrics", "/cluster", "/node"]
        .iter()
        .any(|prefix| route.starts_with(prefix))
}

/// Subtracts an active request on exit, even if the future is dropped in the middle.
struct ActiveRequest<'a> {
    observer: &'a Observer,
    method: &'static str,
}

impl<'a> ActiveRequest<'a> {
    fn start(observer: &'a Observer, method: &'static str) -> Self {
        let this = Self { observer, method };
        this.add(1.0);
        this
    }

    fn add(&self, delta: f64) {
        self.observer.telemetry.registry.add(
            &HTTP_SERVER_ACTIVE,
            &[
                ("http.request.method", self.method),
                ("url.scheme", "http"),
                ("server.port", &self.observer.port_label),
            ],
            delta,
        );
    }
}

impl Drop for ActiveRequest<'_> {
    fn drop(&mut self) {
        self.add(-1.0);
    }
}

async fn observe(State(observer): State<Observer>, request: Request, next: Next) -> Response {
    let method = method_label(request.method());
    // `MatchedPath` is an `Arc<str>`: cloning it does not copy the route.
    let route = request.extensions().get::<MatchedPath>().cloned();
    let request_bytes = request
        .headers()
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());
    let _active = ActiveRequest::start(&observer, method);
    let started = Instant::now();
    // One trace per client operation; the operations routes and the inter-node ones generate no trace.
    let span = route
        .as_ref()
        .map(MatchedPath::as_str)
        .filter(|route| is_client_operation(route))
        .map(|route| trace::server_span(method, route, observer.port, request.headers()));

    let response = match &span {
        Some(span) => next.run(request).instrument(span.clone()).await,
        None => next.run(request).await,
    };
    if let Some(span) = &span {
        trace::finish_server_span(span, response.status().as_u16());
    }

    let status_code = response.status();
    let status = status_code.as_str();
    let mut labels: Vec<(&str, &str)> = Vec::with_capacity(6);
    labels.extend([
        ("http.request.method", method),
        ("http.response.status_code", status),
        ("url.scheme", "http"),
        ("server.port", &observer.port_label),
    ]);
    if let Some(route) = &route {
        labels.push(("http.route", route.as_str()));
    }
    if response.status().is_server_error() {
        labels.push(("error.type", status));
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

/// Counts an open connection while its service exists.
struct ConnectionGuard {
    observer: Observer,
}

impl ConnectionGuard {
    fn open(observer: Observer) -> Arc<Self> {
        observer.telemetry.registry.add(
            &NODE_CONNECTIONS,
            &[("server.port", &observer.port_label)],
            1.0,
        );
        Arc::new(Self { observer })
    }
}

impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        self.observer.telemetry.registry.add(
            &NODE_CONNECTIONS,
            &[("server.port", &self.observer.port_label)],
            -1.0,
        );
    }
}

/// A connection's service: the router plus the address of whoever connected (`ConnectInfo`).
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

/// Builds the service of each connection the listener accepts.
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

/// How long a connection's TLS handshake can take before it is dropped.
const HANDSHAKE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// How long the stop waits for the open connections to finish what they are doing.
const GRACEFUL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// Like `serve`, but with mutual TLS (the cluster port, doc 06 section 8.0.1): the handshake requires the
/// certificate on the other side, and what fails in it never reaches a route. The caller is recognized
/// by the connection's IP (`ConnectInfo`), as on the plain HTTP port.
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
                    tracing::warn!(error = %e, "failed to accept a cluster connection");
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
                    tracing::debug!(%peer, error = %e, "cluster TLS handshake refused");
                    return;
                }
                Err(_) => {
                    tracing::debug!(%peer, "cluster TLS handshake did not finish in time");
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
                tracing::debug!(%peer, error = %e, "cluster connection ended with an error");
            }
        });
    }
    let _ = tokio::time::timeout(GRACEFUL_TIMEOUT, graceful.shutdown()).await;
    Ok(())
}

/// Serves the listener with the router, measuring requests and connections, until `cancel`. Calls in
/// progress finish before the function returns.
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
