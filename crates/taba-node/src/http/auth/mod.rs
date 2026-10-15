//! Token authentication and authorization (doc 06a, section 8.1.3).
//!
//! Three layers on the client routes, from the outside in:
//! 1. [`authenticate`] finds out **who** is calling (root token, work token or nobody) and
//!    stores the result; it refuses nothing, because the rate limit comes next and needs the
//!    key (the token's `id`, or the IP when there is no valid token);
//! 2. the rate limit (`http::ratelimit`);
//! 3. [`authorize`] refuses: `401` without a valid token, `403 auth.forbidden` without the route's
//!    permission and `403 auth.out-of-scope` outside the token's prefixes.
//!
//! With authentication off (no root token, or always in development) everyone is
//! [`Principal::Open`] and nothing is checked. The operations and cluster routes do not go through here.

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use axum::extract::{Extension, MatchedPath, RawPathParams, Request, State};
use axum::http::{HeaderMap, Method, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{Value, json};

use super::AppState;
use super::common::ApiError;
use crate::cluster::state::DrainState;
use crate::config::{Auth as AuthConfig, RootToken};
use crate::core::error::{ErrorCode, TabaError};
use crate::core::time::format_ms;
use crate::core::token::{Permission, TokenRecord, constant_time_eq};
use crate::engine::tokens::Tokens;
use crate::telemetry::metrics::AUTH_REQUESTS;

/// Who is calling.
#[derive(Debug, Clone)]
pub enum Principal {
    /// Authentication off: full access, no checking.
    Open,
    /// The root token: all permissions, no prefixes, and the only one that manages `/tokens`.
    Root,
    Token(Arc<TokenRecord>),
}

impl Principal {
    pub fn allows(&self, permission: Permission) -> bool {
        match self {
            Principal::Open | Principal::Root => true,
            Principal::Token(record) => record.allows(permission),
        }
    }

    pub fn reaches(&self, topic: &str) -> bool {
        match self {
            Principal::Open | Principal::Root => true,
            Principal::Token(record) => record.reaches(topic),
        }
    }

    /// The token's prefixes; empty reaches every topic.
    pub fn prefixes(&self) -> &[String] {
        match self {
            Principal::Token(record) => &record.topic_prefixes,
            _ => &[],
        }
    }

    /// A distribution prefix is only accepted if **every** queue it reaches is within the token's
    /// reach: it itself starts with one of the token's prefixes (otherwise the token would inject
    /// messages into queues outside its reach).
    pub fn covers_distribution_prefix(&self, prefix: &str) -> bool {
        let own = self.prefixes();
        own.is_empty() || own.iter().any(|p| prefix.starts_with(p.as_str()))
    }

    /// Identification in logs and traces: the token's `id`, or `root`.
    pub fn log_id(&self) -> &str {
        match self {
            Principal::Open => "-",
            Principal::Root => "root",
            Principal::Token(record) => &record.id,
        }
    }

    pub(super) fn is_root(&self) -> bool {
        matches!(self, Principal::Open | Principal::Root)
    }
}

/// What `authenticate` found out: `None` is "no valid token".
#[derive(Debug, Clone)]
pub struct Authn(pub Option<Principal>);

/// The root token's secrets: the inline one, or the up to two lines of the file (the current and the
/// previous), re-read every `auth.reload-interval`.
pub struct RootTokens {
    values: RwLock<Vec<String>>,
    file: Option<PathBuf>,
}

impl RootTokens {
    pub fn from_config(config: &RootToken) -> Self {
        match config {
            RootToken::None => Self {
                values: RwLock::new(Vec::new()),
                file: None,
            },
            RootToken::Inline(secret) => Self {
                values: RwLock::new(vec![secret.expose().to_owned()]),
                file: None,
            },
            RootToken::File(path) => {
                let values = read_lines(path).unwrap_or_default();
                Self {
                    values: RwLock::new(values),
                    file: Some(path.clone()),
                }
            }
        }
    }

    pub fn has_file(&self) -> bool {
        self.file.is_some()
    }

    /// Re-reads the file. If it fails (vanished, became empty), it carries on with what it had.
    pub fn reload(&self) {
        let Some(path) = &self.file else { return };
        match read_lines(path) {
            Some(values) => {
                *self.values.write().unwrap_or_else(|e| e.into_inner()) = values;
            }
            None => tracing::warn!(
                "could not re-read auth.root-token-file; the previous root token keeps applying"
            ),
        }
    }

    /// Checks in constant time against all the values, without stopping at the first one that matches.
    pub fn matches(&self, presented: &str) -> bool {
        let values = self.values.read().unwrap_or_else(|e| e.into_inner());
        values.iter().fold(false, |found, value| {
            constant_time_eq(value.as_bytes(), presented.as_bytes()) | found
        })
    }
}

/// The non-empty lines of the file, at most two. `None` if it could not be read or there are none.
fn read_lines(path: &Path) -> Option<Vec<String>> {
    let text = std::fs::read_to_string(path).ok()?;
    let lines: Vec<String> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .take(2)
        .map(str::to_owned)
        .collect();
    (!lines.is_empty()).then_some(lines)
}

/// The node's authentication state.
pub struct Auth {
    pub enabled: bool,
    pub root: RootTokens,
    pub tokens: Arc<Tokens>,
}

impl Auth {
    pub fn new(config: &AuthConfig, tokens: Arc<Tokens>) -> Self {
        Self {
            enabled: config.enabled,
            root: RootTokens::from_config(&config.root_token),
            tokens,
        }
    }

    /// The token from the `Authorization: Bearer <token>` header.
    fn principal(&self, headers: &HeaderMap) -> Option<Principal> {
        let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
        let (scheme, presented) = value.split_once(' ')?;
        let presented = presented.trim();
        if !scheme.eq_ignore_ascii_case("bearer") || presented.is_empty() {
            return None;
        }
        if self.root.matches(presented) {
            return Some(Principal::Root);
        }
        self.tokens
            .authenticate(presented)
            .map(|record| Principal::Token(Arc::new(record)))
    }
}

/// The outer layer: finds out who is calling. In drain the node does not validate the token (doc 12: it counts by
/// IP and forwards); with authentication off, everyone is `Open`.
pub async fn authenticate(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Response {
    let principal = if !state.auth.enabled {
        Some(Principal::Open)
    } else if state.node.drain_state() != DrainState::Active {
        None
    } else {
        state.auth.principal(request.headers())
    };
    request.extensions_mut().insert(Authn(principal));
    next.run(request).await
}

/// What the route requires.
enum Need {
    /// Any valid token (topic reads, within the prefixes).
    Any,
    Permission(Permission),
    /// Only the root token (`/tokens`).
    Root,
}

fn need(method: &Method, route: &str) -> Option<Need> {
    match route {
        // Pruning a queue empties it: the `admin` permission of the topic.
        "/topics/{topic}/messages" if method == Method::DELETE => {
            Some(Need::Permission(Permission::Admin))
        }
        "/topics" if method == Method::POST => Some(Need::Permission(Permission::Create)),
        "/topics" | "/topics/{topic}" if method == Method::GET => Some(Need::Any),
        "/topics/{topic}" => Some(Need::Permission(Permission::Admin)),
        "/topics/{topic}/messages" if method == Method::POST => {
            Some(Need::Permission(Permission::Publish))
        }
        "/topics/{topic}/messages"
        | "/topics/{topic}/messages/{id}"
        | "/topics/{topic}/messages/{id}/visibility" => Some(Need::Permission(Permission::Consume)),
        "/tokens" | "/tokens/{id}" => Some(Need::Root),
        // What any valid token sees within its scope: who it is, the nodes, the topic statistics.
        "/auth/me" | "/nodes" => Some(Need::Any),
        "/stats/topics" | "/stats/topics/{topic}" | "/stats/topics/{topic}/groups" => {
            Some(Need::Any)
        }
        // The cluster and the nodes' insides: the root token.
        "/cluster/status"
        | "/cluster/backup"
        | "/cluster/backup/flush"
        | "/cluster/stop"
        | "/nodes/{node_id}/metrics"
        | "/nodes/{node_id}/drain"
        | "/nodes/{node_id}/backup"
        | "/nodes/{node_id}/backup/flush" => Some(Need::Root),
        _ => None,
    }
}

fn unauthorized() -> ApiError {
    // The response is the same for a missing, malformed, unknown, revoked or expired token.
    ApiError(TabaError::new(
        ErrorCode::AuthUnauthorized,
        "Missing, invalid or expired token.",
    ))
}

/// `403 auth.forbidden`, saying what was missing: `root` or the permission's name.
fn forbidden(required: &str) -> ApiError {
    ApiError(
        TabaError::new(
            ErrorCode::AuthForbidden,
            "The token does not have the permission this route requires.",
        )
        .with_required(required),
    )
}

pub fn out_of_scope() -> TabaError {
    TabaError::new(
        ErrorCode::AuthOutOfScope,
        "The topic is outside the token's prefixes.",
    )
}

/// The inner layer: `401`, `403 auth.forbidden` and `403 auth.out-of-scope`; passes the
/// [`Principal`] to the handlers.
pub async fn authorize(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Response {
    // `taba.auth.requests` only exists with authentication on; the token's `id` is never an attribute.
    let count = |outcome: &str| {
        if state.auth.enabled {
            state
                .telemetry
                .registry
                .add(&AUTH_REQUESTS, &[("taba.outcome", outcome)], 1.0);
        }
    };
    let principal = request
        .extensions()
        .get::<Authn>()
        .and_then(|authn| authn.0.clone());
    let Some(principal) = principal else {
        count("unauthorized");
        tracing::warn!("call refused: no valid token");
        return unauthorized().into_response();
    };
    let route = request
        .extensions()
        .get::<MatchedPath>()
        .map(|m| m.as_str().to_owned())
        .unwrap_or_default();
    let refused = match need(request.method(), &route) {
        Some(Need::Root) if !principal.is_root() => Some("root"),
        Some(Need::Permission(permission)) if !principal.allows(permission) => {
            Some(permission.as_str())
        }
        _ => None,
    };
    if let Some(required) = refused {
        count("forbidden");
        tracing::warn!(token.id = %principal.log_id(), route = %route, "call refused: no permission");
        return forbidden(required).into_response();
    }
    if route.contains("{topic}") {
        let (mut parts, body) = request.into_parts();
        let topic = {
            use axum::extract::FromRequestParts;
            RawPathParams::from_request_parts(&mut parts, &())
                .await
                .ok()
                .and_then(|params| {
                    params
                        .iter()
                        .find(|(name, _)| *name == "topic")
                        .map(|(_, value)| value.to_owned())
                })
        };
        request = Request::from_parts(parts, body);
        if let Some(topic) = topic
            && !principal.reaches(&topic)
        {
            count("forbidden");
            tracing::warn!(token.id = %principal.log_id(), "call refused: outside the prefixes");
            return ApiError(out_of_scope()).into_response();
        }
    }
    count("ok");
    request.extensions_mut().insert(principal);
    next.run(request).await
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/auth/me", get(me))
}

/// `GET /auth/me`: who the token is and what it can do, so a client can enable or disable what it
/// shows before calling. With authentication off everyone is the `open` caller, with every permission.
async fn me(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
) -> Json<Value> {
    let all: Vec<&str> = Permission::ALL.iter().map(|p| p.as_str()).collect();
    Json(match &principal {
        Principal::Token(record) => json!({
            "authEnabled": state.auth.enabled,
            "root": false,
            "id": record.id,
            "name": record.name,
            "permissions": record.permissions.iter().map(|p| p.as_str()).collect::<Vec<_>>(),
            "topicPrefixes": record.topic_prefixes,
            "expiresAt": record.expires_ms.map(format_ms),
        }),
        Principal::Root | Principal::Open => json!({
            "authEnabled": state.auth.enabled,
            "root": true,
            "id": Value::Null,
            "name": if state.auth.enabled { "root" } else { "open" },
            "permissions": all,
            "topicPrefixes": Vec::<String>::new(),
            "expiresAt": Value::Null,
        }),
    })
}

#[cfg(test)]
mod tests;
