//! What the engine offers to the restoration of the past lives (doc 07, sections 10.3 to 10.6): the check of
//! the topic a message belonged to, the answer to "which of these IDs does your log of excluded messages have?",
//! and the reading of the folder of a life (a store over `${storage.dir}/_lives/<ID>`).

use std::io;
use std::path::Path;

use super::{BlockMessage, Messages};
use crate::core::error::TabaError;
use crate::storage::messages::{ExcludedRow, MessageStore, merged_since};

/// What a restoration finds out about the topic of a message before looking at the dead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TopicCheck {
    /// The topic exists and is the one the message belonged to.
    Alive,
    /// The topic was deleted (or never was a queue): the message is discarded.
    Gone,
    /// The topic exists with another `topicId`: it was deleted and recreated, the message is discarded.
    Recreated,
}

impl Messages {
    /// Is the message's topic the same one? `life_topic_id` is what the folder of the life recorded; without it
    /// (a folder from before the record) nothing proves the topic was recreated, and it counts as the same.
    pub fn check_topic(&self, topic: &str, life_topic_id: Option<&str>) -> TopicCheck {
        let Ok(view) = self.topics.get(topic) else {
            return TopicCheck::Gone;
        };
        if view.definition.queue().is_none() {
            return TopicCheck::Gone;
        }
        match life_topic_id {
            Some(id) if id != view.definition.id => TopicCheck::Recreated,
            _ => TopicCheck::Alive,
        }
    }

    /// The `topicId` the topic has today.
    pub fn current_topic_id(&self, topic: &str) -> Option<String> {
        self.topics
            .get(topic)
            .ok()
            .map(|view| view.definition.id.clone())
    }

    /// Which of the `ids` the node's log of excluded messages has (doc 07, section 10.6).
    pub fn excluded_among(&self, topic: &str, ids: &[String]) -> Vec<String> {
        ids.iter()
            .filter(|id| self.is_excluded(topic, id))
            .cloned()
            .collect()
    }

    /// A store over the folder of a past life.
    pub fn open_life(&self, dir: &Path) -> MessageStore {
        self.store.open_life(dir)
    }

    /// The message the folder of a life has, with the body; `None` for the FIFO's short row and for a body that
    /// vanished.
    pub fn life_message(
        &self,
        life: &MessageStore,
        topic: &str,
        id: &str,
    ) -> Result<Option<BlockMessage>, TabaError> {
        let Some(meta) = life
            .get(topic, id)
            .map_err(|e| self.disk_error("failed to read the message of the life", &e))?
        else {
            return Ok(None);
        };
        if meta.content_type.is_empty() {
            return Ok(None);
        }
        match life.body(topic, id) {
            Ok(body) => Ok(Some(BlockMessage {
                topic: topic.to_owned(),
                meta,
                body: body.into(),
                delete_at_ms: None,
            })),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(self.disk_error("failed to read the body of the life", &e)),
        }
    }

    /// Adds to the node's log the exclusions the folder of a life recorded, for the topics that still exist (doc 07,
    /// section 10.3), and moves the start of the recording back if the life and the node's log meet with no hole
    /// (`life_since` and `life_seen` come from the manifest). They only add evidence of death.
    pub fn import_life_log(
        &self,
        life: &MessageStore,
        life_since: Option<u64>,
        life_seen: u64,
    ) -> io::Result<()> {
        for topic in life.topics_on_disk()? {
            if self.topics.get(&topic).is_err() {
                continue;
            }
            let rows: Vec<ExcludedRow> = life.excluded_all(&topic)?;
            if !rows.is_empty() {
                self.store.import_excluded(&topic, &rows)?;
            }
        }
        if let Some(past) = life_since {
            let own = self.excluded_since();
            let merged = merged_since(own, past, life_seen);
            if merged < own {
                self.store.set_excluded_since(merged)?;
            }
        }
        Ok(())
    }
}
