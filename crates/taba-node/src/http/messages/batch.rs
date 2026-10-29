//! The messages in batch (doc 06a, "Messages in batch"): publish, ack and change the invisibility of many messages
//! in one request. Each item goes through the same path as its single call (the same checks, the same quorum, the
//! same answer), so a batch is not atomic: one item that fails does not undo nor block the others. The answer is
//! `200` with one result per item, in the order of the request, each with the status the single call would have
//! answered. What a batch saves is the HTTP request of each message; between nodes the items of a batch already go
//! together in the protobuf batch of the channel (`cluster::service::batch`).

use std::collections::HashMap;

use axum::Json;
use axum::body::{Body, Bytes};
use axum::extract::{Path, State};
use axum::http::HeaderMap;
use futures_util::stream::{self, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use serde_json::value::RawValue;
use utoipa::ToSchema;

use super::{annotate, blocking, delivery_json};
use crate::core::content::{self, ContentType, DEFAULT_CONTENT_TYPE};
use crate::core::error::{ErrorCode, ErrorDetail, ErrorResponse, TabaError};
use crate::core::time;
use crate::engine::messages::{PublishRequest, Published, VisibilityUpdated};
use crate::http::common::ApiError;
use crate::http::openapi::{AuthErrors, ClusterUnavailable, RateLimited};
use crate::http::{AppState, json};
use crate::telemetry::trace;

/// Items of a batch in flight at once on a cluster node: enough to fill the protobuf batch of the channel, without
/// a batch of thousands starting thousands of operations at the same instant.
const IN_FLIGHT: usize = 128;

/// A publication of a batch: the same fields as `POST /topics/{topic}/messages`, with the body in the form the `GET`
/// delivers it.
#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PublishItem {
    /// For `application/json` (or `*/*+json`), the JSON itself, kept byte by byte; for `text/*` in UTF-8, a string;
    /// for any other type, a string in base64 with `bodyEncoding: "base64"` (which also works for JSON and text).
    #[schema(value_type = serde_json::Value)]
    body: Box<RawValue>,
    /// What the bytes are (default `application/json`), as the `Content-Type` header of the single publication.
    content_type: Option<String>,
    /// `base64` when the `body` is a string in base64.
    body_encoding: Option<String>,
    /// Required on a FIFO queue; elsewhere it guides the fair delivery.
    group_id: Option<String>,
    /// FIFO queues only: the deduplication key inside the group.
    deduplication_id: Option<String>,
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PublishBatch {
    /// 1 to `api.batch.max-messages`. On a FIFO queue the order of the list is the order of publication inside each
    /// group.
    messages: Vec<PublishItem>,
}

/// An ack of a batch: the `id` and the `receiptToken` of the last delivery.
#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AckItem {
    id: String,
    receipt_token: Option<String>,
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AckBatch {
    /// 1 to `api.batch.max-messages`.
    messages: Vec<AckItem>,
}

/// A change of invisibility of a batch, as `PUT /topics/{topic}/messages/{id}/visibility`.
#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct VisibilityItem {
    id: String,
    receipt_token: Option<String>,
    /// Seconds from now, 0 to `delivery.visibility-timeout.max`.
    visibility_timeout: Option<i64>,
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct VisibilityBatch {
    /// 1 to `api.batch.max-messages`.
    messages: Vec<VisibilityItem>,
}

/// What failed in one item: the same fields as the `ErrorResponse` of the single call, without the `requestId`
/// (the request is one).
#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ItemError {
    #[schema(value_type = String, example = "message.stale-receipt")]
    code: ErrorCode,
    message: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    details: Vec<ErrorDetail>,
}

/// The result of one item: the `status` the single call would have answered and its body.
#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ItemResult {
    /// `201` (published), `200` (a FIFO duplicate, a distributor, a new invisibility), `204` (acked) or the status
    /// of the error.
    status: u16,
    /// The message `id`: of the publication, of the ack or of the invisibility.
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    /// FIFO queues only: the publication repeated one inside the deduplication window.
    #[serde(skip_serializing_if = "Option::is_none")]
    duplicate: Option<bool>,
    /// Distributor only: how many queues the message reached.
    #[serde(skip_serializing_if = "Option::is_none")]
    matched: Option<usize>,
    /// Distributor only: what happened in each destination queue.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Option<Vec<Object>>)]
    deliveries: Option<Vec<Value>>,
    /// Invisibility only: the new deadline.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(format = DateTime)]
    invisible_until: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<ItemError>,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct BatchResponse {
    /// One per item, in the order of the request.
    results: Vec<ItemResult>,
}

impl ItemResult {
    fn ok(status: u16, id: Option<String>) -> Self {
        Self {
            status,
            id,
            duplicate: None,
            matched: None,
            deliveries: None,
            invisible_until: None,
            error: None,
        }
    }

    fn failed(error: TabaError) -> Self {
        let mut result = Self::ok(error.status(), None);
        result.error = Some(ItemError {
            code: error.code,
            message: error.message,
            details: error.details,
        });
        result
    }

    fn published(published: Result<Published, TabaError>) -> Self {
        match published {
            Ok(Published::Queue { id, duplicate }) => {
                trace::message_id(&id);
                let mut result =
                    Self::ok(if duplicate == Some(true) { 200 } else { 201 }, Some(id));
                result.duplicate = duplicate;
                result
            }
            Ok(Published::Distributor { deliveries }) => {
                let mut result = Self::ok(200, None);
                result.matched = Some(deliveries.len());
                result.deliveries = Some(deliveries.iter().map(delivery_json).collect());
                result
            }
            Err(error) => Self::failed(error),
        }
    }

    fn acked(acked: Result<(), TabaError>, id: String) -> Self {
        match acked {
            Ok(()) => Self::ok(204, Some(id)),
            Err(error) => Self::failed(error),
        }
    }

    fn visibility(updated: Result<VisibilityUpdated, TabaError>) -> Self {
        match updated {
            Ok(updated) => {
                let mut result = Self::ok(200, Some(updated.id));
                result.invisible_until = Some(time::format_ms(updated.invisible_until_ms));
                result
            }
            Err(error) => Self::failed(error),
        }
    }
}

/// Reads the body of a batch up to `api.batch.max-bytes`. Past that: `413`, without reading the rest.
async fn read_batch(body: Body, limit: u64) -> Result<Bytes, TabaError> {
    let max = usize::try_from(limit).unwrap_or(usize::MAX);
    axum::body::to_bytes(body, max).await.map_err(|e| {
        let too_large = std::error::Error::source(&e).is_some_and(|source| {
            source
                .downcast_ref::<http_body_util::LengthLimitError>()
                .is_some()
        });
        if too_large {
            TabaError::new(
                ErrorCode::MessageTooLarge,
                format!("The batch exceeds api.batch.max-bytes ({limit} bytes)."),
            )
        } else {
            TabaError::new(
                ErrorCode::ValidationFailed,
                "Could not read the request body.",
            )
        }
    })
}

/// The list must have 1 to `api.batch.max-messages` items: `400 validation.out-of-range` otherwise.
fn check_count(count: usize, max: usize) -> Result<(), TabaError> {
    if (1..=max).contains(&count) {
        return Ok(());
    }
    Err(TabaError::new(
        ErrorCode::ValidationOutOfRange,
        format!("A batch has 1 to {max} messages (api.batch.max-messages); this one has {count}."),
    )
    .with_detail(
        "messages",
        ErrorCode::ValidationOutOfRange,
        format!("1 to {max} items."),
    ))
}

/// Reads and checks the request of a batch: the body up to the bytes, then the JSON, then the count of items.
async fn read_items<T: serde::de::DeserializeOwned>(
    state: &AppState,
    headers: &HeaderMap,
    body: Body,
    items: impl FnOnce(&T) -> usize,
) -> Result<T, TabaError> {
    let (max_items, max_bytes) = state.messages.batch_limits();
    let body = read_batch(body, max_bytes).await?;
    let request: T = json::typed(headers, &body)?;
    check_count(items(&request), max_items)?;
    Ok(request)
}

impl PublishItem {
    /// The single publication this item is. A body above the topic's limit is `413` in this item only.
    fn into_request(self, limit: u64) -> Result<PublishRequest, TabaError> {
        let content_type =
            ContentType::parse(self.content_type.as_deref().unwrap_or(DEFAULT_CONTENT_TYPE))?;
        let body = content::Body::decode(&content_type, &self.body, self.body_encoding.as_deref())?;
        if body.len() as u64 > limit {
            return Err(TabaError::new(
                ErrorCode::MessageTooLarge,
                format!("The body exceeds the topic's limit of {limit} bytes."),
            ));
        }
        Ok(PublishRequest {
            body: Bytes::from(body),
            content_type: self.content_type,
            group_id: self.group_id,
            deduplication_id: self.deduplication_id,
        })
    }
}

/// Runs `lanes` on a cluster node, at most [`IN_FLIGHT`] at a time. Each lane is a list of items done one after the
/// other (the items of one FIFO group); the result keeps the position of each item in the request.
async fn run_lanes<I, R, F, Fut>(lanes: Vec<Vec<(usize, I)>>, total: usize, run: F) -> Vec<R>
where
    F: Fn(I) -> Fut + Clone,
    Fut: Future<Output = R>,
{
    let done: Vec<Vec<(usize, R)>> = stream::iter(lanes)
        .map(|lane| {
            let run = run.clone();
            async move {
                let mut out = Vec::with_capacity(lane.len());
                for (index, item) in lane {
                    out.push((index, run(item).await));
                }
                out
            }
        })
        .buffer_unordered(IN_FLIGHT)
        .collect()
        .await;
    let mut slots: Vec<Option<R>> = (0..total).map(|_| None).collect();
    for (index, result) in done.into_iter().flatten() {
        slots[index] = Some(result);
    }
    slots.into_iter().flatten().collect()
}

/// One lane per item, except on a FIFO queue: there the items of a group share a lane, in the order of the request.
fn lanes<I>(
    items: Vec<I>,
    fifo: bool,
    group: impl Fn(&I) -> Option<String>,
) -> Vec<Vec<(usize, I)>> {
    if !fifo {
        return items
            .into_iter()
            .enumerate()
            .map(|item| vec![item])
            .collect();
    }
    let mut lanes: Vec<Vec<(usize, I)>> = Vec::new();
    let mut by_group: HashMap<String, usize> = HashMap::new();
    for (index, item) in items.into_iter().enumerate() {
        match group(&item) {
            Some(key) => {
                let lane = *by_group.entry(key).or_insert_with(|| {
                    lanes.push(Vec::new());
                    lanes.len() - 1
                });
                lanes[lane].push((index, item));
            }
            None => lanes.push(vec![(index, item)]),
        }
    }
    lanes
}

/// Publishes up to `api.batch.max-messages` messages in one request. Each item is the single publication
/// (`POST /topics/{topic}/messages`) with the body in the form the `GET` delivers it: JSON as it is, text as a string,
/// any type in base64 with `bodyEncoding: "base64"`. The items are independent: one that fails does not undo the
/// others. On a FIFO queue, the items of a group are published in the order of the list.
///
/// Permission: `publish`.
#[utoipa::path(
    post,
    path = "/topics/{topic}/messages/batch",
    tag = "Messages",
    operation_id = "publishMessages",
    params(("topic" = String, Path, description = "The topic name.")),
    request_body(content = PublishBatch, content_type = "application/json"),
    responses(
        (status = 200, description = "One result per item, in order: `201` with the `id`, `200` for a FIFO duplicate or a distributor, or the error of that item.", body = BatchResponse),
        (status = 400, description = "`validation.out-of-range` (no items, or more than `api.batch.max-messages`), `validation.unknown-field`, `validation.required` or `validation.failed`.", body = ErrorResponse),
        (status = 404, description = "`topic.not-found`.", body = ErrorResponse),
        (status = 413, description = "`message.too-large`: the request exceeds `api.batch.max-bytes`.", body = ErrorResponse),
        (status = 415, description = "`media.unsupported`: the request is not `application/json`.", body = ErrorResponse),
        AuthErrors, RateLimited, ClusterUnavailable
    )
)]
pub(super) async fn publish_batch(
    State(state): State<AppState>,
    Path(topic): Path<String>,
    headers: HeaderMap,
    body: Body,
) -> Result<Json<BatchResponse>, ApiError> {
    annotate(&state, &topic, "send");
    // The topic is checked (`404`) before reading the body.
    let limit = state.messages.publish_limit(&topic)?;
    let request: PublishBatch =
        read_items(&state, &headers, body, |r: &PublishBatch| r.messages.len()).await?;
    trace::batch_count(request.messages.len());
    let fifo = state
        .topics
        .get(&topic)?
        .definition
        .queue()
        .is_some_and(|q| q.fifo);
    let items: Vec<Result<PublishRequest, TabaError>> = request
        .messages
        .into_iter()
        .map(|item| item.into_request(limit))
        .collect();
    let total = items.len();
    let results = match &state.replicated {
        Some(cluster) => {
            let lanes = lanes(items, fifo, |item| {
                item.as_ref().ok().and_then(|r| r.group_id.clone())
            });
            run_lanes(lanes, total, |item: Result<PublishRequest, TabaError>| {
                let (cluster, topic) = (cluster.clone(), topic.clone());
                async move {
                    match item {
                        Ok(request) => cluster.publish(&topic, request).await,
                        Err(error) => Err(error),
                    }
                }
            })
            .await
        }
        None => {
            blocking(&state, move |messages| {
                Ok(items
                    .into_iter()
                    .map(|item| item.and_then(|request| messages.publish(&topic, request)))
                    .collect::<Vec<_>>())
            })
            .await?
        }
    };
    Ok(Json(BatchResponse {
        results: results.into_iter().map(ItemResult::published).collect(),
    }))
}

/// Acks up to `api.batch.max-messages` messages in one request, each as `DELETE /topics/{topic}/messages/{id}`: by
/// quorum, and a failure before the quorum leaves that message available. The items are independent.
///
/// Permission: `consume`.
#[utoipa::path(
    post,
    path = "/topics/{topic}/messages/ack",
    tag = "Messages",
    operation_id = "ackMessages",
    params(("topic" = String, Path, description = "The queue name.")),
    request_body(content = AckBatch, content_type = "application/json"),
    responses(
        (status = 200, description = "One result per item, in order: `204` (acked) or the error of that item (`404 message.not-found`, `409 message.stale-receipt`, `400 validation.required`).", body = BatchResponse),
        (status = 400, description = "`validation.out-of-range` (no items, or more than `api.batch.max-messages`), `validation.unknown-field`, `validation.required` or `validation.failed`.", body = ErrorResponse),
        (status = 404, description = "`topic.not-found`.", body = ErrorResponse),
        (status = 409, description = "`topic.wrong-type`: the topic is a distributor.", body = ErrorResponse),
        (status = 413, description = "`message.too-large`: the request exceeds `api.batch.max-bytes`.", body = ErrorResponse),
        (status = 415, description = "`media.unsupported`: the request is not `application/json`.", body = ErrorResponse),
        AuthErrors, RateLimited, ClusterUnavailable
    )
)]
pub(super) async fn ack_batch(
    State(state): State<AppState>,
    Path(topic): Path<String>,
    headers: HeaderMap,
    body: Body,
) -> Result<Json<BatchResponse>, ApiError> {
    annotate(&state, &topic, "settle");
    state.messages.queue_view(&topic)?;
    let request: AckBatch =
        read_items(&state, &headers, body, |r: &AckBatch| r.messages.len()).await?;
    trace::batch_count(request.messages.len());
    let total = request.messages.len();
    let results = match &state.replicated {
        Some(cluster) => {
            let lanes = lanes(request.messages, false, |_| None);
            run_lanes(lanes, total, |item: AckItem| {
                let (cluster, topic) = (cluster.clone(), topic.clone());
                async move {
                    let acked = cluster
                        .ack(&topic, &item.id, item.receipt_token.as_deref())
                        .await;
                    ItemResult::acked(acked, item.id)
                }
            })
            .await
        }
        None => {
            blocking(&state, move |messages| {
                Ok(request
                    .messages
                    .into_iter()
                    .map(|item| {
                        let acked = messages.ack(&topic, &item.id, item.receipt_token.as_deref());
                        ItemResult::acked(acked, item.id)
                    })
                    .collect::<Vec<_>>())
            })
            .await?
        }
    };
    Ok(Json(BatchResponse { results }))
}

/// Changes the invisibility of up to `api.batch.max-messages` messages already delivered in one request, each as
/// `PUT /topics/{topic}/messages/{id}/visibility`. The items are independent.
///
/// Permission: `consume`.
#[utoipa::path(
    post,
    path = "/topics/{topic}/messages/visibility",
    tag = "Messages",
    operation_id = "setMessagesVisibility",
    params(("topic" = String, Path, description = "The queue name.")),
    request_body(content = VisibilityBatch, content_type = "application/json"),
    responses(
        (status = 200, description = "One result per item, in order: `200` with the new `invisibleUntil`, or the error of that item.", body = BatchResponse),
        (status = 400, description = "`validation.out-of-range` (no items, or more than `api.batch.max-messages`), `validation.unknown-field`, `validation.required` or `validation.failed`.", body = ErrorResponse),
        (status = 404, description = "`topic.not-found`.", body = ErrorResponse),
        (status = 409, description = "`topic.wrong-type`: the topic is a distributor.", body = ErrorResponse),
        (status = 413, description = "`message.too-large`: the request exceeds `api.batch.max-bytes`.", body = ErrorResponse),
        (status = 415, description = "`media.unsupported`: the request is not `application/json`.", body = ErrorResponse),
        AuthErrors, RateLimited, ClusterUnavailable
    )
)]
pub(super) async fn visibility_batch(
    State(state): State<AppState>,
    Path(topic): Path<String>,
    headers: HeaderMap,
    body: Body,
) -> Result<Json<BatchResponse>, ApiError> {
    annotate(&state, &topic, "settle");
    state.messages.queue_view(&topic)?;
    let request: VisibilityBatch = read_items(&state, &headers, body, |r: &VisibilityBatch| {
        r.messages.len()
    })
    .await?;
    trace::batch_count(request.messages.len());
    let total = request.messages.len();
    let results = match &state.replicated {
        Some(cluster) => {
            let lanes = lanes(request.messages, false, |_| None);
            run_lanes(lanes, total, |item: VisibilityItem| {
                let (cluster, topic) = (cluster.clone(), topic.clone());
                async move {
                    ItemResult::visibility(
                        cluster
                            .set_visibility(
                                &topic,
                                &item.id,
                                item.receipt_token.as_deref(),
                                item.visibility_timeout,
                            )
                            .await,
                    )
                }
            })
            .await
        }
        None => {
            blocking(&state, move |messages| {
                Ok(request
                    .messages
                    .into_iter()
                    .map(|item| {
                        ItemResult::visibility(messages.set_visibility(
                            &topic,
                            &item.id,
                            item.receipt_token.as_deref(),
                            item.visibility_timeout,
                        ))
                    })
                    .collect::<Vec<_>>())
            })
            .await?
        }
    };
    Ok(Json(BatchResponse { results }))
}
