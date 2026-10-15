//! The messages by **block** of the ring (doc 05, section 7.3): what the node lists and hands to whoever
//! synchronizes (`GET /cluster/blocks/{id}/ids` and `/messages`), what it applies when it is the one
//! synchronizing, and the cleanup of what stopped being its own.
//!
//! A block is a set of ring position ranges (`Ring::block_ranges`). Each message's position
//! is in the metadata (`MessageMeta::position`): on disk, the `position` column answers
//! for the range; in memory, the scan of the topic's map.

use super::{ErrorCode, Messages, Place, Stored, TabaError, TopicState, lock, message_not_found};
use crate::storage::messages::MessageMeta;

/// A row of a block's ID listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockEntry {
    pub topic: String,
    pub id: String,
    /// FIFO queue only: the acknowledged message that deduplication still remembers.
    pub delete_at_ms: Option<u64>,
}

/// A message of the block, with the body.
#[derive(Debug, Clone)]
pub struct BlockMessage {
    pub topic: String,
    pub meta: MessageMeta,
    pub body: Vec<u8>,
    /// FIFO only: it is the short row of an acknowledged message (no body).
    pub delete_at_ms: Option<u64>,
}

/// What `apply_load` did with a message that came from another node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Loaded {
    /// Stored.
    Stored,
    /// The node already had it (an event arrived before the load, or the load repeated).
    Existing,
    /// It already exceeded the `ttl`: not worth copying (doc 13, section 12.10).
    Expired,
    /// The topic does not exist on this node yet.
    UnknownTopic,
}

fn in_ranges(ranges: &[(u64, u64)], position: u64) -> bool {
    ranges
        .iter()
        .any(|(low, high)| (*low..=*high).contains(&position))
}

impl Messages {
    /// The topics that have messages here, in memory or on disk.
    fn message_topics(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .existing_states()
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        if let Ok(on_disk) = self.store.topics_on_disk() {
            names.extend(on_disk);
        }
        names.sort();
        names.dedup();
        names.retain(|name| self.topics.get(name).is_ok());
        names
    }

    /// The IDs the node keeps in the block (the position ranges), of all topics, in ID order,
    /// between `from` and `to` (inclusive), at most `limit`. Messages in memory and on disk.
    pub fn block_entries(
        &self,
        ranges: &[(u64, u64)],
        from: Option<&str>,
        to: Option<&str>,
        limit: usize,
    ) -> Result<Vec<BlockEntry>, TabaError> {
        let mut out: Vec<BlockEntry> = Vec::new();
        for topic in self.message_topics() {
            let rows = self
                .store
                .block_rows(&topic, ranges, from, to, limit)
                .map_err(|e| self.disk_error("failed to list the block on disk", &e))?;
            out.extend(rows.into_iter().map(|row| BlockEntry {
                topic: topic.clone(),
                id: row.id,
                delete_at_ms: row.delete_at_ms,
            }));
            let state = self.state_of(&topic);
            let state = lock(&state);
            let lower = from.unwrap_or("");
            let mut taken = 0;
            for (id, stored) in state
                .memory
                .range::<str, _>((std::ops::Bound::Included(lower), std::ops::Bound::Unbounded))
            {
                if taken >= limit || to.is_some_and(|to| id.as_str() > to) {
                    break;
                }
                if in_ranges(ranges, stored.meta.position) {
                    out.push(BlockEntry {
                        topic: topic.clone(),
                        id: id.clone(),
                        delete_at_ms: None,
                    });
                    taken += 1;
                }
            }
        }
        out.sort_by(|a, b| a.id.cmp(&b.id).then_with(|| a.topic.cmp(&b.topic)));
        out.dedup_by(|a, b| a.id == b.id && a.topic == b.topic);
        out.truncate(limit);
        Ok(out)
    }

    /// Reads the message the node keeps (memory or disk), without checking whether it is in the index or
    /// among the replicas: it is the read of whoever serves the synchronization.
    fn read_held(
        &self,
        topic: &str,
        id: &str,
    ) -> Result<Option<(MessageMeta, Vec<u8>)>, TabaError> {
        {
            let state = self.state_of(topic);
            let state = lock(&state);
            if let Some(stored) = state.memory.get(id) {
                return Ok(Some((stored.meta.clone(), stored.body.clone())));
            }
        }
        let Some(meta) = self
            .store
            .get(topic, id)
            .map_err(|e| self.disk_error("failed to read the message on disk", &e))?
        else {
            return Ok(None);
        };
        if meta.content_type.is_empty() {
            // The FIFO's short row (no body) only serves deduplication.
            return Ok(None);
        }
        match self.store.body(topic, id) {
            Ok(body) => Ok(Some((meta, body))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(self.disk_error("failed to read the body on disk", &e)),
        }
    }

    /// The message that only the previous life's disk still has (PRB-048): the restart recovery
    /// looks for it here after looking at what the node keeps today.
    pub fn held_previous_life(
        &self,
        topic: &str,
        id: &str,
    ) -> Result<Option<BlockMessage>, TabaError> {
        let Some(store) = lock(&self.previous_life).clone() else {
            return Ok(None);
        };
        let Some(meta) = store
            .get(topic, id)
            .map_err(|e| self.disk_error("failed to read the message on disk", &e))?
        else {
            return Ok(None);
        };
        if meta.content_type.is_empty() {
            return Ok(None);
        }
        match store.body(topic, id) {
            Ok(body) => Ok(Some(BlockMessage {
                topic: topic.to_owned(),
                meta,
                body,
                delete_at_ms: None,
            })),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(self.disk_error("failed to read the body on disk", &e)),
        }
    }

    /// The message the node still keeps, in memory or on disk: it is what the recovery of the returning
    /// node looks for for each loss on the list (doc 04, section 6.6, step 2). Only content that
    /// actually exists counts: the FIFO's short row and a body that vanished from disk do not count.
    pub fn held_message(&self, topic: &str, id: &str) -> Result<Option<BlockMessage>, TabaError> {
        Ok(self.read_held(topic, id)?.map(|(meta, body)| BlockMessage {
            topic: topic.to_owned(),
            meta,
            body,
            delete_at_ms: None,
        }))
    }

    /// The block's messages in the ID range `from`–`to`, with the body, up to `max_items` or until
    /// exceeding `max_bytes` of body (the first always goes in). The second value says whether there are more.
    pub fn block_messages(
        &self,
        ranges: &[(u64, u64)],
        from: Option<&str>,
        to: Option<&str>,
        max_items: usize,
        max_bytes: u64,
    ) -> Result<(Vec<BlockMessage>, bool), TabaError> {
        let mut cursor = from.map(str::to_owned);
        loop {
            let entries =
                self.block_entries(ranges, cursor.as_deref(), to, max_items.saturating_add(1))?;
            let more_listed = entries.len() > max_items;
            let mut out = Vec::new();
            let mut bytes = 0u64;
            let mut stopped_early = false;
            let mut last_seen = None;
            for entry in entries.into_iter().take(max_items) {
                last_seen = Some(entry.id.clone());
                if let Some(delete_at_ms) = entry.delete_at_ms {
                    // The short row goes without a body, with the `deleteAt` (doc 17, section 16.7).
                    if let Some(meta) = self
                        .store
                        .get(&entry.topic, &entry.id)
                        .map_err(|e| self.disk_error("failed to read the row on disk", &e))?
                    {
                        out.push(BlockMessage {
                            topic: entry.topic,
                            meta,
                            body: Vec::new(),
                            delete_at_ms: Some(delete_at_ms),
                        });
                    }
                    continue;
                }
                // The message may have been acknowledged between the listing and the read: it simply drops out.
                let Some((meta, body)) = self.read_held(&entry.topic, &entry.id)? else {
                    continue;
                };
                if !out.is_empty() && bytes.saturating_add(body.len() as u64) > max_bytes {
                    stopped_early = true;
                    break;
                }
                bytes += body.len() as u64;
                out.push(BlockMessage {
                    topic: entry.topic,
                    meta,
                    body,
                    delete_at_ms: None,
                });
            }
            if out.is_empty() && more_listed {
                // Everything the window listed vanished: it goes on to after the last one, without returning an
                // empty stretch with "there is more" (the receiver would stay in the same place).
                if let Some(last) = last_seen {
                    cursor = Some(format!("{last} "));
                    continue;
                }
            }
            return Ok((out, more_listed || stopped_early));
        }
    }

    /// A message of the block (`GET /cluster/blocks/{id}/messages/{messageId}`).
    pub fn block_message(
        &self,
        ranges: &[(u64, u64)],
        topic_hint: Option<&str>,
        id: &str,
    ) -> Result<BlockMessage, TabaError> {
        let topics = match topic_hint {
            Some(topic) => vec![topic.to_owned()],
            None => self.message_topics(),
        };
        for topic in topics {
            if let Some((meta, body)) = self.read_held(&topic, id)?
                && in_ranges(ranges, meta.position)
            {
                return Ok(BlockMessage {
                    topic,
                    meta,
                    body,
                    delete_at_ms: None,
                });
            }
        }
        Err(message_not_found())
    }

    /// Applies the message that came from another node in the synchronization: stores it as a copy (the node in
    /// synchronization owns nothing), unless it has already expired by `ttl` or the node already
    /// has it. Idempotent.
    pub fn apply_load(
        &self,
        topic: &str,
        meta: MessageMeta,
        body: Vec<u8>,
    ) -> Result<Loaded, TabaError> {
        let Ok(view) = self.topics.get(topic) else {
            return Ok(Loaded::UnknownTopic);
        };
        if view.definition.queue().is_none() {
            return Err(TabaError::new(
                ErrorCode::TopicWrongType,
                "A distributor topic does not store messages.",
            ));
        }
        let now = self.clock.now_millis();
        let ttl_ms = view.ttl_secs().unwrap_or(u64::MAX).saturating_mul(1000);
        if meta.age_base_ms().saturating_add(ttl_ms) <= now {
            return Ok(Loaded::Expired);
        }
        if self.holds(topic, &meta.id) {
            return Ok(Loaded::Existing);
        }
        self.apply_publish(topic, meta, body)?;
        Ok(Loaded::Stored)
    }

    /// Applies the short row of an acknowledged FIFO message that came from another node in the synchronization
    /// (doc 17, section 16.7): it only stays for deduplication until the `deleteAt`. If the node already has the
    /// message, nothing changes (the ack event, repeated, handles it). Idempotent.
    pub fn apply_tombstone(
        &self,
        topic: &str,
        mut meta: MessageMeta,
        delete_at_ms: u64,
    ) -> Result<Loaded, TabaError> {
        let Ok(view) = self.topics.get(topic) else {
            return Ok(Loaded::UnknownTopic);
        };
        let now = self.clock.now_millis();
        if delete_at_ms <= now {
            return Ok(Loaded::Expired);
        }
        meta.position = self.position_for(topic, &meta.id, meta.group_id.as_deref());
        let state = self.state_of(&view.definition.name);
        let mut state = lock(&state);
        if state.index.contains(&meta.id) || state.replicas.contains(&meta.id) {
            return Ok(Loaded::Existing);
        }
        if let (Some(group), Some(key)) = (&meta.group_id, &meta.deduplication_id) {
            state
                .dedup
                .entry((group.clone(), key.clone()))
                .or_insert((meta.id.clone(), meta.published_ms));
        }
        if let Some(group) = &meta.group_id {
            let last = state.group_ms.entry(group.clone()).or_default();
            *last = (*last).max(meta.published_ms);
        }
        self.store
            .tombstone(topic, &meta, delete_at_ms)
            .map_err(|e| self.disk_error("failed to write the deduplication row", &e))?;
        Ok(Loaded::Stored)
    }

    /// Does the node keep the message (in the index or among the replicas)?
    pub fn holds(&self, topic: &str, id: &str) -> bool {
        let state = self.state_of(topic);
        let state = lock(&state);
        state.index.contains(id) || state.replicas.contains(id)
    }

    /// Deletes the copies the node keeps and is no longer responsible for (doc 05, section 7.3,
    /// step 7): `responsible` says, by ring position, whether the node is still one of the block's nodes. It only
    /// touches the copies that are not the owner's (the owner's, in the index, always belong to its block).
    /// Returns how many it deleted.
    pub fn forget_foreign(&self, responsible: &dyn Fn(u64) -> bool) -> usize {
        let mut removed = 0;
        for (name, state) in self.existing_states() {
            let mut state = lock(&state);
            for id in state.replicas.ids() {
                let held = self.held_meta(&state, &name, &id);
                let Some(meta) = held else {
                    // The copy vanished from storage: only its record is left.
                    state.replicas.remove(&id);
                    continue;
                };
                if responsible(meta.position) {
                    continue;
                }
                let place = if state.memory.contains_key(&id) {
                    Place::Memory
                } else {
                    Place::Disk
                };
                match self.remove_message(&mut state, &name, &id, place) {
                    Ok(()) => removed += 1,
                    Err(_) => {
                        tracing::error!(topic = %name, id = %id, "failed to delete the copy that stopped being the node's");
                    }
                }
            }
            self.ready_changed(&name, &mut state);
        }
        removed
    }

    /// The copy's metadata, wherever it is.
    pub(super) fn held_meta(
        &self,
        state: &TopicState,
        topic: &str,
        id: &str,
    ) -> Option<MessageMeta> {
        match state.memory.get(id) {
            Some(Stored { meta, .. }) => Some(meta.clone()),
            None => self.store.get(topic, id).ok().flatten(),
        }
    }
}
