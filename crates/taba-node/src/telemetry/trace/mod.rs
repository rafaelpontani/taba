//! Os spans do nó (doc 11, "Traces"). Nascem no `tracing`; a camada `tracing-opentelemetry`
//! (ligada por `telemetry::attach_otlp`) os leva ao OTLP. Sem a camada, criar um span custa quase
//! nada e não exporta nada.
//!
//! Os campos que os handlers preenchem depois (`messaging.*`) são declarados vazios na criação do
//! span HTTP, já que o `tracing` só deixa gravar campo declarado.

use axum::http::HeaderMap;
use opentelemetry::propagation::{Extractor, TextMapPropagator};
use opentelemetry_sdk::propagation::TraceContextPropagator;
use tracing::Span;
use tracing_opentelemetry::OpenTelemetrySpanExt;

/// O valor de `messaging.system`.
pub const MESSAGING_SYSTEM: &str = "taba";

/// O span da requisição HTTP de uma operação de cliente: nome `{método} {rota}`, atributos
/// `http.*` e, se a chamada trouxe um `traceparent`, filho dele (o W3C Trace Context; a decisão
/// de amostragem do pai vale com o amostrador `parentbased_*`).
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

/// Fecha o span da requisição: o status HTTP e, nas respostas `5xx`, o status de erro do span e a
/// marca de evento de durabilidade (o Collector as mantém na amostragem por cauda).
pub fn finish_server_span(span: &Span, status: u16) {
    span.record("http.response.status_code", i64::from(status));
    if status >= 500 {
        span.record("otel.status_code", "ERROR");
        span.record("taba.durability_event", true);
    }
}

/// Os atributos de mensageria do span corrente: o tópico, a operação (`send`, `receive` ou
/// `settle`) e as tags do tópico como `taba.topic.tag.<chave>`.
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

/// O ID da mensagem no span corrente (vai em traces e logs, nunca em métrica).
pub fn message_id(id: &str) {
    Span::current().record("messaging.message.id", id);
}

/// A quantidade de mensagens de um consumo, no span corrente.
pub fn batch_count(count: usize) {
    Span::current().record("messaging.batch.message_count", count as i64);
}

/// Marca o span corrente como evento de durabilidade (rollback, perda, teto de commit esgotado).
pub fn durability_event() {
    Span::current().record("taba.durability_event", true);
}

/// O span de uma chamada a um nó (registro, commit, rollback, voto...): `name` vira o nome do
/// span e `peer` o atributo `taba.peer.id`. O número da tentativa
/// (`taba.attempt`) é gravado por quem repete a chamada. `durability` marca o span como evento de
/// durabilidade (rollback), que o Collector mantém na amostragem por cauda.
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

/// Roda `call` dentro de `span` e marca o span com erro se ela falhar.
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

/// O número da tentativa no span corrente.
pub fn attempt(number: u32) {
    Span::current().record("taba.attempt", i64::from(number));
}

/// O span raiz de uma operação do cluster (ingresso, saída, votação): um trace por operação,
/// com um span filho por nó consultado. `node` é o nó que entra, sai ou é votado
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

/// Injeta o contexto do span corrente no cabeçalho `traceparent` de uma chamada a outro nó. Sem
/// span válido (uma tarefa de fundo, como o sinal de vida) não injeta nada.
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

/// O span de uma chamada que chegou de outro nó. Só nasce se a chamada trouxe um `traceparent`
/// (veio de uma operação com trace); as de fundo, como o sinal de vida, não geram span.
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
