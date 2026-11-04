//! The `ReceiptHandle` (doc 39). SQS gives the client one opaque string to delete or hide a message later; Taba wants
//! the message `id` **and** the `receiptToken` that proves who received it. The handle carries both, so the gateway
//! keeps no state: `base64url("<id>:<receiptToken>")`.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;

use crate::gateway::aws::errors::AwsError;

/// The longest handle SQS accepts.
const MAX_LEN: usize = 1024;

pub fn encode(id: &str, receipt_token: &str) -> String {
    URL_SAFE_NO_PAD.encode(format!("{id}:{receipt_token}"))
}

/// Opens a handle into the message `id` and the `receiptToken`. A text that is not one of ours is
/// `ReceiptHandleIsInvalid`.
pub fn decode(handle: &str) -> Result<(String, String), AwsError> {
    if handle.is_empty() || handle.len() > MAX_LEN {
        return Err(AwsError::receipt_handle_is_invalid());
    }
    let bytes = URL_SAFE_NO_PAD
        .decode(handle)
        .map_err(|_| AwsError::receipt_handle_is_invalid())?;
    let text = String::from_utf8(bytes).map_err(|_| AwsError::receipt_handle_is_invalid())?;
    match text.split_once(':') {
        Some((id, token)) if !id.is_empty() && !token.is_empty() => {
            Ok((id.to_owned(), token.to_owned()))
        }
        _ => Err(AwsError::receipt_handle_is_invalid()),
    }
}

#[cfg(test)]
mod tests;
