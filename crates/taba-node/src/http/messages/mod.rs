//! Mensagens da API de cliente (doc 06a, "Mensagens"): `POST` (publicar) e `GET`
//! (consumir) em `/topics/{topic}/messages`. Aqui só se lê a requisição e se monta a
//! resposta; as regras ficam no `engine`.

use std::collections::HashMap;

use axum::body::{Body, Bytes};
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, put};
use axum::{Json, Router};
use http_body_util::LengthLimitError;
use serde::Serialize;
use serde_json::{Map, Value, json};

use super::common::ApiError;
use super::{AppState, json};
use crate::core::content;
use crate::core::error::{ErrorCode, TabaError};
use crate::core::time;
use crate::engine::messages::{
    ConsumeQuery, Delivered, DistributorDelivery, Messages, PublishRequest, Published,
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/topics/{topic}/messages", get(consume).post(publish))
        .route("/topics/{topic}/messages/{id}", delete(ack))
        .route("/topics/{topic}/messages/{id}/visibility", put(visibility))
}

/// Lê o corpo da requisição até `limit` bytes. Passou disso: `413`, sem ler o resto.
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
                    format!("O corpo passa do limite de {limit} bytes do tópico."),
                )
            } else {
                TabaError::new(
                    ErrorCode::ValidationFailed,
                    "Não foi possível ler o corpo da requisição.",
                )
            })
        }
    }
}

/// O `Content-Type` como texto. Presente mas ilegível: o `engine` o recusa como inválido.
fn content_type(headers: &HeaderMap) -> Option<String> {
    headers.get(header::CONTENT_TYPE).map(|value| {
        value
            .to_str()
            .map_or_else(|_| "\u{fffd}".to_owned(), str::to_owned)
    })
}

async fn publish(
    State(state): State<AppState>,
    Path(topic): Path<String>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
    body: Body,
) -> Result<Response, ApiError> {
    // O tópico é conferido (`404`) e o limite descoberto antes de ler o corpo.
    let limit = state.messages.publish_limit(&topic)?;
    let body = read_body(body, limit).await?;
    let request = PublishRequest {
        body,
        content_type: content_type(&headers),
        group_id: params.get("groupId").cloned(),
        deduplication_id: params.get("deduplicationId").cloned(),
    };
    let published = blocking(&state, move |messages| messages.publish(&topic, request)).await?;
    Ok(match published {
        Published::Queue { id } => (StatusCode::CREATED, Json(json!({ "id": id }))).into_response(),
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

/// O `Message` da API (doc 06a, "DTOs").
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
    dead_lettered_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    dead_letter_reason: Option<String>,
}

/// `ConsumeResponse`: serializado direto, para o `body` JSON não passar por um `Value` e perder
/// a forma em que foi enviado.
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
            dead_lettered_at: m.dead_lettered_ms.map(time::format_ms),
            dead_letter_reason: m.dead_letter_reason,
        }
    }
}

async fn consume(
    State(state): State<AppState>,
    Path(topic): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<ConsumeResponse>, ApiError> {
    let delivered = state
        .messages
        .consume(
            &topic,
            ConsumeQuery {
                max_messages: params.get("maxMessages").cloned(),
                visibility_timeout: params.get("visibilityTimeout").cloned(),
                wait_time: params.get("waitTime").cloned(),
            },
        )
        .await?;
    Ok(Json(ConsumeResponse {
        messages: delivered.into_iter().map(Into::into).collect(),
    }))
}

async fn ack(
    State(state): State<AppState>,
    Path((topic, id)): Path<(String, String)>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<StatusCode, ApiError> {
    let receipt = params.get("receiptToken").cloned();
    blocking(&state, move |messages| {
        messages.ack(&topic, &id, receipt.as_deref())
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn visibility(
    State(state): State<AppState>,
    Path((topic, id)): Path<(String, String)>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<Value>, ApiError> {
    let fields = json::object(&headers, &body)?;
    fields.reject_unknown(&["visibilityTimeout"])?;
    let receipt = params.get("receiptToken").cloned();
    let timeout = fields.integer("visibilityTimeout")?;
    let updated = blocking(&state, move |messages| {
        messages.set_visibility(&topic, &id, receipt.as_deref(), timeout)
    })
    .await?;
    Ok(Json(json!({
        "id": updated.id,
        "invisibleUntil": time::format_ms(updated.invisible_until_ms),
    })))
}

/// Roda uma operação de mensagem fora do executor assíncrono: ela bloqueia no disco (doc 18,
/// seção 17.6).
async fn blocking<T, F>(state: &AppState, operation: F) -> Result<T, ApiError>
where
    T: Send + 'static,
    F: FnOnce(&Messages) -> Result<T, TabaError> + Send + 'static,
{
    let messages = state.messages.clone();
    tokio::task::spawn_blocking(move || operation(&messages))
        .await
        .map_err(|_| ApiError(TabaError::internal()))?
        .map_err(ApiError)
}
