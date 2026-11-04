//! Reading the parameters of an AWS action (doc 39). Both protocols give the action the same JSON shape (the Query protocol
//! is reshaped by the service), so the readers below are written once for SQS and SNS. A number comes as a JSON number (JSON
//! protocol) or as text (Query protocol).

use serde_json::Value;

use super::errors::AwsError;

/// A text parameter; an empty text counts as absent.
pub fn text<'a>(params: &'a Value, name: &str) -> Option<&'a str> {
    params
        .get(name)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
}

pub fn required_text<'a>(params: &'a Value, name: &str) -> Result<&'a str, AwsError> {
    text(params, name).ok_or_else(|| AwsError::missing_parameter(name))
}

/// A whole number parameter, sent as a JSON number (JSON protocol) or as text (Query protocol).
pub fn number(params: &Value, name: &str) -> Result<Option<i64>, AwsError> {
    match params.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(n)) => n.as_i64().map(Some).ok_or_else(|| not_a_number(name)),
        Some(Value::String(s)) if s.trim().is_empty() => Ok(None),
        Some(Value::String(s)) => s
            .trim()
            .parse::<i64>()
            .map(Some)
            .map_err(|_| not_a_number(name)),
        Some(_) => Err(not_a_number(name)),
    }
}

fn not_a_number(name: &str) -> AwsError {
    AwsError::invalid_parameter(format!("The parameter {name} must be a whole number."))
}

/// A number inside a range.
pub fn number_in(params: &Value, name: &str, low: i64, high: i64) -> Result<Option<i64>, AwsError> {
    let value = number(params, name)?;
    if let Some(n) = value
        && !(low..=high).contains(&n)
    {
        return Err(AwsError::invalid_parameter(format!(
            "Value {n} for parameter {name} is invalid. Reason: Must be between {low} and {high}."
        )));
    }
    Ok(value)
}

/// A list of texts (`AttributeNames`, `TagKeys`).
pub fn texts(params: &Value, name: &str) -> Vec<String> {
    match params.get(name) {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect(),
        Some(Value::String(one)) => vec![one.clone()],
        _ => Vec::new(),
    }
}

/// A map of texts (`Attributes`, `Tags`).
pub fn text_map(params: &Value, name: &str) -> Vec<(String, String)> {
    match params.get(name) {
        Some(Value::Object(map)) => map
            .iter()
            .map(|(k, v)| {
                (
                    k.clone(),
                    v.as_str().map_or_else(|| v.to_string(), str::to_owned),
                )
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Escapes a path segment or a query value for the call to the client API.
pub fn url_encode(text: &str) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else {
            let _ = write!(out, "%{byte:02X}");
        }
    }
    out
}

#[cfg(test)]
mod tests;
