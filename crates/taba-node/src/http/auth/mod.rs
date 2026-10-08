//! Autenticação e autorização por token (doc 06a, seção 8.1.3).
//!
//! Três camadas nas rotas de cliente, de fora para dentro:
//! 1. [`authenticate`] descobre **quem** chama (token raiz, token de trabalho ou ninguém) e
//!    guarda o resultado; não recusa nada, porque o rate limit vem em seguida e precisa da
//!    chave (o `id` do token, ou o IP quando não há token válido);
//! 2. o rate limit (`http::ratelimit`);
//! 3. [`authorize`] recusa: `401` sem token válido, `403 auth.forbidden` sem a permissão da
//!    rota e `403 auth.out-of-scope` fora dos prefixos do token.
//!
//! Com a autenticação desligada (sem token raiz, ou sempre em desenvolvimento) todo mundo é
//! [`Principal::Open`] e nada é conferido. As rotas de operação e do cluster não passam por aqui.

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use axum::extract::{MatchedPath, RawPathParams, Request, State};
use axum::http::{HeaderMap, Method, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use super::AppState;
use super::common::ApiError;
use crate::cluster::state::DrainState;
use crate::config::{Auth as AuthConfig, RootToken};
use crate::core::error::{ErrorCode, TabaError};
use crate::core::token::{Permission, TokenRecord, constant_time_eq};
use crate::engine::tokens::Tokens;
use crate::telemetry::metrics::AUTH_REQUESTS;

/// Quem está chamando.
#[derive(Debug, Clone)]
pub enum Principal {
    /// Autenticação desligada: acesso total, sem conferência.
    Open,
    /// O token raiz: todas as permissões, sem prefixos, e o único que gerencia `/tokens`.
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

    /// Os prefixos do token; vazio alcança todos os tópicos.
    pub fn prefixes(&self) -> &[String] {
        match self {
            Principal::Token(record) => &record.topic_prefixes,
            _ => &[],
        }
    }

    /// Um prefixo de distribuição só é aceito se **toda** fila que ele alcança está no alcance
    /// do token: ele mesmo começa com um dos prefixos do token (senão o token injetaria
    /// mensagens em filas fora do seu alcance).
    pub fn covers_distribution_prefix(&self, prefix: &str) -> bool {
        let own = self.prefixes();
        own.is_empty() || own.iter().any(|p| prefix.starts_with(p.as_str()))
    }

    /// Identificação em logs e traces: o `id` do token, ou `root`.
    pub fn log_id(&self) -> &str {
        match self {
            Principal::Open => "-",
            Principal::Root => "root",
            Principal::Token(record) => &record.id,
        }
    }

    fn is_root(&self) -> bool {
        matches!(self, Principal::Open | Principal::Root)
    }
}

/// O que `authenticate` descobriu: `None` é "sem token válido".
#[derive(Debug, Clone)]
pub struct Authn(pub Option<Principal>);

/// Os segredos do token raiz: o inline, ou as até duas linhas do arquivo (a atual e a
/// anterior), relido a cada `auth.reload-interval`.
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

    /// Relê o arquivo. Se não der (sumiu, ficou vazio), segue com o que tinha.
    pub fn reload(&self) {
        let Some(path) = &self.file else { return };
        match read_lines(path) {
            Some(values) => {
                *self.values.write().unwrap_or_else(|e| e.into_inner()) = values;
            }
            None => tracing::warn!(
                "não foi possível reler auth.root-token-file; o token raiz anterior segue valendo"
            ),
        }
    }

    /// Confere em tempo constante contra todos os valores, sem parar no primeiro que casa.
    pub fn matches(&self, presented: &str) -> bool {
        let values = self.values.read().unwrap_or_else(|e| e.into_inner());
        values.iter().fold(false, |found, value| {
            constant_time_eq(value.as_bytes(), presented.as_bytes()) | found
        })
    }
}

/// As linhas não vazias do arquivo, no máximo duas. `None` se não leu ou não há nenhuma.
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

/// O estado de autenticação do nó.
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

    /// O token do cabeçalho `Authorization: Bearer <token>`.
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

/// A camada de fora: descobre quem chama. Em drain o nó não valida o token (doc 12: conta por
/// IP e repassa); desligada a autenticação, todo mundo é `Open`.
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

/// O que a rota exige.
enum Need {
    /// Qualquer token válido (leitura de tópicos, dentro dos prefixos).
    Any,
    Permission(Permission),
    /// Só o token raiz (`/tokens`).
    Root,
}

fn need(method: &Method, route: &str) -> Option<Need> {
    match route {
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
        _ => None,
    }
}

fn unauthorized() -> ApiError {
    // A resposta é igual para token ausente, mal formado, desconhecido, revogado ou vencido.
    ApiError(TabaError::new(
        ErrorCode::AuthUnauthorized,
        "Token ausente, inválido ou vencido.",
    ))
}

fn forbidden() -> ApiError {
    ApiError(TabaError::new(
        ErrorCode::AuthForbidden,
        "O token não tem a permissão que esta rota exige.",
    ))
}

pub fn out_of_scope() -> TabaError {
    TabaError::new(
        ErrorCode::AuthOutOfScope,
        "O tópico está fora dos prefixos do token.",
    )
}

/// A camada de dentro: `401`, `403 auth.forbidden` e `403 auth.out-of-scope`; passa o
/// [`Principal`] aos handlers.
pub async fn authorize(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Response {
    // `taba.auth.requests` só existe com a autenticação ligada; o `id` do token nunca é atributo.
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
        tracing::warn!("chamada recusada: sem token válido");
        return unauthorized().into_response();
    };
    let route = request
        .extensions()
        .get::<MatchedPath>()
        .map(|m| m.as_str().to_owned())
        .unwrap_or_default();
    let refused = match need(request.method(), &route) {
        Some(Need::Root) => !principal.is_root(),
        Some(Need::Permission(permission)) => !principal.allows(permission),
        Some(Need::Any) | None => false,
    };
    if refused {
        count("forbidden");
        tracing::warn!(token.id = %principal.log_id(), route = %route, "chamada recusada: sem permissão");
        return forbidden().into_response();
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
            tracing::warn!(token.id = %principal.log_id(), "chamada recusada: fora dos prefixos");
            return ApiError(out_of_scope()).into_response();
        }
    }
    count("ok");
    request.extensions_mut().insert(principal);
    next.run(request).await
}

#[cfg(test)]
mod tests;
