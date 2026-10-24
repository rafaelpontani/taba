//! Client API topics (doc 06a): `POST` and `GET /topics`, and `GET`, `PATCH` and
//! `DELETE /topics/{topic}`. Here the request is only read and the response built; the
//! rules live in the `engine`.

mod request;

use std::collections::HashMap;

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Extension, Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde_json::{Map, Value};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::auth::{Principal, out_of_scope};
use super::common::ApiError;
use super::openapi::{
    AuthErrors, ClusterUnavailable, RateLimited, Topic, TopicCreateRequest, TopicPage,
    TopicUpdateRequest,
};
use super::{AppState, json};
use crate::core::error::ErrorResponse;
use crate::engine::topics::ListQuery;

pub fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list, create))
        .routes(routes!(read, update, remove))
}

/// The token only creates a topic within its prefixes; on a distributor, each distribution prefix too
/// (otherwise it would inject messages into queues out of its reach).
fn check_scope(
    principal: &Principal,
    name: Option<&str>,
    prefixes: Option<&[String]>,
) -> Result<(), ApiError> {
    let name_ok = name.is_none_or(|n| principal.reaches(n));
    let prefixes_ok =
        prefixes.is_none_or(|list| list.iter().all(|p| principal.covers_distribution_prefix(p)));
    if name_ok && prefixes_ok {
        Ok(())
    } else {
        Err(out_of_scope().into())
    }
}

/// Creates a topic. It generates the `id`, propagates the definition to **all nodes** and answers only afterwards.
/// Every queue gets its `{name}-dead-letter` with it.
///
/// Permission: `create`, within the token's prefixes (on a distributor, each prefix too).
#[utoipa::path(
    post,
    path = "/topics",
    tag = "Topics",
    operation_id = "createTopic",
    request_body(content = TopicCreateRequest, content_type = "application/json"),
    responses(
        (status = 201, description = "Created.", body = Topic,
            headers(("Location" = String, description = "`/topics/{name}`"))),
        (status = 400, description = "Invalid body: `validation.failed`, `invalid-name`, `reserved-name`, `not-applicable`, `out-of-range`, `max-bytes-above-hard-limit` or `unknown-field`.", body = ErrorResponse),
        (status = 409, description = "`topic.already-exists`.", body = ErrorResponse),
        AuthErrors, RateLimited, ClusterUnavailable
    )
)]
async fn create(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let fields = json::object(&headers, &body)?;
    let request = request::create(&fields)?;
    check_scope(&principal, Some(&request.name), request.prefixes.as_deref())?;
    let view = state.topics.create(request).await?;
    let location = format!("/topics/{}", view.definition.name);
    Ok((
        StatusCode::CREATED,
        [(header::LOCATION, location)],
        Json(view.to_json()),
    )
        .into_response())
}

/// Lists the topics, paginated by `name` (or the chosen `sort`). It is answered locally. With authentication on, only
/// the topics within the token's prefixes come, and the dead-letter queues are included.
///
/// Permission: any valid token.
#[utoipa::path(
    get,
    path = "/topics",
    tag = "Topics",
    operation_id = "listTopics",
    params(
        ("limit" = Option<u32>, Query, description = "Page size. Default 100."),
        ("cursor" = Option<String>, Query, description = "The `nextCursor` of the previous page."),
        ("sort" = Option<String>, Query, description = "`name` (default), `id` or `createdAt`; a leading `-` sorts descending."),
        ("name" = Option<String>, Query, description = "Only topics whose name contains the text (case-sensitive).")
    ),
    responses(
        (status = 200, description = "A page of topics.", body = TopicPage),
        (status = 400, description = "`validation.out-of-range`, `invalid-cursor` or `failed`.", body = ErrorResponse),
        AuthErrors, RateLimited
    )
)]
async fn list(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    let result = state.topics.list(ListQuery {
        limit: params.get("limit").cloned(),
        cursor: params.get("cursor").cloned(),
        sort: params.get("sort").cloned(),
        name: params.get("name").cloned(),
        scope: principal.prefixes().to_vec(),
    })?;
    let mut page = Map::new();
    page.insert(
        "items".into(),
        Value::Array(result.items.iter().map(|v| v.to_json()).collect()),
    );
    if let Some(cursor) = result.next_cursor {
        page.insert("nextCursor".into(), Value::String(cursor));
    }
    Ok(Json(Value::Object(page)))
}

/// Reads a topic. On a dead-letter queue the values are those of the source queue, read-only, with `deadLetter` and
/// `source`.
///
/// Permission: any valid token, within its prefixes.
#[utoipa::path(
    get,
    path = "/topics/{topic}",
    tag = "Topics",
    operation_id = "getTopic",
    params(("topic" = String, Path, description = "The topic name.")),
    responses(
        (status = 200, description = "The topic.", body = Topic),
        (status = 404, description = "`topic.not-found`.", body = ErrorResponse),
        AuthErrors, RateLimited
    )
)]
async fn read(
    State(state): State<AppState>,
    Path(topic): Path<String>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(state.topics.get(&topic)?.to_json()))
}

/// Changes a topic: only what is sent changes. It also applies to the messages already stored (a new `ttl` at the next
/// sweep).
///
/// Permission: `manage`, within the token's prefixes.
#[utoipa::path(
    patch,
    path = "/topics/{topic}",
    tag = "Topics",
    operation_id = "updateTopic",
    params(("topic" = String, Path, description = "The topic name.")),
    request_body(content = TopicUpdateRequest, content_type = "application/json"),
    responses(
        (status = 200, description = "The topic after the change.", body = Topic),
        (status = 400, description = "Invalid body, or an immutable field (`validation.immutable-field`).", body = ErrorResponse),
        (status = 404, description = "`topic.not-found`.", body = ErrorResponse),
        (status = 409, description = "`topic.read-only`: a dead-letter queue follows its source queue.", body = ErrorResponse),
        AuthErrors, RateLimited, ClusterUnavailable
    )
)]
async fn update(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(topic): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<Value>, ApiError> {
    let fields = json::object(&headers, &body)?;
    let request = request::update(&fields)?;
    check_scope(&principal, None, request.prefixes.as_deref())?;
    let view = state.topics.update(&topic, request).await?;
    Ok(Json(view.to_json()))
}

/// Removes the topic and **all its messages, even the pending ones**, and its dead-letter queue. It always works, with
/// no check. On `503 cluster.commit-timeout` the removal will complete.
///
/// Permission: `manage`, within the token's prefixes.
#[utoipa::path(
    delete,
    path = "/topics/{topic}",
    tag = "Topics",
    operation_id = "deleteTopic",
    params(("topic" = String, Path, description = "The topic name.")),
    responses(
        (status = 204, description = "Removed."),
        (status = 404, description = "`topic.not-found`.", body = ErrorResponse),
        (status = 409, description = "`topic.read-only`: remove the source queue instead.", body = ErrorResponse),
        AuthErrors, RateLimited, ClusterUnavailable
    )
)]
async fn remove(
    State(state): State<AppState>,
    Path(topic): Path<String>,
) -> Result<StatusCode, ApiError> {
    state.topics.delete(&topic).await?;
    Ok(StatusCode::NO_CONTENT)
}
