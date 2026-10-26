//! What the engine reads from the messages for the index. The index is pure (no I/O) and keeps the
//! entry of a message only while it is in the ready window or in the invisible list; for the rest it
//! only counts. Whoever needs the rest reads it here, from the metadata (the topic's memory and its
//! database), and hands it to the index before calling it: the index stays free of I/O and of
//! errors, and a failed read never leaves it half changed.
//!
//! - [`Messages::know`] makes the index keep a message it counts (before an ack, a removal, a new
//!   deadline).
//! - [`Messages::refill_window`] reads the ready messages of the gap for the window.
//! - [`Messages::pick_next`] chooses the next message, reading the end of a group when the window
//!   cannot tell it.
//! - The `ttl`, the delivery limit and the on-demand statistics read the messages in pages.

use super::index::{Fact, GroupKey, Pick, RefillAsk, Resolved};
use super::{Messages, TopicState};
use crate::storage::messages::{MessageMeta, PageQuery, ScanRow};
use std::collections::{BTreeSet, HashMap};
use std::ops::Bound;

/// Messages read from each source per round trip.
const PAGE: usize = 64;

/// The most messages the `ttl` removes in one pass (the rest goes in the next).
const EXPIRE_BATCH: usize = 10_000;

/// What the index needs of a message.
pub(super) fn fact_of(meta: &MessageMeta) -> Fact {
    Fact {
        id: meta.id.clone(),
        group: meta.group_id.clone(),
        age_ms: meta.age_base_ms(),
        deliveries: meta.delivery_count,
    }
}

/// A message read from memory or from the database.
struct Row {
    fact: Fact,
    in_memory: bool,
}

impl Row {
    fn from_disk(row: ScanRow) -> Self {
        Self {
            fact: Fact {
                id: row.id,
                group: row.group_id,
                age_ms: row.age_ms,
                deliveries: row.delivery_count,
            },
            in_memory: false,
        }
    }
}

/// Which messages [`Messages::page`] brings, from after `cursor` toward `far`.
#[derive(Default, Clone, Copy)]
struct Walk<'a> {
    cursor: Option<&'a str>,
    far: Option<&'a str>,
    desc: bool,
    group: Option<&'a str>,
    delivered: bool,
    /// Leaves out the disk rows that have a pair in the `expiry` table (they are in processing).
    off_table: bool,
}

impl Messages {
    /// The metadata of a live message, wherever it is.
    pub(super) fn live_meta(
        &self,
        name: &str,
        state: &TopicState,
        id: &str,
    ) -> Option<MessageMeta> {
        match state.memory.get(id) {
            Some(stored) => Some(stored.meta.clone()),
            None => self.store.get_live(name, id).ok().flatten(),
        }
    }

    /// The pair the message has in the `expiry` table: it is in processing. The queued writes go
    /// first, so the table is current.
    fn table_deadline(&self, name: &str, state: &mut TopicState, id: &str) -> Option<u64> {
        self.expiry_changed(name, state);
        self.store.expiry_find(name, id).ok().flatten()
    }

    /// The node owns the live message and the index keeps its entry (reading it, if it did not
    /// keep it). Call it before any operation on a message the index may only be counting.
    pub(super) fn know(&self, name: &str, state: &mut TopicState, id: &str) -> bool {
        if state.index.contains(id) {
            return true;
        }
        if state.replicas.contains(id) {
            return false;
        }
        let Some(meta) = self.live_meta(name, state, id) else {
            return false;
        };
        let until = self.table_deadline(name, state, id);
        state.index.adopt(fact_of(&meta), until);
        true
    }

    /// The message is ready for the index (not in processing, in the list or in the table).
    fn is_ready(&self, name: &str, state: &mut TopicState, row: &Row) -> bool {
        if state.replicas.contains(&row.fact.id) || state.index.is_processing(&row.fact.id) {
            return false;
        }
        // A disk row read with `off_table` already left the table's pairs out.
        !row.in_memory || self.table_deadline(name, state, &row.fact.id).is_none()
    }

    /// A page of the live messages in `id` order (memory and disk merged), from after `walk.cursor`
    /// toward `walk.far`. The second value is where to continue, `None` when there is nothing more.
    fn page(&self, name: &str, state: &TopicState, walk: Walk<'_>) -> (Vec<Row>, Option<String>) {
        let query = PageQuery {
            after: if walk.desc { walk.far } else { walk.cursor }.map(str::to_owned),
            before: if walk.desc { walk.cursor } else { walk.far }.map(str::to_owned),
            desc: walk.desc,
            limit: PAGE,
            delivered: walk.delivered,
            off_table: walk.off_table,
            group: walk.group.map(str::to_owned),
            tombstones: false,
        };
        let disk = match self.store.page(name, &query) {
            Ok(rows) => rows,
            Err(e) => {
                self.disk_error("failed to read the messages on disk", &e);
                Vec::new()
            }
        };
        let (low, high) = if walk.desc {
            (walk.far, walk.cursor)
        } else {
            (walk.cursor, walk.far)
        };
        let mut memory = Vec::new();
        if low.zip(high).is_none_or(|(a, b)| a < b) {
            let range = state.memory.range::<str, _>((
                low.map_or(Bound::Unbounded, Bound::Excluded),
                high.map_or(Bound::Unbounded, Bound::Excluded),
            ));
            let keep = |(_, stored): &(&String, &super::Stored)| {
                (!walk.delivered || stored.meta.delivery_count > 0)
                    && walk
                        .group
                        .is_none_or(|g| stored.meta.group_id.as_deref().unwrap_or_default() == g)
            };
            let rows = |(_, stored): (&String, &super::Stored)| Row {
                fact: fact_of(&stored.meta),
                in_memory: true,
            };
            if walk.desc {
                memory.extend(range.rev().filter(keep).take(PAGE).map(rows));
            } else {
                memory.extend(range.filter(keep).take(PAGE).map(rows));
            }
        }
        // Past the last ID of a full page, the other source may have more: stop there.
        let mut horizon: Option<String> = None;
        let mut limit = |last: Option<&str>| {
            if let Some(last) = last {
                let tighter = horizon
                    .as_deref()
                    .is_none_or(|h| if walk.desc { last > h } else { last < h });
                if tighter {
                    horizon = Some(last.to_owned());
                }
            }
        };
        if disk.len() >= PAGE {
            limit(disk.last().map(|r| r.id.as_str()));
        }
        if memory.len() >= PAGE {
            limit(memory.last().map(|r| r.fact.id.as_str()));
        }
        let mut rows: Vec<Row> = disk.into_iter().map(Row::from_disk).collect();
        rows.extend(memory);
        rows.sort_by(|a, b| {
            a.fact
                .id
                .cmp(&b.fact.id)
                .then(b.in_memory.cmp(&a.in_memory))
        });
        rows.dedup_by(|b, a| a.fact.id == b.fact.id);
        if walk.desc {
            rows.reverse();
        }
        if let Some(h) = &horizon {
            rows.retain(|r| {
                if walk.desc {
                    r.fact.id.as_str() >= h.as_str()
                } else {
                    r.fact.id.as_str() <= h.as_str()
                }
            });
        }
        (rows, horizon)
    }

    // ---- The ready window ----

    /// The ready messages of the gap, from one end: up to `ask.room`.
    fn ready_from(
        &self,
        name: &str,
        state: &mut TopicState,
        ask: &RefillAsk,
        desc: bool,
    ) -> Vec<Fact> {
        let (mut cursor, far) = if desc {
            (ask.before.clone(), ask.after.clone())
        } else {
            (ask.after.clone(), ask.before.clone())
        };
        let mut out = Vec::new();
        loop {
            let walk = Walk {
                cursor: cursor.as_deref(),
                far: far.as_deref(),
                desc,
                off_table: true,
                ..Walk::default()
            };
            let (rows, next) = self.page(name, state, walk);
            for row in rows {
                if state.index.in_window(&row.fact.id) || !self.is_ready(name, state, &row) {
                    continue;
                }
                out.push(row.fact);
                if out.len() >= ask.room {
                    return out;
                }
            }
            match next {
                Some(next) => cursor = Some(next),
                None => return out,
            }
        }
    }

    /// Fills the ready window up to `delivery.ready-list.size`, reading the ready messages of the gap
    /// from both ends. It reads nothing while the window is full.
    pub(super) fn refill_window(&self, name: &str, state: &mut TopicState) {
        if state.index.refill_ask().is_none() {
            return;
        }
        self.expiry_changed(name, state);
        // Each round brings what the cursors left; it stops when a round brings nothing.
        for _ in 0..1024 {
            let Some(ask) = state.index.refill_ask() else {
                return;
            };
            let old = self.ready_from(name, state, &ask, false);
            let new = self.ready_from(name, state, &ask, true);
            if state.index.refill_with(old, new) == 0 {
                return;
            }
        }
    }

    /// The oldest (or newest) ready message of the group, read from the messages.
    fn group_end(
        &self,
        name: &str,
        state: &mut TopicState,
        group: &str,
        oldest: bool,
    ) -> Option<Fact> {
        let mut cursor: Option<String> = None;
        loop {
            let walk = Walk {
                cursor: cursor.as_deref(),
                desc: !oldest,
                group: Some(group),
                off_table: true,
                ..Walk::default()
            };
            let (rows, next) = self.page(name, state, walk);
            for row in rows {
                if self.is_ready(name, state, &row) {
                    return Some(row.fact);
                }
            }
            cursor = Some(next?);
        }
    }

    /// The next message to deliver. Refills the window first (the room a delivery opened), and
    /// reads the end of a group when the window cannot tell it.
    pub(super) fn pick_next(&self, name: &str, state: &mut TopicState) -> Option<String> {
        if state.index.refill_due() {
            self.refill_window(name, state);
        }
        let mut resolved = Resolved::new();
        loop {
            match state.index.pick_with(&resolved) {
                Pick::Id(id) => return Some(id),
                Pick::Empty => return None,
                Pick::Need(need) => {
                    self.expiry_changed(name, state);
                    for (key, oldest) in need {
                        let found = match &key {
                            GroupKey::Named(group) => self.group_end(name, state, group, oldest),
                            GroupKey::Overflow => None,
                        };
                        resolved.insert((key, oldest), found);
                    }
                }
            }
        }
    }

    // ---- Reading the messages in pages ----

    /// Calls `visit` with each live message the node owns (replicas left out), in `id` order,
    /// until it returns `false`.
    fn walk_owned(
        &self,
        name: &str,
        state: &TopicState,
        delivered: bool,
        mut visit: impl FnMut(&Fact) -> bool,
    ) {
        let mut cursor: Option<String> = None;
        loop {
            let walk = Walk {
                cursor: cursor.as_deref(),
                delivered,
                ..Walk::default()
            };
            let (rows, next) = self.page(name, state, walk);
            for row in rows {
                if !state.replicas.contains(&row.fact.id) && !visit(&row.fact) {
                    return;
                }
            }
            match next {
                Some(next) => cursor = Some(next),
                None => return,
            }
        }
    }

    /// The `id`s of the messages the node owns.
    pub(super) fn owned_ids_of(&self, name: &str, state: &TopicState) -> Vec<String> {
        let mut ids = Vec::new();
        self.walk_owned(name, state, false, |fact| {
            ids.push(fact.id.clone());
            true
        });
        ids
    }

    /// Every message of the topic, owned or replica, in memory or on disk (the FIFO queue's short
    /// rows left out).
    pub(super) fn all_ids_of(&self, name: &str, state: &TopicState) -> Vec<String> {
        let mut ids = Vec::new();
        let mut cursor: Option<String> = None;
        loop {
            let walk = Walk {
                cursor: cursor.as_deref(),
                ..Walk::default()
            };
            let (rows, next) = self.page(name, state, walk);
            ids.extend(rows.into_iter().map(|row| row.fact.id));
            match next {
                Some(next) => cursor = Some(next),
                None => return ids,
            }
        }
    }

    /// The owned messages with their facts, in `id` order.
    pub(super) fn owned_facts(&self, name: &str, state: &TopicState) -> Vec<Fact> {
        let mut facts = Vec::new();
        self.walk_owned(name, state, false, |fact| {
            facts.push(fact.clone());
            true
        });
        facts
    }

    /// The age of the oldest message the node owns (what the `ttl` counts from).
    pub(super) fn oldest_owned_age(&self, name: &str, state: &TopicState) -> Option<u64> {
        // The first of the age order that the node owns (the replicas in front of it are skipped).
        let mut oldest = state
            .memory
            .by_age()
            .find(|(_, id)| !state.replicas.contains(id))
            .map(|(age, _)| *age);
        let mut after: Option<(u64, String)> = None;
        'pages: loop {
            let page = match self.store.by_age(name, u64::MAX, after.as_ref(), PAGE) {
                Ok(page) => page,
                Err(e) => {
                    self.disk_error("failed to read the messages on disk", &e);
                    break;
                }
            };
            for (age, id) in &page {
                if !state.replicas.contains(id) && !state.memory.contains_key(id) {
                    oldest = Some(oldest.map_or(*age, |o| o.min(*age)));
                    break 'pages;
                }
            }
            if page.len() < PAGE {
                break;
            }
            after = page.last().cloned();
        }
        oldest
    }

    /// The owned messages whose `ttl` expired (`age + ttl ≤ now`), at most one batch.
    pub(super) fn expired_owned(
        &self,
        name: &str,
        state: &mut TopicState,
        now: u64,
        ttl_ms: u64,
    ) -> Vec<String> {
        let Some(limit) = now.checked_sub(ttl_ms) else {
            return Vec::new();
        };
        let mut ids: BTreeSet<String> = state
            .memory
            .iter()
            .filter(|(id, stored)| {
                stored.meta.age_base_ms() <= limit && !state.replicas.contains(id)
            })
            .map(|(id, _)| id.clone())
            .collect();
        let mut after: Option<(u64, String)> = None;
        while ids.len() < EXPIRE_BATCH {
            let page = match self.store.by_age(name, limit, after.as_ref(), PAGE * 4) {
                Ok(page) => page,
                Err(e) => {
                    self.disk_error("failed to read the messages on disk", &e);
                    break;
                }
            };
            let full = page.len() == PAGE * 4;
            after = page.last().cloned();
            ids.extend(
                page.into_iter()
                    .map(|(_, id)| id)
                    .filter(|id| !state.replicas.contains(id)),
            );
            if !full {
                break;
            }
        }
        ids.into_iter().take(EXPIRE_BATCH).collect()
    }

    /// The owned messages ready and already delivered, with the deliveries: the candidates for the
    /// dead-letter when `maxDeliveries` reaches them.
    pub(super) fn ready_delivered_of(
        &self,
        name: &str,
        state: &mut TopicState,
    ) -> Vec<(String, u32)> {
        self.expiry_changed(name, state);
        let mut found = Vec::new();
        let mut cursor: Option<String> = None;
        loop {
            let walk = Walk {
                cursor: cursor.as_deref(),
                delivered: true,
                off_table: true,
                ..Walk::default()
            };
            let (rows, next) = self.page(name, state, walk);
            for row in rows {
                if self.is_ready(name, state, &row) {
                    found.push((row.fact.id, row.fact.deliveries));
                }
            }
            match next {
                Some(next) => cursor = Some(next),
                None => return found,
            }
        }
    }

    /// The group of the owned message delivered most times, above `over` (the `group-stalled` condition).
    pub(super) fn stalled_group_of(
        &self,
        name: &str,
        state: &TopicState,
        over: u32,
    ) -> Option<String> {
        let mut worst: Option<(u32, Option<String>)> = None;
        self.walk_owned(name, state, true, |fact| {
            if fact.deliveries > over && worst.as_ref().is_none_or(|(n, _)| fact.deliveries > *n) {
                worst = Some((fact.deliveries, fact.group.clone()));
            }
            true
        });
        worst.map(
            |(_, group)| match state.index.counted_key(group.as_deref()) {
                GroupKey::Named(name) => name,
                GroupKey::Overflow => String::new(),
            },
        )
    }

    /// The age of each group's oldest message.
    pub(super) fn oldest_by_group(&self, name: &str, state: &TopicState) -> HashMap<GroupKey, u64> {
        let mut oldest: HashMap<GroupKey, u64> = HashMap::new();
        self.walk_owned(name, state, false, |fact| {
            let key = state.index.counted_key(fact.group.as_deref());
            oldest
                .entry(key)
                .and_modify(|age| *age = (*age).min(fact.age_ms))
                .or_insert(fact.age_ms);
            true
        });
        oldest
    }

    // ---- The invisible list ----

    /// If the invisible list is empty or below the reload threshold and the table has pairs, moves
    /// the earliest ones to the list (and deletes the rows read), checking each against the
    /// metadata. `true` if some pair entered.
    pub(super) fn reload_expiry(&self, name: &str, state: &mut TopicState) -> bool {
        let wanted = state.index.reload_wanted();
        if wanted == 0 {
            return false;
        }
        if let Err(e) = self.flush_expiry(name, state) {
            self.disk_error("failed to write the expiry table", &e);
            return false;
        }
        let rows = match self.store.expiry_earliest(name, wanted) {
            Ok(rows) => rows,
            Err(e) => {
                self.disk_error("failed to read the expiry table", &e);
                return false;
            }
        };
        // The message still exists and is the node's; otherwise the pair is a ghost and goes away.
        let rows = rows
            .into_iter()
            .map(|(until, id)| {
                let fact = (!state.replicas.contains(&id))
                    .then(|| self.live_meta(name, state, &id))
                    .flatten()
                    .map(|meta| fact_of(&meta));
                (until, id, fact)
            })
            .collect();
        let loaded = state.index.reload(rows);
        if let Err(e) = self.flush_expiry(name, state) {
            self.disk_error("failed to write the expiry table", &e);
        }
        loaded > 0
    }
}
