//! Tópicos da API de cliente (doc 06a): `POST` e `GET /topics`, e `GET`, `PATCH` e
//! `DELETE /topics/{topic}`. Aqui só se lê a requisição e se monta a resposta; as
//! regras ficam no `engine`.

mod request;

use std::collections::HashMap;

use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{Map, Value};

use super::common::ApiError;
use super::{AppState, json};
use crate::engine::topics::ListQuery;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/topics", get(list).post(create))
        .route("/topics/{topic}", get(read).patch(update).delete(remove))
}

async fn create(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let fields = json::object(&headers, &body)?;
    let view = state.topics.create(request::create(&fields)?).await?;
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
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    let result = state.topics.list(ListQuery {
        limit: params.get("limit").cloned(),
        cursor: params.get("cursor").cloned(),
        sort: params.get("sort").cloned(),
        name: params.get("name").cloned(),
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
    Path(topic): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<Value>, ApiError> {
    let fields = json::object(&headers, &body)?;
    let view = state
        .topics
        .update(&topic, request::update(&fields)?)
        .await?;
    Ok(Json(view.to_json()))
}

async fn remove(
    State(state): State<AppState>,
    Path(topic): Path<String>,
) -> Result<StatusCode, ApiError> {
    state.topics.delete(&topic).await?;
    Ok(StatusCode::NO_CONTENT)
}
