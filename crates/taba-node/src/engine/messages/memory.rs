//! The messages a topic keeps in memory, in `id` order, with a second ordering by age so the oldest
//! one is found without walking them all.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Deref;

use super::Stored;

#[derive(Default)]
pub(super) struct Memory {
    by_id: BTreeMap<String, Stored>,
    /// `(age, id)` of each message: the first one is the oldest.
    by_age: BTreeSet<(u64, String)>,
}

/// Reads go straight to the map; changes go through the methods below, which keep the age order.
impl Deref for Memory {
    type Target = BTreeMap<String, Stored>;

    fn deref(&self) -> &Self::Target {
        &self.by_id
    }
}

impl Memory {
    pub(super) fn insert(&mut self, id: String, stored: Stored) -> Option<Stored> {
        self.by_age.insert((stored.meta.age_base_ms(), id.clone()));
        let previous = self.by_id.insert(id.clone(), stored);
        if let Some(previous) = &previous {
            self.forget_age(previous.meta.age_base_ms(), &id);
        }
        previous
    }

    pub(super) fn remove(&mut self, id: &str) -> Option<Stored> {
        let stored = self.by_id.remove(id)?;
        self.forget_age(stored.meta.age_base_ms(), id);
        Some(stored)
    }

    /// Replaces the metadata of a message that is here; the age order follows.
    pub(super) fn set_meta(&mut self, meta: &crate::storage::messages::MessageMeta) {
        let Some(stored) = self.by_id.get_mut(&meta.id) else {
            return;
        };
        let before = stored.meta.age_base_ms();
        stored.meta = meta.clone();
        let after = stored.meta.age_base_ms();
        if before != after {
            self.by_age.remove(&(before, meta.id.clone()));
            self.by_age.insert((after, meta.id.clone()));
        }
    }

    /// The messages from the oldest to the newest: `(age, id)`.
    pub(super) fn by_age(&self) -> impl Iterator<Item = &(u64, String)> {
        self.by_age.iter()
    }

    fn forget_age(&mut self, age: u64, id: &str) {
        // Nothing to remove when the id is still here with the same age (a replacement).
        if !self
            .by_id
            .get(id)
            .is_some_and(|now| now.meta.age_base_ms() == age)
        {
            self.by_age.remove(&(age, id.to_owned()));
        }
    }
}
