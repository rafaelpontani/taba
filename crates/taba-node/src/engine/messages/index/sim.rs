//! A stand-in for the engine around the index, for the tests: the "database" holds every message,
//! the `expiry` table is a set, and each operation does what the engine does before calling the
//! pure index: it reads what the index only counts (`adopt`), it fills the window from the gap and
//! it resolves the end of a group when the window cannot tell.

use super::*;
use std::collections::{BTreeMap, BTreeSet};
use std::ops::{Deref, DerefMut};

struct Msg {
    group: Option<String>,
    age_ms: u64,
    deliveries: u32,
}

pub struct Sim {
    pub index: TopicIndex,
    db: BTreeMap<String, Msg>,
    table: BTreeSet<(u64, String)>,
    reads: Cell<u64>,
    needs: u64,
}

impl Deref for Sim {
    type Target = TopicIndex;

    fn deref(&self) -> &TopicIndex {
        &self.index
    }
}

impl DerefMut for Sim {
    fn deref_mut(&mut self) -> &mut TopicIndex {
        &mut self.index
    }
}

impl Sim {
    pub fn new(rules: IndexRules) -> Self {
        Self {
            index: TopicIndex::new(rules),
            db: BTreeMap::new(),
            table: BTreeSet::new(),
            reads: Cell::new(0),
            needs: 0,
        }
    }

    /// Messages read from the "database" so far.
    pub fn reads(&self) -> u64 {
        self.reads.get()
    }

    /// How many times the window could not tell the end of a group and it was read.
    pub fn needs(&self) -> u64 {
        self.needs
    }

    /// The pairs the "table" holds.
    pub fn table(&self) -> &BTreeSet<(u64, String)> {
        &self.table
    }

    /// Applies the table operations the index queued.
    pub fn apply(&mut self) {
        for op in self.index.take_spill_ops() {
            match op {
                SpillOp::Put(until, id) => {
                    self.table.insert((until, id));
                }
                SpillOp::Delete(until, id) => {
                    self.table.remove(&(until, id));
                }
            }
        }
    }

    fn fact(&self, id: &str) -> Option<Fact> {
        self.db.get(id).map(|m| Fact {
            id: id.to_owned(),
            group: m.group.clone(),
            age_ms: m.age_ms,
            deliveries: m.deliveries,
        })
    }

    fn in_table(&self, id: &str) -> Option<u64> {
        self.table
            .iter()
            .find(|(_, who)| who == id)
            .map(|(until, _)| *until)
    }

    /// Makes the index keep the message (the engine's `know`).
    pub fn know(&mut self, id: &str) -> bool {
        if self.index.contains(id) {
            return true;
        }
        self.apply();
        let Some(fact) = self.fact(id) else {
            return false;
        };
        let until = self.in_table(id);
        self.index.adopt(fact, until);
        true
    }

    pub fn insert(
        &mut self,
        id: &str,
        group: Option<&str>,
        age_ms: u64,
        until_ms: u64,
        deliveries: u32,
        now_ms: u64,
    ) {
        if self.db.contains_key(id) {
            self.remove(id);
        }
        self.db.insert(
            id.to_owned(),
            Msg {
                group: group.map(str::to_owned),
                age_ms,
                deliveries,
            },
        );
        self.index
            .insert(id, group, age_ms, until_ms, deliveries, now_ms);
    }

    pub fn remove(&mut self, id: &str) -> bool {
        if !self.know(id) {
            return false;
        }
        self.index.remove(id);
        self.db.remove(id);
        self.index.forget_adopted();
        true
    }

    pub fn set_until(&mut self, id: &str, until_ms: u64, now_ms: u64) {
        self.know(id);
        self.index.set_until(id, until_ms, now_ms);
        self.index.forget_adopted();
    }

    pub fn deliver(&mut self, id: &str, until_ms: u64, now_ms: u64, deliveries: u32) {
        self.know(id);
        if let Some(msg) = self.db.get_mut(id) {
            msg.deliveries = deliveries;
        }
        self.index.deliver(id, until_ms, now_ms, deliveries);
        self.index.forget_adopted();
    }

    /// Ready messages of the gap, from one end.
    fn ready(&self, ask: &RefillAsk, desc: bool) -> Vec<Fact> {
        let inside = |id: &&String| {
            ask.after.as_ref().is_none_or(|a| *id > a)
                && ask.before.as_ref().is_none_or(|b| *id < b)
        };
        let ready = |id: &String| {
            self.reads.set(self.reads.get() + 1);
            !self.index.in_window(id)
                && !self.index.is_processing(id)
                && self.in_table(id).is_none()
        };
        let ids: Box<dyn Iterator<Item = &String>> = if desc {
            Box::new(self.db.keys().rev())
        } else {
            Box::new(self.db.keys())
        };
        ids.filter(inside)
            .filter(|id| ready(id))
            .take(ask.room)
            .filter_map(|id| self.fact(id))
            .collect()
    }

    /// Fills the window (the engine's `refill_window`); returns how many came in.
    pub fn refill(&mut self) -> usize {
        self.apply();
        let mut total = 0;
        while let Some(ask) = self.index.refill_ask() {
            let (old, new) = (self.ready(&ask, false), self.ready(&ask, true));
            let added = self.index.refill_with(old, new);
            total += added;
            if added == 0 {
                break;
            }
        }
        total
    }

    /// Rebuilds the window from both ends, as when the topic is opened.
    pub fn reset_window(&mut self) {
        self.index.reset_window();
        self.refill();
    }

    fn group_end(&self, key: &GroupKey, oldest: bool) -> Option<Fact> {
        let GroupKey::Named(group) = key else {
            return None;
        };
        let ids: Box<dyn Iterator<Item = &String>> = if oldest {
            Box::new(self.db.keys())
        } else {
            Box::new(self.db.keys().rev())
        };
        for id in ids {
            self.reads.set(self.reads.get() + 1);
            let msg = &self.db[id];
            if msg.group.as_deref().unwrap_or_default() == group
                && !self.index.is_processing(id)
                && self.in_table(id).is_none()
            {
                return self.fact(id);
            }
        }
        None
    }

    /// The next message to deliver (the engine's `pick_next`).
    pub fn pick(&mut self) -> Option<String> {
        self.refill();
        let mut resolved = Resolved::new();
        loop {
            match self.index.pick_with(&resolved) {
                Pick::Id(id) => return Some(id),
                Pick::Empty => return None,
                Pick::Need(need) => {
                    self.needs += 1;
                    for (key, oldest) in need {
                        let found = self.group_end(&key, oldest);
                        resolved.insert((key, oldest), found);
                    }
                }
            }
        }
    }

    /// Reads from the table what the list asks for; returns how many pairs entered the list.
    pub fn reload(&mut self) -> usize {
        self.apply();
        let wanted = self.index.reload_wanted();
        let rows = self
            .table
            .iter()
            .take(wanted)
            .map(|(until, id)| (*until, id.clone(), self.fact(id)))
            .collect();
        let loaded = self.index.reload(rows);
        self.apply();
        loaded
    }

    /// Returns to ready what expired, reloading the list from the table while it empties with due
    /// pairs left.
    pub fn release_due(&mut self, now_ms: u64) -> Vec<String> {
        let mut released = Vec::new();
        loop {
            released.extend(
                self.index
                    .release_due_counted(now_ms)
                    .into_iter()
                    .map(|(id, _)| id),
            );
            if self.reload() == 0 {
                break;
            }
        }
        self.index.forget_adopted();
        released
    }
}
