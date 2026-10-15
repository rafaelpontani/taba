//! The message's `Content-Type` and the shape of the body on delivery (doc 06a, "The message
//! body"). Taba stores the bytes as they came; only the `GET` response decides whether the
//! `body` comes out as native JSON, text or base64.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde::Serialize;
use serde_json::value::RawValue;

use super::error::{ErrorCode, TabaError};

/// `contentType` when the publication does not carry the header.
pub const DEFAULT_CONTENT_TYPE: &str = "application/json";
const MAX_LEN: usize = 255;

/// How the body is validated and shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyKind {
    /// `application/json` or `*/*+json`.
    Json,
    /// `text/*` in UTF-8 (or without `charset`).
    Text,
    /// Any other, including `text/*` with another `charset`.
    Binary,
}

/// A valid media type, stored as it came (with the parameters).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentType {
    text: String,
    kind: BodyKind,
}

fn invalid(message: &str) -> TabaError {
    TabaError::new(ErrorCode::ValidationInvalidContentType, message)
}

fn is_token(s: &str) -> bool {
    const SYMBOLS: &[u8] = b"!#$%&'*+-.^_`|~";
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || SYMBOLS.contains(&b))
}

/// Splits the parameters at `;`, without breaking inside quotes.
fn split_params(rest: &str) -> Vec<&str> {
    let (mut parts, mut start, mut quoted, mut escaped) = (Vec::new(), 0, false, false);
    for (i, c) in rest.char_indices() {
        match c {
            _ if escaped => escaped = false,
            '\\' if quoted => escaped = true,
            '"' => quoted = !quoted,
            ';' if !quoted => {
                parts.push(&rest[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&rest[start..]);
    parts
}

/// The value of a parameter: a token or a quoted string.
fn param_value(value: &str) -> Option<String> {
    if let Some(inner) = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) {
        let mut out = String::new();
        let mut chars = inner.chars();
        while let Some(c) = chars.next() {
            match c {
                '\\' => out.push(chars.next()?),
                '"' => return None,
                c => out.push(c),
            }
        }
        return Some(out);
    }
    is_token(value).then(|| value.to_owned())
}

impl ContentType {
    /// Checks the media type (`type/subtype` and parameters `; key=value`), from 1 to 255
    /// characters. Invalid: `400` `validation.invalid-content-type`.
    pub fn parse(raw: &str) -> Result<Self, TabaError> {
        if raw.is_empty() || raw.chars().count() > MAX_LEN {
            return Err(invalid("The Content-Type must have 1 to 255 characters."));
        }
        let (essence, rest) = match raw.split_once(';') {
            Some((essence, rest)) => (essence, Some(rest)),
            None => (raw, None),
        };
        let essence = essence.trim().to_ascii_lowercase();
        let Some((main, sub)) = essence.split_once('/') else {
            return Err(invalid("The Content-Type must be `type/subtype`."));
        };
        if !is_token(main) || !is_token(sub) {
            return Err(invalid("The Content-Type must be `type/subtype`."));
        }
        let mut charset = None;
        for param in rest.map(split_params).unwrap_or_default() {
            let Some((key, value)) = param.split_once('=') else {
                return Err(invalid("Content-Type parameter without `=`."));
            };
            let (key, value) = (key.trim(), value.trim());
            let value = param_value(value)
                .filter(|_| is_token(key))
                .ok_or_else(|| invalid("Invalid Content-Type parameter."))?;
            if key.eq_ignore_ascii_case("charset") {
                charset = Some(value);
            }
        }
        let utf8 = charset.is_none_or(|c| c.eq_ignore_ascii_case("utf-8"));
        let kind = if essence == "application/json" || sub.ends_with("+json") {
            BodyKind::Json
        } else if main == "text" && utf8 {
            BodyKind::Text
        } else {
            BodyKind::Binary
        };
        Ok(Self {
            text: raw.to_owned(),
            kind,
        })
    }

    pub fn as_str(&self) -> &str {
        &self.text
    }

    pub fn kind(&self) -> BodyKind {
        self.kind
    }

    /// An `application/json` body must be JSON, and `text/*` must be UTF-8; the other
    /// types are not validated. Invalid: `400` `validation.invalid-body`.
    pub fn validate_body(&self, body: &[u8]) -> Result<(), TabaError> {
        let bad = |message: &str| TabaError::new(ErrorCode::ValidationInvalidBody, message);
        match self.kind {
            BodyKind::Json => serde_json::from_slice::<serde::de::IgnoredAny>(body)
                .map(|_| ())
                .map_err(|e| bad(&format!("The body is not valid JSON: {e}."))),
            BodyKind::Text => std::str::from_utf8(body)
                .map(|_| ())
                .map_err(|_| bad("The body is not valid UTF-8 text.")),
            BodyKind::Binary => Ok(()),
        }
    }
}

/// The `body` (and the `bodyEncoding`) of a delivered message.
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum Body {
    /// The JSON as it was sent, without reserializing.
    Json(Box<RawValue>),
    Text(String),
    Base64(String),
}

impl Body {
    /// Builds the `body` from the stored bytes, in the form the `contentType` dictates.
    /// The bytes already went through `validate_body` at publication.
    pub fn render(content_type: &ContentType, bytes: &[u8]) -> Self {
        let as_text = || String::from_utf8_lossy(bytes).into_owned();
        match content_type.kind {
            BodyKind::Json => RawValue::from_string(as_text())
                .map(Self::Json)
                .unwrap_or_else(|_| Self::Base64(STANDARD.encode(bytes))),
            BodyKind::Text => Self::Text(as_text()),
            BodyKind::Binary => Self::Base64(STANDARD.encode(bytes)),
        }
    }

    /// `Some("base64")` only for binary.
    pub fn encoding(&self) -> Option<&'static str> {
        matches!(self, Self::Base64(_)).then_some("base64")
    }
}

#[cfg(test)]
mod tests;
