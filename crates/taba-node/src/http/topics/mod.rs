//! Client API topics (doc 06a): `POST` and `GET /topics`, and `GET`, `PATCH` and
//! `DELETE /topics/{topic}`. Here the request is only read and the response built; the
//! rules live in the `engine`.

mod request;

use std::collections::HashMap;

use axum::body::Bytes;
use axum::extract::{Extension, Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{Map, Value};

use super::auth::{Principal, out_of_scope};
use super::common::ApiError;
use super::{AppState, json};
use crate::engine::topics::ListQuery;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/topics", get(list).post(create))
        .route("/topics/{topic}", get(read).patch(update).delete(remove))
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

async fn read(
    State(state): State<AppState>,
    Path(topic): Path<String>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(state.topics.get(&topic)?.to_json()))
}

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

async fn remove(
    State(state): State<AppState>,
    Path(topic): Path<String>,
) -> Result<StatusCode, ApiError> {
    state.topics.delete(&topic).await?;
    Ok(StatusCode::NO_CONTENT)
}
