//! The messages the node keeps but is **not** the delivery owner of (doc 13, section 12.7): the
//! replicas. Only the block's primary puts the message in the ready and invisible lists; the
//! replicas stay here, with only the `id` and the age, so the `ttl` applies to them too (each node deletes its
//! own copies, doc 13, section 12.10) and so the node knows what it has when it becomes the owner
//! (`Messages::reown`).

use std::collections::{BTreeSet, HashMap};

#[derive(Debug, Default)]
pub struct ReplicaSet {
    /// `id` → age (the instant the `ttl` counts from).
    by_id: HashMap<String, u64>,
    by_age: BTreeSet<(u64, String)>,
}

impl ReplicaSet {
    pub fn insert(&mut self, id: &str, age_ms: u64) {
        self.remove(id);
        self.by_id.insert(id.to_owned(), age_ms);
        self.by_age.insert((age_ms, id.to_owned()));
    }

    pub fn remove(&mut self, id: &str) -> bool {
        match self.by_id.remove(id) {
            Some(age) => {
                self.by_age.remove(&(age, id.to_owned()));
                true
            }
            None => false,
        }
    }

    pub fn contains(&self, id: &str) -> bool {
        self.by_id.contains_key(id)
    }

    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }

    #[cfg(test)]
    pub fn age_of(&self, id: &str) -> Option<u64> {
        self.by_id.get(id).copied()
    }

    /// The `id`s, in age order.
    pub fn ids(&self) -> Vec<String> {
        self.by_age.iter().map(|(_, id)| id.clone()).collect()
    }

    /// Those past the `ttl`: `age + ttl ≤ now`.
    pub fn expired(&self, now_ms: u64, ttl_ms: u64) -> Vec<String> {
        let Some(limit) = now_ms.checked_sub(ttl_ms) else {
            return Vec::new();
        };
        self.by_age
            .iter()
            .take_while(|(age, _)| *age <= limit)
            .map(|(_, id)| id.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests;
