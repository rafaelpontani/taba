//! Client API access tokens (doc 06a, "Access tokens"): `POST` and `GET /tokens` and
//! `GET`, `PATCH` and `DELETE /tokens/{id}`. They only exist with authentication on, and only the root
//! token and the `admin` tokens use them (the `authorize` layer refuses the rest); an `admin` token itself
//! is only created, changed and revoked by the root. The full secret shows up **only** in the
//! creation response.

use std::collections::HashMap;

use axum::body::Bytes;
use axum::extract::{Extension, Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{Map, Value, json};

use super::auth::{Principal, forbidden};
use super::common::ApiError;
use super::{AppState, json as body};
use crate::core::time::format_ms;
use crate::core::token::{Permission, TokenRecord};
use crate::engine::tokens::{CreateToken, ListQuery, UpdateToken};

const CREATE_FIELDS: &[&str] = &["name", "permissions", "topicPrefixes", "expiresAt"];
const PATCH_FIELDS: &[&str] = &["name", "expiresAt"];
/// Fields the `PATCH` recognizes only to refuse them as immutable.
const IMMUTABLE_FIELDS: &[&str] = &["permissions", "topicPrefixes", "id", "createdAt", "token"];

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/tokens", get(list).post(create))
        .route("/tokens/{id}", get(read).patch(update).delete(revoke))
}

/// The doc 06a `Token`: never carries the secret or the hash.
fn token_json(record: &TokenRecord) -> Map<String, Value> {
    let mut object = Map::new();
    object.insert("id".into(), json!(record.id));
    object.insert("name".into(), json!(record.name));
    object.insert(
        "permissions".into(),
        json!(
            record
                .permissions
                .iter()
                .map(|p| p.as_str())
                .collect::<Vec<_>>()
        ),
    );
    object.insert("topicPrefixes".into(), json!(record.topic_prefixes));
    object.insert("createdAt".into(), json!(format_ms(record.created_ms)));
    object.insert(
        "expiresAt".into(),
        record
            .expires_ms
            .map_or(Value::Null, |ms| json!(format_ms(ms))),
    );
    object
}

/// An `admin` token is only created, changed and revoked by the root token: otherwise an `admin` could
/// raise others to its own level or remove its peers.
fn root_for_admin_tokens(principal: &Principal, is_admin_token: bool) -> Result<(), ApiError> {
    if is_admin_token && !principal.is_root() {
        return Err(forbidden("root"));
    }
    Ok(())
}

fn is_admin_token(state: &AppState, id: &str) -> bool {
    state
        .auth
        .tokens
        .record(id)
        .is_some_and(|record| record.permissions.contains(&Permission::Admin))
}

async fn create(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    headers: HeaderMap,
    bytes: Bytes,
) -> Result<Response, ApiError> {
    let fields = body::object(&headers, &bytes)?;
    fields.reject_unknown(CREATE_FIELDS)?;
    fields.require(&["name", "permissions"])?;
    let request = CreateToken {
        name: fields.string("name")?.unwrap_or_default(),
        permissions: fields.string_list("permissions")?.unwrap_or_default(),
        topic_prefixes: fields.string_list("topicPrefixes")?.unwrap_or_default(),
        expires_ms: fields.date("expiresAt")?,
    };
    root_for_admin_tokens(&principal, request.permissions.iter().any(|p| p == "admin"))?;
    let created = state.auth.tokens.create(request).await?;
    let mut object = token_json(&created.record);
    object.insert("token".into(), json!(created.token));
    let location = format!("/tokens/{}", created.record.id);
    Ok((
        StatusCode::CREATED,
        [(header::LOCATION, location)],
        Json(Value::Object(object)),
    )
        .into_response())
}

async fn list(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    let result = state.auth.tokens.list(ListQuery {
        limit: params.get("limit").cloned(),
        cursor: params.get("cursor").cloned(),
        sort: params.get("sort").cloned(),
        name: params.get("name").cloned(),
    })?;
    let mut page = Map::new();
    page.insert(
        "items".into(),
        Value::Array(
            result
                .items
                .iter()
                .map(|t| Value::Object(token_json(t)))
                .collect(),
        ),
    );
    if let Some(cursor) = result.next_cursor {
        page.insert("nextCursor".into(), Value::String(cursor));
    }
    Ok(Json(Value::Object(page)))
}

async fn read(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(Value::Object(token_json(
        &state.auth.tokens.get(&id)?,
    ))))
}

async fn update(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Extension(principal): Extension<Principal>,
    headers: HeaderMap,
    bytes: Bytes,
) -> Result<Json<Value>, ApiError> {
    root_for_admin_tokens(&principal, is_admin_token(&state, &id))?;
    let fields = body::object(&headers, &bytes)?;
    let known: Vec<&str> = PATCH_FIELDS
        .iter()
        .chain(IMMUTABLE_FIELDS)
        .copied()
        .collect();
    fields.reject_unknown(&known)?;
    let request = UpdateToken {
        immutable_present: IMMUTABLE_FIELDS
            .iter()
            .filter(|f| fields.has(f))
            .map(|f| (*f).to_owned())
            .collect(),
        name: fields.string("name")?,
        expires_ms: fields.patch_date("expiresAt")?,
    };
    let record = state.auth.tokens.update(&id, request).await?;
    Ok(Json(Value::Object(token_json(&record))))
}

async fn revoke(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Extension(principal): Extension<Principal>,
) -> Result<StatusCode, ApiError> {
    root_for_admin_tokens(&principal, is_admin_token(&state, &id))?;
    state.auth.tokens.revoke(&id).await?;
    Ok(StatusCode::NO_CONTENT)
}
