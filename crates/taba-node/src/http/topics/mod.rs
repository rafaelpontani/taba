//! Client API topics (doc 06a): `POST` and `GET /topics`, and `GET`, `PATCH` and
//! `DELETE /topics/{topic}`. Here the request is only read and the response built; the
//! rules live in the `engine`.

mod request;

use std::collections::HashMap;

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Extension, Path, Query, State};
use axum::http::{HeaderMap, HeaderName, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde_json::{Map, Value};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::auth::{Principal, out_of_scope};
use super::common::ApiError;
use super::openapi::{
    AuthErrors, ClusterUnavailable, RateLimited, Topic, TopicCreateRequest, TopicPage,
    TopicPutRequest, TopicUpdateRequest,
};
use super::{AppState, json, topic_stats};
use crate::core::error::{ErrorCode, ErrorResponse, TabaError};
use crate::engine::topics::{ListQuery, PutOutcome, Recreate};

pub fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list, create))
        .routes(routes!(read, update, remove, put))
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

/// Takes a topic to the declared state: creates it if it does not exist, otherwise applies the body as the whole
/// state (what is absent goes back to the default). Nothing to change: `200` without a new version. Another `type`,
/// `fifo` or `volatile` cannot change in place: `409 topic.immutable-mismatch`, unless `recreate` asks to remove the
/// topic (with its messages and dead-letter queue) and create it again. The owner of the definition decides, so two
/// `PUT`s at the same time do not step on each other; repeating a `PUT` is harmless.
///
/// Permission: `manage` (which includes `create`), within the token's prefixes (on a distributor, each prefix too).
#[utoipa::path(
    put,
    path = "/topics/{topic}",
    tag = "Topics",
    operation_id = "putTopic",
    params(
        ("topic" = String, Path, description = "The topic name."),
        ("recreate" = Option<String>, Query, description = "Only when the `type`, `fifo` or `volatile` differ: `if-empty` recreates only if the queue and its dead-letter hold no message (`409 topic.not-empty` otherwise); `always` recreates and loses the messages. Absent: `409 topic.immutable-mismatch`.")
    ),
    request_body(content = TopicPutRequest, content_type = "application/json"),
    responses(
        (status = 200, description = "It already existed and is now as declared (also when nothing changed).", body = Topic),
        (status = 201, description = "Created, or recreated (then with `Taba-Topic-Recreated: true` and a new `id`).", body = Topic,
            headers(
                ("Location" = String, description = "`/topics/{name}`"),
                ("Taba-Topic-Recreated" = Option<String>, description = "`true` when the topic was removed and created again.")
            )),
        (status = 400, description = "Invalid body or query: `validation.failed`, `required`, `invalid-name`, `reserved-name`, `not-applicable`, `out-of-range`, `max-bytes-above-hard-limit` or `unknown-field` (`name` included).", body = ErrorResponse),
        (status = 409, description = "`topic.immutable-mismatch` (another `type`, `fifo` or `volatile`, with each in `details`), `topic.not-empty` (`recreate=if-empty` on a queue with messages) or `cluster.conflict` (the topic changed during the request: repeat it).", body = ErrorResponse),
        AuthErrors, RateLimited, ClusterUnavailable
    )
)]
async fn put(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(topic): Path<String>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let fields = json::object(&headers, &body)?;
    let request = request::put(&fields, &topic)?;
    check_scope(&principal, Some(&request.name), request.prefixes.as_deref())?;
    let (recreate, expected_id) =
        match request::recreate(params.get("recreate").map(String::as_str))? {
            // Counted here, where the messages of every node can be asked for; the owner recreates only the `id`
            // that was found empty.
            Recreate::IfEmpty => match state.topics.needs_recreate(&request)? {
                Some(id) => {
                    ensure_empty(&state, &topic).await?;
                    (Recreate::Always, Some(id))
                }
                None => (Recreate::IfEmpty, None),
            },
            other => (other, None),
        };
    let (view, outcome) = state.topics.put(request, recreate, expected_id).await?;
    let json = Json(view.to_json());
    let location = format!("/topics/{}", view.definition.name);
    Ok(match outcome {
        PutOutcome::Created => {
            (StatusCode::CREATED, [(header::LOCATION, location)], json).into_response()
        }
        PutOutcome::Recreated => (
            StatusCode::CREATED,
            [
                (header::LOCATION, location),
                (HeaderName::from_static(RECREATED_HEADER), "true".to_owned()),
            ],
            json,
        )
            .into_response(),
        PutOutcome::Updated | PutOutcome::Unchanged => (StatusCode::OK, json).into_response(),
    })
}

/// The header that says a `PUT` removed the topic and created it again.
const RECREATED_HEADER: &str = "taba-topic-recreated";

/// `409 topic.not-empty` when the queue or its dead-letter holds messages on some node.
async fn ensure_empty(state: &AppState, queue: &str) -> Result<(), ApiError> {
    let held = topic_stats::messages_held(state, queue).await?;
    if held == 0 {
        return Ok(());
    }
    let message =
        format!("The queue and its dead-letter hold {held} messages; nothing was changed.");
    Err(TabaError::new(ErrorCode::TopicNotEmpty, message.clone())
        .with_detail("messages", ErrorCode::TopicNotEmpty, message)
        .into())
}

/// Removes the topic and **all its messages, even the pending ones**, and its dead-letter queue. Without `ifEmpty`
/// it always works, with no check. On `503 cluster.commit-timeout` the removal will complete.
///
/// Permission: `manage`, within the token's prefixes.
#[utoipa::path(
    delete,
    path = "/topics/{topic}",
    tag = "Topics",
    operation_id = "deleteTopic",
    params(
        ("topic" = String, Path, description = "The topic name."),
        ("ifEmpty" = Option<bool>, Query, description = "`true`: only removes a queue whose messages and dead-letter are empty on every node (`409 topic.not-empty` otherwise; `503` if a node does not answer the count). A distributor holds no messages.")
    ),
    responses(
        (status = 204, description = "Removed."),
        (status = 400, description = "`validation.failed`: `ifEmpty` is not `true` or `false`.", body = ErrorResponse),
        (status = 404, description = "`topic.not-found`.", body = ErrorResponse),
        (status = 409, description = "`topic.read-only` (remove the source queue instead) or `topic.not-empty` (with `ifEmpty=true`).", body = ErrorResponse),
        AuthErrors, RateLimited, ClusterUnavailable
    )
)]
async fn remove(
    State(state): State<AppState>,
    Path(topic): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<StatusCode, ApiError> {
    if request::if_empty(params.get("ifEmpty").map(String::as_str))? {
        // The read-only dead-letter and the missing topic fall to the plain removal, which answers them.
        if let Some(definition) = state.topics.definition(&topic) {
            if definition.queue().is_some() {
                ensure_empty(&state, &topic).await?;
            }
            // Only the life that was counted: a topic recreated meanwhile is not removed.
            state.topics.delete_expected(&topic, &definition.id).await?;
            return Ok(StatusCode::NO_CONTENT);
        }
    }
    state.topics.delete(&topic).await?;
    Ok(StatusCode::NO_CONTENT)
}
