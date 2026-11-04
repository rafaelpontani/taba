//! The SNS gateway (doc 39): one port that speaks the SNS Query protocol and turns each action into calls to the Taba client
//! API through the [`Backend`].
//!
//! An SNS topic is a Taba **distributor** with the same name, and a subscription of a queue is one of its `prefixes`: the
//! distributor already copies each publication to the queues whose name starts with a prefix, on every node, so the gateway
//! does no fan-out and keeps no state. Every request goes through the same steps as in SQS: read the body (bounded), check
//! who is calling, decode the form into the parameters of the action, run the action, and encode the result or the error.

pub mod protocol;
pub mod publish;
pub mod subscriptions;
pub mod topics;

use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use axum::body::to_bytes;
use axum::extract::{ConnectInfo, Request, State};
use axum::http::StatusCode;
use axum::response::Response;
use serde_json::Value;

use super::aws::auth;
use super::aws::credentials::Credentials;
use super::aws::errors::AwsError;
use super::aws::reply::build;
use super::backend::{Backend, Caller};
use crate::core::clock::Clock;

/// The actions of SNS, the ones served and the ones refused with a message: the port of the gateway uses it to tell an unsigned
/// SNS call from an SQS one.
pub const ACTIONS: &[&str] = &[
    "CreateTopic",
    "DeleteTopic",
    "ListTopics",
    "GetTopicAttributes",
    "SetTopicAttributes",
    "TagResource",
    "UntagResource",
    "ListTagsForResource",
    "Publish",
    "PublishBatch",
    "Subscribe",
    "Unsubscribe",
    "ListSubscriptions",
    "ListSubscriptionsByTopic",
    "GetSubscriptionAttributes",
    "SetSubscriptionAttributes",
    "ConfirmSubscription",
    "GetDataProtectionPolicy",
    "PutDataProtectionPolicy",
    "CreatePlatformApplication",
    "CreatePlatformEndpoint",
    "CheckIfPhoneNumberIsOptedOut",
    "OptInPhoneNumber",
    "SetSMSAttributes",
];

/// What every action shares.
pub struct SnsState {
    pub backend: Backend,
    pub credentials: Arc<Credentials>,
    /// The Taba authentication is on: the calls must be signed with a key of the credentials file.
    pub auth_enabled: bool,
    pub clock: Arc<dyn Clock>,
    /// The largest request body read (`api.batch.max-bytes`).
    pub max_body: usize,
}

/// The request being served.
pub struct Ctx<'a> {
    pub state: &'a SnsState,
    pub caller: &'a Caller,
    /// The region of the ARNs: the one the request was signed for.
    pub region: String,
}

impl Ctx<'_> {
    pub fn backend(&self) -> &Backend {
        &self.state.backend
    }
}

/// The router of the SNS port: every path and method goes to the same handler.
pub fn router(state: Arc<SnsState>) -> Router {
    Router::new().fallback(handle).with_state(state)
}

async fn handle(
    State(state): State<Arc<SnsState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    request: Request,
) -> Response {
    let request_id = crate::core::id::new_at(state.clock.now_millis());
    let (parts, body) = request.into_parts();
    let fail = |error: AwsError| respond_error(&error, &request_id);

    let body = match to_bytes(body, state.max_body).await {
        Ok(body) => body,
        Err(_) => {
            return fail(AwsError::sns_invalid_parameter(
                "The request is larger than the limit of the gateway.",
            ));
        }
    };
    let identity = match auth::authenticate(
        "sns",
        &state.credentials,
        state.auth_enabled,
        state.clock.now_millis(),
        &parts,
        &body,
    ) {
        Ok(identity) => identity,
        Err(error) => return fail(error),
    };
    let text = match std::str::from_utf8(&body) {
        Ok(text) => text,
        Err(_) => {
            return fail(AwsError::sns_invalid_parameter(
                "The request body is not valid UTF-8.",
            ));
        }
    };
    let (action, params) = match protocol::decode(parts.uri.query().unwrap_or(""), text) {
        Ok(decoded) => decoded,
        Err(error) => return fail(error),
    };
    let caller = Caller::of(identity.token, peer, &parts.headers);
    let ctx = Ctx {
        state: &state,
        caller: &caller,
        region: identity.region,
    };
    match run(&ctx, &action, &params).await {
        Ok(result) => build(
            StatusCode::OK,
            protocol::CONTENT_TYPE,
            protocol::encode_ok(&action, &result, &request_id).into_bytes(),
            &request_id,
            &[],
        ),
        Err(error) => fail(error),
    }
}

fn respond_error(error: &AwsError, request_id: &str) -> Response {
    let status = StatusCode::from_u16(error.status).unwrap_or(StatusCode::BAD_REQUEST);
    let mut extra: Vec<(&'static str, String)> = Vec::new();
    if let Some(seconds) = error.retry_after {
        extra.push(("retry-after", seconds.to_string()));
    }
    build(
        status,
        protocol::CONTENT_TYPE,
        protocol::encode_error(error, request_id).into_bytes(),
        request_id,
        &extra,
    )
}

/// Runs the action. What the distributor cannot do is refused with a clear error, never accepted and ignored.
pub async fn run(ctx: &Ctx<'_>, action: &str, params: &Value) -> Result<Value, AwsError> {
    match action {
        "CreateTopic" => topics::create_topic(ctx, params).await,
        "DeleteTopic" => topics::delete_topic(ctx, params).await,
        "ListTopics" => topics::list_topics(ctx, params).await,
        "GetTopicAttributes" => topics::get_topic_attributes(ctx, params).await,
        "SetTopicAttributes" => topics::set_topic_attributes(ctx, params).await,
        "TagResource" => topics::tag_resource(ctx, params).await,
        "UntagResource" => topics::untag_resource(ctx, params).await,
        "ListTagsForResource" => topics::list_tags_for_resource(ctx, params).await,
        "Publish" => publish::publish(ctx, params).await,
        "PublishBatch" => publish::publish_batch(ctx, params).await,
        "Subscribe" => subscriptions::subscribe(ctx, params).await,
        "Unsubscribe" => subscriptions::unsubscribe(ctx, params).await,
        "ListSubscriptions" => subscriptions::list_subscriptions(ctx, params).await,
        "ListSubscriptionsByTopic" => subscriptions::list_subscriptions_by_topic(ctx, params).await,
        "GetSubscriptionAttributes" => {
            subscriptions::get_subscription_attributes(ctx, params).await
        }
        "SetSubscriptionAttributes" => {
            subscriptions::set_subscription_attributes(ctx, params).await
        }
        "ConfirmSubscription"
        | "AddPermission"
        | "RemovePermission"
        | "GetDataProtectionPolicy"
        | "PutDataProtectionPolicy"
        | "CreatePlatformApplication"
        | "CreatePlatformEndpoint"
        | "CheckIfPhoneNumberIsOptedOut"
        | "OptInPhoneNumber"
        | "SetSMSAttributes" => Err(AwsError::sns_invalid_parameter(format!(
            "The action {action} is not supported by this gateway."
        ))),
        other => Err(AwsError::invalid_action(other)),
    }
}

#[cfg(test)]
mod tests;
