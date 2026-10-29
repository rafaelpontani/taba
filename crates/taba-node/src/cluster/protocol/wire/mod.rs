//! The protobuf form of the calls between nodes (doc 18): the **batch** of message operations (`POST /cluster/batch`)
//! and its reply. `proto/taba-cluster.proto` is the contract; the structs below are the same schema written with
//! `prost`'s derive, so the build needs no `protoc`, and a golden-vector test keeps the two together.
//!
//! A batch is one message with a `repeated` field. Every item is an operation of a two-phase write (`register`,
//! `commit` or `rollback` of a publication, an invisibility or an ack) and has its own result in the reply, in the
//! same order: an error in one item (a `409`, a message that does not exist) does not touch the others, and an item
//! of a kind this node does not know becomes an error of that item, not of the batch.
//!
//! The wire types are twins of the domain ones (`MessageRegisterRequest`, `MessageRecord`...): those keep their
//! `serde` form, which the synchronization log still stores. The conversions move the values, they do not copy them,
//! and the body of a message is the only large field: decoding from a slice copies it once per message, which is
//! wanted (a `Bytes` cut from the batch would keep the whole batch alive for as long as one message stays in memory).

use bytes::Bytes;
use prost::Message;

use super::{
    Ack, AckRegisterRequest, MessageRecord, MessageRegisterRequest, OperationRef, Request, Step,
    VisibilityKind, VisibilityRegisterRequest, WriteKind,
};
use crate::core::error::{ErrorCode, ErrorDetail, TabaError};

/// The `Content-Type` of every call between nodes that carries a protobuf body.
pub const CONTENT_TYPE: &str = "application/x-protobuf";

/// An operation of a batch and, when the call that asked for it is being traced, the context of the trace, so the
/// node that serves it continues the same trace (the batch is sent by a task of its own, not by that call).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchItem {
    pub request: Request,
    /// The W3C `traceparent`; only while an OTLP pipeline is attached.
    pub traceparent: Option<String>,
}

impl BatchItem {
    /// An operation of the batch, with the trace context of the current span (if it has one).
    pub fn traced(request: Request) -> Self {
        Self {
            request,
            traceparent: crate::telemetry::trace::current_traceparent(),
        }
    }
}

/// What a batch carries when it is decoded: an item the node understood, or the error that item deserves.
pub type Entry = Result<BatchItem, TabaError>;

/// The reply to one item.
pub type Outcome = Result<Ack, TabaError>;

// ---- The schema ------------------------------------------------------------------------------------------------

#[derive(Clone, PartialEq, Message)]
pub struct BatchRequest {
    #[prost(message, repeated, tag = "1")]
    pub items: Vec<Item>,
}

#[derive(Clone, PartialEq, Message)]
pub struct BatchReply {
    #[prost(message, repeated, tag = "1")]
    pub results: Vec<ItemResult>,
}

/// Which family of operation an item belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, prost::Enumeration)]
#[repr(i32)]
pub enum Kind {
    Unspecified = 0,
    Write = 1,
    Visibility = 2,
    Ack = 3,
}

/// The three phases of a two-phase operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, prost::Enumeration)]
#[repr(i32)]
pub enum Phase {
    Unspecified = 0,
    Register = 1,
    Commit = 2,
    Rollback = 3,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, prost::Enumeration)]
#[repr(i32)]
pub enum WriteMode {
    Unspecified = 0,
    Publish = 1,
    DeadLetter = 2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, prost::Enumeration)]
#[repr(i32)]
pub enum VisibilityMode {
    Unspecified = 0,
    Delivery = 1,
    Change = 2,
}

#[derive(Clone, PartialEq, Message)]
pub struct Item {
    #[prost(enumeration = "Kind", tag = "1")]
    pub kind: i32,
    #[prost(enumeration = "Phase", tag = "2")]
    pub phase: i32,
    #[prost(string, tag = "3")]
    pub topic: String,
    /// The message the operation is about.
    #[prost(string, tag = "4")]
    pub id: String,
    #[prost(string, tag = "5")]
    pub operation_id: String,
    /// Only on `register`, and the one that matches the `kind`.
    #[prost(oneof = "item::Body", tags = "6, 7, 8")]
    pub body: Option<item::Body>,
    /// The trace context of the call that asked for the operation; only with OTLP attached.
    #[prost(string, optional, tag = "9")]
    pub traceparent: Option<String>,
}

pub mod item {
    #[derive(Clone, PartialEq, prost::Oneof)]
    pub enum Body {
        #[prost(message, tag = "6")]
        Write(Box<super::WriteRegister>),
        #[prost(message, tag = "7")]
        Visibility(super::VisibilityRegister),
        #[prost(message, tag = "8")]
        Ack(super::AckRegister),
    }
}

#[derive(Clone, PartialEq, Message)]
pub struct WriteRegister {
    #[prost(string, tag = "1")]
    pub coordinator_id: String,
    #[prost(enumeration = "WriteMode", tag = "2")]
    pub mode: i32,
    #[prost(message, optional, tag = "3")]
    pub message: Option<Record>,
    /// Only on `dead-letter`.
    #[prost(string, optional, tag = "4")]
    pub source_topic: Option<String>,
}

#[derive(Clone, PartialEq, Message)]
pub struct VisibilityRegister {
    #[prost(string, tag = "1")]
    pub coordinator_id: String,
    #[prost(enumeration = "VisibilityMode", tag = "2")]
    pub mode: i32,
    #[prost(uint64, tag = "3")]
    pub invisible_until_ms: u64,
    #[prost(string, tag = "4")]
    pub receipt_token: String,
    #[prost(uint32, optional, tag = "5")]
    pub delivery_count: Option<u32>,
}

#[derive(Clone, PartialEq, Message)]
pub struct AckRegister {
    #[prost(string, tag = "1")]
    pub coordinator_id: String,
    #[prost(string, tag = "2")]
    pub receipt_token: String,
    #[prost(uint64, optional, tag = "3")]
    pub delete_at_ms: Option<u64>,
    #[prost(bool, tag = "4")]
    pub discard: bool,
}

/// The message as the nodes exchange it (`MessageRecord`), with the body as bytes.
#[derive(Clone, PartialEq, Message)]
pub struct Record {
    #[prost(string, tag = "1")]
    pub id: String,
    #[prost(string, tag = "2")]
    pub topic: String,
    #[prost(bytes = "bytes", tag = "3")]
    pub body: Bytes,
    #[prost(string, tag = "4")]
    pub content_type: String,
    #[prost(string, optional, tag = "5")]
    pub group_id: Option<String>,
    #[prost(string, optional, tag = "6")]
    pub deduplication_id: Option<String>,
    #[prost(uint32, tag = "7")]
    pub delivery_count: u32,
    #[prost(uint64, optional, tag = "8")]
    pub invisible_until_ms: Option<u64>,
    #[prost(string, optional, tag = "9")]
    pub receipt_token: Option<String>,
    #[prost(uint64, optional, tag = "10")]
    pub dead_lettered_ms: Option<u64>,
    #[prost(string, optional, tag = "11")]
    pub dead_letter_reason: Option<String>,
    #[prost(uint64, optional, tag = "12")]
    pub delete_at_ms: Option<u64>,
}

#[derive(Clone, PartialEq, Message)]
pub struct ItemResult {
    /// The HTTP status the item would have had as a call of its own.
    #[prost(uint32, tag = "1")]
    pub status: u32,
    #[prost(string, tag = "2")]
    pub operation_id: String,
    #[prost(bool, tag = "3")]
    pub accepted: bool,
    #[prost(message, optional, tag = "4")]
    pub error: Option<ErrorBody>,
}

#[derive(Clone, PartialEq, Message)]
pub struct ErrorBody {
    #[prost(string, tag = "1")]
    pub code: String,
    #[prost(string, tag = "2")]
    pub message: String,
    #[prost(message, repeated, tag = "3")]
    pub details: Vec<ErrorItem>,
    #[prost(uint64, optional, tag = "4")]
    pub retry_after: Option<u64>,
    #[prost(string, optional, tag = "5")]
    pub required: Option<String>,
}

#[derive(Clone, PartialEq, Message)]
pub struct ErrorItem {
    #[prost(string, tag = "1")]
    pub field: String,
    #[prost(string, tag = "2")]
    pub code: String,
    #[prost(string, tag = "3")]
    pub message: String,
}

// ---- Domain → wire ---------------------------------------------------------------------------------------------

fn record_of(message: &MessageRecord) -> Record {
    Record {
        id: message.id.clone(),
        topic: message.topic.clone(),
        body: message.body.clone(),
        content_type: message.content_type.clone(),
        group_id: message.group_id.clone(),
        deduplication_id: message.deduplication_id.clone(),
        delivery_count: message.delivery_count,
        invisible_until_ms: message.invisible_until_ms,
        receipt_token: message.receipt_token.clone(),
        dead_lettered_ms: message.dead_lettered_ms,
        dead_letter_reason: message.dead_letter_reason.clone(),
        delete_at_ms: message.delete_at_ms,
    }
}

fn phase_of<R>(step: &Step<R>) -> Phase {
    match step {
        Step::Register(_) => Phase::Register,
        Step::Commit(_) => Phase::Commit,
        Step::Rollback(_) => Phase::Rollback,
    }
}

fn reference_of<R>(step: &Step<R>, operation_of: impl Fn(&R) -> &str) -> String {
    match step {
        Step::Register(register) => operation_of(register).to_owned(),
        Step::Commit(op) | Step::Rollback(op) => op.operation_id.clone(),
    }
}

/// The item for a request of a two-phase write; `None` for any other request.
fn item_of(entry: &BatchItem) -> Option<Item> {
    let mut item = item_without_trace(&entry.request)?;
    item.traceparent = entry.traceparent.clone();
    Some(item)
}

fn item_without_trace(request: &Request) -> Option<Item> {
    Some(match request {
        Request::Write { topic, id, step } => Item {
            kind: Kind::Write as i32,
            phase: phase_of(step) as i32,
            topic: topic.clone(),
            id: id.clone(),
            operation_id: reference_of(step, |r| &r.operation_id),
            body: match step {
                Step::Register(register) => Some(item::Body::Write(Box::new(WriteRegister {
                    coordinator_id: register.coordinator_id.clone(),
                    mode: match register.kind {
                        WriteKind::Publish => WriteMode::Publish,
                        WriteKind::DeadLetter => WriteMode::DeadLetter,
                    } as i32,
                    message: Some(record_of(&register.message)),
                    source_topic: register.source_topic.clone(),
                }))),
                _ => None,
            },
            traceparent: None,
        },
        Request::Visibility { topic, id, step } => Item {
            kind: Kind::Visibility as i32,
            phase: phase_of(step) as i32,
            topic: topic.clone(),
            id: id.clone(),
            operation_id: reference_of(step, |r| &r.operation_id),
            body: match step {
                Step::Register(register) => Some(item::Body::Visibility(VisibilityRegister {
                    coordinator_id: register.coordinator_id.clone(),
                    mode: match register.kind {
                        VisibilityKind::Delivery => VisibilityMode::Delivery,
                        VisibilityKind::Change => VisibilityMode::Change,
                    } as i32,
                    invisible_until_ms: register.invisible_until_ms,
                    receipt_token: register.receipt_token.clone(),
                    delivery_count: register.delivery_count,
                })),
                _ => None,
            },
            traceparent: None,
        },
        Request::Ack { topic, id, step } => Item {
            kind: Kind::Ack as i32,
            phase: phase_of(step) as i32,
            topic: topic.clone(),
            id: id.clone(),
            operation_id: reference_of(step, |r| &r.operation_id),
            body: match step {
                Step::Register(register) => Some(item::Body::Ack(AckRegister {
                    coordinator_id: register.coordinator_id.clone(),
                    receipt_token: register.receipt_token.clone(),
                    delete_at_ms: register.delete_at_ms,
                    discard: register.discard,
                })),
                _ => None,
            },
            traceparent: None,
        },
        _ => return None,
    })
}

/// The body of a `POST /cluster/batch`. An entry that is not a message operation (or already an error) goes out as an
/// item of an unspecified kind, which the receiver answers with an error of its own.
pub fn encode_batch(entries: &[Entry]) -> Vec<u8> {
    BatchRequest {
        items: entries
            .iter()
            .map(|entry| {
                entry.as_ref().ok().and_then(item_of).unwrap_or(Item {
                    kind: Kind::Unspecified as i32,
                    phase: Phase::Unspecified as i32,
                    topic: String::new(),
                    id: String::new(),
                    operation_id: String::new(),
                    body: None,
                    traceparent: None,
                })
            })
            .collect(),
    }
    .encode_to_vec()
}

// ---- Wire → domain ---------------------------------------------------------------------------------------------

fn invalid(field: &str, message: &str) -> TabaError {
    TabaError::new(
        ErrorCode::ValidationFailed,
        "The item of the batch is not valid.",
    )
    .with_detail(field, ErrorCode::ValidationFailed, message)
}

fn required(field: &str) -> TabaError {
    TabaError::new(
        ErrorCode::ValidationRequired,
        "Required fields are missing.",
    )
    .with_detail(field, ErrorCode::ValidationRequired, "Required field.")
}

fn message_of(record: Record) -> MessageRecord {
    MessageRecord {
        id: record.id,
        topic: record.topic,
        body: record.body,
        content_type: record.content_type,
        group_id: record.group_id,
        deduplication_id: record.deduplication_id,
        delivery_count: record.delivery_count,
        invisible_until_ms: record.invisible_until_ms,
        receipt_token: record.receipt_token,
        dead_lettered_ms: record.dead_lettered_ms,
        dead_letter_reason: record.dead_letter_reason,
        delete_at_ms: record.delete_at_ms,
    }
}

fn request_of(item: Item) -> Entry {
    let traceparent = item.traceparent.clone();
    request_without_trace(item).map(|request| BatchItem {
        request,
        traceparent,
    })
}

fn request_without_trace(item: Item) -> Result<Request, TabaError> {
    let Item {
        kind,
        phase,
        topic,
        id,
        operation_id,
        body,
        traceparent: _,
    } = item;
    let kind = Kind::try_from(kind).map_err(|_| invalid("kind", "Unknown kind of operation."))?;
    let phase =
        Phase::try_from(phase).map_err(|_| invalid("phase", "Unknown phase of the operation."))?;
    if kind == Kind::Unspecified {
        return Err(required("kind"));
    }
    let reference = |operation_id: String| OperationRef { operation_id };
    // The step of the phase: `register` carries the body of its kind, the other two only the operation.
    macro_rules! step {
        ($variant:ident, $build:expr) => {
            match phase {
                Phase::Register => match body {
                    Some(item::Body::$variant(wire)) => Step::Register($build(operation_id, wire)?),
                    _ => return Err(required("body")),
                },
                Phase::Commit => Step::Commit(reference(operation_id)),
                Phase::Rollback => Step::Rollback(reference(operation_id)),
                Phase::Unspecified => return Err(required("phase")),
            }
        };
    }
    Ok(match kind {
        Kind::Write => Request::Write {
            topic,
            id,
            step: step!(Write, |operation_id, wire: Box<WriteRegister>| {
                let wire = *wire;
                let message = wire.message.ok_or_else(|| required("message"))?;
                Ok::<_, TabaError>(MessageRegisterRequest {
                    operation_id,
                    coordinator_id: wire.coordinator_id,
                    kind: match WriteMode::try_from(wire.mode) {
                        Ok(WriteMode::Publish) => WriteKind::Publish,
                        Ok(WriteMode::DeadLetter) => WriteKind::DeadLetter,
                        _ => return Err(invalid("kind", "Unknown kind of write.")),
                    },
                    message: message_of(message),
                    source_topic: wire.source_topic,
                })
            }),
        },
        Kind::Visibility => Request::Visibility {
            topic,
            id,
            step: step!(Visibility, |operation_id, wire: VisibilityRegister| {
                Ok::<_, TabaError>(VisibilityRegisterRequest {
                    operation_id,
                    coordinator_id: wire.coordinator_id,
                    kind: match VisibilityMode::try_from(wire.mode) {
                        Ok(VisibilityMode::Delivery) => VisibilityKind::Delivery,
                        Ok(VisibilityMode::Change) => VisibilityKind::Change,
                        _ => return Err(invalid("kind", "Unknown kind of visibility.")),
                    },
                    invisible_until_ms: wire.invisible_until_ms,
                    receipt_token: wire.receipt_token,
                    delivery_count: wire.delivery_count,
                })
            }),
        },
        Kind::Ack => Request::Ack {
            topic,
            id,
            step: step!(Ack, |operation_id, wire: AckRegister| {
                Ok::<_, TabaError>(AckRegisterRequest {
                    operation_id,
                    coordinator_id: wire.coordinator_id,
                    receipt_token: wire.receipt_token,
                    delete_at_ms: wire.delete_at_ms,
                    discard: wire.discard,
                })
            }),
        },
        Kind::Unspecified => unreachable!("handled above"),
    })
}

/// Reads the body of a `POST /cluster/batch`. A body that is not a valid batch is an error of the whole call; an item
/// that is not valid is an error of that item.
pub fn decode_batch(body: &[u8]) -> Result<Vec<Entry>, TabaError> {
    let batch = BatchRequest::decode(body).map_err(|e| {
        TabaError::new(
            ErrorCode::ValidationFailed,
            format!("The body is not a valid batch: {e}."),
        )
    })?;
    Ok(batch.items.into_iter().map(request_of).collect())
}

// ---- The reply -------------------------------------------------------------------------------------------------

fn error_body_of(error: &TabaError) -> ErrorBody {
    ErrorBody {
        code: error.code.as_str().to_owned(),
        message: error.message.clone(),
        details: error
            .details
            .iter()
            .map(|d| ErrorItem {
                field: d.field.clone(),
                code: d.code.as_str().to_owned(),
                message: d.message.clone(),
            })
            .collect(),
        retry_after: error.retry_after,
        required: error.required.clone(),
    }
}

fn error_of(body: ErrorBody) -> TabaError {
    let code = ErrorCode::parse(&body.code).unwrap_or(ErrorCode::InternalError);
    let mut error = TabaError::new(code, body.message);
    error.details = body
        .details
        .into_iter()
        .map(|d| ErrorDetail {
            field: d.field,
            code: ErrorCode::parse(&d.code).unwrap_or(code),
            message: d.message,
        })
        .collect();
    error.retry_after = body.retry_after;
    error.required = body.required;
    error
}

/// The body of the reply to a batch: one result per item, in the order of the items.
pub fn encode_results(results: &[Outcome]) -> Vec<u8> {
    BatchReply {
        results: results
            .iter()
            .map(|result| match result {
                Ok(ack) => ItemResult {
                    status: 200,
                    operation_id: ack.operation_id.clone(),
                    accepted: ack.accepted,
                    error: None,
                },
                Err(error) => ItemResult {
                    status: u32::from(error.status()),
                    operation_id: String::new(),
                    accepted: false,
                    error: Some(error_body_of(error)),
                },
            })
            .collect(),
    }
    .encode_to_vec()
}

/// Reads the reply to a batch.
pub fn decode_results(body: &[u8]) -> Result<Vec<Outcome>, TabaError> {
    let reply = BatchReply::decode(body).map_err(|e| {
        TabaError::new(
            ErrorCode::InternalError,
            format!("The reply is not a valid batch reply: {e}."),
        )
    })?;
    Ok(reply
        .results
        .into_iter()
        .map(|result| match result.error {
            Some(error) => Err(error_of(error)),
            None => Ok(Ack {
                operation_id: result.operation_id,
                accepted: result.accepted,
            }),
        })
        .collect())
}

#[cfg(test)]
mod tests;
