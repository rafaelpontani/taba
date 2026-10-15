//! The bodies of the definition routes between nodes (doc 06c, "Topics and tokens") and the contract of the
//! synchronization streams (section 8.2.1): the proposal to the owner, the decided change and the page of an
//! NDJSON stream.
//!
//! The topic definition and the token record travel as JSON (`TopicDefinition` and `TokenRecord`
//! of doc 06c): the node that receives converts them to the `core` types.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::core::error::{ErrorCode, TabaError};
use crate::core::time;
use crate::core::token::{Permission, TokenRecord};

/// The type of a definition shared by all nodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DefinitionKind {
    Topic,
    Token,
}

impl DefinitionKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            DefinitionKind::Topic => "topic",
            DefinitionKind::Token => "token",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DefinitionOperation {
    Create,
    Update,
    /// Removes the topic or revokes the token.
    Delete,
}

/// A change proposal, from whoever received the client's call to the definition's owner.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DefinitionProposal {
    pub kind: DefinitionKind,
    pub operation: DefinitionOperation,
    /// The topic name or the token `id`.
    pub key: String,
    /// The full `TopicDefinition` or `TokenRecord` (the token's carries the salt and the salted hash,
    /// **never the secret**); on `delete`, the topic carries `name` and `id`, and the token, `id`.
    pub body: Value,
    /// On `delete` by the managing node of a volatile queue: the `id` the topic must have.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_id: Option<String>,
}

/// The change already decided by the owner, with the version it gave, as each node applies it. It only applies if the
/// version is not lower than the one the node already has; repeating is harmless.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DefinitionApply {
    pub kind: DefinitionKind,
    pub key: String,
    pub version: u64,
    /// `true`: the topic was removed or the token revoked (the version stays as a tombstone).
    pub deleted: bool,
    #[serde(default)]
    pub body: Value,
    /// `true` on the copy the owner writes to the **heir**: whoever receives it, besides applying it, queues it
    /// for all peers in the channel's `definition` frame (D127). The owner does the same.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub relay: bool,
}

/// The doc 06c `TokenRecord`, as JSON.
pub fn token_to_json(token: &TokenRecord) -> Value {
    json!({
        "id": token.id,
        "name": token.name,
        "permissions": token.permissions.iter().map(|p| p.as_str()).collect::<Vec<_>>(),
        "topicPrefixes": token.topic_prefixes,
        "createdAt": time::format_ms(token.created_ms),
        "expiresAt": token.expires_ms.map(time::format_ms),
        "salt": token.salt,
        "secretHash": token.secret_hash,
    })
}

/// The inverse of [`token_to_json`].
pub fn token_from_json(value: &Value) -> Result<TokenRecord, TabaError> {
    let bad = |what: &str| {
        TabaError::new(
            ErrorCode::ValidationFailed,
            format!("The token between nodes is invalid: {what}."),
        )
    };
    let object = value.as_object().ok_or_else(|| bad("not an object"))?;
    let text = |key: &str| {
        object
            .get(key)
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| bad(&format!("falta `{key}`")))
    };
    let date = |key: &str| -> Result<Option<u64>, TabaError> {
        match object.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(s)) => time::parse_ms(s)
                .map(Some)
                .ok_or_else(|| bad(&format!("`{key}` invalid"))),
            Some(_) => Err(bad(&format!("`{key}` invalid"))),
        }
    };
    let list = |key: &str| -> Result<Vec<String>, TabaError> {
        object
            .get(key)
            .and_then(Value::as_array)
            .ok_or_else(|| bad(&format!("falta `{key}`")))?
            .iter()
            .map(|v| {
                v.as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| bad(&format!("`{key}` has an item that is not text")))
            })
            .collect()
    };
    let mut permissions = Vec::new();
    for name in list("permissions")? {
        let permission = Permission::ALL
            .into_iter()
            .find(|p| p.as_str() == name)
            .ok_or_else(|| bad(&format!("unknown permission `{name}`")))?;
        permissions.push(permission);
    }
    Ok(TokenRecord {
        id: text("id")?,
        name: text("name")?,
        permissions,
        topic_prefixes: list("topicPrefixes")?,
        created_ms: date("createdAt")?.ok_or_else(|| bad("falta `createdAt`"))?,
        expires_ms: date("expiresAt")?,
        salt: text("salt")?,
        secret_hash: text("secretHash")?,
    })
}

/// The query of a synchronization stream: `from` (inclusive) and `limit` (lowers the ceiling).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StreamQuery {
    pub from: Option<String>,
    pub to: Option<String>,
    pub limit: Option<u32>,
}

/// `StreamEnd` (doc 06d, section 8.3.7): the final line of every stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StreamEnd {
    pub end: bool,
    pub count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last: Option<String>,
    pub has_more: bool,
}

/// A stretch of a stream: the data lines and the final line. Whoever asks for the next stretch repeats the
/// query with `from = last` (the repeated line is ignored).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamPage {
    pub items: Vec<Value>,
    pub end: StreamEnd,
}

impl StreamPage {
    /// Builds the stretch with each line's `id` as `last`.
    pub fn new(items: Vec<Value>, has_more: bool) -> Self {
        let last = items
            .last()
            .and_then(|item| item.get("id"))
            .and_then(Value::as_str)
            .map(str::to_owned);
        Self {
            end: StreamEnd {
                end: true,
                count: items.len() as u64,
                last,
                has_more,
            },
            items,
        }
    }

    /// NDJSON: one object per line and, last, the `StreamEnd`.
    pub fn to_ndjson(&self) -> String {
        let mut out = String::new();
        for item in &self.items {
            out.push_str(&item.to_string());
            out.push('\n');
        }
        out.push_str(&serde_json::to_string(&self.end).unwrap_or_default());
        out.push('\n');
        out
    }

    /// The inverse. Without the final line the stretch is **flawed**, even if the connection closed
    /// cleanly (doc 06c, section 8.2.1).
    pub fn from_ndjson(text: &str) -> Result<Self, TabaError> {
        let bad = |what: &str| {
            TabaError::new(
                ErrorCode::ValidationFailed,
                format!("Incomplete or invalid stream: {what}."),
            )
        };
        let mut items = Vec::new();
        for line in text.lines().filter(|l| !l.trim().is_empty()) {
            let value: Value = serde_json::from_str(line).map_err(|_| bad("unreadable line"))?;
            if value.get("error").is_some() {
                return Err(bad("the sender reported an error"));
            }
            if value.get("end").is_some() {
                let end: StreamEnd =
                    serde_json::from_value(value).map_err(|_| bad("invalid final line"))?;
                if end.count != items.len() as u64 {
                    return Err(bad("the final line's count does not match"));
                }
                return Ok(Self { items, end });
            }
            items.push(value);
        }
        Err(bad("the final line is missing"))
    }
}
