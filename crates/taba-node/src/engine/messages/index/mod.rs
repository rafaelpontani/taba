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
//! `ttl` and the head's dead-letter release the group for the next one, in `id` order. The FIFO
//! list is never bounded by the middle rule: a returning message enters by the position of its ID.
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
//!
//! **Bounded lists** (doc 12, section 13.1; doc 14). The **ready list** holds at most
//! `delivery.ready-list.size` IDs (a *window*): two cursors walk the node's messages by GUID v7,
//! one from the oldest forward and one from the newest backward, and the window is refilled
//! alternating the two ends (accordion) only when room opens. Every ready ID that is not in the
//! window lies in the gap between the cursors. A message that becomes ready enters the window
//! straight away if there is room and nothing waits in the gap, or if it falls on the
//! already-walked side of a cursor; with the window full, the ID at the **middle** (the border of
//! a walked side, next to the gap) leaves to make room and is found again by the cursor later: no
//! message is lost. The counters (ready, in processing, per group) are exact and do not depend on
//! the window.
//!
//! The **invisible list** holds at most `delivery.invisible-list.size` pairs (deadline, ID), ordered
//! by deadline. The pairs that do not fit go to the `expiry` table: the index only queues
//! [`SpillOp`]s, and whoever owns the I/O applies them and answers
//! [`TopicIndex::reload_wanted`] with [`TopicIndex::reload`]. Invariant: everything in the list
//! expires before everything in the table.

use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::ops::Bound;

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
    /// `delivery.ready-list.size`: IDs in the ready window (at least 1).
    pub ready_cap: usize,
    /// `delivery.invisible-list.size`: pairs in the invisible list (at least 1).
    pub invisible_cap: usize,
    /// `delivery.invisible.reload-threshold`: below it (or empty) the list is reloaded from the table.
    pub reload_threshold: usize,
}

impl Default for IndexRules {
    /// Fair delivery with weight 0.1 and no ceilings: both lists are complete.
    fn default() -> Self {
        Self {
            ready_weight: 0.1,
            max_groups: None,
            normal_every: 0,
            fifo: false,
            ready_cap: usize::MAX,
            invisible_cap: usize::MAX,
            reload_threshold: 0,
        }
    }
}

/// What the invisible list asks of the `expiry` table: the index does no I/O, it only queues these.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpillOp {
    /// Writes the pair (it did not fit in the list).
    Put(u64, String),
    /// Deletes the pair (ack, change, new delivery or reload).
    Delete(u64, String),
}

/// A group's counters. `ready` holds the group's ready IDs: all of them on FIFO, only the ones in
/// the window on the fair queue; `ready_n` is always the exact count.
#[derive(Default)]
struct Group {
    ready: BTreeSet<String>,
    ready_n: u64,
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
    entries: BTreeMap<String, Entry>,
    groups: HashMap<GroupKey, Group>,
    /// The groups with some ready message, by band.
    bands: BTreeSet<(u32, GroupKey)>,
    /// Ready messages, exact (the window may hold fewer).
    ready_total: usize,
    /// The ready window (fair queue): at most `ready_cap` IDs, all of them ready.
    window: BTreeSet<String>,
    /// Every ready ID up to here is in the window (the oldest cursor).
    cursor_old: Option<String>,
    /// Every ready ID from here on is in the window (the newest cursor).
    cursor_new: Option<String>,
    refill_old_next: bool,
    evict_old_next: bool,
    /// Messages visited to fill the window or to find the end of a group (for the tests).
    scan_steps: Cell<u64>,
    /// Messages in processing, exact (the list holds the earliest ones).
    invisible_total: usize,
    /// The invisible list, by deadline: at most `invisible_cap` pairs.
    invisible: BTreeSet<(u64, String)>,
    /// Table operations waiting for whoever does the I/O.
    spill_ops: Vec<SpillOp>,
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
    pub fn new(mut rules: IndexRules) -> Self {
        rules.ready_cap = rules.ready_cap.max(1);
        rules.invisible_cap = rules.invisible_cap.max(1);
        rules.reload_threshold = rules.reload_threshold.min(rules.invisible_cap);
        Self {
            rules,
            entries: BTreeMap::new(),
            groups: HashMap::new(),
            bands: BTreeSet::new(),
            ready_total: 0,
            window: BTreeSet::new(),
            cursor_old: None,
            cursor_new: None,
            refill_old_next: true,
            evict_old_next: true,
            scan_steps: Cell::new(0),
            invisible_total: 0,
            invisible: BTreeSet::new(),
            spill_ops: Vec::new(),
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
        (self.ready_total, self.invisible_total)
    }

    #[cfg(test)]
    /// Ready and in processing for a group.
    pub fn group_counts(&self, key: &GroupKey) -> Option<(usize, u64)> {
        self.groups
            .get(key)
            .map(|g| (g.ready_n as usize, g.processing))
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
        band(group.processing as f64 + self.rules.ready_weight * group.ready_n as f64)
    }

    /// Applies `change` to the group keeping its position in the band table and deleting the group
    /// that was left with nothing.
    fn mutate(&mut self, key: &GroupKey, change: impl FnOnce(&mut Group)) {
        let mut group = self.groups.remove(key).unwrap_or_default();
        if group.ready_n > 0 {
            self.bands.remove(&(self.band_of(&group), key.clone()));
        }
        change(&mut group);
        if group.ready_n > 0 {
            self.bands.insert((self.band_of(&group), key.clone()));
        }
        if self.rules.fifo {
            self.refresh_head(key, &group);
        }
        if !(group.ready_n == 0 && group.processing == 0) {
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

    // ---- The ready window ----

    /// Ready IDs outside the window (they wait in the gap between the cursors).
    fn gap_ready(&self) -> usize {
        self.ready_total - self.window.len()
    }

    fn in_walked_side(&self, id: &str) -> bool {
        self.cursor_old.as_deref().is_some_and(|c| id <= c)
            || self.cursor_new.as_deref().is_some_and(|c| id >= c)
    }

    /// Puts the ID in the window (and in its group's window subset). The caller counts it as ready.
    fn window_put(&mut self, id: &str) {
        self.window.insert(id.to_owned());
        if let Some(key) = self.entries.get(id).map(|e| e.group.clone()) {
            self.mutate(&key, |g| {
                g.ready.insert(id.to_owned());
            });
        }
    }

    /// Takes the ID out of the window only (it stays ready, in the gap).
    fn window_drop(&mut self, id: &str) {
        if self.window.remove(id)
            && let Some(key) = self.entries.get(id).map(|e| e.group.clone())
        {
            self.mutate(&key, |g| {
                g.ready.remove(id);
            });
        }
    }

    /// Makes room for `incoming`: the ID at the middle (the border of a walked side, next to the
    /// gap) leaves the window and its cursor steps back, so it falls in the gap again. The oldest ID
    /// never leaves for a newer one: the window always holds the oldest ready message. `false` if
    /// nothing could leave.
    fn evict_middle(&mut self, incoming: &str) -> bool {
        let first = self.window.first().cloned();
        let keeps_oldest =
            |victim: &String| !(first.as_ref() == Some(victim) && incoming > victim.as_str());
        let old_side = self
            .cursor_old
            .clone()
            .and_then(|c| {
                self.window
                    .range::<str, _>((Bound::Unbounded, Bound::Included(c.as_str())))
                    .next_back()
                    .cloned()
            })
            .filter(keeps_oldest);
        let new_side = self
            .cursor_new
            .clone()
            .and_then(|c| {
                self.window
                    .range::<str, _>((Bound::Included(c.as_str()), Bound::Unbounded))
                    .next()
                    .cloned()
            })
            .filter(keeps_oldest);
        let use_old = match (&old_side, &new_side) {
            (Some(_), Some(_)) => self.evict_old_next,
            (Some(_), None) => true,
            (None, Some(_)) => false,
            (None, None) => {
                // No walked side can give one: the largest ID goes (it is in the gap's reach).
                let Some(last) = self.window.last().cloned().filter(keeps_oldest) else {
                    return false;
                };
                self.window_drop(&last);
                self.unclaim(&last);
                return true;
            }
        };
        self.evict_old_next = !use_old;
        let victim = if use_old { old_side } else { new_side }.expect("checked");
        self.window_drop(&victim);
        self.unclaim(&victim);
        true
    }

    /// The ID is ready and outside the window: no cursor may claim it as walked. Each cursor steps
    /// back to the nearest window ID on its side (or to nothing).
    fn unclaim(&mut self, id: &str) {
        if self.cursor_old.as_deref().is_some_and(|c| id <= c) {
            self.cursor_old = self
                .window
                .range::<str, _>((Bound::Unbounded, Bound::Excluded(id)))
                .next_back()
                .cloned();
        }
        if self.cursor_new.as_deref().is_some_and(|c| id >= c) {
            self.cursor_new = self
                .window
                .range::<str, _>((Bound::Excluded(id), Bound::Unbounded))
                .next()
                .cloned();
        }
    }

    /// Decides whether a message that just became ready enters the window now, and makes the room.
    /// Returns whether it is in the window. `ready_total` does not count it yet.
    fn window_admit(&mut self, id: &str) -> bool {
        let gap_before = self.gap_ready();
        if gap_before == 0
            && self.window.len() >= self.rules.ready_cap
            && self.cursor_old.is_none()
            && self.cursor_new.is_none()
        {
            // The window is complete and full: from here the oldest cursor has walked all of it.
            self.cursor_old = self.window.last().cloned();
        }
        // An ID older than the whole window is the new oldest: it must be in it.
        let must = self.in_walked_side(id) || self.window.first().is_some_and(|f| id < f.as_str());
        let room = self.window.len() < self.rules.ready_cap;
        if !must && !(room && gap_before == 0) {
            return false;
        }
        if !room && !self.evict_middle(id) {
            self.unclaim(id);
            return false;
        }
        self.window.insert(id.to_owned());
        true
    }

    /// The first ready ID outside the window in the gap, walking from the oldest end (or the newest).
    fn scan_gap(&mut self, from_old: bool) -> Option<String> {
        let lower = self
            .cursor_old
            .clone()
            .map_or(Bound::Unbounded, Bound::Excluded);
        let upper = self
            .cursor_new
            .clone()
            .map_or(Bound::Unbounded, Bound::Excluded);
        if let (Bound::Excluded(a), Bound::Excluded(b)) = (&lower, &upper)
            && a >= b
        {
            return None;
        }
        let range = self.entries.range::<String, _>((lower, upper));
        let mut steps = 0;
        let mut found = None;
        if from_old {
            for (id, entry) in range {
                steps += 1;
                if !entry.processing && !self.window.contains(id) {
                    found = Some(id.clone());
                    break;
                }
            }
        } else {
            for (id, entry) in range.rev() {
                steps += 1;
                if !entry.processing && !self.window.contains(id) {
                    found = Some(id.clone());
                    break;
                }
            }
        }
        self.scan_steps.set(self.scan_steps.get() + steps);
        found
    }

    /// Fills the window up to `delivery.ready-list.size` from the two cursors, alternating the
    /// oldest and the newest end, and returns how many IDs came in. It does nothing, and scans
    /// nothing, while the window is full or the gap is empty: whoever frees room (a delivery that
    /// commits, an ID that leaves) calls it, and only the room that opened is refilled.
    pub fn refill(&mut self) -> usize {
        if self.rules.fifo {
            return 0;
        }
        let mut added = 0;
        while self.window.len() < self.rules.ready_cap && self.gap_ready() > 0 {
            // The window must hold the oldest ready message: with nothing on the walked old side, that
            // end goes first, whatever the alternation says.
            let old_side_empty = self
                .cursor_old
                .as_deref()
                .is_none_or(|c| self.window.first().is_none_or(|first| first.as_str() > c));
            let from_old = self.refill_old_next || old_side_empty;
            let found = match self.scan_gap(from_old) {
                Some(id) => Some((id, from_old)),
                None => self.scan_gap(!from_old).map(|id| (id, !from_old)),
            };
            let Some((id, old)) = found else {
                break;
            };
            self.refill_old_next = !old;
            if old {
                self.cursor_old = Some(id.clone());
            } else {
                self.cursor_new = Some(id.clone());
            }
            self.window_put(&id);
            added += 1;
        }
        added
    }

    /// Empties the window and fills it again from the two ends: after the index is rebuilt, the
    /// window starts with the oldest and the newest instead of only the first ones that came in.
    pub fn reset_window(&mut self) {
        if self.rules.fifo {
            return;
        }
        for id in self.window.iter().cloned().collect::<Vec<_>>() {
            self.window_drop(&id);
        }
        self.cursor_old = None;
        self.cursor_new = None;
        self.refill_old_next = true;
        self.refill();
    }

    /// IDs the window holds right now (for the tests and the statistics).
    #[cfg(test)]
    pub fn window_len(&self) -> usize {
        self.window.len()
    }

    /// Messages visited by the scans so far (for the tests).
    #[cfg(test)]
    pub fn scan_steps(&self) -> u64 {
        self.scan_steps.get()
    }

    // ---- Ready and processing ----

    fn make_ready(&mut self, id: &str, key: &GroupKey) {
        let in_window = if self.rules.fifo {
            true
        } else {
            self.window_admit(id)
        };
        self.ready_total += 1;
        self.mutate(key, |g| {
            g.ready_n += 1;
            if in_window {
                g.ready.insert(id.to_owned());
            }
        });
    }

    /// The message leaves ready (it goes to processing or is deleted).
    fn leave_ready(&mut self, id: &str, key: &GroupKey) {
        self.ready_total -= 1;
        self.window.remove(id);
        self.mutate(key, |g| {
            g.ready_n -= 1;
            g.ready.remove(id);
        });
    }

    fn make_processing(&mut self, id: &str, key: &GroupKey, until_ms: u64) {
        self.invisible_add(until_ms, id);
        self.mutate(key, |g| g.processing += 1);
    }

    // ---- The invisible list ----

    /// Pairs that did not fit in the list and live in the `expiry` table.
    fn spilled(&self) -> usize {
        self.invisible_total - self.invisible.len()
    }

    /// Rule of inclusion. With room and nothing in the table, the pair enters the list. Otherwise,
    /// if its deadline is earlier than the last one on the list, it enters and the last one goes
    /// to the table; if not, it goes straight to the table. So everything in the list expires
    /// before everything in the table.
    fn invisible_add(&mut self, until_ms: u64, id: &str) {
        let room = self.invisible.len() < self.rules.invisible_cap;
        let table_empty = self.spilled() == 0;
        self.invisible_total += 1;
        if room && table_empty {
            self.invisible.insert((until_ms, id.to_owned()));
            return;
        }
        let earlier = self
            .invisible
            .last()
            .is_some_and(|(last, _)| until_ms < *last);
        if !earlier {
            self.spill_ops.push(SpillOp::Put(until_ms, id.to_owned()));
            return;
        }
        self.invisible.insert((until_ms, id.to_owned()));
        if self.invisible.len() > self.rules.invisible_cap
            && let Some((last, last_id)) = self.invisible.pop_last()
        {
            self.spill_ops.push(SpillOp::Put(last, last_id));
        }
    }

    /// Takes the pair out wherever it is: the list, or the table (a delete is queued).
    fn invisible_remove(&mut self, until_ms: u64, id: &str) {
        self.invisible_total -= 1;
        if !self.invisible.remove(&(until_ms, id.to_owned())) {
            self.spill_ops
                .push(SpillOp::Delete(until_ms, id.to_owned()));
        }
    }

    /// The table operations queued since the last call, in order.
    pub fn take_spill_ops(&mut self) -> Vec<SpillOp> {
        std::mem::take(&mut self.spill_ops)
    }

    /// Puts back, at the front, operations that could not be applied.
    pub fn restore_spill_ops(&mut self, mut ops: Vec<SpillOp>) {
        ops.append(&mut self.spill_ops);
        self.spill_ops = ops;
    }

    /// How many pairs to read from the table (the earliest ones), `0` for none: the list is
    /// empty or below `delivery.invisible.reload-threshold` and the table has pairs.
    pub fn reload_wanted(&self) -> usize {
        let spilled = self.spilled();
        let len = self.invisible.len();
        if spilled == 0 || (len != 0 && len >= self.rules.reload_threshold) {
            return 0;
        }
        (self.rules.invisible_cap - len).min(spilled)
    }

    /// Moves to the list the pairs read from the table (the earliest by deadline), checking each one
    /// against what the index knows: the message still exists, still in processing, and the
    /// deadline is still the pair's. A pair that fails the check is dropped. Every pair read is
    /// deleted from the table. Returns how many entered the list.
    pub fn reload(&mut self, rows: Vec<(u64, String)>) -> usize {
        let mut loaded = 0;
        for (until, id) in rows {
            self.spill_ops.push(SpillOp::Delete(until, id.clone()));
            let valid = self
                .entries
                .get(&id)
                .is_some_and(|e| e.processing && e.until_ms == until);
            if valid && self.invisible.len() < self.rules.invisible_cap {
                self.invisible.insert((until, id));
                loaded += 1;
            } else if valid {
                // No room (should not happen): it goes back to the table as it was.
                self.spill_ops.push(SpillOp::Put(until, id));
            }
        }
        loaded
    }

    /// Pairs in the invisible list right now (for the tests and the statistics).
    #[cfg(test)]
    pub fn invisible_list_len(&self) -> usize {
        self.invisible.len()
    }

    /// The invisible list's pairs, by deadline (for the tests).
    #[cfg(test)]
    pub fn invisible_pairs(&self) -> Vec<(u64, String)> {
        self.invisible.iter().cloned().collect()
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
            self.invisible_remove(entry.until_ms, id);
            self.mutate(&entry.group, |g| g.processing -= 1);
        } else {
            self.leave_ready(id, &entry.group);
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
            self.invisible_remove(old_until, id);
        }
        // The entry already says what the message is when it becomes ready again.
        if let Some(entry) = self.entries.get_mut(id) {
            entry.processing = now_processing;
            entry.until_ms = if now_processing { until_ms } else { 0 };
        }
        match (was_processing, now_processing) {
            (false, true) => {
                self.leave_ready(id, &key);
                self.make_processing(id, &key, until_ms);
            }
            (true, true) => {
                self.invisible_add(until_ms, id);
            }
            (true, false) => {
                self.mutate(&key, |g| g.processing -= 1);
                self.make_ready(id, &key);
            }
            (false, false) => {}
        }
    }

    /// Records the delivery: one more in the counter and the new end of invisibility.
    pub fn deliver(&mut self, id: &str, until_ms: u64, now_ms: u64, deliveries: u32) {
        if let Some(entry) = self.entries.get_mut(id) {
            entry.deliveries = deliveries;
        }
        self.set_until(id, until_ms, now_ms);
    }

    /// Returns to ready the messages whose invisibility expired, and which ones they were. The head
    /// of the list is taken out and checked against what the index knows (the message still
    /// exists and the deadline is still the pair's); a pair that fails the check is dropped and the
    /// walk goes on. Nothing is propagated: it is only the index following the metadata. It stops
    /// when the list is out of due pairs; if it emptied with pairs still in the table,
    /// [`TopicIndex::reload_wanted`] says so and the caller reloads and calls again.
    pub fn release_due(&mut self, now_ms: u64) -> Vec<String> {
        let mut released = Vec::new();
        while let Some((until, id)) = self.invisible.first().cloned() {
            if until > now_ms {
                break;
            }
            self.invisible.pop_first();
            let valid = self
                .entries
                .get(&id)
                .is_some_and(|e| e.processing && e.until_ms == until);
            if !valid {
                // The pair is not the message's any more: drop it; the count follows the message.
                self.invisible_total -= 1;
                continue;
            }
            self.invisible_total -= 1;
            // `set_until` would take the pair out again; do the rest of the return by hand.
            if let Some(entry) = self.entries.get_mut(&id) {
                entry.processing = false;
                entry.until_ms = 0;
            }
            let key = self.entries[&id].group.clone();
            self.mutate(&key, |g| g.processing -= 1);
            self.make_ready(&id, &key);
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

    /// When the next invisible one becomes deliverable again (the head of the list; with the list
    /// empty and pairs in the table, [`TopicIndex::reload_wanted`] asks for the reload first).
    pub fn next_visible(&self) -> Option<u64> {
        self.invisible.first().map(|(until, _)| *until)
    }

    pub fn has_ready(&self) -> bool {
        if self.rules.fifo {
            return !self.free_heads.is_empty();
        }
        self.ready_total > 0
    }

    /// A group's oldest (or newest) ready ID. The window is enough when it holds all of the group's
    /// ready IDs, or when the group has an ID on the walked side of the cursor on that end (nothing
    /// older, or newer, is outside the window). Otherwise the messages are scanned.
    fn group_end(&self, key: &GroupKey, group: &Group, oldest: bool) -> Option<String> {
        let in_window = if oldest {
            group.ready.first()
        } else {
            group.ready.last()
        };
        let sure = group.ready.len() as u64 == group.ready_n
            || in_window.is_some_and(|id| {
                if oldest {
                    self.cursor_old.as_deref().is_some_and(|c| id.as_str() <= c)
                } else {
                    self.cursor_new.as_deref().is_some_and(|c| id.as_str() >= c)
                }
            });
        if sure {
            return in_window.cloned();
        }
        let mut steps = 0;
        let mut found = None;
        let mut visit = |id: &String, entry: &Entry| {
            steps += 1;
            if !entry.processing && &entry.group == key {
                found = Some(id.clone());
                return true;
            }
            false
        };
        if oldest {
            for (id, entry) in &self.entries {
                if visit(id, entry) {
                    break;
                }
            }
        } else {
            for (id, entry) in self.entries.iter().rev() {
                if visit(id, entry) {
                    break;
                }
            }
        }
        self.scan_steps.set(self.scan_steps.get() + steps);
        found
    }

    /// The next ID to deliver (without taking it off the ready list; whoever delivers calls
    /// [`TopicIndex::deliver`]). `None` if there is no ready one.
    pub fn pick(&mut self) -> Option<String> {
        if self.rules.fifo {
            return self.free_heads.first().cloned();
        }
        // Room that opened since the last delivery is refilled now; a full window costs nothing.
        if self.window.len() < self.rules.ready_cap && self.gap_ready() > 0 {
            self.refill();
        }
        let every = u64::from(self.rules.normal_every);
        let normal = every > 0 && (self.picks + 1).is_multiple_of(every);
        self.picks += 1;
        if normal {
            return self.window.first().cloned();
        }
        let lowest = self.bands.first()?.0;
        let newest = self.newest_next;
        self.newest_next = !self.newest_next;
        let candidates = self
            .bands
            .range((lowest, GroupKey::Named(String::new()))..)
            .take_while(|(b, _)| *b == lowest)
            .filter_map(|(_, key)| self.groups.get(key).map(|g| (key, g)))
            .filter_map(|(key, g)| self.group_end(key, g, !newest));
        if newest {
            candidates.max()
        } else {
            candidates.min()
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

    /// The ready IDs the window holds, in `id` order (for the tests).
    #[cfg(test)]
    pub fn ready_ids(&self) -> Vec<String> {
        self.window.iter().cloned().collect()
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
        self.ready_total
    }

    pub fn processing_count(&self) -> usize {
        self.invisible_total
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
            totals.largest = totals.largest.max(group.ready_n + group.processing);
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
                ready: group.ready_n,
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
mod bounded_tests;
#[cfg(test)]
mod tests;
