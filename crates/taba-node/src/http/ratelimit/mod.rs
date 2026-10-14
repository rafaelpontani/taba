//! O rate limit por nó (doc 12, "Rate limits"): token bucket por categoria e por chave,
//! contadores **locais**, sem troca entre nós. Estourado, `429 rate-limit.exceeded` com
//! `Retry-After`.
//!
//! A chave do contador (doc 12): o **`id` do token** quando a chamada traz um token válido
//! (o raiz tem o contador `root`), e o **IP do cliente** nas demais (autenticação desligada,
//! `401`, rotas de operação). Um `403` também conta no token, porque a autorização vem depois.
//!
//! Categorias: publicação, consumo e administração (as rotas de tópico, `/tokens` e `/node/drain`) e a
//! das consultas de operação que falam com outros nós (`/health/cluster?scope=cluster`).
//! `/health/live`, `/health/ready` e `/health/cluster?scope=node` são isentas.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};

use axum::extract::{ConnectInfo, MatchedPath, Request, State};
use axum::http::Method;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use super::AppState;
use super::auth::{Authn, Principal};
use super::common::ApiError;
use crate::config::RateLimit;
use crate::core::clock::Clock;
use crate::core::error::{ErrorCode, TabaError};
use crate::telemetry::metrics::RATELIMIT_REJECTED;

/// Acima disto o mapa de contadores é podado dos que já voltaram ao estouro cheio.
const PRUNE_ABOVE: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Category {
    Publish,
    Consume,
    Admin,
    Ops,
}

impl Category {
    pub const fn as_str(self) -> &'static str {
        match self {
            Category::Publish => "publish",
            Category::Consume => "consume",
            Category::Admin => "admin",
            Category::Ops => "ops",
        }
    }
}

/// De quem é o contador.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Key {
    Token(String),
    Ip(IpAddr),
}

impl From<IpAddr> for Key {
    fn from(ip: IpAddr) -> Self {
        Key::Ip(ip)
    }
}

/// Um contador: `tokens` no instante `at_ms`.
struct Bucket {
    tokens: f64,
    at_ms: u64,
}

pub struct RateLimiter {
    clock: Arc<dyn Clock>,
    /// Requisições por segundo de cada categoria; sem valor, a categoria não tem limite.
    rates: HashMap<Category, f64>,
    /// O estouro: tokens que cabem no contador.
    capacity: HashMap<Category, f64>,
    buckets: Mutex<HashMap<(Category, Key), Bucket>>,
}

impl RateLimiter {
    /// Sem `ratelimit.burst`, o estouro de cada categoria é a sua taxa arredondada para cima
    /// (e no mínimo 1): um segundo de folga.
    pub fn from_config(config: &RateLimit, clock: Arc<dyn Clock>) -> Self {
        let mut rates = HashMap::new();
        let mut capacity = HashMap::new();
        for (category, rps) in [
            (Category::Publish, config.publish_rps),
            (Category::Consume, config.consume_rps),
            (Category::Admin, config.admin_rps),
            (Category::Ops, config.ops_rps),
        ] {
            let Some(rps) = rps.filter(|r| *r > 0.0) else {
                continue;
            };
            rates.insert(category, rps);
            let burst = config.burst.map_or_else(|| rps.ceil(), f64::from);
            capacity.insert(category, burst.max(1.0));
        }
        Self {
            clock,
            rates,
            capacity,
            buckets: Mutex::new(HashMap::new()),
        }
    }

    /// Conta uma chamada. `Err(segundos)` é o `Retry-After`: o tempo até o contador ter de
    /// novo um token inteiro.
    pub fn check(&self, category: Category, key: impl Into<Key>) -> Result<(), u64> {
        let key = key.into();
        let (Some(&rate), Some(&capacity)) =
            (self.rates.get(&category), self.capacity.get(&category))
        else {
            return Ok(());
        };
        let now = self.clock.now_millis();
        let mut buckets = self.buckets.lock().unwrap_or_else(|e| e.into_inner());
        if buckets.len() > PRUNE_ABOVE {
            buckets.retain(|(c, _), b| {
                let refilled = refill(b, self.rate_of(*c), self.capacity_of(*c), now);
                refilled < self.capacity_of(*c)
            });
        }
        let bucket = buckets.entry((category, key)).or_insert(Bucket {
            tokens: capacity,
            at_ms: now,
        });
        bucket.tokens = refill(bucket, rate, capacity, now);
        bucket.at_ms = now;
        if bucket.tokens >= 1.0 {
            bucket.tokens -= 1.0;
            Ok(())
        } else {
            let wait = (1.0 - bucket.tokens) / rate;
            Err((wait.ceil() as u64).max(1))
        }
    }

    fn rate_of(&self, category: Category) -> f64 {
        self.rates.get(&category).copied().unwrap_or(1.0)
    }

    fn capacity_of(&self, category: Category) -> f64 {
        self.capacity.get(&category).copied().unwrap_or(1.0)
    }

    /// Quantos contadores existem (para os testes).
    pub fn tracked(&self) -> usize {
        self.buckets.lock().unwrap_or_else(|e| e.into_inner()).len()
    }
}

/// Os tokens do contador no instante `now_ms`, sem passar do estouro.
fn refill(bucket: &Bucket, rate: f64, capacity: f64, now_ms: u64) -> f64 {
    let elapsed = now_ms.saturating_sub(bucket.at_ms) as f64 / 1000.0;
    (bucket.tokens + elapsed * rate).min(capacity)
}

/// A categoria de uma chamada, pela rota casada e pelo verbo; `None` é isenta.
pub fn classify(method: &Method, route: &str, query: Option<&str>) -> Option<Category> {
    match route {
        "/topics"
        | "/topics/{topic}"
        | "/tokens"
        | "/tokens/{id}"
        | "/node/drain"
        | "/node/backup/flush"
        | "/cluster/backup/flush"
        | "/cluster/stop" => Some(Category::Admin),
        "/topics/{topic}/messages" if method == Method::POST => Some(Category::Publish),
        "/topics/{topic}/messages"
        | "/topics/{topic}/messages/{id}"
        | "/topics/{topic}/messages/{id}/visibility" => Some(Category::Consume),
        // As métricas de tópico, em JSON, medem cada tópico: têm a categoria própria.
        "/metrics/topics" | "/metrics/topics/{topic}" | "/metrics/topics/{topic}/groups" => {
            Some(Category::Ops)
        }
        // Só o `scope=cluster` (o padrão) consulta outros nós.
        "/health/cluster" => {
            let node_only = query
                .into_iter()
                .flat_map(|q| q.split('&'))
                .any(|pair| pair == "scope=node");
            (!node_only).then_some(Category::Ops)
        }
        _ => None,
    }
}

/// O middleware das rotas com limite: acha a categoria, resolve o IP do cliente e conta.
pub async fn rate_limit(State(state): State<AppState>, request: Request, next: Next) -> Response {
    // Uma chamada repassada já foi contada por quem repassou (doc 04, seção 6.9).
    if request
        .extensions()
        .get::<super::forward::Forwarded>()
        .is_some()
    {
        return next.run(request).await;
    }
    let category = request
        .extensions()
        .get::<MatchedPath>()
        .and_then(|route| classify(request.method(), route.as_str(), request.uri().query()));
    if let Some(category) = category {
        let peer = request
            .extensions()
            .get::<ConnectInfo<SocketAddr>>()
            .map(|ConnectInfo(addr)| *addr);
        // O `id` do token só vira chave depois de validado (`authenticate`); sem token válido
        // a chave é o IP.
        let key = match request
            .extensions()
            .get::<Authn>()
            .and_then(|a| a.0.as_ref())
        {
            Some(Principal::Token(record)) => Some(Key::Token(record.id.clone())),
            Some(Principal::Root) => Some(Key::Token("root".to_owned())),
            _ => peer.map(|p| Key::Ip(state.client_ip.resolve(p, request.headers()))),
        };
        if let Some(key) = key
            && let Err(retry_after) = state.rate_limiter.check(category, key.clone())
        {
            tracing::debug!(category = category.as_str(), ?key, "rate limit excedido");
            // O `id` do token nunca é atributo de métrica: só diz se estourou o do token ou o do IP.
            state.telemetry.registry.add(
                &RATELIMIT_REJECTED,
                &[
                    ("taba.category", category.as_str()),
                    (
                        "taba.key",
                        match key {
                            Key::Token(_) => "token",
                            Key::Ip(_) => "ip",
                        },
                    ),
                ],
                1.0,
            );
            return ApiError(
                TabaError::new(
                    ErrorCode::RateLimitExceeded,
                    "Limite de requisições excedido; tente de novo depois do Retry-After.",
                )
                .with_retry_after(retry_after),
            )
            .into_response();
        }
    }
    next.run(request).await
}

#[cfg(test)]
mod tests;
