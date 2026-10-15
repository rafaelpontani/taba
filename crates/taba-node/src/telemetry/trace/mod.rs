//! The node's spans (doc 11, "Traces"). They are born in `tracing`; the `tracing-opentelemetry` layer
//! (attached by `telemetry::attach_otlp`) carries them to OTLP. Without the layer, creating a span costs almost
//! nothing and exports nothing.
//!
//! The fields the handlers fill in later (`messaging.*`) are declared empty at the creation of the
//! HTTP span, since `tracing` only lets a declared field be recorded.

use axum::http::HeaderMap;
use opentelemetry::propagation::{Extractor, TextMapPropagator};
use opentelemetry_sdk::propagation::TraceContextPropagator;
use tracing::Span;
use tracing_opentelemetry::OpenTelemetrySpanExt;

/// The value of `messaging.system`.
pub const MESSAGING_SYSTEM: &str = "taba";

/// The span of an HTTP request of a client operation: name `{method} {route}`, `http.*`
/// attributes and, if the call carried a `traceparent`, a child of it (the W3C Trace Context; the parent's
/// sampling decision applies with the `parentbased_*` sampler).
pub fn server_span(method: &str, route: &str, port: u16, headers: &HeaderMap) -> Span {
    let span = tracing::info_span!(
        "http.server",
        otel.name = %format!("{method} {route}"),
        otel.kind = "server",
        otel.status_code = tracing::field::Empty,
        http.request.method = method,
        http.route = route,
        server.port = i64::from(port),
        http.response.status_code = tracing::field::Empty,
        messaging.system = tracing::field::Empty,
        messaging.destination.name = tracing::field::Empty,
        messaging.operation.type = tracing::field::Empty,
        messaging.message.id = tracing::field::Empty,
        messaging.batch.message_count = tracing::field::Empty,
        taba.durability_event = tracing::field::Empty,
    );
    if headers.contains_key("traceparent") {
        let context = TraceContextPropagator::new().extract(&Headers(headers));
        let _ = span.set_parent(context);
    }
    span
}

struct Headers<'a>(&'a HeaderMap);

impl Extractor for Headers<'_> {
    fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).and_then(|v| v.to_str().ok())
    }

    fn keys(&self) -> Vec<&str> {
        self.0.keys().map(|k| k.as_str()).collect()
    }
}

/// Closes the request span: the HTTP status and, on `5xx` responses, the span's error status and the
/// durability event mark (the Collector keeps them in tail sampling).
pub fn finish_server_span(span: &Span, status: u16) {
    span.record("http.response.status_code", i64::from(status));
    if status >= 500 {
        span.record("otel.status_code", "ERROR");
        span.record("taba.durability_event", true);
    }
}

/// The messaging attributes of the current span: the topic, the operation (`send`, `receive` or
/// `settle`) and the topic's tags as `taba.topic.tag.<key>`.
pub fn annotate_messaging<'a>(
    topic: &str,
    operation: &'static str,
    tags: impl IntoIterator<Item = (&'a String, &'a String)>,
) {
    let span = Span::current();
    span.record("messaging.system", MESSAGING_SYSTEM);
    span.record("messaging.destination.name", topic);
    span.record("messaging.operation.type", operation);
    for (key, value) in tags {
        span.set_attribute(format!("taba.topic.tag.{key}"), value.clone());
    }
}

/// The message ID on the current span (goes in traces and logs, never in a metric).
pub fn message_id(id: &str) {
    Span::current().record("messaging.message.id", id);
}

/// The number of messages of a consume, on the current span.
pub fn batch_count(count: usize) {
    Span::current().record("messaging.batch.message_count", count as i64);
}

/// Marks the current span as a durability event (rollback, loss, commit ceiling exhausted).
pub fn durability_event() {
    Span::current().record("taba.durability_event", true);
}

/// The span of a call to a node (registration, commit, rollback, vote...): `name` becomes the name of the
/// span and `peer` the `taba.peer.id` attribute. The attempt number
/// (`taba.attempt`) is recorded by whoever repeats the call. `durability` marks the span as a durability
/// event (rollback), which the Collector keeps in tail sampling.
pub fn peer_span(name: &'static str, peer: &str, durability: bool) -> Span {
    let span = tracing::info_span!(
        "peer.call",
        otel.name = name,
        otel.kind = "client",
        otel.status_code = tracing::field::Empty,
        taba.peer.id = peer,
        taba.attempt = tracing::field::Empty,
        taba.durability_event = tracing::field::Empty,
    );
    if durability {
        span.record("taba.durability_event", true);
    }
    span
}

/// Runs `call` inside `span` and marks the span with an error if it fails.
pub async fn traced<T, E>(
    span: Span,
    call: impl std::future::Future<Output = Result<T, E>>,
) -> Result<T, E> {
    use tracing::Instrument;
    let result = call.instrument(span.clone()).await;
    if result.is_err() {
        span.record("otel.status_code", "ERROR");
    }
    result
}

/// The attempt number on the current span.
pub fn attempt(number: u32) {
    Span::current().record("taba.attempt", i64::from(number));
}

/// The root span of a cluster operation (join, leave, vote): one trace per operation,
/// with a child span per node queried. `node` is the node that joins, leaves or is voted on
/// (`taba.node.id`).
pub fn operation_span(name: &'static str, node: &str) -> Span {
    tracing::info_span!(
        "cluster.operation",
        otel.name = name,
        otel.status_code = tracing::field::Empty,
        taba.node.id = node,
        taba.durability_event = tracing::field::Empty,
    )
}

/// Injects the current span's context into the `traceparent` header of a call to another node. Without a
/// valid span (a background task, like the heartbeat) it injects nothing.
pub fn inject(headers: &mut Vec<(String, String)>) {
    use opentelemetry::propagation::{Injector, TextMapPropagator};
    use opentelemetry::trace::TraceContextExt;

    struct Out<'a>(&'a mut Vec<(String, String)>);
    impl Injector for Out<'_> {
        fn set(&mut self, key: &str, value: String) {
            self.0.retain(|(k, _)| k != key);
            self.0.push((key.to_owned(), value));
        }
    }
    let context = Span::current().context();
    if context.span().span_context().is_valid() {
        TraceContextPropagator::new().inject_context(&context, &mut Out(headers));
    }
}

/// The span of a call that arrived from another node. It is only born if the call carried a `traceparent`
/// (came from an operation with a trace); background ones, like the heartbeat, generate no span.
pub fn inbound_span(name: &str, headers: &[(String, String)]) -> Option<Span> {
    struct In<'a>(&'a [(String, String)]);
    impl Extractor for In<'_> {
        fn get(&self, key: &str) -> Option<&str> {
            self.0
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(key))
                .map(|(_, v)| v.as_str())
        }

        fn keys(&self) -> Vec<&str> {
            self.0.iter().map(|(k, _)| k.as_str()).collect()
        }
    }
    if !headers
        .iter()
        .any(|(k, _)| k.eq_ignore_ascii_case("traceparent"))
    {
        return None;
    }
    let span = tracing::info_span!(
        "cluster.inbound",
        otel.name = name,
        otel.kind = "server",
        otel.status_code = tracing::field::Empty,
    );
    let context = TraceContextPropagator::new().extract(&In(headers));
    let _ = span.set_parent(context);
    Some(span)
}
