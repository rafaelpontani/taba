//! Reading the routes' JSON body (doc 06d, section 8.3.1): `415` for another
//! `Content-Type`, `validation.unknown-field` for a field the route does not define and
//! `validation.required` for what is missing. Type errors become `validation.failed`
//! with the field in `details`.

use std::collections::BTreeMap;

use axum::http::{HeaderMap, header};
use serde_json::{Map, Value};

use crate::core::error::{ErrorCode, TabaError};
use crate::engine::topics::Patch;

pub struct Fields(Map<String, Value>);

fn wrong_type(field: &str, expected: &str) -> TabaError {
    TabaError::new(
        ErrorCode::ValidationFailed,
        "The request has invalid fields.",
    )
    .with_detail(
        field,
        ErrorCode::ValidationFailed,
        format!("Expected {expected}."),
    )
}

/// `415` if the `Content-Type` is not `application/json`.
fn require_json(headers: &HeaderMap) -> Result<(), TabaError> {
    let media_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(';').next())
        .map(|v| v.trim().to_ascii_lowercase());
    if media_type.as_deref() == Some("application/json") {
        Ok(())
    } else {
        Err(TabaError::new(
            ErrorCode::MediaUnsupported,
            "Use `Content-Type: application/json`.",
        ))
    }
}

/// The body as a JSON object. An empty body counts as `{}`: whoever requires fields complains about them.
pub fn object(headers: &HeaderMap, body: &[u8]) -> Result<Fields, TabaError> {
    if body.iter().all(u8::is_ascii_whitespace) {
        return Ok(Fields(Map::new()));
    }
    require_json(headers)?;
    match serde_json::from_slice::<Value>(body) {
        Ok(Value::Object(map)) => Ok(Fields(map)),
        Ok(_) => Err(TabaError::new(
            ErrorCode::ValidationFailed,
            "The body must be a JSON object.",
        )),
        Err(e) => Err(TabaError::new(
            ErrorCode::ValidationFailed,
            format!("The body is not valid JSON: {e}."),
        )),
    }
}

impl Fields {
    /// The first field the route does not define.
    pub fn reject_unknown(&self, allowed: &[&str]) -> Result<(), TabaError> {
        match self.0.keys().find(|k| !allowed.contains(&k.as_str())) {
            None => Ok(()),
            Some(field) => Err(TabaError::new(
                ErrorCode::ValidationUnknownField,
                format!("The field `{field}` does not exist on this route."),
            )
            .with_detail(field, ErrorCode::ValidationUnknownField, "Unknown field.")),
        }
    }

    /// All the missing fields (absent or `null`), each one in `details`.
    pub fn require(&self, fields: &[&str]) -> Result<(), TabaError> {
        let mut error = TabaError::new(
            ErrorCode::ValidationRequired,
            "Required fields are missing.",
        );
        for field in fields {
            if self.0.get(*field).is_none_or(Value::is_null) {
                error = error.with_detail(*field, ErrorCode::ValidationRequired, "Required field.");
            }
        }
        if error.details.is_empty() {
            Ok(())
        } else {
            Err(error)
        }
    }

    /// The field came, even if `null`.
    pub fn has(&self, field: &str) -> bool {
        self.0.contains_key(field)
    }

    fn present(&self, field: &str) -> Option<&Value> {
        self.0.get(field).filter(|v| !v.is_null())
    }

    pub fn string(&self, field: &str) -> Result<Option<String>, TabaError> {
        match self.present(field) {
            None => Ok(None),
            Some(Value::String(s)) => Ok(Some(s.clone())),
            Some(_) => Err(wrong_type(field, "a text")),
        }
    }

    pub fn boolean(&self, field: &str) -> Result<Option<bool>, TabaError> {
        match self.present(field) {
            None => Ok(None),
            Some(Value::Bool(b)) => Ok(Some(*b)),
            Some(_) => Err(wrong_type(field, "`true` or `false`")),
        }
    }

    /// Integer. A number that does not fit in `i64` is out of range; a decimal is a type error.
    pub fn integer(&self, field: &str) -> Result<Option<i64>, TabaError> {
        match self.present(field) {
            None => Ok(None),
            Some(Value::Number(n)) => match n.as_i64() {
                Some(v) => Ok(Some(v)),
                None if n.is_u64() => Err(TabaError::new(
                    ErrorCode::ValidationOutOfRange,
                    format!("{field} is out of range."),
                )
                .with_detail(
                    field,
                    ErrorCode::ValidationOutOfRange,
                    "Out of range.",
                )),
                None => Err(wrong_type(field, "an integer")),
            },
            Some(_) => Err(wrong_type(field, "an integer")),
        }
    }

    pub fn tags(&self, field: &str) -> Result<Option<BTreeMap<String, String>>, TabaError> {
        match self.present(field) {
            None => Ok(None),
            Some(Value::Object(map)) => map
                .iter()
                .map(|(k, v)| match v {
                    Value::String(s) => Ok((k.clone(), s.clone())),
                    _ => Err(wrong_type(&format!("{field}.{k}"), "a text")),
                })
                .collect::<Result<_, _>>()
                .map(Some),
            Some(_) => Err(wrong_type(field, "a text-to-text object")),
        }
    }

    pub fn string_list(&self, field: &str) -> Result<Option<Vec<String>>, TabaError> {
        match self.present(field) {
            None => Ok(None),
            Some(Value::Array(items)) => items
                .iter()
                .enumerate()
                .map(|(i, v)| {
                    v.as_str()
                        .map(str::to_owned)
                        .ok_or_else(|| wrong_type(&format!("{field}[{i}]"), "a text"))
                })
                .collect::<Result<_, _>>()
                .map(Some),
            Some(_) => Err(wrong_type(field, "a list of texts")),
        }
    }

    /// Date in the API format (`2026-10-06T12:00:00.000Z`), in milliseconds since the epoch.
    pub fn date(&self, field: &str) -> Result<Option<u64>, TabaError> {
        match self.string(field)? {
            None => Ok(None),
            Some(text) => crate::core::time::parse_ms(&text)
                .map(Some)
                .ok_or_else(|| wrong_type(field, "a date like 2026-10-06T12:00:00.000Z")),
        }
    }

    /// Date in a `PATCH`: absent, `null` (removes) or a date.
    pub fn patch_date(&self, field: &str) -> Result<Patch<u64>, TabaError> {
        match self.0.get(field) {
            None => Ok(Patch::Absent),
            Some(Value::Null) => Ok(Patch::Null),
            Some(_) => Ok(self.date(field)?.map_or(Patch::Absent, Patch::Value)),
        }
    }

    /// A `PATCH` field: absent, `null` (goes back to the default) or an integer.
    pub fn patch_integer(&self, field: &str) -> Result<Patch<i64>, TabaError> {
        match self.0.get(field) {
            None => Ok(Patch::Absent),
            Some(Value::Null) => Ok(Patch::Null),
            Some(_) => Ok(self.integer(field)?.map_or(Patch::Absent, Patch::Value)),
        }
    }
}
