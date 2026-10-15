//! The translation between the typed calls and the cluster HTTP API (doc 06c): each [`Request`] becomes
//! a method, a path and a body, and back. Whoever serves the cluster port uses
//! [`Request::from_http`] and [`Response::into_http`]; the HTTP `Transport` (step 16) uses
//! [`Request::to_http`] and [`Request::parse_reply`]. The tests check that both ends
//! understand each other for every call.

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use super::{
    AckRegisterRequest, BackupSaved, DefinitionApply, DefinitionProposal, FifoPublished,
    ForwardRequest, ForwardResponse, IdleResponse, JoinRequest, LossAck, LossRecord, MessageRecord,
    MessageRegisterRequest, NodeRegisterRequest, NodeUnregisterRequest, OperationRef,
    RecoverResult, Request, Response, Step, StreamPage, StreamQuery, TakeRequest, TakeResponse,
    VisibilityRegisterRequest, VoteRequest, VoteResponse, parse_recover_body, recover_body,
};
use crate::cluster::members::MemberTable;
use crate::core::error::{ErrorCode, ErrorDetail, TabaError};

/// The internal header with the already-resolved client IP, only on the forward.
pub const CLIENT_IP_HEADER: &str = "x-taba-client-ip";

/// An HTTP call ready to go out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpCall {
    pub method: String,
    /// The path with the query.
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

/// An HTTP response, as it arrived or will go out.
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

/// Body of `POST /cluster/topics/{topic}/prune`.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PruneBody {
    prune_id: String,
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

/// `415` if the body is not `application/json`; an empty body counts as `{}`.
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
            TabaError::new(
                ErrorCode::ValidationRequired,
                "Required fields are missing.",
            )
            .with_detail(field, ErrorCode::ValidationRequired, "Required field.")
        } else if message.starts_with("unknown field") {
            TabaError::new(
                ErrorCode::ValidationUnknownField,
                format!("The field `{field}` does not exist on this route."),
            )
            .with_detail(field, ErrorCode::ValidationUnknownField, "Unknown field.")
        } else {
            bad_body(format!("The body is not valid: {message}."))
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

/// `replicate` from the `recover` query (`true` if absent); any other parameter is refused.
fn parse_replicate(query: &str) -> Result<bool, TabaError> {
    let mut replicate = true;
    for pair in query.split('&').filter(|p| !p.is_empty()) {
        match pair.split_once('=').unwrap_or((pair, "")) {
            ("replicate", "true") => replicate = true,
            ("replicate", "false") => replicate = false,
            ("replicate", _) => {
                return Err(TabaError::new(
                    ErrorCode::ValidationFailed,
                    "`replicate` must be `true` or `false`.",
                )
                .with_detail(
                    "replicate",
                    ErrorCode::ValidationFailed,
                    "Expected true or false.",
                ));
            }
            (other, _) => {
                return Err(TabaError::new(
                    ErrorCode::ValidationUnknownField,
                    format!("The parameter `{other}` does not exist on this route."),
                )
                .with_detail(
                    other,
                    ErrorCode::ValidationUnknownField,
                    "Unknown parameter.",
                ));
            }
        }
    }
    Ok(replicate)
}

/// `from`, `to` and `limit` of a stream's query; any other parameter is refused.
fn parse_stream_query(query: &str) -> Result<StreamQuery, TabaError> {
    let mut out = StreamQuery::default();
    for pair in query.split('&').filter(|p| !p.is_empty()) {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        match key {
            "from" => out.from = Some(value.to_owned()),
            "to" => out.to = Some(value.to_owned()),
            "limit" => {
                out.limit = Some(value.parse().map_err(|_| {
                    TabaError::new(ErrorCode::ValidationFailed, "`limit` must be an integer.")
                        .with_detail("limit", ErrorCode::ValidationFailed, "Expected an integer.")
                })?);
            }
            other => {
                return Err(TabaError::new(
                    ErrorCode::ValidationUnknownField,
                    format!("The parameter `{other}` does not exist on this route."),
                )
                .with_detail(
                    other,
                    ErrorCode::ValidationUnknownField,
                    "Unknown parameter.",
                ));
            }
        }
    }
    Ok(out)
}

impl Request {
    /// This request's HTTP call (doc 06c).
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
            Request::DefinitionPropose(body) => {
                json_call("POST", "/cluster/definitions/propose".to_owned(), body)
            }
            Request::DefinitionApply(body) => {
                json_call("POST", "/cluster/definitions/apply".to_owned(), body)
            }
            Request::ListTopics(query) => get_call(stream_path("/cluster/topics", query)),
            Request::ListTokens(query) => get_call(stream_path("/cluster/tokens", query)),
            Request::ListTombstones(query) => {
                get_call(stream_path("/cluster/definitions/tombstones", query))
            }
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
            Request::BackupFlush => HttpCall {
                method: "POST".to_owned(),
                path: "/cluster/backup/flush".to_owned(),
                headers: Vec::new(),
                body: Vec::new(),
            },
            Request::BackupStop => HttpCall {
                method: "POST".to_owned(),
                path: "/cluster/backup/stop".to_owned(),
                headers: Vec::new(),
                body: Vec::new(),
            },
            Request::BackupSaved(body) => {
                json_call("POST", "/cluster/backup/saved".to_owned(), body)
            }
            Request::Prune { topic, prune_id } => json_call(
                "POST",
                format!("/cluster/topics/{topic}/prune"),
                &serde_json::json!({ "pruneId": prune_id }),
            ),
            Request::TopicStats => get_call("/cluster/stats/topics".to_owned()),
            Request::TopicGroups { topic } => {
                get_call(format!("/cluster/stats/topics/{topic}/groups"))
            }
            Request::BackupStatus => get_call("/cluster/backup/status".to_owned()),
            Request::NodeCall(call) => {
                let mut headers = call.headers.clone();
                if let Some(ip) = &call.client_ip {
                    headers.push((CLIENT_IP_HEADER.to_owned(), ip.clone()));
                }
                HttpCall {
                    method: call.method.clone(),
                    path: format!("/cluster/node-call{}", call.path_and_query),
                    headers,
                    body: call.body.clone(),
                }
            }
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

    /// The inverse of [`to_http`](Self::to_http): reads a call that arrived on the cluster port.
    /// A path or verb that does not exist: `404 route.not-found`; invalid body: `400`/`415`.
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
            ["cluster", "node-call", rest @ ..] => {
                let mut called = format!("/{}", rest.join("/"));
                if !query.is_empty() {
                    called.push('?');
                    called.push_str(query);
                }
                Ok(Request::NodeCall(ForwardRequest {
                    method: method.to_ascii_uppercase(),
                    path_and_query: called,
                    headers: headers.to_vec(),
                    body: body.to_vec(),
                    client_ip: None,
                }))
            }
            ["cluster", "stats", "topics"] if is("GET") => Ok(Request::TopicStats),
            ["cluster", "stats", "topics", topic, "groups"] if is("GET") => {
                Ok(Request::TopicGroups {
                    topic: (*topic).to_owned(),
                })
            }
            ["cluster", "backup", "status"] if is("GET") => Ok(Request::BackupStatus),
            ["cluster", "topics", topic, "prune"] if post() => {
                let body: PruneBody = parse_body(headers, body)?;
                Ok(Request::Prune {
                    topic: (*topic).to_owned(),
                    prune_id: body.prune_id,
                })
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
            ["cluster", "tokens"] if is("GET") => {
                Ok(Request::ListTokens(parse_stream_query(query)?))
            }
            ["cluster", "definitions", "tombstones"] if is("GET") => {
                Ok(Request::ListTombstones(parse_stream_query(query)?))
            }
            ["cluster", "definitions", "propose"] if post() => Ok(Request::DefinitionPropose(
                parse_body::<DefinitionProposal>(headers, body)?,
            )),
            ["cluster", "definitions", "apply"] if post() => Ok(Request::DefinitionApply(
                parse_body::<DefinitionApply>(headers, body)?,
            )),
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
            ["cluster", "backup", "flush"] if post() => Ok(Request::BackupFlush),
            ["cluster", "backup", "stop"] if post() => Ok(Request::BackupStop),
            ["cluster", "backup", "saved"] if post() => {
                Ok(Request::BackupSaved(parse_body::<BackupSaved>(
                    headers, body,
                )?))
            }
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

    /// Reads the HTTP response that came back to this request. `4xx` and `5xx` become the peer's error (except in the
    /// forward, where any status is the destination's response); what cannot be read becomes
    /// `502 forward.bad-gateway`.
    pub fn parse_reply(&self, reply: HttpReply) -> Result<Response, TabaError> {
        if let Request::Forward(_) | Request::NodeCall(_) = self {
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
                format!("The node's response could not be read: {what}."),
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
            | Request::ListTombstones(_)
            | Request::BlockIds { .. }
            | Request::BlockMessages { .. } => {
                let text = String::from_utf8(reply.body).map_err(|_| unreadable("not UTF-8"))?;
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
            Request::Vote(_) => {
                let vote: VoteResponse =
                    serde_json::from_slice(&reply.body).map_err(|e| unreadable(&e.to_string()))?;
                Ok(Response::Vote(vote))
            }
            Request::Status => Ok(Response::Status(json(&reply)?)),
            Request::BackupFlush | Request::BackupStop | Request::BackupStatus => {
                Ok(Response::Backup(json(&reply)?))
            }
            Request::DefinitionPropose(_) | Request::DefinitionApply(_) => {
                let change: DefinitionApply =
                    serde_json::from_slice(&reply.body).map_err(|e| unreadable(&e.to_string()))?;
                Ok(Response::Definition(change))
            }
            Request::MessageLoss(_) => {
                let text = String::from_utf8(reply.body).map_err(|_| unreadable("not UTF-8"))?;
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
            Request::Forward(_) | Request::NodeCall(_) => unreachable!("handled above"),
            Request::Prune { .. } | Request::TopicStats | Request::TopicGroups { .. } => {
                Ok(Response::Json(json(&reply)?))
            }
            _ => {
                let ack =
                    serde_json::from_slice(&reply.body).map_err(|e| unreadable(&e.to_string()))?;
                Ok(Response::Ack(ack))
            }
        }
    }
}

/// The error of a `4xx`/`5xx` response: the body's `ErrorResponse` and the `Retry-After`.
pub fn error_from_reply(reply: &HttpReply) -> TabaError {
    let value: Value = serde_json::from_slice(&reply.body).unwrap_or(Value::Null);
    let code = value
        .get("code")
        .and_then(Value::as_str)
        .and_then(ErrorCode::parse)
        .unwrap_or(ErrorCode::InternalError);
    let message = value
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("The node answered with an error.")
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

/// The `ErrorResponse` of an error, with the `Retry-After`.
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
    /// The `200` HTTP response (or the destination's status, in the forward).
    pub fn into_http(self) -> HttpReply {
        let json = |value: &dyn erased::Json| HttpReply {
            status: 200,
            headers: json_headers(),
            body: value.to_bytes(),
        };
        match self {
            Response::Members(table) => json(&table),
            Response::Ack(ack) => json(&ack),
            Response::Definition(change) => json(&change),
            Response::Taken(taken) => json(&taken),
            Response::Idle(idle) => json(&idle),
            Response::Record(record) => json(&record),
            Response::Vote(vote) => json(&vote),
            Response::Loss(ack) => json(&ack),
            Response::FifoPublished(published) => json(&published),
            Response::Recovered(result) => json(&result),
            Response::Status(status) => json(&status),
            Response::Backup(report) => json(&report),
            Response::Json(value) => json(&value),
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

/// Serialize without naming the type.
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
