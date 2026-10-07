//! O que vale para as três APIs (doc 06d, seção 8.3): `X-Request-Id` em toda
//! resposta, `ErrorResponse` em todo erro, `404 route.not-found` e `500 internal.error`.

use std::any::Any;
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{HeaderName, HeaderValue, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use tower_http::catch_panic::CatchPanicLayer;

use super::AppState;
use crate::core::clock::Clock;
use crate::core::error::TabaError;
use crate::core::id;

const REQUEST_ID: HeaderName = HeaderName::from_static("x-request-id");

/// Um erro de handler. O corpo só é montado pelo middleware de `X-Request-Id`, que é
/// quem sabe o `requestId`; até lá o erro viaja numa extensão da resposta.
pub struct ApiError(pub TabaError);

impl From<TabaError> for ApiError {
    fn from(err: TabaError) -> Self {
        Self(err)
    }
}

#[derive(Clone)]
struct ErrorSlot(TabaError);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status =
            StatusCode::from_u16(self.0.status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        let mut response = Response::new(Body::empty());
        *response.status_mut() = status;
        if let Some(seconds) = self.0.retry_after {
            response
                .headers_mut()
                .insert(header::RETRY_AFTER, HeaderValue::from(seconds));
        }
        if status == StatusCode::UNAUTHORIZED {
            response
                .headers_mut()
                .insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
        }
        response.extensions_mut().insert(ErrorSlot(self.0));
        response
    }
}

/// `X-Request-Id` do cliente: de 1 a 128 caracteres de `A-Za-z0-9_-.:`.
fn client_request_id(request: &Request) -> Option<String> {
    let value = request.headers().get(&REQUEST_ID)?.to_str().ok()?;
    let valid = !value.is_empty()
        && value.len() <= 128
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_-.:".contains(c));
    valid.then(|| value.to_owned())
}

async fn request_context(
    State(clock): State<Arc<dyn Clock>>,
    request: Request,
    next: Next,
) -> Response {
    let request_id = client_request_id(&request).unwrap_or_else(|| id::new_at(clock.now_millis()));
    let mut response = next.run(request).await;

    if let Some(ErrorSlot(error)) = response.extensions_mut().remove::<ErrorSlot>() {
        let body = serde_json::to_vec(&error.to_response(&request_id)).unwrap_or_default();
        response.headers_mut().remove(header::CONTENT_LENGTH);
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
        *response.body_mut() = Body::from(body);
    }
    if let Ok(value) = HeaderValue::from_str(&request_id) {
        response.headers_mut().insert(REQUEST_ID, value);
    }
    response
}

async fn route_not_found() -> ApiError {
    TabaError::route_not_found().into()
}

fn panic_to_500(_: Box<dyn Any + Send + 'static>) -> Response {
    ApiError(TabaError::internal()).into_response()
}

/// Acrescenta ao roteador o que as três APIs têm em comum.
pub fn finish(routes: Router<AppState>, state: AppState) -> Router {
    let clock = state.clock.clone();
    routes
        .fallback(route_not_found)
        .with_state(state)
        .layer(CatchPanicLayer::custom(panic_to_500))
        .layer(middleware::from_fn_with_state(clock, request_context))
}

#[cfg(test)]
mod tests;
