//! The object names in the bucket (`backup.s3.prefix` already comes in front, from `Prefixed`).
//!
//! ```text
//! topics/<topic>.json                                  definition (or tombstone)
//! tokens/<id>.json                                      token
//! nodes/<id>.json                                       node heartbeat
//! messages/<topic>/<topicId>/<firstId>_<writer>_<seq>.seg
//! journal/<topic>/<topicId>/<instant>_<writer>_<seq>.jrn
//! restore/lock.json                                     lock of whoever restores
//! cluster/stop.json                                     list of the planned stop
//! ```
//!
//! The `topicId` in the path stops the messages of a deleted topic from being resurrected in a new topic
//! with the same name. The segment name starts with the `id` of the first message (GUID v7): the names
//! sort by time and give the age for the `ttl`.

pub const TOPICS_PREFIX: &str = "topics/";
pub const TOKENS_PREFIX: &str = "tokens/";
pub const NODES_PREFIX: &str = "nodes/";
pub const MESSAGES_PREFIX: &str = "messages/";
pub const JOURNAL_PREFIX: &str = "journal/";
pub const RESTORE_LOCK: &str = "restore/lock.json";
pub const STOP_LIST: &str = "cluster/stop.json";

pub fn topic(name: &str) -> String {
    format!("{TOPICS_PREFIX}{name}.json")
}

pub fn token(id: &str) -> String {
    format!("{TOKENS_PREFIX}{id}.json")
}

pub fn node(id: &str) -> String {
    format!("{NODES_PREFIX}{id}.json")
}

/// The `id` of a `nodes/<id>.json`.
pub fn node_id_of(key: &str) -> Option<&str> {
    key.strip_prefix(NODES_PREFIX)?.strip_suffix(".json")
}

/// Everything that belongs to a topic's messages (all its incarnations).
pub fn topic_messages_prefix(topic: &str) -> String {
    format!("{MESSAGES_PREFIX}{topic}/")
}

pub fn topic_journal_prefix(topic: &str) -> String {
    format!("{JOURNAL_PREFIX}{topic}/")
}

pub fn segments_prefix(topic: &str, topic_id: &str) -> String {
    format!("{MESSAGES_PREFIX}{topic}/{topic_id}/")
}

pub fn journals_prefix(topic: &str, topic_id: &str) -> String {
    format!("{JOURNAL_PREFIX}{topic}/{topic_id}/")
}

pub fn segment(topic: &str, topic_id: &str, first_id: &str, writer: &str, seq: u64) -> String {
    format!(
        "{}{first_id}_{writer}_{seq:06}.seg",
        segments_prefix(topic, topic_id)
    )
}

pub fn journal(topic: &str, topic_id: &str, at_ms: u64, writer: &str, seq: u64) -> String {
    format!(
        "{}{at_ms:016}_{writer}_{seq:06}.jrn",
        journals_prefix(topic, topic_id)
    )
}

/// The `(topic, topicId)` of a segment or journal.
pub fn topic_of(key: &str) -> Option<(&str, &str)> {
    let rest = key
        .strip_prefix(MESSAGES_PREFIX)
        .or_else(|| key.strip_prefix(JOURNAL_PREFIX))?;
    let mut parts = rest.splitn(3, '/');
    Some((parts.next()?, parts.next()?))
}

/// The `id` of a segment's first message, by name.
pub fn first_id_of(segment_key: &str) -> Option<&str> {
    let name = segment_key.rsplit('/').next()?;
    name.split('_').next()
}

/// Who wrote the segment (the node's `id`), by name.
pub fn writer_of(segment_key: &str) -> Option<&str> {
    let name = segment_key.rsplit('/').next()?.strip_suffix(".seg")?;
    name.split('_').nth(1)
}

#[cfg(test)]
mod tests;
