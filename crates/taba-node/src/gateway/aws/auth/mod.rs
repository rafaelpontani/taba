//! Who is calling an AWS port (doc 39): the signature check that SQS and SNS share. Only the name of the service in the
//! credential scope differs between them.
//!
//! With the Taba authentication **on**, the call must be signed (SigV4) with a key of the credentials file, and the Taba
//! token of that key is the one the calls to the client API carry. With it **off**, nobody is checked and the calls go
//! without a token.

use axum::http::request::Parts;

use super::credentials::Credentials;
use super::errors::AwsError;
use super::sigv4::{self, SigError, Signed};
use crate::gateway::naming;

/// What a request resolved to once it is authenticated.
pub struct Identity {
    /// The Taba token whose permissions apply; `None` when the authentication is off.
    pub token: Option<String>,
    /// The region the request was signed for (the one the ARNs carry).
    pub region: String,
}

/// Checks who is calling. `service` is the name the credential scope must carry (`sqs`, `sns`).
pub fn authenticate(
    service: &str,
    credentials: &Credentials,
    auth_enabled: bool,
    now_ms: u64,
    parts: &Parts,
    body: &[u8],
) -> Result<Identity, AwsError> {
    let parsed = sigv4::parse(&parts.headers);
    if !auth_enabled {
        let region = parsed
            .map(|p| p.region)
            .unwrap_or_else(|_| naming::DEFAULT_REGION.to_owned());
        return Ok(Identity {
            token: None,
            region,
        });
    }
    let parsed = parsed.map_err(|why| match why {
        SigError::Missing => AwsError::missing_authentication_token(),
        SigError::Malformed(message) => AwsError::incomplete_signature(message),
        SigError::Expired => AwsError::request_expired(),
        SigError::Mismatch => AwsError::signature_does_not_match(),
    })?;
    if parsed.service != service {
        return Err(AwsError::incomplete_signature(format!(
            "The credential scope names a service other than {service}."
        )));
    }
    let entry = credentials
        .lookup(&parsed.access_key)
        .ok_or_else(AwsError::invalid_client_token_id)?;
    let signed = Signed {
        method: parts.method.as_str(),
        path: parts.uri.path(),
        query: parts.uri.query().unwrap_or(""),
        headers: &parts.headers,
        body,
    };
    parsed
        .verify(entry.secret.expose(), &signed, now_ms)
        .map_err(|why| match why {
            SigError::Expired => AwsError::request_expired(),
            SigError::Malformed(message) => AwsError::incomplete_signature(message),
            _ => AwsError::signature_does_not_match(),
        })?;
    Ok(Identity {
        token: Some(entry.token.expose().to_owned()),
        region: parsed.region,
    })
}

#[cfg(test)]
mod tests;
