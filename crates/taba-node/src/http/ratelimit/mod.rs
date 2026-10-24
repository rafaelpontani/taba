//! The per-node rate limit (doc 12, "Rate limits"): token bucket per category and per key,
//! **local** counters, with no exchange between nodes. Exceeded, `429 rate-limit.exceeded` with
//! `Retry-After`.
//!
//! The counter key (doc 12): the **token's `id`** when the call carries a valid token
//! (the root has the `root` counter), and the **client IP** otherwise (authentication off,
//! `401`, operations routes). A `403` also counts against the token, because authorization comes later.
//!
//! Categories: publish, consume and admin (the topic routes, `/tokens`, the drain and emptying a queue) and that
//! of the operations queries that talk to other nodes (`/health/cluster?scope=cluster`, the nodes, the
//! statistics).
//! `/health/live`, `/health/ready` and `/health/cluster?scope=node` are exempt.

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

/// Above this, the counter map is pruned of those that already went back to a full burst.
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

/// Whose counter it is.
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

/// A counter: `tokens` at instant `at_ms`.
struct Bucket {
    tokens: f64,
    at_ms: u64,
}

pub struct RateLimiter {
    clock: Arc<dyn Clock>,
    /// Requests per second of each category; without a value, the category has no limit.
    rates: HashMap<Category, f64>,
    /// The burst: tokens that fit in the counter.
    capacity: HashMap<Category, f64>,
    buckets: Mutex<HashMap<(Category, Key), Bucket>>,
}

impl RateLimiter {
    /// Without `ratelimit.burst`, each category's burst is its rate rounded up
    /// (and at least 1): one second of slack.
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

    /// Counts a call. `Err(seconds)` is the `Retry-After`: the time until the counter has a whole
    /// token again.
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

    /// How many counters exist (for the tests).
    pub fn tracked(&self) -> usize {
        self.buckets.lock().unwrap_or_else(|e| e.into_inner()).len()
    }
}

/// The counter's tokens at instant `now_ms`, without exceeding the burst.
fn refill(bucket: &Bucket, rate: f64, capacity: f64, now_ms: u64) -> f64 {
    let elapsed = now_ms.saturating_sub(bucket.at_ms) as f64 / 1000.0;
    (bucket.tokens + elapsed * rate).min(capacity)
}

/// A call's category, by the matched route and the verb; `None` is exempt.
pub fn classify(method: &Method, route: &str, query: Option<&str>) -> Option<Category> {
    match route {
        "/topics"
        | "/topics/{topic}"
        | "/tokens"
        | "/tokens/{id}"
        | "/node/drain"
        | "/node/backup/flush"
        | "/cluster/backup/flush"
        | "/cluster/stop"
        | "/cluster/restore/force"
        | "/nodes/{nodeId}/drain"
        | "/nodes/{nodeId}/backup/flush" => Some(Category::Admin),
        // Emptying a queue is an administrative act.
        "/topics/{topic}/messages" if method == Method::DELETE => Some(Category::Admin),
        "/topics/{topic}/messages" if method == Method::POST => Some(Category::Publish),
        "/topics/{topic}/messages"
        | "/topics/{topic}/messages/{id}"
        | "/topics/{topic}/messages/{id}/visibility" => Some(Category::Consume),
        // The topic metrics, in JSON, measure each topic: they have a category of their own.
        "/metrics/topics" | "/metrics/topics/{topic}" | "/metrics/topics/{topic}/groups" => {
            Some(Category::Ops)
        }
        // The client API's copies of the monitoring routes: they too measure or ask other nodes.
        "/stats/topics"
        | "/stats/topics/{topic}"
        | "/stats/topics/{topic}/groups"
        | "/nodes"
        | "/cluster/status"
        | "/cluster/backup"
        | "/nodes/{nodeId}/metrics"
        | "/nodes/{nodeId}/backup" => Some(Category::Ops),
        // Only `scope=cluster` (the default) queries other nodes.
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

/// The middleware of the routes with a limit: finds the category, resolves the client IP and counts.
pub async fn rate_limit(State(state): State<AppState>, request: Request, next: Next) -> Response {
    // A forwarded call was already counted by whoever forwarded it (doc 04, section 6.9).
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
        // The token's `id` only becomes a key after being validated (`authenticate`); without a valid token
        // the key is the IP.
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
            // The token's `id` is never a metric attribute: it only says whether the token's or the IP's was exceeded.
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
                    "Request limit exceeded; try again after the Retry-After.",
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
