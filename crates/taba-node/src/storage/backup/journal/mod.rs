//! The backup's journal (`.jrn`) (doc 23, "S3 backup"): the messages' **mutable state**, which
//! is not in the segments (`delivery_count`, `invisible_until`, `receipt_token`, dead-letter).
//!
//! A journal is a file of JSON lines, written in batch every `backup.meta.interval`: only the
//! **last state** of each changed message goes in. An ack goes in as a **removal**: a loose message is
//! never deleted from the bucket (only the whole segment, when all of its messages died).
//!
//! ```text
//! { "topic", "topicId", "writer", "atMs" }          first line
//! { "id", "removed": true }                         removal (ack, `ttl`, dead-letter)
//! { "id", "deliveryCount", "invisibleUntilMs", ... } state
//! ```

use std::io;

use serde::{Deserialize, Serialize};

use crate::storage::messages::MessageMeta;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Header {
    pub topic: String,
    #[serde(rename = "topicId")]
    pub topic_id: String,
    pub writer: String,
    #[serde(rename = "atMs")]
    pub at_ms: u64,
}

/// The state of a message, or its removal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    pub id: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub removed: bool,
    #[serde(default, rename = "deliveryCount")]
    pub delivery_count: u32,
    #[serde(default, rename = "invisibleUntilMs")]
    pub invisible_until_ms: u64,
    #[serde(
        default,
        rename = "receiptToken",
        skip_serializing_if = "Option::is_none"
    )]
    pub receipt_token: Option<String>,
    #[serde(
        default,
        rename = "deadLetteredMs",
        skip_serializing_if = "Option::is_none"
    )]
    pub dead_lettered_ms: Option<u64>,
    #[serde(
        default,
        rename = "deadLetterReason",
        skip_serializing_if = "Option::is_none"
    )]
    pub dead_letter_reason: Option<String>,
}

impl Record {
    pub fn removal(id: &str) -> Self {
        Self {
            id: id.to_owned(),
            removed: true,
            delivery_count: 0,
            invisible_until_ms: 0,
            receipt_token: None,
            dead_lettered_ms: None,
            dead_letter_reason: None,
        }
    }

    pub fn state(meta: &MessageMeta) -> Self {
        Self {
            id: meta.id.clone(),
            removed: false,
            delivery_count: meta.delivery_count,
            invisible_until_ms: meta.invisible_until_ms,
            receipt_token: meta.receipt_token.clone(),
            dead_lettered_ms: meta.dead_lettered_ms,
            dead_letter_reason: meta.dead_letter_reason.clone(),
        }
    }

    /// Applies the written state to the metadata read from the segment.
    pub fn apply_to(&self, meta: &mut MessageMeta) {
        meta.delivery_count = self.delivery_count;
        meta.invisible_until_ms = self.invisible_until_ms;
        meta.receipt_token.clone_from(&self.receipt_token);
        meta.dead_lettered_ms = self.dead_lettered_ms;
        meta.dead_letter_reason.clone_from(&self.dead_letter_reason);
    }
}

pub fn encode(header: &Header, records: &[Record]) -> Vec<u8> {
    let mut out = serde_json::to_vec(header).expect("header serializes");
    out.push(b'\n');
    for record in records {
        out.extend_from_slice(&serde_json::to_vec(record).expect("registro serializa"));
        out.push(b'\n');
    }
    out
}

pub fn decode(bytes: &[u8]) -> io::Result<(Header, Vec<Record>)> {
    let invalid =
        |what: String| io::Error::new(io::ErrorKind::InvalidData, format!("journal: {what}"));
    let text = std::str::from_utf8(bytes).map_err(|e| invalid(e.to_string()))?;
    let mut lines = text.lines().filter(|l| !l.trim().is_empty());
    let header = lines
        .next()
        .ok_or_else(|| invalid("empty".into()))
        .and_then(|l| serde_json::from_str(l).map_err(|e| invalid(e.to_string())))?;
    let records = lines
        .map(|l| serde_json::from_str(l).map_err(|e| invalid(e.to_string())))
        .collect::<Result<_, _>>()?;
    Ok((header, records))
}

#[cfg(test)]
mod tests;
