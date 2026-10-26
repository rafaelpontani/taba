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
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
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

/// What the index needs to know about a message it does not keep: the caller reads it from the
/// metadata (memory or the topic's database) and hands it over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fact {
    pub id: String,
    /// The raw `groupId` (the index maps it to its group key).
    pub group: Option<String>,
    pub age_ms: u64,
    pub deliveries: u32,
}

/// What the index asks of whoever reads the messages to refill the ready window: up to `room`
/// ready messages after `after` (ascending `id`) and up to `room` before `before` (descending).
/// Both bounds are exclusive; `None` is no bound.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefillAsk {
    pub room: usize,
    pub after: Option<String>,
    pub before: Option<String>,
}

/// The oldest (`true`) or the newest (`false`) ready message of a group, as the reader found it.
pub type Resolved = HashMap<(GroupKey, bool), Option<Fact>>;

/// The answer of [`TopicIndex::pick_with`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pick {
    /// The next ID to deliver.
    Id(String),
    /// Nothing ready.
    Empty,
    /// The window alone cannot tell the end of these groups: read them and ask again.
    Need(Vec<(GroupKey, bool)>),
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
    /// What the index keeps of a message: all of them on a FIFO queue, and on the fair queue only
    /// the ones in the window or in the invisible list (the others are counted and read when needed).
    entries: BTreeMap<String, Entry>,
    /// Messages the index counts, ready and in processing (`entries` may hold fewer).
    total: usize,
    /// Entries handed over by [`TopicIndex::adopt`] that may not be worth keeping once the caller is done.
    adopted: Vec<String>,
    /// A lower bound of the age of every message: the `ttl` check is only worth a read after it.
    age_floor: u64,
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
            total: 0,
            adopted: Vec::new(),
            age_floor: u64::MAX,
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
            picks: 0,
            newest_next: false,
            free_heads: BTreeSet::new(),
            head_of: HashMap::new(),
        }
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.total
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.total == 0
    }

    /// The index keeps the message's entry (always true for the window, the invisible list and a
    /// FIFO queue). A message it counts but does not keep is read from the metadata and handed
    /// over with [`TopicIndex::adopt`] before anything is done to it.
    pub fn contains(&self, id: &str) -> bool {
        self.entries.contains_key(id)
    }

    /// Entries kept right now (for the tests: the memory the index spends per message).
    #[cfg(test)]
    pub fn kept(&self) -> usize {
        self.entries.len()
    }

    /// The entry stays only if the message is in the window or in the invisible list; a FIFO queue
    /// keeps all of them, and so does a message counted in the shared overflow group (the group
    /// it was counted under cannot be told again from the metadata).
    fn keeps(&self, id: &str, entry: &Entry) -> bool {
        self.rules.fifo
            || entry.group == GroupKey::Overflow
            || self.window.contains(id)
            || (entry.processing && self.invisible.contains(&(entry.until_ms, id.to_owned())))
    }

    /// Drops the message's entry if the index has no reason to keep it.
    fn shrink(&mut self, id: &str) {
        if self
            .entries
            .get(id)
            .is_some_and(|entry| !self.keeps(id, entry))
        {
            self.entries.remove(id);
        }
    }

    /// Drops the entries [`TopicIndex::adopt`] brought that the index has no reason to keep (the
    /// caller is done with them).
    pub fn forget_adopted(&mut self) {
        for id in std::mem::take(&mut self.adopted) {
            self.shrink(&id);
        }
    }

    /// Hands the index a message it counts but does not keep. `until_ms` is `Some` when the message is
    /// in processing (its pair is in the `expiry` table). It changes no counter. `false` if the
    /// index already keeps it.
    pub fn adopt(&mut self, fact: Fact, until_ms: Option<u64>) -> bool {
        if self.entries.contains_key(&fact.id) {
            return false;
        }
        let group = self.key_for(fact.group.as_deref());
        if self.adopted.len() >= 4096 {
            self.forget_adopted();
        }
        self.adopted.push(fact.id.clone());
        self.entries.insert(
            fact.id,
            Entry {
                group,
                until_ms: until_ms.unwrap_or(0),
                age_ms: fact.age_ms,
                deliveries: fact.deliveries,
                processing: until_ms.is_some(),
            },
        );
        true
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

    #[cfg(test)]
    pub fn deliveries(&self, id: &str) -> Option<u32> {
        self.entries.get(id).map(|e| e.deliveries)
    }

    /// Is the ID in the ready window?
    pub fn in_window(&self, id: &str) -> bool {
        self.window.contains(id)
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
        if self.window.remove(id) {
            if let Some(key) = self.entries.get(id).map(|e| e.group.clone()) {
                self.mutate(&key, |g| {
                    g.ready.remove(id);
                });
            }
            self.shrink(id);
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

    /// What the window asks to be refilled, `None` while it is full or the gap is empty: whoever
    /// reads the messages brings up to `room` ready ones from each end of the gap (see
    /// [`TopicIndex::refill_with`]). Nothing is read while the window is full: only the room that
    /// opened is refilled.
    pub fn refill_ask(&self) -> Option<RefillAsk> {
        if self.rules.fifo || self.window.len() >= self.rules.ready_cap || self.gap_ready() == 0 {
            return None;
        }
        if let (Some(old), Some(new)) = (&self.cursor_old, &self.cursor_new)
            && old >= new
        {
            return None;
        }
        Some(RefillAsk {
            room: self.rules.ready_cap - self.window.len(),
            after: self.cursor_old.clone(),
            before: self.cursor_new.clone(),
        })
    }

    /// Is a refill worth a read now? Delivering opens room one by one, and a read per delivery is
    /// wasteful: the window waits until a quarter of it is free, unless it has nothing on the
    /// walked old side (then the oldest ready message would be out of it) or is empty.
    pub fn refill_due(&self) -> bool {
        let Some(ask) = self.refill_ask() else {
            return false;
        };
        let old_side_empty = self
            .cursor_old
            .as_deref()
            .is_none_or(|c| self.window.first().is_none_or(|first| first.as_str() > c));
        old_side_empty || self.window.is_empty() || ask.room >= (self.rules.ready_cap / 4).max(1)
    }

    /// Next candidate from one end that still lies in the open gap, is not in the window and is
    /// ready. A candidate past the other cursor ends that list: everything after it is further out.
    fn next_candidate(&mut self, list: &mut VecDeque<Fact>, from_old: bool) -> Option<Fact> {
        while let Some(fact) = list.pop_front() {
            self.scan_steps.set(self.scan_steps.get() + 1);
            let beyond = if from_old {
                self.cursor_new
                    .as_deref()
                    .is_some_and(|c| fact.id.as_str() >= c)
            } else {
                self.cursor_old
                    .as_deref()
                    .is_some_and(|c| fact.id.as_str() <= c)
            };
            if beyond {
                list.clear();
                return None;
            }
            let before_start = if from_old {
                self.cursor_old
                    .as_deref()
                    .is_some_and(|c| fact.id.as_str() <= c)
            } else {
                self.cursor_new
                    .as_deref()
                    .is_some_and(|c| fact.id.as_str() >= c)
            };
            let known = self.entries.get(&fact.id);
            if before_start || self.window.contains(&fact.id) || known.is_some_and(|e| e.processing)
            {
                continue;
            }
            return Some(fact);
        }
        None
    }

    /// Fills the window up to `delivery.ready-list.size` with what was read for [`RefillAsk`],
    /// alternating the oldest and the newest end, and returns how many IDs came in. The caller
    /// asks again while the window has room and this brings something.
    pub fn refill_with(&mut self, old: Vec<Fact>, new: Vec<Fact>) -> usize {
        if self.rules.fifo {
            return 0;
        }
        let (mut old, mut new) = (VecDeque::from(old), VecDeque::from(new));
        let mut added = 0;
        while self.window.len() < self.rules.ready_cap && self.gap_ready() > 0 {
            // The window must hold the oldest ready message: with nothing on the walked old side, that
            // end goes first, whatever the alternation says.
            let old_side_empty = self
                .cursor_old
                .as_deref()
                .is_none_or(|c| self.window.first().is_none_or(|first| first.as_str() > c));
            let from_old = self.refill_old_next || old_side_empty;
            let found =
                match self.next_candidate(if from_old { &mut old } else { &mut new }, from_old) {
                    Some(fact) => Some((fact, from_old)),
                    None => self
                        .next_candidate(if from_old { &mut new } else { &mut old }, !from_old)
                        .map(|fact| (fact, !from_old)),
                };
            let Some((fact, old_end)) = found else {
                break;
            };
            self.refill_old_next = !old_end;
            if old_end {
                self.cursor_old = Some(fact.id.clone());
            } else {
                self.cursor_new = Some(fact.id.clone());
            }
            let id = fact.id.clone();
            self.adopt(fact, None);
            self.window_put(&id);
            added += 1;
        }
        added
    }

    /// Empties the window and forgets the cursors: after the index is rebuilt, the window starts
    /// with the oldest and the newest (the caller refills it) instead of only the first ones that
    /// came in.
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
    }

    /// IDs the window holds right now (for the tests and the statistics).
    #[cfg(test)]
    pub fn window_len(&self) -> usize {
        self.window.len()
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
            self.spill_ops.push(SpillOp::Put(last, last_id.clone()));
            self.shrink(&last_id);
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

    /// Moves to the list the pairs read from the table (the earliest by deadline). The caller
    /// checked each one against the metadata and brings the message's [`Fact`], or `None` when
    /// the message no longer exists (or is not in processing): that pair is dropped. Every pair read
    /// is deleted from the table. Returns how many entered the list.
    pub fn reload(&mut self, rows: Vec<(u64, String, Option<Fact>)>) -> usize {
        let mut loaded = 0;
        for (until, id, fact) in rows {
            self.spill_ops.push(SpillOp::Delete(until, id.clone()));
            let Some(fact) = fact else {
                continue;
            };
            // A message the index keeps already says what its pair is: another deadline is a ghost.
            if self
                .entries
                .get(&id)
                .is_some_and(|e| !e.processing || e.until_ms != until)
            {
                continue;
            }
            if self.invisible.len() < self.rules.invisible_cap {
                self.adopt(fact, Some(until));
                self.invisible.insert((until, id));
                loaded += 1;
            } else {
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
        self.total += 1;
        self.age_floor = self.age_floor.min(age_ms);
        if processing {
            self.make_processing(id, &key, until_ms);
        } else {
            self.make_ready(id, &key);
        }
        self.shrink(id);
    }

    /// Takes the message out of the index (ack, `ttl`, dead-letter). `false` if it was not there.
    pub fn remove(&mut self, id: &str) -> bool {
        let Some(entry) = self.entries.remove(id) else {
            return false;
        };
        self.total -= 1;
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
        self.shrink(id);
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
    #[cfg(test)]
    pub fn release_due(&mut self, now_ms: u64) -> Vec<String> {
        self.release_due_counted(now_ms)
            .into_iter()
            .map(|(id, _)| id)
            .collect()
    }

    /// Like [`TopicIndex::release_due`], with the deliveries each released message already had.
    pub fn release_due_counted(&mut self, now_ms: u64) -> Vec<(String, u32)> {
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
            let (key, deliveries) = {
                let entry = &self.entries[&id];
                (entry.group.clone(), entry.deliveries)
            };
            self.mutate(&key, |g| g.processing -= 1);
            self.make_ready(&id, &key);
            self.shrink(&id);
            released.push((id, deliveries));
        }
        released
    }

    /// Can some message have its `ttl` expired (`age + ttl ≤ now`)? The index only knows a lower
    /// bound of the ages; whoever reads the messages finds the ones that expired and gives the
    /// bound back with [`TopicIndex::set_age_floor`].
    pub fn ttl_due(&self, now_ms: u64, ttl_ms: u64) -> bool {
        now_ms
            .checked_sub(ttl_ms)
            .is_some_and(|limit| self.total > 0 && self.age_floor <= limit)
    }

    /// The age of the oldest message left after a read of the messages (`None`: none).
    pub fn set_age_floor(&mut self, oldest_ms: Option<u64>) {
        self.age_floor = oldest_ms.unwrap_or(u64::MAX);
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
    /// older, or newer, is outside the window). Otherwise the answer is the one the caller read
    /// from the messages, or `Err` if it was not read yet.
    fn group_end(
        &self,
        key: &GroupKey,
        group: &Group,
        oldest: bool,
        resolved: &Resolved,
    ) -> Result<Option<Fact>, ()> {
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
            // The window's own ID: its entry is kept.
            return Ok(in_window.map(|id| Fact {
                id: id.clone(),
                group: None,
                age_ms: 0,
                deliveries: 0,
            }));
        }
        if *key == GroupKey::Overflow {
            // The shared group keeps the entry of every message: the walk needs no read.
            let mut steps = 0;
            let mut visit = |(id, entry): (&String, &Entry)| {
                steps += 1;
                (!entry.processing && entry.group == GroupKey::Overflow).then(|| Fact {
                    id: id.clone(),
                    group: None,
                    age_ms: entry.age_ms,
                    deliveries: entry.deliveries,
                })
            };
            let found = if oldest {
                self.entries.iter().find_map(&mut visit)
            } else {
                self.entries.iter().rev().find_map(&mut visit)
            };
            self.scan_steps.set(self.scan_steps.get() + steps);
            return Ok(found);
        }
        resolved.get(&(key.clone(), oldest)).cloned().ok_or(())
    }

    /// The next ID to deliver (without taking it off the ready list; whoever delivers calls
    /// [`TopicIndex::deliver`]). When the window cannot tell a group's end, the answer is
    /// [`Pick::Need`]: nothing changed, the caller reads those ends and asks again with them.
    pub fn pick_with(&mut self, resolved: &Resolved) -> Pick {
        if self.rules.fifo {
            return self
                .free_heads
                .first()
                .cloned()
                .map_or(Pick::Empty, Pick::Id);
        }
        let every = u64::from(self.rules.normal_every);
        let normal = every > 0 && (self.picks + 1).is_multiple_of(every);
        let chosen = if normal {
            Ok(self.window.first().cloned().map(|id| (id, None)))
        } else {
            self.choose_fair(resolved)
        };
        match chosen {
            Err(need) => Pick::Need(need),
            Ok(chosen) => {
                self.picks += 1;
                if !normal && self.bands.first().is_some() {
                    self.newest_next = !self.newest_next;
                }
                match chosen {
                    None => Pick::Empty,
                    Some((id, fact)) => {
                        // An ID read from the messages becomes known (ready) so it can be delivered.
                        if let Some(fact) = fact {
                            self.adopt(fact, None);
                        }
                        Pick::Id(id)
                    }
                }
            }
        }
    }

    /// The fair choice: from the lowest band, the oldest (or the newest, alternating) of the groups'
    /// ends. `Err` lists the ends that were not read yet.
    #[allow(clippy::type_complexity)]
    fn choose_fair(
        &self,
        resolved: &Resolved,
    ) -> Result<Option<(String, Option<Fact>)>, Vec<(GroupKey, bool)>> {
        let Some(lowest) = self.bands.first().map(|(band, _)| *band) else {
            return Ok(None);
        };
        let newest = self.newest_next;
        let mut need = Vec::new();
        let mut best: Option<(String, Option<Fact>)> = None;
        for (_, key) in self
            .bands
            .range((lowest, GroupKey::Named(String::new()))..)
            .take_while(|(b, _)| *b == lowest)
        {
            let Some(group) = self.groups.get(key) else {
                continue;
            };
            match self.group_end(key, group, !newest, resolved) {
                Err(()) => need.push((key.clone(), !newest)),
                Ok(None) => {}
                Ok(Some(fact)) => {
                    let better = best
                        .as_ref()
                        .is_none_or(|(id, _)| if newest { fact.id > *id } else { fact.id < *id });
                    if better {
                        let kept = self.entries.contains_key(&fact.id);
                        best = Some((fact.id.clone(), (!kept).then_some(fact)));
                    }
                }
            }
        }
        if need.is_empty() { Ok(best) } else { Err(need) }
    }

    /// The next ID to deliver when the index is complete (the tests): the groups' ends all come
    /// from the window.
    #[cfg(test)]
    pub fn pick(&mut self) -> Option<String> {
        match self.pick_with(&Resolved::new()) {
            Pick::Id(id) => Some(id),
            Pick::Empty => None,
            Pick::Need(need) => panic!("the window cannot tell the ends of {need:?}"),
        }
    }

    /// The message is in processing (invisible, already delivered and not acknowledged)?
    pub fn is_processing(&self, id: &str) -> bool {
        self.entries.get(id).is_some_and(|e| e.processing)
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
        self.total
    }

    pub fn ready_count(&self) -> usize {
        self.ready_total
    }

    pub fn processing_count(&self) -> usize {
        self.invisible_total
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

    /// The key a message of the group is counted under, for a message the index already counts:
    /// the group has a counter of its own, or the message is in the shared one.
    pub fn counted_key(&self, group: Option<&str>) -> GroupKey {
        let named = GroupKey::Named(group.unwrap_or_default().to_owned());
        if self.rules.max_groups.is_some() && !self.rules.fifo && !self.groups.contains_key(&named)
        {
            GroupKey::Overflow
        } else {
            named
        }
    }

    /// Each group with its counts and the band; `oldest` is the age of each group's oldest
    /// message, read from the messages by whoever calls (on-demand queries, not the hot path).
    pub fn group_infos(&self, oldest: &HashMap<GroupKey, u64>) -> Vec<GroupInfo> {
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
mod sim;
#[cfg(test)]
mod tests;
