//! The backup's segment (`.seg`) format (doc 23, "S3 backup").
//!
//! ```text
//! "TABASEG1"
//! entry*   = u32 (LE) metadata size | metadata (JSON) | body
//! footer   = JSON { topic, topicId, writer, entries: [{ id, offset, len }] }
//! u32 (LE) footer size | "TABAFTR1"
//! ```
//!
//! The index footer lets you list a segment without downloading it (`get_suffix`) and read a single message by
//! `Range` (`offset` and `len` cover the whole entry: metadata and body).

use std::io;

use serde::{Deserialize, Serialize};

use crate::storage::messages::MessageMeta;

const HEADER: &[u8; 8] = b"TABASEG1";
const TRAILER: &[u8; 8] = b"TABAFTR1";
/// What is read from the end of the object on the first attempt to find the footer.
pub const TAIL_GUESS: u64 = 16 * 1024;

/// A message's metadata as the backup stores it: the same as [`MessageMeta`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupMessage {
    pub id: String,
    pub content_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deduplication_id: Option<String>,
    pub published_ms: u64,
    #[serde(default)]
    pub delivery_count: u32,
    #[serde(default)]
    pub invisible_until_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dead_lettered_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dead_letter_reason: Option<String>,
    #[serde(default)]
    pub position: u64,
}

impl From<&MessageMeta> for BackupMessage {
    fn from(meta: &MessageMeta) -> Self {
        Self {
            id: meta.id.clone(),
            content_type: meta.content_type.clone(),
            group_id: meta.group_id.clone(),
            deduplication_id: meta.deduplication_id.clone(),
            published_ms: meta.published_ms,
            delivery_count: meta.delivery_count,
            invisible_until_ms: meta.invisible_until_ms,
            receipt_token: meta.receipt_token.clone(),
            dead_lettered_ms: meta.dead_lettered_ms,
            dead_letter_reason: meta.dead_letter_reason.clone(),
            position: meta.position,
        }
    }
}

impl From<BackupMessage> for MessageMeta {
    fn from(m: BackupMessage) -> Self {
        Self {
            id: m.id,
            content_type: m.content_type,
            group_id: m.group_id,
            deduplication_id: m.deduplication_id,
            published_ms: m.published_ms,
            delivery_count: m.delivery_count,
            invisible_until_ms: m.invisible_until_ms,
            receipt_token: m.receipt_token,
            dead_lettered_ms: m.dead_lettered_ms,
            dead_letter_reason: m.dead_letter_reason,
            position: m.position,
        }
    }
}

/// Where a message is inside the segment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexEntry {
    pub id: String,
    pub offset: u64,
    pub len: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Footer {
    pub topic: String,
    #[serde(rename = "topicId")]
    pub topic_id: String,
    pub writer: String,
    pub entries: Vec<IndexEntry>,
}

/// Builds a segment, message by message.
pub struct SegmentBuilder {
    footer: Footer,
    buf: Vec<u8>,
}

impl SegmentBuilder {
    pub fn new(topic: &str, topic_id: &str, writer: &str) -> Self {
        Self {
            footer: Footer {
                topic: topic.to_owned(),
                topic_id: topic_id.to_owned(),
                writer: writer.to_owned(),
                entries: Vec::new(),
            },
            buf: HEADER.to_vec(),
        }
    }

    /// Appends the message and returns where it ended up.
    pub fn push(&mut self, meta: &MessageMeta, body: &[u8]) -> IndexEntry {
        let head = serde_json::to_vec(&BackupMessage::from(meta)).expect("metadados serializam");
        let offset = self.buf.len() as u64;
        self.buf
            .extend_from_slice(&(head.len() as u32).to_le_bytes());
        self.buf.extend_from_slice(&head);
        self.buf.extend_from_slice(body);
        let entry = IndexEntry {
            id: meta.id.clone(),
            offset,
            len: self.buf.len() as u64 - offset,
        };
        self.footer.entries.push(entry.clone());
        entry
    }

    pub fn count(&self) -> usize {
        self.footer.entries.len()
    }

    /// The bytes of the entries so far (without the footer).
    pub fn bytes(&self) -> usize {
        self.buf.len()
    }

    pub fn first_id(&self) -> Option<&str> {
        self.footer.entries.first().map(|e| e.id.as_str())
    }

    pub fn entries(&self) -> &[IndexEntry] {
        &self.footer.entries
    }

    /// Closes the segment: the entries, the footer and the footer size.
    pub fn finish(mut self) -> Vec<u8> {
        let footer = serde_json::to_vec(&self.footer).expect("footer serializes");
        self.buf.extend_from_slice(&footer);
        self.buf
            .extend_from_slice(&(footer.len() as u32).to_le_bytes());
        self.buf.extend_from_slice(TRAILER);
        self.buf
    }
}

fn invalid(what: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("invalid segment: {what}"),
    )
}

/// What `read_footer` decides from the end of the object.
#[derive(Debug, PartialEq, Eq)]
pub enum FooterRead {
    Done(Footer),
    /// The part read is short: the whole footer has this size (with the end), read again.
    NeedMore(u64),
}

/// Reads the footer from the end of the segment (`tail` are the object's last bytes).
pub fn read_footer(tail: &[u8]) -> io::Result<FooterRead> {
    if tail.len() < 12 || &tail[tail.len() - 8..] != TRAILER {
        return Err(invalid("without the expected end"));
    }
    let size_at = tail.len() - 12;
    let footer_len = u32::from_le_bytes(tail[size_at..size_at + 4].try_into().expect("4 bytes"));
    let needed = u64::from(footer_len) + 12;
    if needed > tail.len() as u64 {
        return Ok(FooterRead::NeedMore(needed));
    }
    let body = &tail[size_at - footer_len as usize..size_at];
    serde_json::from_slice(body)
        .map(FooterRead::Done)
        .map_err(|e| invalid(&format!("footer: {e}")))
}

/// Reads an entry (metadata and body), as the footer's `offset` and `len` delimit it.
pub fn decode_entry(entry: &[u8]) -> io::Result<(BackupMessage, Vec<u8>)> {
    if entry.len() < 4 {
        return Err(invalid("short entry"));
    }
    let head_len = u32::from_le_bytes(entry[..4].try_into().expect("4 bytes")) as usize;
    if entry.len() < 4 + head_len {
        return Err(invalid("truncated entry"));
    }
    let meta = serde_json::from_slice(&entry[4..4 + head_len])
        .map_err(|e| invalid(&format!("metadados: {e}")))?;
    Ok((meta, entry[4 + head_len..].to_vec()))
}

/// An entry read from the segment: the metadata and the body.
pub type Entry = (BackupMessage, Vec<u8>);

/// Reads the whole segment: the footer and all the entries.
pub fn decode_all(whole: &[u8]) -> io::Result<(Footer, Vec<Entry>)> {
    if whole.len() < HEADER.len() || &whole[..HEADER.len()] != HEADER {
        return Err(invalid("without the expected beginning"));
    }
    let footer = match read_footer(whole)? {
        FooterRead::Done(footer) => footer,
        FooterRead::NeedMore(_) => return Err(invalid("truncated footer")),
    };
    let mut messages = Vec::with_capacity(footer.entries.len());
    for entry in &footer.entries {
        let (from, to) = (entry.offset as usize, (entry.offset + entry.len) as usize);
        if to > whole.len() {
            return Err(invalid("entry outside the object"));
        }
        messages.push(decode_entry(&whole[from..to])?);
    }
    Ok((footer, messages))
}

#[cfg(test)]
mod tests;
