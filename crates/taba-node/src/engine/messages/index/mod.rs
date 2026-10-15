//! A topic's derived index (doc 13, sections 12.6 and 12.11; doc 14, section 13.1): the ready
//! list, the invisible list and the per-group counters. It is **only an index**: the source of
//! truth is the message metadata, in memory or in the topic's database, and the index is rebuilt
//! from it when the topic is opened (restart or new owner).
//!
//! **FIFO queue** (doc 17, section 16.6). A group is *free* when it has no message being processed
//! and has ready ones, and *busy* when it has one in flight. The ready list has **one entry
//! per free group**, the head (the group's oldest), and delivery takes the oldest head
//! among the free groups, with no bands or `normal-every`. Since the delivered one leaves the group busy, the
//! same batch never brings two messages of the same group; the ack, the end of invisibility, the
//! `ttl` and the head's dead-letter release the group for the next one, in `id` order.
//!
//! No I/O and no clock of its own (whoever calls passes `now`), to be tested on its own.
//!
//! **Fair delivery.** Each group has two counts, the **ready** ones and those **in processing**
//! (invisible). The size is `in processing + weight × ready`, and the **band** is
//! `⌊log2(size)⌋` (a size under 1 counts as 1). Delivery takes from the lowest band that has a
//! ready message; within it, it alternates the oldest and the newest (accordion effect). Every
//! `normal-every` deliveries one takes the oldest of all, from any group, so the
//! big group never waits without limit. Since the band is recalculated on each choice, the "ID that changed
//! band" never goes stale in the list.

use std::collections::{BTreeSet, HashMap};

/// A group's key. Groups above `delivery.fair.max-groups` share a single counter.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum GroupKey {
    /// The `groupId`; a message without a group is the group with an empty name.
    Named(String),
    Overflow,
}

#[derive(Debug, Clone, Copy)]
pub struct IndexRules {
    /// `delivery.fair.ready-weight`, from 0 to 1.
    pub ready_weight: f64,
    /// `delivery.fair.max-groups`; `None`, no ceiling.
    pub max_groups: Option<usize>,
    /// `delivery.fair.normal-every`; `0` desliga a garantia.
    pub normal_every: u32,
    /// FIFO queue: one group, one message in flight; fair delivery does not apply.
    pub fifo: bool,
}

#[derive(Default)]
struct Group {
    ready: BTreeSet<String>,
    processing: u64,
}

struct Entry {
    group: GroupKey,
    /// End of invisibility; only applies with `processing`.
    until_ms: u64,
    age_ms: u64,
    deliveries: u32,
    processing: bool,
}

pub struct TopicIndex {
    rules: IndexRules,
    entries: HashMap<String, Entry>,
    groups: HashMap<GroupKey, Group>,
    /// The groups with some ready message, by band.
    bands: BTreeSet<(u32, GroupKey)>,
    ready_all: BTreeSet<String>,
    /// The invisible ones, by deadline.
    invisible: BTreeSet<(u64, String)>,
    /// All, by age (the `ttl` counts from here).
    by_age: BTreeSet<(u64, String)>,
    picks: u64,
    newest_next: bool,
    /// FIFO only: the head of each free group.
    free_heads: BTreeSet<String>,
    head_of: HashMap<GroupKey, String>,
}

/// `⌊log2(size)⌋`, with a size under 1 counted as 1.
pub fn band(size: f64) -> u32 {
    let size = size.max(1.0);
    size.log2().floor() as u32
}

impl TopicIndex {
    pub fn new(rules: IndexRules) -> Self {
        Self {
            rules,
            entries: HashMap::new(),
            groups: HashMap::new(),
            bands: BTreeSet::new(),
            ready_all: BTreeSet::new(),
            invisible: BTreeSet::new(),
            by_age: BTreeSet::new(),
            picks: 0,
            newest_next: false,
            free_heads: BTreeSet::new(),
            head_of: HashMap::new(),
        }
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn contains(&self, id: &str) -> bool {
        self.entries.contains_key(id)
    }

    #[cfg(test)]
    /// Ready and in processing for the whole topic.
    pub fn counts(&self) -> (usize, usize) {
        (self.ready_all.len(), self.invisible.len())
    }

    #[cfg(test)]
    /// Ready and in processing for a group.
    pub fn group_counts(&self, key: &GroupKey) -> Option<(usize, u64)> {
        self.groups.get(key).map(|g| (g.ready.len(), g.processing))
    }

    #[cfg(test)]
    /// The group's current band (`None` if it does not exist).
    pub fn group_band(&self, key: &GroupKey) -> Option<u32> {
        self.groups.get(key).map(|g| self.band_of(g))
    }

    #[cfg(test)]
    pub fn group_count(&self) -> usize {
        self.groups.len()
    }

    /// The key the `groupId` is counted under: new groups above the ceiling go to the shared
    /// counter.
    pub fn key_for(&self, group: Option<&str>) -> GroupKey {
        let named = GroupKey::Named(group.unwrap_or_default().to_owned());
        // On FIFO the group serializes delivery: two groups in a single counter would get in each other's way.
        let Some(max) = self.rules.max_groups.filter(|_| !self.rules.fifo) else {
            return named;
        };
        let tracked =
            self.groups.len() - usize::from(self.groups.contains_key(&GroupKey::Overflow));
        if self.groups.contains_key(&named) || tracked < max {
            named
        } else {
            GroupKey::Overflow
        }
    }

    #[cfg(test)]
    pub fn entry_group(&self, id: &str) -> Option<&GroupKey> {
        self.entries.get(id).map(|e| &e.group)
    }

    /// All the `id`s the index knows.
    pub fn ids(&self) -> Vec<String> {
        self.entries.keys().cloned().collect()
    }

    /// The instant the `ttl` counts from for the message.
    pub fn age_of(&self, id: &str) -> Option<u64> {
        self.entries.get(id).map(|e| e.age_ms)
    }

    pub fn deliveries(&self, id: &str) -> Option<u32> {
        self.entries.get(id).map(|e| e.deliveries)
    }

    fn band_of(&self, group: &Group) -> u32 {
        band(group.processing as f64 + self.rules.ready_weight * group.ready.len() as f64)
    }

    /// Applies `change` to the group keeping its position in the band table and deleting the group
    /// that was left with nothing.
    fn mutate(&mut self, key: &GroupKey, change: impl FnOnce(&mut Group)) {
        let mut group = self.groups.remove(key).unwrap_or_default();
        if !group.ready.is_empty() {
            self.bands.remove(&(self.band_of(&group), key.clone()));
        }
        change(&mut group);
        if !group.ready.is_empty() {
            self.bands.insert((self.band_of(&group), key.clone()));
        }
        if self.rules.fifo {
            self.refresh_head(key, &group);
        }
        if !(group.ready.is_empty() && group.processing == 0) {
            self.groups.insert(key.clone(), group);
        }
    }

    /// Maintains the free-group list: the group's head is only in it if the group has no
    /// message in processing.
    fn refresh_head(&mut self, key: &GroupKey, group: &Group) {
        if let Some(old) = self.head_of.remove(key) {
            self.free_heads.remove(&old);
        }
        if group.processing == 0
            && let Some(head) = group.ready.first()
        {
            self.free_heads.insert(head.clone());
            self.head_of.insert(key.clone(), head.clone());
        }
    }

    fn make_ready(&mut self, id: &str, key: &GroupKey) {
        self.ready_all.insert(id.to_owned());
        self.mutate(key, |g| {
            g.ready.insert(id.to_owned());
        });
    }

    fn make_processing(&mut self, id: &str, key: &GroupKey, until_ms: u64) {
        self.invisible.insert((until_ms, id.to_owned()));
        self.mutate(key, |g| g.processing += 1);
    }

    /// Makes the message known to the index: ready, or in processing if `until_ms` has not
    /// arrived yet. Repeating the same `id` replaces what was there.
    pub fn insert(
        &mut self,
        id: &str,
        group: Option<&str>,
        age_ms: u64,
        until_ms: u64,
        deliveries: u32,
        now_ms: u64,
    ) {
        self.remove(id);
        let key = self.key_for(group);
        let processing = until_ms > now_ms;
        self.entries.insert(
            id.to_owned(),
            Entry {
                group: key.clone(),
                until_ms,
                age_ms,
                deliveries,
                processing,
            },
        );
        self.by_age.insert((age_ms, id.to_owned()));
        if processing {
            self.make_processing(id, &key, until_ms);
        } else {
            self.make_ready(id, &key);
        }
    }

    /// Takes the message out of the index (ack, `ttl`, dead-letter). `false` if it was not there.
    pub fn remove(&mut self, id: &str) -> bool {
        let Some(entry) = self.entries.remove(id) else {
            return false;
        };
        self.by_age.remove(&(entry.age_ms, id.to_owned()));
        if entry.processing {
            self.invisible.remove(&(entry.until_ms, id.to_owned()));
            self.mutate(&entry.group, |g| g.processing -= 1);
        } else {
            self.ready_all.remove(id);
            self.mutate(&entry.group, |g| {
                g.ready.remove(id);
            });
        }
        true
    }

    /// Changes the end of invisibility (delivery, `PUT visibility`). With `until_ms` in the future the
    /// message stays in processing; with `until_ms ≤ now` it goes back to ready.
    pub fn set_until(&mut self, id: &str, until_ms: u64, now_ms: u64) {
        let Some(entry) = self.entries.get(id) else {
            return;
        };
        let (key, was_processing, old_until) =
            (entry.group.clone(), entry.processing, entry.until_ms);
        let now_processing = until_ms > now_ms;
        if was_processing {
            self.invisible.remove(&(old_until, id.to_owned()));
        }
        match (was_processing, now_processing) {
            (false, true) => {
                self.ready_all.remove(id);
                self.mutate(&key, |g| {
                    g.ready.remove(id);
                });
                self.make_processing(id, &key, until_ms);
            }
            (true, true) => {
                self.invisible.insert((until_ms, id.to_owned()));
            }
            (true, false) => {
                self.mutate(&key, |g| g.processing -= 1);
                self.make_ready(id, &key);
            }
            (false, false) => {}
        }
        if let Some(entry) = self.entries.get_mut(id) {
            entry.processing = now_processing;
            entry.until_ms = if now_processing { until_ms } else { 0 };
        }
    }

    /// Records the delivery: one more in the counter and the new end of invisibility.
    pub fn deliver(&mut self, id: &str, until_ms: u64, now_ms: u64, deliveries: u32) {
        if let Some(entry) = self.entries.get_mut(id) {
            entry.deliveries = deliveries;
        }
        self.set_until(id, until_ms, now_ms);
    }

    /// Returns to ready the messages whose invisibility expired, and which ones they were. Nothing is
    /// propagated: it is only the index following the metadata.
    pub fn release_due(&mut self, now_ms: u64) -> Vec<String> {
        let mut released = Vec::new();
        while let Some((until, id)) = self.invisible.first().cloned() {
            if until > now_ms {
                break;
            }
            self.set_until(&id, 0, now_ms);
            released.push(id);
        }
        released
    }

    /// The messages whose `ttl` expired: `age + ttl ≤ now`.
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

    /// When the next invisible one becomes deliverable again.
    pub fn next_visible(&self) -> Option<u64> {
        self.invisible.first().map(|(until, _)| *until)
    }

    pub fn has_ready(&self) -> bool {
        if self.rules.fifo {
            return !self.free_heads.is_empty();
        }
        !self.ready_all.is_empty()
    }

    /// The next ID to deliver (without taking it off the ready list; whoever delivers calls
    /// [`TopicIndex::deliver`]). `None` if there is no ready one.
    pub fn pick(&mut self) -> Option<String> {
        if self.rules.fifo {
            return self.free_heads.first().cloned();
        }
        let every = u64::from(self.rules.normal_every);
        let normal = every > 0 && (self.picks + 1).is_multiple_of(every);
        self.picks += 1;
        if normal {
            return self.ready_all.first().cloned();
        }
        let lowest = self.bands.first()?.0;
        let in_band = self
            .bands
            .range((lowest, GroupKey::Named(String::new()))..)
            .take_while(|(b, _)| *b == lowest)
            .filter_map(|(_, key)| self.groups.get(key));
        let newest = self.newest_next;
        self.newest_next = !self.newest_next;
        if newest {
            in_band.filter_map(|g| g.ready.last()).max().cloned()
        } else {
            in_band.filter_map(|g| g.ready.first()).min().cloned()
        }
    }

    /// The message is in processing (invisible, already delivered and not acknowledged)?
    pub fn is_processing(&self, id: &str) -> bool {
        self.entries.get(id).is_some_and(|e| e.processing)
    }

    /// The ready ones that were already delivered at some point, with the number of deliveries: the candidates for
    /// the dead-letter when `maxDeliveries` reaches them.
    pub fn ready_delivered(&self) -> Vec<(String, u32)> {
        self.entries
            .iter()
            .filter(|(_, e)| !e.processing && e.deliveries > 0)
            .map(|(id, e)| (id.clone(), e.deliveries))
            .collect()
    }

    /// The ready IDs, in `id` order (for the tests).
    #[cfg(test)]
    pub fn ready_ids(&self) -> Vec<String> {
        self.ready_all.iter().cloned().collect()
    }

    // ---- Statistics (doc 11) ----

    /// The index is that of a FIFO queue.
    pub fn is_fifo(&self) -> bool {
        self.rules.fifo
    }

    /// How many messages the index knows, ready and in processing.
    pub fn message_count(&self) -> usize {
        self.entries.len()
    }

    pub fn ready_count(&self) -> usize {
        self.ready_all.len()
    }

    pub fn processing_count(&self) -> usize {
        self.invisible.len()
    }

    /// The instant the oldest not-yet-acknowledged message counts from (the publication, or the
    /// entry into the dead-letter): it is the same age index as the `ttl`, so it scans nothing.
    pub fn oldest_age_ms(&self) -> Option<u64> {
        self.by_age.first().map(|(age, _)| *age)
    }

    /// The group totals: how many there are, how many messages the largest has and how many have a message in
    /// processing.
    pub fn group_totals(&self) -> GroupTotals {
        let mut totals = GroupTotals::default();
        for group in self.groups.values() {
            totals.groups += 1;
            totals.largest = totals
                .largest
                .max(group.ready.len() as u64 + group.processing);
            totals.busy += u64::from(group.processing > 0);
        }
        totals
    }

    /// Each group with its counts, the band and the oldest message (scans the messages
    /// once: it is for on-demand queries, not for the hot path).
    pub fn group_infos(&self) -> Vec<GroupInfo> {
        let mut oldest: HashMap<&GroupKey, u64> = HashMap::new();
        for entry in self.entries.values() {
            oldest
                .entry(&entry.group)
                .and_modify(|age| *age = (*age).min(entry.age_ms))
                .or_insert(entry.age_ms);
        }
        self.groups
            .iter()
            .map(|(key, group)| GroupInfo {
                key: key.clone(),
                ready: group.ready.len() as u64,
                processing: group.processing,
                band: self.band_of(group),
                oldest_age_ms: oldest.get(key).copied(),
            })
            .collect()
    }

    /// The group whose message was already delivered more than `over` times (the one with the most deliveries), for the
    /// `group-stalled` condition. Scans the messages.
    pub fn stalled_group(&self, over: u32) -> Option<String> {
        self.entries
            .values()
            .filter(|entry| entry.deliveries > over)
            .max_by_key(|entry| entry.deliveries)
            .map(|entry| match &entry.group {
                GroupKey::Named(name) => name.clone(),
                GroupKey::Overflow => String::new(),
            })
    }

    /// The group's band for the message, at the present instant.
    pub fn band_of_entry(&self, id: &str) -> Option<u32> {
        let entry = self.entries.get(id)?;
        self.groups.get(&entry.group).map(|g| self.band_of(g))
    }
}

/// What `group_totals` counts.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct GroupTotals {
    pub groups: u64,
    /// Messages of the largest group.
    pub largest: u64,
    /// Groups with a message in processing.
    pub busy: u64,
}

/// A group as `group_infos` sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupInfo {
    pub key: GroupKey,
    pub ready: u64,
    pub processing: u64,
    pub band: u32,
    pub oldest_age_ms: Option<u64>,
}

#[cfg(test)]
mod tests;
