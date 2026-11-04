//! The gateway for the AWS SDKs (doc 39): an application that already uses SQS or SNS points its client at the port of the
//! Taba node and keeps its code. SQS and SNS are two modules behind one port ([`dispatch`]), each off by default;
//! `node.rs` is the only caller.
//!
//! The gateway keeps no state. It translates a call of the foreign protocol into calls to the **client API**, in the process
//! ([`backend`]), so authentication, permissions, rate limit, drain and forwarding between nodes are the ones of every other
//! client. The shape of the code, and what each part tests, is in the plan of doc 39.
//!
//! * [`aws`]: what SQS and SNS share (signature, credentials, errors, XML, form).
//! * [`backend`]: the only door into Taba.
//! * [`envelope`]: where the message attributes live.
//! * [`naming`]: queue names, URLs and ARNs.
//! * [`dispatch`]: the one port; it hands each request to the service it is for.
//! * [`sqs`]: the SQS protocols and actions.
//! * [`sns`]: the SNS protocol and actions, over the distributor of Taba.

pub mod aws;
pub mod backend;
pub mod dispatch;
pub mod envelope;
pub mod naming;
pub mod sns;
pub mod sqs;

use std::sync::Arc;

use axum::Router;

use crate::config::Config;
use crate::core::clock::Clock;
use aws::credentials::Credentials;
use backend::Backend;

/// Opens the credentials file of the AWS protocols (`gateway.aws.credentials-file`). A file that cannot be read or is not valid
/// is an error at the start: a silent empty set would refuse every call without saying why.
pub fn open_credentials(config: &Config) -> Result<Arc<Credentials>, String> {
    match &config.gateway.credentials_file {
        Some(path) => Credentials::open(path).map(Arc::new),
        None => Ok(Arc::new(Credentials::none())),
    }
}

/// The router of the port of the gateway: the SQS and the SNS protocols behind one address, each only when its key is on.
/// `client` is the router of the client API the calls are made against.
pub fn aws_router(
    config: &Config,
    client: Router,
    credentials: Arc<Credentials>,
    auth_enabled: bool,
    clock: Arc<dyn Clock>,
) -> Router {
    let (sqs_on, sns_on) = (config.gateway.sqs.enabled, config.gateway.sns.enabled);
    if auth_enabled && credentials.is_empty() {
        tracing::warn!(
            "gateway is on with the Taba authentication, but gateway.aws.credentials-file has no keys: every call will be refused"
        );
    }
    if !auth_enabled {
        tracing::warn!(
            "gateway is on without authentication: any credential is accepted and the signature is not checked"
        );
    }
    let max_body = usize::try_from(config.batch.max_bytes).unwrap_or(usize::MAX);
    let sqs = sqs_on.then(|| {
        sqs::router(Arc::new(sqs::SqsState::new(
            Backend::new(client.clone()),
            credentials.clone(),
            auth_enabled,
            config.gateway.sqs.public_url.clone(),
            clock.clone(),
            max_body,
        )))
    });
    let sns = sns_on.then(|| {
        sns::router(Arc::new(sns::SnsState {
            backend: Backend::new(client.clone()),
            credentials: credentials.clone(),
            auth_enabled,
            clock: clock.clone(),
            max_body,
        }))
    });
    dispatch::router(sqs, sns, max_body)
}

#[cfg(test)]
mod tests;
