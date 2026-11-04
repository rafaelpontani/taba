//! Where the attributes of a message live (doc 39). A Taba message has a body, a `contentType`, a `groupId` and a
//! `deduplicationId`; an SQS message also has **message attributes** (name, type and value) that come back on receipt
//! with an MD5 that the SDK checks.
//!
//! * **Without attributes** the body is stored as it is, as `text/plain`: a native Taba consumer reads exactly what
//!   the application sent.
//! * **With attributes** the message is stored as `application/json` with the envelope
//!   `{"tabaSqs":1,"body":"...","attributes":{"name":{"type":"String","value":"..."}}}`; a native consumer can read it
//!   as JSON and recognises it by `tabaSqs`.
//!
//! * **On a FIFO queue**, a group or a deduplication id that Taba does not accept (SQS allows more punctuation) is sent as
//!   a hash and the original goes in the envelope as `group` and `dedup`, so the receiver gets it back; such a message
//!   is an envelope even without attributes.
//!
//! Nothing changes in the server, in the formats on disk or in the replication.

use std::collections::BTreeMap;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use super::aws::errors::AwsError;
use super::aws::sigv4::hex;

/// The key that marks a body as an envelope.
const MARK: &str = "tabaSqs";
pub const PLAIN_CONTENT_TYPE: &str = "text/plain; charset=utf-8";
pub const ENVELOPE_CONTENT_TYPE: &str = "application/json";

/// The most attributes a message may carry (the same limit as SQS).
pub const MAX_ATTRIBUTES: usize = 10;

/// One message attribute, as the SDK sends it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attribute {
    /// `String`, `Number` or `Binary`, with an optional `.label` (`String.json`).
    pub data_type: String,
    /// The text of a `String` or `Number`.
    pub string_value: Option<String>,
    /// The bytes of a `Binary`, in base64 as they travel.
    pub binary_value: Option<String>,
}

impl Attribute {
    fn is_binary(&self) -> bool {
        self.data_type.starts_with("Binary")
    }
}

pub type Attributes = BTreeMap<String, Attribute>;

/// A message ready to be published.
pub struct Packed {
    pub content_type: &'static str,
    /// The body as the batch route takes it: a string for text, the object for JSON.
    pub value: Value,
    /// The same, as the bytes of a single publication.
    pub bytes: Vec<u8>,
}

/// The SHA-256 of the bytes, in hexadecimal.
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

/// The group and deduplication id of a FIFO message as the application wrote them, when Taba got a hash instead.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Origin {
    pub group: Option<String>,
    pub dedup: Option<String>,
}

impl Origin {
    fn is_empty(&self) -> bool {
        self.group.is_none() && self.dedup.is_none()
    }
}

/// Packs the body and the attributes for publishing.
pub fn pack(body: &str, attributes: &Attributes) -> Packed {
    pack_with(body, attributes, &Origin::default())
}

/// Packs the body, the attributes and the original ids of a FIFO message for publishing.
pub fn pack_with(body: &str, attributes: &Attributes, origin: &Origin) -> Packed {
    if attributes.is_empty() && origin.is_empty() {
        return Packed {
            content_type: PLAIN_CONTENT_TYPE,
            value: Value::String(body.to_owned()),
            bytes: body.as_bytes().to_vec(),
        };
    }
    let mut stored = Map::new();
    for (name, attribute) in attributes {
        let value = if attribute.is_binary() {
            attribute.binary_value.clone().unwrap_or_default()
        } else {
            attribute.string_value.clone().unwrap_or_default()
        };
        stored.insert(
            name.clone(),
            json!({"type": attribute.data_type, "value": value}),
        );
    }
    let mut value = json!({MARK: 1, "body": body, "attributes": stored});
    if let Some(group) = &origin.group {
        value["group"] = json!(group);
    }
    if let Some(dedup) = &origin.dedup {
        value["dedup"] = json!(dedup);
    }
    Packed {
        content_type: ENVELOPE_CONTENT_TYPE,
        bytes: serde_json::to_vec(&value).unwrap_or_default(),
        value,
    }
}

/// What a received Taba message holds, as the SQS client sees it.
pub struct Unpacked {
    pub body: String,
    pub attributes: Attributes,
    /// The original ids, when the message was sent with ones Taba does not accept.
    pub origin: Origin,
}

/// Opens the `body` of a message delivered by the client API (`bodyEncoding` is `base64` for a type that is not text
/// or JSON). A message that was not published through the gateway has no attributes and its body is read as text.
pub fn unpack(body: &Value, body_encoding: Option<&str>) -> Unpacked {
    let plain = |text: String| Unpacked {
        body: text,
        attributes: Attributes::new(),
        origin: Origin::default(),
    };
    match body {
        Value::String(text) if body_encoding == Some("base64") => {
            let bytes = STANDARD.decode(text).unwrap_or_default();
            plain(String::from_utf8_lossy(&bytes).into_owned())
        }
        Value::String(text) => plain(text.clone()),
        Value::Object(object) if object.get(MARK).and_then(Value::as_i64) == Some(1) => {
            let text = object
                .get("body")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            let mut attributes = Attributes::new();
            if let Some(Value::Object(stored)) = object.get("attributes") {
                for (name, entry) in stored {
                    let data_type = entry
                        .get("type")
                        .and_then(Value::as_str)
                        .unwrap_or("String")
                        .to_owned();
                    let value = entry
                        .get("value")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned();
                    let attribute = if data_type.starts_with("Binary") {
                        Attribute {
                            data_type,
                            string_value: None,
                            binary_value: Some(value),
                        }
                    } else {
                        Attribute {
                            data_type,
                            string_value: Some(value),
                            binary_value: None,
                        }
                    };
                    attributes.insert(name.clone(), attribute);
                }
            }
            let original = |key: &str| object.get(key).and_then(Value::as_str).map(str::to_owned);
            Unpacked {
                body: text,
                attributes,
                origin: Origin {
                    group: original("group"),
                    dedup: original("dedup"),
                },
            }
        }
        other => plain(other.to_string()),
    }
}

/// Reads the `MessageAttributes` of a request (name to `{DataType, StringValue, BinaryValue}`) and checks the rules
/// of SQS: up to 10, a valid name, a type, and the value that goes with the type.
pub fn parse_attributes(value: Option<&Value>) -> Result<Attributes, AwsError> {
    let Some(Value::Object(map)) = value else {
        return Ok(Attributes::new());
    };
    if map.len() > MAX_ATTRIBUTES {
        return Err(AwsError::invalid_parameter(
            "Number of message attributes exceeds the allowed maximum of 10.",
        ));
    }
    let mut attributes = Attributes::new();
    for (name, entry) in map {
        if name.is_empty()
            || name.len() > 256
            || !name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
            || name.starts_with('.')
            || name.ends_with('.')
            || name.contains("..")
        {
            return Err(AwsError::invalid_parameter(format!(
                "Invalid message attribute name `{name}`: use up to 256 letters, digits, underscores, hyphens and periods."
            )));
        }
        let lower = name.to_ascii_lowercase();
        if lower.starts_with("aws.") || lower.starts_with("amazon.") {
            return Err(AwsError::invalid_parameter(format!(
                "The message attribute name `{name}` uses a reserved prefix."
            )));
        }
        let data_type = entry
            .get("DataType")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                AwsError::invalid_parameter(format!(
                    "The message attribute `{name}` must contain a non-empty DataType."
                ))
            })?
            .to_owned();
        let base = data_type.split('.').next().unwrap_or_default();
        let string_value = entry
            .get("StringValue")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let binary_value = entry
            .get("BinaryValue")
            .and_then(Value::as_str)
            .map(str::to_owned);
        match base {
            "String" | "Number" => {
                let text = string_value.as_ref().ok_or_else(|| {
                    AwsError::invalid_parameter(format!(
                        "The message attribute `{name}` of type {data_type} must contain a StringValue."
                    ))
                })?;
                if text.is_empty() {
                    return Err(AwsError::invalid_parameter(format!(
                        "The message attribute `{name}` must contain a non-empty value."
                    )));
                }
                if base == "Number" && text.parse::<f64>().is_err() {
                    return Err(AwsError::invalid_parameter(format!(
                        "The message attribute `{name}` of type Number is not a number."
                    )));
                }
            }
            "Binary" => {
                let text = binary_value.as_ref().ok_or_else(|| {
                    AwsError::invalid_parameter(format!(
                        "The message attribute `{name}` of type {data_type} must contain a BinaryValue."
                    ))
                })?;
                if STANDARD.decode(text).is_err() {
                    return Err(AwsError::invalid_parameter(format!(
                        "The BinaryValue of the message attribute `{name}` is not valid base64."
                    )));
                }
            }
            _ => {
                return Err(AwsError::invalid_parameter(format!(
                    "The message attribute `{name}` has the unsupported type {data_type}: use String, Number or Binary."
                )));
            }
        }
        attributes.insert(
            name.clone(),
            Attribute {
                data_type,
                string_value,
                binary_value,
            },
        );
    }
    Ok(attributes)
}

/// A length (4 bytes, big endian) followed by the bytes.
fn put(data: &mut Vec<u8>, bytes: &[u8]) {
    data.extend_from_slice(&u32::try_from(bytes.len()).unwrap_or(u32::MAX).to_be_bytes());
    data.extend_from_slice(bytes);
}

/// `MD5OfMessageBody`: the MD5 of the body bytes, in hex.
pub fn md5_of_body(body: &str) -> String {
    format!("{:x}", md5::compute(body.as_bytes()))
}

/// `MD5OfMessageAttributes`: the MD5 of the attributes sorted by name, each written as the length (4 bytes, big
/// endian) and the bytes of the name, of the type, then one byte for the kind of value (1 text, 2 binary) and the length
/// and bytes of the value. The SDKs compute the same and compare.
pub fn md5_of_attributes(attributes: &Attributes) -> Option<String> {
    if attributes.is_empty() {
        return None;
    }
    let mut data = Vec::new();
    for (name, attribute) in attributes {
        put(&mut data, name.as_bytes());
        put(&mut data, attribute.data_type.as_bytes());
        if attribute.is_binary() {
            data.push(2);
            let bytes = attribute
                .binary_value
                .as_deref()
                .and_then(|text| STANDARD.decode(text).ok())
                .unwrap_or_default();
            put(&mut data, &bytes);
        } else {
            data.push(1);
            put(
                &mut data,
                attribute
                    .string_value
                    .as_deref()
                    .unwrap_or_default()
                    .as_bytes(),
            );
        }
    }
    Some(format!("{:x}", md5::compute(&data)))
}

#[cfg(test)]
mod tests;
