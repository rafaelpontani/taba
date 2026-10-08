//! A tradução entre as chamadas tipadas e a API HTTP do cluster (doc 06c): cada [`Request`] vira
//! um método, um caminho e um corpo, e volta. Quem serve a porta do cluster usa
//! [`Request::from_http`] e [`Response::into_http`]; o `Transport` HTTP (etapa 16) usa
//! [`Request::to_http`] e [`Request::parse_reply`]. Os testes conferem que as duas pontas se
//! entendem para todas as chamadas.

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use super::{
    AckRegisterRequest, ChannelBatch, Confirm, FifoPublished, ForwardRequest, ForwardResponse,
    IdleResponse, JoinRequest, LossAck, LossRecord, MessageRecord, MessageRegisterRequest,
    NodeRegisterRequest, NodeUnregisterRequest, OperationRef, RecoverResult, Request, Response,
    Step, StreamPage, StreamQuery, TakeRequest, TakeResponse, TokenRegisterRequest,
    TopicRegisterRequest, VisibilityRegisterRequest, VoteRequest, VoteResponse, parse_recover_body,
    recover_body,
};
use crate::cluster::members::MemberTable;
use crate::core::error::{ErrorCode, ErrorDetail, TabaError};

/// O cabeçalho interno com o IP do cliente já resolvido, só no repasse.
pub const CLIENT_IP_HEADER: &str = "x-taba-client-ip";

/// Uma chamada HTTP pronta para sair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpCall {
    pub method: String,
    /// O caminho com a query.
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

/// Uma resposta HTTP, como chegou ou vai sair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpReply {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

const JSON: &str = "application/json";
const NDJSON: &str = "application/x-ndjson";

fn json_headers() -> Vec<(String, String)> {
    vec![("content-type".to_owned(), JSON.to_owned())]
}

fn json_call<T: Serialize>(method: &str, path: String, body: &T) -> HttpCall {
    HttpCall {
        method: method.to_owned(),
        path,
        headers: json_headers(),
        body: serde_json::to_vec(body).unwrap_or_default(),
    }
}

fn get_call(path: String) -> HttpCall {
    HttpCall {
        method: "GET".to_owned(),
        path,
        headers: Vec::new(),
        body: Vec::new(),
    }
}

fn step_call<R: Serialize>(prefix: &str, step: &Step<R>) -> HttpCall {
    let path = format!("{prefix}/{}", step.phase());
    match step {
        Step::Register(body) => json_call("POST", path, body),
        Step::Commit(op) | Step::Rollback(op) => json_call("POST", path, op),
    }
}

fn stream_path(base: &str, query: &StreamQuery) -> String {
    let mut params = Vec::new();
    if let Some(from) = &query.from {
        params.push(format!("from={from}"));
    }
    if let Some(to) = &query.to {
        params.push(format!("to={to}"));
    }
    if let Some(limit) = query.limit {
        params.push(format!("limit={limit}"));
    }
    if params.is_empty() {
        base.to_owned()
    } else {
        format!("{base}?{}", params.join("&"))
    }
}

fn bad_body(message: String) -> TabaError {
    TabaError::new(ErrorCode::ValidationFailed, message)
}

/// `415` se o corpo não é `application/json`; o corpo vazio vale como `{}`.
fn parse_body<T: DeserializeOwned>(
    headers: &[(String, String)],
    body: &[u8],
) -> Result<T, TabaError> {
    let empty = body.iter().all(u8::is_ascii_whitespace);
    if !empty {
        let media = headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("content-type"))
            .and_then(|(_, value)| value.split(';').next())
            .map(|value| value.trim().to_ascii_lowercase());
        if media.as_deref() != Some(JSON) {
            return Err(TabaError::new(
                ErrorCode::MediaUnsupported,
                "Use `Content-Type: application/json`.",
            ));
        }
    }
    let source: &[u8] = if empty { b"{}" } else { body };
    serde_json::from_slice(source).map_err(|e| {
        let message = e.to_string();
        let field = message.split('`').nth(1).unwrap_or_default().to_owned();
        if message.starts_with("missing field") {
            TabaError::new(ErrorCode::ValidationRequired, "Faltam campos obrigatórios.")
                .with_detail(field, ErrorCode::ValidationRequired, "Campo obrigatório.")
        } else if message.starts_with("unknown field") {
            TabaError::new(
                ErrorCode::ValidationUnknownField,
                format!("O campo `{field}` não existe nesta rota."),
            )
            .with_detail(
                field,
                ErrorCode::ValidationUnknownField,
                "Campo desconhecido.",
            )
        } else {
            bad_body(format!("O corpo não é válido: {message}."))
        }
    })
}

fn parse_step<R: DeserializeOwned>(
    phase: &str,
    headers: &[(String, String)],
    body: &[u8],
) -> Result<Step<R>, TabaError> {
    Ok(match phase {
        "register" => Step::Register(parse_body(headers, body)?),
        "commit" => Step::Commit(parse_body(headers, body)?),
        "rollback" => Step::Rollback(parse_body(headers, body)?),
        _ => return Err(TabaError::route_not_found()),
    })
}

/// `replicate` da query do `recover` (`true` se ausente); qualquer outro parâmetro é recusado.
fn parse_replicate(query: &str) -> Result<bool, TabaError> {
    let mut replicate = true;
    for pair in query.split('&').filter(|p| !p.is_empty()) {
        match pair.split_once('=').unwrap_or((pair, "")) {
            ("replicate", "true") => replicate = true,
            ("replicate", "false") => replicate = false,
            ("replicate", _) => {
                return Err(TabaError::new(
                    ErrorCode::ValidationFailed,
                    "`replicate` deve ser `true` ou `false`.",
                )
                .with_detail(
                    "replicate",
                    ErrorCode::ValidationFailed,
                    "Esperado true ou false.",
                ));
            }
            (other, _) => {
                return Err(TabaError::new(
                    ErrorCode::ValidationUnknownField,
                    format!("O parâmetro `{other}` não existe nesta rota."),
                )
                .with_detail(
                    other,
                    ErrorCode::ValidationUnknownField,
                    "Parâmetro desconhecido.",
                ));
            }
        }
    }
    Ok(replicate)
}

/// `from`, `to` e `limit` da query de um stream; qualquer outro parâmetro é recusado.
fn parse_stream_query(query: &str) -> Result<StreamQuery, TabaError> {
    let mut out = StreamQuery::default();
    for pair in query.split('&').filter(|p| !p.is_empty()) {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        match key {
            "from" => out.from = Some(value.to_owned()),
            "to" => out.to = Some(value.to_owned()),
            "limit" => {
                out.limit = Some(value.parse().map_err(|_| {
                    TabaError::new(ErrorCode::ValidationFailed, "`limit` deve ser um inteiro.")
                        .with_detail("limit", ErrorCode::ValidationFailed, "Esperado um inteiro.")
                })?);
            }
            other => {
                return Err(TabaError::new(
                    ErrorCode::ValidationUnknownField,
                    format!("O parâmetro `{other}` não existe nesta rota."),
                )
                .with_detail(
                    other,
                    ErrorCode::ValidationUnknownField,
                    "Parâmetro desconhecido.",
                ));
            }
        }
    }
    Ok(out)
}

impl Request {
    /// A chamada HTTP desta requisição (doc 06c).
    pub fn to_http(&self) -> HttpCall {
        match self {
            Request::ListNodes => get_call("/cluster/nodes".to_owned()),
            Request::Join(body) => json_call("POST", "/cluster/nodes/join".to_owned(), body),
            Request::Register(body) => {
                json_call("POST", "/cluster/nodes/register".to_owned(), body)
            }
            Request::Unregister(body) => {
                json_call("POST", "/cluster/nodes/unregister".to_owned(), body)
            }
            Request::Commit(body) => json_call("POST", "/cluster/nodes/commit".to_owned(), body),
            Request::Rollback(body) => {
                json_call("POST", "/cluster/nodes/rollback".to_owned(), body)
            }
            Request::Topic(step) => step_call("/cluster/topics", step),
            Request::ListTopics(query) => get_call(stream_path("/cluster/topics", query)),
            Request::Token(step) => step_call("/cluster/tokens", step),
            Request::ListTokens(query) => get_call(stream_path("/cluster/tokens", query)),
            Request::Write { topic, id, step } => {
                step_call(&format!("/cluster/topics/{topic}/messages/{id}"), step)
            }
            Request::Visibility { topic, id, step } => step_call(
                &format!("/cluster/topics/{topic}/messages/{id}/visibility"),
                step,
            ),
            Request::Ack { topic, id, step } => {
                step_call(&format!("/cluster/topics/{topic}/messages/{id}/ack"), step)
            }
            Request::Idle { topic_id } => get_call(format!("/cluster/topics/{topic_id}/idle")),
            Request::Take { topic, request } => json_call(
                "POST",
                format!("/cluster/topics/{topic}/messages/take"),
                request,
            ),
            Request::Channel(batch) => json_call("POST", "/cluster/channel".to_owned(), batch),
            Request::Vote(vote) => json_call("POST", "/cluster/vote-remove".to_owned(), vote),
            Request::Status => get_call("/cluster/status".to_owned()),
            Request::BlockIds { block, query } => {
                get_call(stream_path(&format!("/cluster/blocks/{block}/ids"), query))
            }
            Request::BlockMessages { block, query } => get_call(stream_path(
                &format!("/cluster/blocks/{block}/messages"),
                query,
            )),
            Request::BlockMessage { block, id } => {
                get_call(format!("/cluster/blocks/{block}/messages/{id}"))
            }
            Request::MessageLoss(query) => get_call(stream_path("/cluster/message-loss", query)),
            Request::LossRegister(loss) => {
                json_call("POST", "/cluster/message-loss".to_owned(), loss)
            }
            Request::LossRemove { topic, id } => HttpCall {
                method: "DELETE".to_owned(),
                path: format!("/cluster/message-loss/{topic}/{id}"),
                headers: Vec::new(),
                body: Vec::new(),
            },
            Request::FifoPublish { topic, record } => json_call(
                "POST",
                format!("/cluster/topics/{topic}/fifo/publish"),
                record,
            ),
            Request::Recover { records, replicate } => HttpCall {
                method: "POST".to_owned(),
                path: if *replicate {
                    "/cluster/messages/recover".to_owned()
                } else {
                    "/cluster/messages/recover?replicate=false".to_owned()
                },
                headers: vec![("content-type".to_owned(), NDJSON.to_owned())],
                body: recover_body(records),
            },
            Request::Forward(forward) => {
                let mut headers = forward.headers.clone();
                if let Some(ip) = &forward.client_ip {
                    headers.push((CLIENT_IP_HEADER.to_owned(), ip.clone()));
                }
                HttpCall {
                    method: forward.method.clone(),
                    path: format!("/cluster/forward{}", forward.path_and_query),
                    headers,
                    body: forward.body.clone(),
                }
            }
        }
    }

    /// O inverso de [`to_http`](Self::to_http): lê uma chamada que chegou à porta do cluster.
    /// Caminho ou verbo que não existem: `404 route.not-found`; corpo inválido: `400`/`415`.
    pub fn from_http(
        method: &str,
        path_and_query: &str,
        headers: &[(String, String)],
        body: &[u8],
    ) -> Result<Request, TabaError> {
        let (path, query) = path_and_query
            .split_once('?')
            .unwrap_or((path_and_query, ""));
        let segments: Vec<&str> = path.trim_matches('/').split('/').collect();
        let not_found = TabaError::route_not_found;
        if segments.first() != Some(&"cluster") {
            return Err(not_found());
        }
        let is = |expected: &str| method.eq_ignore_ascii_case(expected);
        let post = || is("POST");
        match segments.as_slice() {
            ["cluster", "forward", rest @ ..] => {
                let mut forwarded = format!("/{}", rest.join("/"));
                if !query.is_empty() {
                    forwarded.push('?');
                    forwarded.push_str(query);
                }
                let client_ip = headers
                    .iter()
                    .find(|(name, _)| name.eq_ignore_ascii_case(CLIENT_IP_HEADER))
                    .map(|(_, value)| value.clone());
                Ok(Request::Forward(ForwardRequest {
                    method: method.to_ascii_uppercase(),
                    path_and_query: forwarded,
                    headers: headers
                        .iter()
                        .filter(|(name, _)| !name.eq_ignore_ascii_case(CLIENT_IP_HEADER))
                        .cloned()
                        .collect(),
                    body: body.to_vec(),
                    client_ip,
                }))
            }
            ["cluster", "nodes"] if is("GET") => Ok(Request::ListNodes),
            ["cluster", "nodes", "join"] if post() => {
                Ok(Request::Join(parse_body::<JoinRequest>(headers, body)?))
            }
            ["cluster", "nodes", "register"] if post() => {
                Ok(Request::Register(parse_body::<NodeRegisterRequest>(
                    headers, body,
                )?))
            }
            ["cluster", "nodes", "unregister"] if post() => {
                Ok(Request::Unregister(parse_body::<NodeUnregisterRequest>(
                    headers, body,
                )?))
            }
            ["cluster", "nodes", "commit"] if post() => {
                Ok(Request::Commit(parse_body::<OperationRef>(headers, body)?))
            }
            ["cluster", "nodes", "rollback"] if post() => {
                Ok(Request::Rollback(parse_body::<OperationRef>(
                    headers, body,
                )?))
            }
            ["cluster", "topics"] if is("GET") => {
                Ok(Request::ListTopics(parse_stream_query(query)?))
            }
            ["cluster", "topics", phase] if post() => {
                Ok(Request::Topic(parse_step::<TopicRegisterRequest>(
                    phase, headers, body,
                )?))
            }
            ["cluster", "tokens"] if is("GET") => {
                Ok(Request::ListTokens(parse_stream_query(query)?))
            }
            ["cluster", "tokens", phase] if post() => {
                Ok(Request::Token(parse_step::<TokenRegisterRequest>(
                    phase, headers, body,
                )?))
            }
            ["cluster", "topics", topic, "fifo", "publish"] if post() => Ok(Request::FifoPublish {
                topic: (*topic).to_owned(),
                record: parse_body::<MessageRecord>(headers, body)?,
            }),
            ["cluster", "topics", topic, "messages", "take"] if post() => Ok(Request::Take {
                topic: (*topic).to_owned(),
                request: parse_body::<TakeRequest>(headers, body)?,
            }),
            ["cluster", "topics", topic, "messages", id, phase] if post() => Ok(Request::Write {
                topic: (*topic).to_owned(),
                id: (*id).to_owned(),
                step: parse_step::<MessageRegisterRequest>(phase, headers, body)?,
            }),
            [
                "cluster",
                "topics",
                topic,
                "messages",
                id,
                "visibility",
                phase,
            ] if post() => Ok(Request::Visibility {
                topic: (*topic).to_owned(),
                id: (*id).to_owned(),
                step: parse_step::<VisibilityRegisterRequest>(phase, headers, body)?,
            }),
            ["cluster", "topics", topic, "messages", id, "ack", phase] if post() => {
                Ok(Request::Ack {
                    topic: (*topic).to_owned(),
                    id: (*id).to_owned(),
                    step: parse_step::<AckRegisterRequest>(phase, headers, body)?,
                })
            }
            ["cluster", "channel"] if post() => {
                Ok(Request::Channel(parse_body::<ChannelBatch>(headers, body)?))
            }
            ["cluster", "vote-remove"] if post() => {
                Ok(Request::Vote(parse_body::<VoteRequest>(headers, body)?))
            }
            ["cluster", "status"] if is("GET") => Ok(Request::Status),
            ["cluster", "topics", id, "idle"] if is("GET") => Ok(Request::Idle {
                topic_id: (*id).to_owned(),
            }),
            ["cluster", "blocks", block, "ids"] if is("GET") => Ok(Request::BlockIds {
                block: (*block).to_owned(),
                query: parse_stream_query(query)?,
            }),
            ["cluster", "blocks", block, "messages"] if is("GET") => Ok(Request::BlockMessages {
                block: (*block).to_owned(),
                query: parse_stream_query(query)?,
            }),
            ["cluster", "blocks", block, "messages", id] if is("GET") => {
                Ok(Request::BlockMessage {
                    block: (*block).to_owned(),
                    id: (*id).to_owned(),
                })
            }
            ["cluster", "message-loss"] if is("GET") => {
                Ok(Request::MessageLoss(parse_stream_query(query)?))
            }
            ["cluster", "message-loss"] if post() => {
                Ok(Request::LossRegister(parse_body::<LossRecord>(
                    headers, body,
                )?))
            }
            ["cluster", "message-loss", topic, id] if is("DELETE") => Ok(Request::LossRemove {
                topic: (*topic).to_owned(),
                id: (*id).to_owned(),
            }),
            ["cluster", "messages", "recover"] if post() => {
                let media = headers
                    .iter()
                    .find(|(name, _)| name.eq_ignore_ascii_case("content-type"))
                    .and_then(|(_, value)| value.split(';').next())
                    .map(|value| value.trim().to_ascii_lowercase());
                if media.as_deref() != Some(NDJSON) {
                    return Err(TabaError::new(
                        ErrorCode::MediaUnsupported,
                        "Use `Content-Type: application/x-ndjson`.",
                    ));
                }
                Ok(Request::Recover {
                    records: parse_recover_body(body)?,
                    replicate: parse_replicate(query)?,
                })
            }
            _ => Err(not_found()),
        }
    }

    /// Lê a resposta HTTP que veio a esta requisição. `4xx` e `5xx` viram o erro do par (exceto no
    /// repasse, em que qualquer status é a resposta do destino); o que não dá para ler vira
    /// `502 forward.bad-gateway`.
    pub fn parse_reply(&self, reply: HttpReply) -> Result<Response, TabaError> {
        if let Request::Forward(_) = self {
            return Ok(Response::Forwarded(ForwardResponse {
                status: reply.status,
                headers: reply.headers,
                body: reply.body,
            }));
        }
        if reply.status >= 400 {
            return Err(error_from_reply(&reply));
        }
        let unreadable = |what: &str| {
            TabaError::new(
                ErrorCode::ForwardBadGateway,
                format!("A resposta do nó não pôde ser lida: {what}."),
            )
        };
        let json = |reply: &HttpReply| -> Result<Value, TabaError> {
            serde_json::from_slice(&reply.body).map_err(|e| unreadable(&e.to_string()))
        };
        match self {
            Request::ListNodes | Request::Join(_) => {
                let table: MemberTable = serde_json::from_value(json(&reply)?)
                    .map_err(|e| unreadable(&e.to_string()))?;
                Ok(Response::Members(table))
            }
            Request::ListTopics(_)
            | Request::ListTokens(_)
            | Request::BlockIds { .. }
            | Request::BlockMessages { .. } => {
                let text = String::from_utf8(reply.body).map_err(|_| unreadable("não é UTF-8"))?;
                StreamPage::from_ndjson(&text).map(Response::Stream)
            }
            Request::Idle { .. } => {
                let response: IdleResponse =
                    serde_json::from_slice(&reply.body).map_err(|e| unreadable(&e.to_string()))?;
                Ok(Response::Idle(response))
            }
            Request::Take { .. } => {
                let response: TakeResponse =
                    serde_json::from_slice(&reply.body).map_err(|e| unreadable(&e.to_string()))?;
                Ok(Response::Taken(response))
            }
            Request::BlockMessage { .. } => {
                let record: MessageRecord =
                    serde_json::from_slice(&reply.body).map_err(|e| unreadable(&e.to_string()))?;
                Ok(Response::Record(record))
            }
            Request::Channel(_) => {
                let confirm: Confirm =
                    serde_json::from_slice(&reply.body).map_err(|e| unreadable(&e.to_string()))?;
                Ok(Response::Confirm(confirm))
            }
            Request::Vote(_) => {
                let vote: VoteResponse =
                    serde_json::from_slice(&reply.body).map_err(|e| unreadable(&e.to_string()))?;
                Ok(Response::Vote(vote))
            }
            Request::Status => Ok(Response::Status(json(&reply)?)),
            Request::MessageLoss(_) => {
                let text = String::from_utf8(reply.body).map_err(|_| unreadable("não é UTF-8"))?;
                StreamPage::from_ndjson(&text).map(Response::Stream)
            }
            Request::LossRegister(_) | Request::LossRemove { .. } => {
                let ack: LossAck =
                    serde_json::from_slice(&reply.body).map_err(|e| unreadable(&e.to_string()))?;
                Ok(Response::Loss(ack))
            }
            Request::FifoPublish { .. } => {
                let published: FifoPublished =
                    serde_json::from_slice(&reply.body).map_err(|e| unreadable(&e.to_string()))?;
                Ok(Response::FifoPublished(published))
            }
            Request::Recover { .. } => {
                let result: RecoverResult =
                    serde_json::from_slice(&reply.body).map_err(|e| unreadable(&e.to_string()))?;
                Ok(Response::Recovered(result))
            }
            Request::Forward(_) => unreachable!("tratado acima"),
            _ => {
                let ack =
                    serde_json::from_slice(&reply.body).map_err(|e| unreadable(&e.to_string()))?;
                Ok(Response::Ack(ack))
            }
        }
    }
}

/// O erro de uma resposta `4xx`/`5xx`: o `ErrorResponse` do corpo e o `Retry-After`.
fn error_from_reply(reply: &HttpReply) -> TabaError {
    let value: Value = serde_json::from_slice(&reply.body).unwrap_or(Value::Null);
    let code = value
        .get("code")
        .and_then(Value::as_str)
        .and_then(ErrorCode::parse)
        .unwrap_or(ErrorCode::InternalError);
    let message = value
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("O nó respondeu com um erro.")
        .to_owned();
    let mut error = TabaError::new(code, message);
    if let Some(details) = value.get("details").and_then(Value::as_array) {
        for detail in details {
            let field = detail
                .get("field")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let detail_code = detail
                .get("code")
                .and_then(Value::as_str)
                .and_then(ErrorCode::parse)
                .unwrap_or(code);
            let text = detail
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or_default();
            error.details.push(ErrorDetail {
                field: field.to_owned(),
                code: detail_code,
                message: text.to_owned(),
            });
        }
    }
    error.retry_after = reply
        .headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("retry-after"))
        .and_then(|(_, value)| value.trim().parse().ok());
    error
}

/// O `ErrorResponse` de um erro, com o `Retry-After`.
pub fn error_reply(error: &TabaError, request_id: &str) -> HttpReply {
    let mut headers = json_headers();
    if let Some(seconds) = error.retry_after {
        headers.push(("retry-after".to_owned(), seconds.to_string()));
    }
    HttpReply {
        status: error.status(),
        headers,
        body: serde_json::to_vec(&error.to_response(request_id)).unwrap_or_default(),
    }
}

impl Response {
    /// A resposta HTTP `200` (ou o status do destino, no repasse).
    pub fn into_http(self) -> HttpReply {
        let json = |value: &dyn erased::Json| HttpReply {
            status: 200,
            headers: json_headers(),
            body: value.to_bytes(),
        };
        match self {
            Response::Members(table) => json(&table),
            Response::Ack(ack) => json(&ack),
            Response::Taken(taken) => json(&taken),
            Response::Idle(idle) => json(&idle),
            Response::Record(record) => json(&record),
            Response::Confirm(confirm) => json(&confirm),
            Response::Vote(vote) => json(&vote),
            Response::Loss(ack) => json(&ack),
            Response::FifoPublished(published) => json(&published),
            Response::Recovered(result) => json(&result),
            Response::Status(status) => json(&status),
            Response::Stream(page) => HttpReply {
                status: 200,
                headers: vec![("content-type".to_owned(), NDJSON.to_owned())],
                body: page.to_ndjson().into_bytes(),
            },
            Response::Forwarded(forwarded) => HttpReply {
                status: forwarded.status,
                headers: forwarded.headers,
                body: forwarded.body,
            },
        }
    }
}

/// Serializar sem nomear o tipo.
mod erased {
    pub trait Json {
        fn to_bytes(&self) -> Vec<u8>;
    }

    impl<T: serde::Serialize> Json for T {
        fn to_bytes(&self) -> Vec<u8> {
            serde_json::to_vec(self).unwrap_or_default()
        }
    }
}
