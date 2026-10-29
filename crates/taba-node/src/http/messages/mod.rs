//! Client API messages (doc 06a, "Messages"): `POST` (publish), `GET`
//! (consume) and `DELETE` (prune, `prune.rs`) on `/topics/{topic}/messages`, and the same operations in batch
//! (`batch.rs`). Here the request is only read and the response built; the rules live in the `engine`.

mod batch;
mod prune;

use std::collections::HashMap;

use axum::Json;
use axum::body::{Body, Bytes};
use axum::extract::{Extension, Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use http_body_util::LengthLimitError;
use serde::Serialize;
use serde_json::{Map, Value, json};
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::auth::Principal;
use super::common::ApiError;
use super::openapi::{
    AuthErrors, ClusterUnavailable, MessageBytes, PublishOutcome, PublishResponse, RateLimited,
    VisibilityRequest, VisibilityResponse,
};
use super::{AppState, json};
use crate::core::content;
use crate::core::error::ErrorResponse;
use crate::core::error::{ErrorCode, TabaError};
use crate::core::time;
use crate::core::token::Permission;
use crate::engine::messages::{
    ConsumeQuery, Delivered, DistributorDelivery, Messages, PublishRequest, Published,
};
use crate::telemetry::trace;

pub fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(consume, publish, prune::prune))
        .routes(routes!(ack))
        .routes(routes!(visibility))
        .routes(routes!(batch::publish_batch))
        .routes(routes!(batch::ack_batch))
        .routes(routes!(batch::visibility_batch))
}

/// Reads the request body up to `limit` bytes. Past that: `413`, without reading the rest.
///
/// The only copy of the body on the publish path: what hyper hands over can be a slice of the connection's
/// read buffer, and a message kept in memory would hold the whole buffer alive (and the memory count, which
/// adds up the body sizes, would be short). From here to the engine the body is shared, never copied.
async fn read_body(body: Body, limit: u64) -> Result<Bytes, TabaError> {
    let limit = usize::try_from(limit).unwrap_or(usize::MAX);
    match axum::body::to_bytes(body, limit).await {
        Ok(bytes) => Ok(Bytes::copy_from_slice(&bytes)),
        Err(e) => {
            let too_large = std::error::Error::source(&e)
                .is_some_and(|source| source.downcast_ref::<LengthLimitError>().is_some());
            Err(if too_large {
                TabaError::new(
                    ErrorCode::MessageTooLarge,
                    format!("The body exceeds the topic's limit of {limit} bytes."),
                )
            } else {
                TabaError::new(
                    ErrorCode::ValidationFailed,
                    "Could not read the request body.",
                )
            })
        }
    }
}

/// The `Content-Type` as text. Present but unreadable: the `engine` refuses it as invalid.
fn content_type(headers: &HeaderMap) -> Option<String> {
    headers.get(header::CONTENT_TYPE).map(|value| {
        value
            .to_str()
            .map_or_else(|_| "\u{fffd}".to_owned(), str::to_owned)
    })
}

/// The messaging attributes of the request span: the topic, the operation and its tags.
fn annotate(state: &AppState, topic: &str, operation: &'static str) {
    // Without a span (no OTLP) there is nothing to annotate, and the topic is not even read.
    if tracing::Span::current().is_disabled() {
        return;
    }
    match state.topics.get(topic) {
        Ok(view) => trace::annotate_messaging(topic, operation, &view.definition.tags),
        Err(_) => trace::annotate_messaging(topic, operation, &std::collections::BTreeMap::new()),
    }
}

/// Publishes a message. The body is the **bytes** of the message and `Content-Type` says what they are (without it,
/// `application/json`); Taba stores the bytes as they came. `application/json` must be valid JSON and `text/*` valid
/// UTF-8. The node that receives is the coordinator and answers when the quorum confirms.
///
/// On a queue the answer is `201` with the `id` (on a FIFO queue, `200` with `duplicate: true` when the publication
/// repeated one inside the deduplication window); on a distributor it is `200` with what happened to each destination
/// queue.
///
/// Permission: `publish`. On a distributor only it is checked, not the destination queues.
#[utoipa::path(
    post,
    path = "/topics/{topic}/messages",
    tag = "Messages",
    operation_id = "publishMessage",
    params(
        ("topic" = String, Path, description = "The topic name."),
        ("groupId" = Option<String>, Query, description = "Group (1 to 128 characters of `A-Za-z0-9_-.:`). Required on a FIFO queue; elsewhere it guides the fair delivery."),
        ("deduplicationId" = Option<String>, Query, description = "FIFO queues only: the deduplication key inside the group (default: the SHA-256 of the body). Elsewhere it is `400 validation.not-applicable`.")
    ),
    request_body(
        content = MessageBytes, content_type = "*/*",
        description = "The message bytes, of any media type up to the topic's `maxMessageBytes` (default 5 MB)."
    ),
    responses(
        (status = 201, description = "Published to a queue.", body = PublishResponse),
        (status = 200, description = "A duplicate on a FIFO queue, or the result of a distributor.", body = PublishOutcome),
        (status = 400, description = "`validation.group-id-required`, `invalid-body`, `invalid-content-type`, `not-applicable` or `failed`.", body = ErrorResponse),
        (status = 404, description = "`topic.not-found`.", body = ErrorResponse),
        (status = 413, description = "`message.too-large`: above the topic's limit.", body = ErrorResponse),
        (status = 422, description = "`distributor.too-many-targets`: refused entirely.", body = ErrorResponse),
        (status = 502, description = "`forward.bad-gateway`: the forward failed after the body was sent. Outside FIFO it is not repeated (it would duplicate).", body = ErrorResponse),
        (status = 504, description = "`forward.timeout`, as the `502`.", body = ErrorResponse),
        AuthErrors, RateLimited, ClusterUnavailable
    )
)]
async fn publish(
    State(state): State<AppState>,
    Path(topic): Path<String>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
    body: Body,
) -> Result<Response, ApiError> {
    annotate(&state, &topic, "send");
    // The topic is checked (`404`) and the limit found out before reading the body.
    let limit = state.messages.publish_limit(&topic)?;
    let body = read_body(body, limit).await?;
    let request = PublishRequest {
        body,
        content_type: content_type(&headers),
        group_id: params.get("groupId").cloned(),
        deduplication_id: params.get("deduplicationId").cloned(),
    };
    let published = match &state.replicated {
        Some(cluster) => cluster.publish(&topic, request).await?,
        None => blocking(&state, move |messages| messages.publish(&topic, request)).await?,
    };
    if let Published::Queue { id, .. } = &published {
        trace::message_id(id);
    }
    Ok(match published {
        // Only the FIFO queue brings `duplicate`; the duplicate is `200` and the created message, `201`.
        Published::Queue {
            id,
            duplicate: None,
        } => (StatusCode::CREATED, Json(Created { id })).into_response(),
        Published::Queue {
            id,
            duplicate: Some(duplicate),
        } => (
            if duplicate {
                StatusCode::OK
            } else {
                StatusCode::CREATED
            },
            Json(json!({ "id": id, "duplicate": duplicate })),
        )
            .into_response(),
        Published::Distributor { deliveries } => Json(json!({
            "matched": deliveries.len(),
            "deliveries": deliveries.iter().map(delivery_json).collect::<Vec<_>>(),
        }))
        .into_response(),
    })
}

/// The `201` of a publication on a queue: `{"id": ...}`, without going through a `serde_json::Value`.
#[derive(Serialize)]
struct Created {
    id: String,
}

fn delivery_json(delivery: &DistributorDelivery) -> Value {
    let mut item = Map::new();
    item.insert("topic".into(), json!(delivery.topic));
    match &delivery.result {
        Ok(id) => item.insert("id".into(), json!(id)),
        Err(e) => item.insert(
            "error".into(),
            json!({ "code": e.code.as_str(), "message": e.message }),
        ),
    };
    Value::Object(item)
}

/// A delivered message.
#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(as = Message)]
struct MessageDto {
    /// GUID v7.
    id: String,
    /// What was published, according to `contentType`: the native JSON for `application/json` (or `*/*+json`), a
    /// string for `text/*` in UTF-8, and the bytes in base64 for any other type (then `bodyEncoding` is `base64`).
    #[schema(value_type = serde_json::Value)]
    body: content::Body,
    /// The one given at publication.
    content_type: String,
    /// `base64`, when the body is not JSON nor text.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Option<String>)]
    body_encoding: Option<&'static str>,
    /// Keep it: it is required to confirm the message and to change its invisibility.
    receipt_token: String,
    /// Deliveries so far, this one included.
    delivery_count: u32,
    #[schema(format = DateTime)]
    published_at: String,
    /// The end of the invisibility of this delivery.
    #[schema(format = DateTime)]
    invisible_until: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    group_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    deduplication_id: Option<String>,
    /// Only on a dead-letter queue.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(format = DateTime)]
    dead_lettered_at: Option<String>,
    /// Only on a dead-letter queue.
    #[serde(skip_serializing_if = "Option::is_none")]
    dead_letter_reason: Option<String>,
}

/// `ConsumeResponse`: serialized directly, so the JSON `body` does not go through a `Value` and lose
/// the form in which it was sent.
#[derive(Serialize, ToSchema)]
struct ConsumeResponse {
    /// Empty when nothing was ready: the answer is then `200`, never `204`.
    messages: Vec<MessageDto>,
}

impl From<Delivered> for MessageDto {
    fn from(m: Delivered) -> Self {
        Self {
            body_encoding: m.body.encoding(),
            id: m.id,
            body: m.body,
            content_type: m.content_type,
            receipt_token: m.receipt_token,
            delivery_count: m.delivery_count,
            published_at: time::format_ms(m.published_ms),
            invisible_until: time::format_ms(m.invisible_until_ms),
            group_id: m.group_id,
            deduplication_id: m.deduplication_id,
            dead_lettered_at: m.dead_lettered_ms.map(time::format_ms),
            dead_letter_reason: m.dead_letter_reason,
        }
    }
}

/// Delivers up to `maxMessages` **ready** messages, each invisible for `visibilityTimeout`. It returns what is ready
/// without waiting to fill `maxMessages` (with `waitTime` above 0 it waits only for the first one). Any `GET` counts as
/// a consumer for the idleness of a volatile queue.
///
/// Permission: `consume` (and `create` if `autoCreate` is going to create the queue).
#[utoipa::path(
    get,
    path = "/topics/{topic}/messages",
    tag = "Messages",
    operation_id = "consumeMessages",
    params(
        ("topic" = String, Path, description = "The queue name."),
        ("maxMessages" = Option<u32>, Query, description = "1 to `delivery.max-messages.max`. Default 10."),
        ("visibilityTimeout" = Option<u32>, Query, description = "Seconds, 0 to `delivery.visibility-timeout.max`. Default 120."),
        ("waitTime" = Option<u32>, Query, description = "Long polling, 0 to 20 seconds. Default 0."),
        ("autoCreate" = Option<bool>, Query, description = "Create the queue if it does not exist. The next parameters only apply at creation."),
        ("volatile" = Option<bool>, Query, description = "With `autoCreate`. Default `true`."),
        ("idleTimeout" = Option<u32>, Query, description = "With `autoCreate`. Default 600."),
        ("fifo" = Option<bool>, Query, description = "With `autoCreate`. Default `false`.")
    ),
    responses(
        (status = 200, description = "The messages that were ready; an empty list if none.", body = ConsumeResponse),
        (status = 400, description = "`validation.out-of-range`, `reserved-name` or `invalid-name`.", body = ErrorResponse),
        (status = 404, description = "`topic.not-found`: it does not exist and there was no `autoCreate`.", body = ErrorResponse),
        (status = 409, description = "`topic.wrong-type`: the topic is a distributor.", body = ErrorResponse),
        (status = 422, description = "`topic.volatile-limit`: the `autoCreate` limit.", body = ErrorResponse),
        AuthErrors, RateLimited, ClusterUnavailable
    )
)]
async fn consume(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(topic): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<ConsumeResponse>, ApiError> {
    annotate(&state, &topic, "receive");
    let query = ConsumeQuery {
        max_messages: params.get("maxMessages").cloned(),
        visibility_timeout: params.get("visibilityTimeout").cloned(),
        wait_time: params.get("waitTime").cloned(),
        auto_create: params.get("autoCreate").cloned(),
        volatile: params.get("volatile").cloned(),
        idle_timeout: params.get("idleTimeout").cloned(),
        fifo: params.get("fifo").cloned(),
        tags: params
            .iter()
            .filter_map(|(k, v)| Some((k.strip_prefix("tag.")?.to_owned(), v.clone())))
            .collect(),
        can_create: principal.allows(Permission::Create),
    };
    let delivered = match &state.replicated {
        Some(cluster) => cluster.consume(&topic, query).await?,
        None => state.messages.consume(&topic, query).await?,
    };
    trace::batch_count(delivered.len());
    if let Some(first) = delivered.first() {
        trace::message_id(&first.id);
    }
    Ok(Json(ConsumeResponse {
        messages: delivered.into_iter().map(Into::into).collect(),
    }))
}

/// Confirms (acknowledges) a message with a decision by quorum; a failure before the quorum cancels the ack and the
/// message stays available.
///
/// Permission: `consume`.
#[utoipa::path(
    delete,
    path = "/topics/{topic}/messages/{id}",
    tag = "Messages",
    operation_id = "ackMessage",
    params(
        ("topic" = String, Path, description = "The queue name."),
        ("id" = String, Path, description = "The message `id`."),
        ("receiptToken" = String, Query, description = "The `receiptToken` of the last delivery.")
    ),
    responses(
        (status = 204, description = "Confirmed and removed."),
        (status = 400, description = "`validation.required`: no `receiptToken`.", body = ErrorResponse),
        (status = 404, description = "`topic.not-found` or `message.not-found` (including a second ack, or a message expired by `ttl`).", body = ErrorResponse),
        (status = 409, description = "`message.stale-receipt` (the message was redelivered to another consumer) or `topic.wrong-type`.", body = ErrorResponse),
        AuthErrors, RateLimited, ClusterUnavailable
    )
)]
async fn ack(
    State(state): State<AppState>,
    Path((topic, id)): Path<(String, String)>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<StatusCode, ApiError> {
    annotate(&state, &topic, "settle");
    trace::message_id(&id);
    let receipt = params.get("receiptToken").cloned();
    match &state.replicated {
        Some(cluster) => cluster.ack(&topic, &id, receipt.as_deref()).await?,
        None => {
            blocking(&state, move |messages| {
                messages.ack(&topic, &id, receipt.as_deref())
            })
            .await?;
        }
    }
    Ok(StatusCode::NO_CONTENT)
}

/// Sets the invisibility deadline of a message **already delivered**, counted from now (not an increment), without
/// incrementing `deliveryCount`. `0` releases the message at once. `PUT` because the value replaces and repeating it
/// gives the same result.
///
/// Permission: `consume`.
#[utoipa::path(
    put,
    path = "/topics/{topic}/messages/{id}/visibility",
    tag = "Messages",
    operation_id = "setMessageVisibility",
    params(
        ("topic" = String, Path, description = "The queue name."),
        ("id" = String, Path, description = "The message `id`."),
        ("receiptToken" = String, Query, description = "The `receiptToken` of the last delivery.")
    ),
    request_body(content = VisibilityRequest, content_type = "application/json"),
    responses(
        (status = 200, description = "The new deadline.", body = VisibilityResponse),
        (status = 400, description = "`validation.required` or `validation.out-of-range`.", body = ErrorResponse),
        (status = 404, description = "`topic.not-found` or `message.not-found` (already confirmed, expired by `ttl`, or the topic is gone).", body = ErrorResponse),
        (status = 409, description = "`message.stale-receipt`.", body = ErrorResponse),
        AuthErrors, RateLimited, ClusterUnavailable
    )
)]
async fn visibility(
    State(state): State<AppState>,
    Path((topic, id)): Path<(String, String)>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<Value>, ApiError> {
    annotate(&state, &topic, "settle");
    trace::message_id(&id);
    let fields = json::object(&headers, &body)?;
    fields.reject_unknown(&["visibilityTimeout"])?;
    let receipt = params.get("receiptToken").cloned();
    let timeout = fields.integer("visibilityTimeout")?;
    let updated = match &state.replicated {
        Some(cluster) => {
            cluster
                .set_visibility(&topic, &id, receipt.as_deref(), timeout)
                .await?
        }
        None => {
            blocking(&state, move |messages| {
                messages.set_visibility(&topic, &id, receipt.as_deref(), timeout)
            })
            .await?
        }
    };
    Ok(Json(json!({
        "id": updated.id,
        "invisibleUntil": time::format_ms(updated.invisible_until_ms),
    })))
}

/// Runs a message operation outside the async executor: it blocks on disk (doc 18,
/// section 17.6).
async fn blocking<T, F>(state: &AppState, operation: F) -> Result<T, ApiError>
where
    T: Send + 'static,
    F: FnOnce(&Messages) -> Result<T, TabaError> + Send + 'static,
{
    let messages = state.messages.clone();
    // The request span follows to the disk thread: the logs there carry the same trace.
    let span = tracing::Span::current();
    tokio::task::spawn_blocking(move || span.in_scope(|| operation(&messages)))
        .await
        .map_err(|_| ApiError(TabaError::internal()))?
        .map_err(ApiError)
}
