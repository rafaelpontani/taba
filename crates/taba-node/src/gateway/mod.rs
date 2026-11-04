//! The gateway for the AWS SDKs (doc 39): an application that already uses SQS (and, later, SNS or RabbitMQ) points its
//! client at a port of the Taba node and keeps its code. Each protocol has a module and a port of its own, off by
//! default; `node.rs` is the only caller.
//!
//! The gateway keeps no state. It translates a call of the foreign protocol into calls to the **client API**, in the process
//! ([`backend`]), so authentication, permissions, rate limit, drain and forwarding between nodes are the ones of every other
//! client. The shape of the code, and what each part tests, is in the plan of doc 39.
//!
//! * [`aws`]: what SQS and SNS share (signature, credentials, errors, XML, form).
//! * [`backend`]: the only door into Taba.
//! * [`envelope`]: where the message attributes live.
//! * [`naming`]: queue names, URLs and ARNs.
//! * [`sqs`]: the SQS protocols and actions.
//! * [`sns`]: the SNS protocol and actions, over the distributor of Taba.

pub mod aws;
pub mod backend;
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

/// The router of the SQS port. `client` is the router of the client API the calls are made against.
pub fn sqs_router(
    config: &Config,
    client: Router,
    credentials: Arc<Credentials>,
    auth_enabled: bool,
    clock: Arc<dyn Clock>,
) -> Router {
    if auth_enabled && credentials.is_empty() {
        tracing::warn!(
            "gateway.sqs is on with the Taba authentication, but gateway.aws.credentials-file has no keys: every SQS call will be refused"
        );
    }
    if !auth_enabled {
        tracing::warn!(
            "gateway.sqs is on without authentication: any credential is accepted and the signature is not checked"
        );
    }
    let state = sqs::SqsState::new(
        Backend::new(client),
        credentials,
        auth_enabled,
        config.gateway.sqs.public_url.clone(),
        clock,
        usize::try_from(config.batch.max_bytes).unwrap_or(usize::MAX),
    );
    sqs::router(Arc::new(state))
}

/// The router of the SNS port. `client` is the router of the client API the calls are made against.
pub fn sns_router(
    config: &Config,
    client: Router,
    credentials: Arc<Credentials>,
    auth_enabled: bool,
    clock: Arc<dyn Clock>,
) -> Router {
    if auth_enabled && credentials.is_empty() {
        tracing::warn!(
            "gateway.sns is on with the Taba authentication, but gateway.aws.credentials-file has no keys: every SNS call will be refused"
        );
    }
    if !auth_enabled {
        tracing::warn!(
            "gateway.sns is on without authentication: any credential is accepted and the signature is not checked"
        );
    }
    let state = sns::SnsState {
        backend: Backend::new(client),
        credentials,
        auth_enabled,
        clock,
        max_body: usize::try_from(config.batch.max_bytes).unwrap_or(usize::MAX),
    };
    sns::router(Arc::new(state))
}

#[cfg(test)]
mod tests;
