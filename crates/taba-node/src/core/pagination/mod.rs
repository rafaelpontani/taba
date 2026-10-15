//! Opaque cursor for key pagination (doc 06a, section 8.1.2): the client never builds it.
//! The cursor holds the last key read, plus the `sort` and the filters in use, and has no
//! server-side state, so it does not expire.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::Serialize;
use serde::de::DeserializeOwned;

use super::error::{ErrorCode, TabaError};

pub fn encode_cursor<T: Serialize>(key: &T) -> String {
    let json = serde_json::to_vec(key).unwrap_or_default();
    URL_SAFE_NO_PAD.encode(json)
}

/// Cursor malformado: `400` `validation.invalid-cursor`.
pub fn decode_cursor<T: DeserializeOwned>(cursor: &str) -> Result<T, TabaError> {
    let invalid = || {
        TabaError::new(ErrorCode::ValidationInvalidCursor, "The cursor is invalid.").with_detail(
            "cursor",
            ErrorCode::ValidationInvalidCursor,
            "Use the `nextCursor` of the previous page.",
        )
    };
    let bytes = URL_SAFE_NO_PAD.decode(cursor).map_err(|_| invalid())?;
    serde_json::from_slice(&bytes).map_err(|_| invalid())
}

/// Validates the query's `limit`: from 1 up to the maximum; without it, the default applies.
pub fn parse_limit(raw: Option<&str>, default: u32, max: u32) -> Result<usize, TabaError> {
    let Some(raw) = raw else {
        return Ok(default as usize);
    };
    let out_of_range = || {
        TabaError::new(
            ErrorCode::ValidationOutOfRange,
            format!("limit must be between 1 and {max}."),
        )
        .with_detail(
            "query.limit",
            ErrorCode::ValidationOutOfRange,
            format!("Between 1 and {max}."),
        )
    };
    match raw.trim().parse::<u32>() {
        Ok(n) if (1..=max).contains(&n) => Ok(n as usize),
        _ => Err(out_of_range()),
    }
}

#[cfg(test)]
mod tests;
