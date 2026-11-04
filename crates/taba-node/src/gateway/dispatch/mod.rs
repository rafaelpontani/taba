//! The one port of the gateway for the AWS SDKs (doc 39): the SQS and the SNS protocols arrive at the same address, as in the
//! emulators, and this module hands each request to the router of its service. It reads nothing but the service: the
//! signature, the body and the action are the business of [`super::sqs`] and [`super::sns`].
//!
//! The service of a request is, in this order:
//!
//! 1. the service in the credential scope of the `Authorization` header (`.../us-east-1/sns/aws4_request`), which every SDK
//!    sends and which the protocol module checks again against the signature;
//! 2. the `X-Amz-Target: AmazonSQS.<Action>` header of the SQS JSON protocol;
//! 3. the `Action` of the form of an unsigned call (only a node with the Taba authentication off accepts one): an action of
//!    SNS goes to SNS, any other to SQS.
//!
//! A service that is off answers `404` with a message that names the key to turn it on.

use std::sync::Arc;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::{Request, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use tower::ServiceExt;

use super::aws::form;
use super::aws::reply::build;
use super::aws::sigv4;
use super::sns;

/// The two services behind the port.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Service {
    Sqs,
    Sns,
}

struct Routes {
    sqs: Option<Router>,
    sns: Option<Router>,
    /// The largest body read to find the action of an unsigned call.
    max_body: usize,
}

/// The router of the port. A service that is `None` is off.
pub fn router(sqs: Option<Router>, sns: Option<Router>, max_body: usize) -> Router {
    Router::new()
        .fallback(handle)
        .with_state(Arc::new(Routes { sqs, sns, max_body }))
}

async fn handle(State(routes): State<Arc<Routes>>, request: Request) -> Response {
    let (service, request) = choose(&routes, request).await;
    let target = match service {
        Service::Sqs => routes.sqs.as_ref(),
        Service::Sns => routes.sns.as_ref(),
    };
    match target {
        Some(router) => match router.clone().oneshot(request).await {
            Ok(response) => response,
            Err(never) => match never {},
        },
        None => not_enabled(service),
    }
}

/// The service of the request, and the request itself (its body is put back when it had to be read).
async fn choose(routes: &Routes, request: Request) -> (Service, Request) {
    if let Some(service) = service_of_headers(request.headers()) {
        return (service, request);
    }
    let (parts, body) = request.into_parts();
    // A body over the limit goes to SQS, whose handler answers it with the error of the gateway.
    let bytes = match to_bytes(body, routes.max_body).await {
        Ok(bytes) => bytes,
        Err(_) => return (Service::Sqs, Request::from_parts(parts, Body::empty())),
    };
    let service = service_of_form(&bytes).unwrap_or(Service::Sqs);
    (service, Request::from_parts(parts, Body::from(bytes)))
}

fn service_of_headers(headers: &HeaderMap) -> Option<Service> {
    if let Ok(signature) = sigv4::parse(headers) {
        return Some(if signature.service == "sns" {
            Service::Sns
        } else {
            Service::Sqs
        });
    }
    let target = headers.get("x-amz-target")?.to_str().ok()?;
    target.starts_with("AmazonSQS").then_some(Service::Sqs)
}

/// The service of a form by its `Action`.
fn service_of_form(body: &[u8]) -> Option<Service> {
    let text = std::str::from_utf8(body).ok()?;
    let pairs = form::pairs(text).ok()?;
    let action = pairs.iter().find(|(name, _)| name == "Action")?.1.as_str();
    Some(if sns::ACTIONS.contains(&action) {
        Service::Sns
    } else {
        Service::Sqs
    })
}

fn not_enabled(service: Service) -> Response {
    let (name, key) = match service {
        Service::Sqs => ("SQS", "gateway.sqs.enabled"),
        Service::Sns => ("SNS", "gateway.sns.enabled"),
    };
    let body = serde_json::json!({
        "__type": "ServiceNotEnabled",
        "message": format!("O gateway {name} está desligado neste nó ({key}=false)."),
    });
    build(
        StatusCode::NOT_FOUND,
        "application/json",
        body.to_string().into_bytes(),
        "",
        &[],
    )
}

#[cfg(test)]
mod tests;
