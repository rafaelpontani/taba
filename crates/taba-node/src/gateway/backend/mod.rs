//! The only door of the gateway into Taba (doc 39): it builds a call to the **client API** and hands it to the same
//! `Router` the `api.port` serves, in the process and without the network. So the authentication, the permissions and
//! prefixes of the token, the rate limit, the drain (with its forward to another node) and the metrics are the ones of
//! every other client, with no code of their own.
//!
//! The call carries the connection address of whoever called the gateway and the client-IP headers the proxy added,
//! so the rate limit still counts by the real client. To become a separate process one day, only this module changes:
//! the in-process call turns into an HTTP call.

use std::net::SocketAddr;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::ConnectInfo;
use axum::http::{HeaderMap, HeaderName, HeaderValue, Method, Request, header};
use serde_json::Value;
use tower::ServiceExt;

use super::aws::errors::AwsError;

/// The largest answer of the client API the gateway reads (a batch of 1000 messages of 5 MB is out of its reach, but a
/// delivery of 10 messages of 1 MiB is not).
const MAX_REPLY: usize = 64 * 1024 * 1024;

/// Who is calling: what the client API needs to know about the caller.
#[derive(Clone)]
pub struct Caller {
    /// The Taba token to present, if the credentials file gave one. `None` when the authentication is off.
    pub token: Option<String>,
    /// The connection the AWS client opened to the gateway.
    pub peer: SocketAddr,
    /// The headers a proxy in front of the gateway added to say who the client is (the client-IP rules read them).
    pub forwarded: Vec<(HeaderName, HeaderValue)>,
}

impl Caller {
    /// The caller of a request that arrived at the gateway: only the headers that say where it came from are kept.
    pub fn of(token: Option<String>, peer: SocketAddr, headers: &HeaderMap) -> Self {
        let forwarded = headers
            .iter()
            .filter(|(name, _)| {
                let name = name.as_str();
                name == "x-forwarded-for" || name == "x-real-ip" || name == "forwarded"
            })
            .map(|(name, value)| (name.clone(), value.clone()))
            .collect();
        Self {
            token,
            peer,
            forwarded,
        }
    }
}

/// What the client API answered.
#[derive(Debug)]
pub struct Reply {
    pub status: u16,
    pub body: Vec<u8>,
    pub retry_after: Option<u64>,
}

impl Reply {
    /// The body as JSON; an empty body (a `204`) is `null`.
    pub fn json(&self) -> Value {
        if self.body.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&self.body).unwrap_or(Value::Null)
        }
    }

    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }

    /// The `code` of a Taba error answer (`topic.not-found`), if the body is one.
    pub fn error_code(&self) -> Option<String> {
        self.json().get("code")?.as_str().map(str::to_owned)
    }

    /// The failure as an AWS error. Only meaningful when [`Self::is_success`] is false.
    pub fn aws_error(&self) -> AwsError {
        let body = self.json();
        let code = body.get("code").and_then(Value::as_str).unwrap_or("");
        let message = body
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("The request could not be completed.");
        AwsError::from_taba(self.status, code, message, self.retry_after)
    }

    /// The same failure as an SNS error (a topic that does not exist is `NotFound`, a refusal is `InvalidParameter`).
    pub fn sns_error(&self) -> AwsError {
        let body = self.json();
        let code = body.get("code").and_then(Value::as_str).unwrap_or("");
        let message = body
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("The request could not be completed.");
        AwsError::from_taba_sns(self.status, code, message, self.retry_after)
    }
}

/// The client API, in the process.
#[derive(Clone)]
pub struct Backend {
    router: Router,
}

impl Backend {
    pub fn new(router: Router) -> Self {
        Self { router }
    }

    /// Runs one call. `body` is sent with `content_type` when there is one.
    pub async fn call(
        &self,
        caller: &Caller,
        method: Method,
        path_and_query: &str,
        content_type: Option<&str>,
        body: Vec<u8>,
    ) -> Result<Reply, AwsError> {
        let mut builder = Request::builder().method(method).uri(path_and_query);
        if let Some(token) = &caller.token {
            builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
        }
        if let Some(content_type) = content_type {
            builder = builder.header(header::CONTENT_TYPE, content_type);
        }
        for (name, value) in &caller.forwarded {
            builder = builder.header(name, value);
        }
        let mut request = builder.body(Body::from(body)).map_err(|_| {
            AwsError::internal("The gateway could not build a call to the client API.")
        })?;
        request.extensions_mut().insert(ConnectInfo(caller.peer));
        let response = match self.router.clone().oneshot(request).await {
            Ok(response) => response,
            Err(never) => match never {},
        };
        let (parts, body) = response.into_parts();
        let body = to_bytes(body, MAX_REPLY)
            .await
            .map_err(|_| AwsError::internal("The answer of the client API is too large."))?;
        let retry_after = parts
            .headers
            .get(header::RETRY_AFTER)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.trim().parse::<u64>().ok());
        Ok(Reply {
            status: parts.status.as_u16(),
            body: body.to_vec(),
            retry_after,
        })
    }

    /// A call with a JSON body (or none).
    pub async fn json(
        &self,
        caller: &Caller,
        method: Method,
        path_and_query: &str,
        body: Option<&Value>,
    ) -> Result<Reply, AwsError> {
        match body {
            Some(value) => {
                self.call(
                    caller,
                    method,
                    path_and_query,
                    Some("application/json"),
                    serde_json::to_vec(value).unwrap_or_default(),
                )
                .await
            }
            None => {
                self.call(caller, method, path_and_query, None, Vec::new())
                    .await
            }
        }
    }

    /// Like [`Self::json`], but a failure of the client API becomes the AWS error and a success returns the JSON.
    pub async fn ok_json(
        &self,
        caller: &Caller,
        method: Method,
        path_and_query: &str,
        body: Option<&Value>,
    ) -> Result<Value, AwsError> {
        let reply = self.json(caller, method, path_and_query, body).await?;
        if reply.is_success() {
            Ok(reply.json())
        } else {
            Err(reply.aws_error())
        }
    }

    /// Like [`Self::ok_json`], with the failures translated for SNS.
    pub async fn ok_json_sns(
        &self,
        caller: &Caller,
        method: Method,
        path_and_query: &str,
        body: Option<&Value>,
    ) -> Result<Value, AwsError> {
        let reply = self.json(caller, method, path_and_query, body).await?;
        if reply.is_success() {
            Ok(reply.json())
        } else {
            Err(reply.sns_error())
        }
    }
}

#[cfg(test)]
mod tests;
