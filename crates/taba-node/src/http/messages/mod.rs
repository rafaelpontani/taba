//! Client API messages (doc 06a, "Messages"): `POST` (publish) and `GET`
//! (consume) on `/topics/{topic}/messages`. Here the request is only read and the
//! response built; the rules live in the `engine`.

use std::collections::HashMap;

use axum::body::{Body, Bytes};
use axum::extract::{Extension, Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, put};
use axum::{Json, Router};
use http_body_util::LengthLimitError;
use serde::Serialize;
use serde_json::{Map, Value, json};

use super::auth::Principal;
use super::common::ApiError;
use super::{AppState, json};
use crate::core::content;
use crate::core::error::{ErrorCode, TabaError};
use crate::core::time;
use crate::core::token::Permission;
use crate::engine::messages::{
    ConsumeQuery, Delivered, DistributorDelivery, Messages, PublishRequest, Published,
};
use crate::telemetry::trace;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/topics/{topic}/messages", get(consume).post(publish))
        .route("/topics/{topic}/messages/{id}", delete(ack))
        .route("/topics/{topic}/messages/{id}/visibility", put(visibility))
}

/// Reads the request body up to `limit` bytes. Past that: `413`, without reading the rest.
async fn read_body(body: Body, limit: u64) -> Result<Vec<u8>, TabaError> {
    let limit = usize::try_from(limit).unwrap_or(usize::MAX);
    match axum::body::to_bytes(body, limit).await {
        Ok(bytes) => Ok(bytes.to_vec()),
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
    match state.topics.get(topic) {
        Ok(view) => trace::annotate_messaging(topic, operation, &view.definition.tags),
        Err(_) => trace::annotate_messaging(topic, operation, &std::collections::BTreeMap::new()),
    }
}

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
        } => (StatusCode::CREATED, Json(json!({ "id": id }))).into_response(),
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

/// The API's `Message` (doc 06a, "DTOs").
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MessageDto {
    id: String,
    body: content::Body,
    content_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    body_encoding: Option<&'static str>,
    receipt_token: String,
    delivery_count: u32,
    published_at: String,
    invisible_until: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    group_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    deduplication_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    dead_lettered_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    dead_letter_reason: Option<String>,
}

/// `ConsumeResponse`: serialized directly, so the JSON `body` does not go through a `Value` and lose
/// the form in which it was sent.
#[derive(Serialize)]
struct ConsumeResponse {
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
