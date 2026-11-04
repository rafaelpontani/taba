//! The SQS gateway (doc 39): one port that speaks the two SQS protocols (JSON and Query) and turns each action into calls
//! to the Taba client API through the [`Backend`].
//!
//! Every request goes through the same steps: read the body (bounded), check who is calling (SigV4 against the credentials
//! file, or nothing when the Taba authentication is off), decode the protocol into the parameters of the action, run the
//! action and encode the result or the error in the protocol the client used.

pub mod messages;
pub mod protocol_json;
pub mod protocol_query;
pub mod queues;
pub mod receipt;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::Router;
use axum::body::to_bytes;
use axum::extract::{ConnectInfo, Request, State};
use axum::http::{StatusCode, header};
use axum::response::Response;
use serde_json::Value;

use super::aws::auth;
use super::aws::credentials::Credentials;
use super::aws::errors::AwsError;
use super::aws::reply::build;
use super::backend::{Backend, Caller};
use super::naming;
use crate::core::clock::Clock;

/// How long the defaults of a queue (visibility, delay, wait) are remembered: a send or a receive would otherwise read the
/// queue before every call. A change made through this gateway forgets it at once; a change made on another node, or
/// through the native API, is seen after this time.
const SETTINGS_TTL: Duration = Duration::from_secs(2);

/// What every action shares.
pub struct SqsState {
    pub backend: Backend,
    pub credentials: Arc<Credentials>,
    /// The Taba authentication is on: the calls must be signed with a key of the credentials file.
    pub auth_enabled: bool,
    /// `gateway.sqs.public-url`; without it, the `Host` of the request.
    pub public_url: Option<String>,
    pub clock: Arc<dyn Clock>,
    /// The largest request body read (`api.batch.max-bytes`).
    pub max_body: usize,
    settings: Mutex<HashMap<String, (Instant, Arc<queues::Settings>)>>,
}

impl SqsState {
    pub fn new(
        backend: Backend,
        credentials: Arc<Credentials>,
        auth_enabled: bool,
        public_url: Option<String>,
        clock: Arc<dyn Clock>,
        max_body: usize,
    ) -> Self {
        Self {
            backend,
            credentials,
            auth_enabled,
            public_url,
            clock,
            max_body,
            settings: Mutex::new(HashMap::new()),
        }
    }

    /// The remembered defaults of a queue, if they are still fresh.
    fn cached_settings(&self, topic: &str) -> Option<Arc<queues::Settings>> {
        let cache = self.settings.lock().unwrap_or_else(|e| e.into_inner());
        cache
            .get(topic)
            .filter(|(at, _)| at.elapsed() < SETTINGS_TTL)
            .map(|(_, settings)| settings.clone())
    }

    fn remember_settings(&self, topic: &str, settings: Arc<queues::Settings>) {
        let mut cache = self.settings.lock().unwrap_or_else(|e| e.into_inner());
        if cache.len() > 4096 {
            cache.retain(|_, (at, _)| at.elapsed() < SETTINGS_TTL);
        }
        cache.insert(topic.to_owned(), (Instant::now(), settings));
    }

    fn forget_settings(&self, topic: &str) {
        self.settings
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(topic);
    }
}

/// The request being served: the caller and the address the client reached.
pub struct Ctx<'a> {
    pub state: &'a SqsState,
    pub caller: &'a Caller,
    /// `http://host:port`, where the `QueueUrl`s point.
    pub base_url: String,
    /// The region of the ARNs: the one the request was signed for.
    pub region: String,
}

impl Ctx<'_> {
    pub fn backend(&self) -> &Backend {
        &self.state.backend
    }
}

/// The router of the SQS port: every path and method goes to the same handler (the Query protocol may post to the queue
/// URL, the JSON protocol posts to `/`).
pub fn router(state: Arc<SqsState>) -> Router {
    Router::new().fallback(handle).with_state(state)
}

async fn handle(
    State(state): State<Arc<SqsState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    request: Request,
) -> Response {
    let request_id = crate::core::id::new_at(state.clock.now_millis());
    let (parts, body) = request.into_parts();
    let json = protocol_json::is_json(&parts.headers);
    let fail = |error: AwsError| respond_error(json, &error, &request_id);

    let body = match to_bytes(body, state.max_body).await {
        Ok(body) => body,
        Err(_) => {
            return fail(AwsError::invalid_parameter(
                "The request is larger than the limit of the gateway.",
            ));
        }
    };
    let identity = match auth::authenticate(
        "sqs",
        &state.credentials,
        state.auth_enabled,
        state.clock.now_millis(),
        &parts,
        &body,
    ) {
        Ok(identity) => identity,
        Err(error) => return fail(error),
    };
    let (action, mut params) = match decode(json, &parts, &body) {
        Ok(decoded) => decoded,
        Err(error) => return fail(error),
    };
    // The Query protocol may name the queue in the path instead of in `QueueUrl`.
    if params.get("QueueUrl").is_none()
        && let Some(object) = params.as_object_mut()
        && naming::name_in_url(parts.uri.path()).is_ok()
    {
        object.insert(
            "QueueUrl".into(),
            Value::String(parts.uri.path().to_owned()),
        );
    }
    let caller = Caller::of(identity.token, peer, &parts.headers);
    let ctx = Ctx {
        state: &state,
        caller: &caller,
        base_url: base_url(&state, &parts.headers),
        region: identity.region,
    };
    match run(&ctx, &action, &params).await {
        Ok(result) => respond_ok(json, &action, &result, &request_id),
        Err(error) => fail(error),
    }
}

/// `gateway.sqs.public-url`, or `http://<Host>`.
fn base_url(state: &SqsState, headers: &header::HeaderMap) -> String {
    if let Some(url) = &state.public_url {
        return url.clone();
    }
    let host = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("localhost");
    format!("http://{host}")
}

fn decode(
    json: bool,
    parts: &axum::http::request::Parts,
    body: &[u8],
) -> Result<(String, Value), AwsError> {
    if json {
        return Ok((
            protocol_json::action(&parts.headers)?,
            protocol_json::decode(body)?,
        ));
    }
    let text = std::str::from_utf8(body)
        .map_err(|_| AwsError::invalid_parameter("The request body is not valid UTF-8."))?;
    protocol_query::decode(parts.uri.query().unwrap_or(""), text)
}

/// Runs the action. Phase two serves the standard queues; the other actions are refused clearly.
pub async fn run(ctx: &Ctx<'_>, action: &str, params: &Value) -> Result<Value, AwsError> {
    match action {
        "CreateQueue" => queues::create_queue(ctx, params).await,
        "GetQueueUrl" => queues::get_queue_url(ctx, params).await,
        "DeleteQueue" => queues::delete_queue(ctx, params).await,
        "PurgeQueue" => queues::purge_queue(ctx, params).await,
        "ListQueues" => queues::list_queues(ctx, params).await,
        "GetQueueAttributes" => queues::get_queue_attributes(ctx, params).await,
        "SetQueueAttributes" => queues::set_queue_attributes(ctx, params).await,
        "TagQueue" => queues::tag_queue(ctx, params).await,
        "UntagQueue" => queues::untag_queue(ctx, params).await,
        "ListQueueTags" => queues::list_queue_tags(ctx, params).await,
        "SendMessage" => messages::send_message(ctx, params).await,
        "SendMessageBatch" => messages::send_message_batch(ctx, params).await,
        "ReceiveMessage" => messages::receive_message(ctx, params).await,
        "DeleteMessage" => messages::delete_message(ctx, params).await,
        "DeleteMessageBatch" => messages::delete_message_batch(ctx, params).await,
        "ChangeMessageVisibility" => messages::change_message_visibility(ctx, params).await,
        "ChangeMessageVisibilityBatch" => {
            messages::change_message_visibility_batch(ctx, params).await
        }
        "ListDeadLetterSourceQueues" => Ok(serde_json::json!({"QueueUrls": []})),
        "AddPermission"
        | "RemovePermission"
        | "StartMessageMoveTask"
        | "CancelMessageMoveTask"
        | "ListMessageMoveTasks" => Err(AwsError::unsupported(format!(
            "The action {action} is not supported by this gateway."
        ))),
        other => Err(AwsError::invalid_action(other)),
    }
}

fn respond_ok(json: bool, action: &str, result: &Value, request_id: &str) -> Response {
    let (content_type, body) = if json {
        (
            protocol_json::CONTENT_TYPE,
            protocol_json::encode_ok(result),
        )
    } else {
        (
            protocol_query::CONTENT_TYPE,
            protocol_query::encode_ok(action, result, request_id).into_bytes(),
        )
    };
    build(StatusCode::OK, content_type, body, request_id, &[])
}

fn respond_error(json: bool, error: &AwsError, request_id: &str) -> Response {
    let status = StatusCode::from_u16(error.status).unwrap_or(StatusCode::BAD_REQUEST);
    let mut extra: Vec<(&'static str, String)> = Vec::new();
    if let Some(seconds) = error.retry_after {
        extra.push(("retry-after", seconds.to_string()));
    }
    if json {
        let (body, query_error) = protocol_json::encode_error(error);
        extra.push(("x-amzn-query-error", query_error));
        build(
            status,
            protocol_json::CONTENT_TYPE,
            body,
            request_id,
            &extra,
        )
    } else {
        build(
            status,
            protocol_query::CONTENT_TYPE,
            protocol_query::encode_error(error, request_id).into_bytes(),
            request_id,
            &extra,
        )
    }
}

// The readers of the parameters are shared with SNS.
pub use super::aws::params::{number, number_in, required_text, text, text_map, texts, url_encode};

#[cfg(test)]
mod tests;
