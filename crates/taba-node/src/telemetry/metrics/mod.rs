//! The node metrics registry (doc 11): a single instrumentation, from which `GET /metrics` (and,
//! later, OTLP) reads. Each metric is a [`Def`] with a name, type and unit in the OpenTelemetry
//! vocabulary (the table is in `defs`); the registry keeps the series (one value per combination of
//! attributes).
//!
//! Two ways of feeding it: whoever is on the request path adds up (`add`, `observe`), and what the
//! `engine` already counts on its own is copied at read time (`set`, `set_histogram`). There is
//! no dependency on the rest of the node.
//!
//! The registry is on the path of every request: an existing series is found under read locks only
//! (several threads at once), by a hash of the attributes computed without allocating, and its value
//! is updated with atomics. Only a series seen for the first time takes the write lock.

mod defs;

pub use defs::*;

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};

use super::histogram::Histogram;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Only grows.
    Counter,
    /// Goes up and down and adds up across nodes.
    UpDownCounter,
    /// A value that does not add up (limit, age, state).
    Gauge,
    Histogram,
}

/// A metric's definition.
#[derive(Debug)]
pub struct Def {
    pub name: &'static str,
    pub kind: Kind,
    /// UCUM unit: `s`, `By`, `1` (ratios only) or a count in braces (`{message}`).
    pub unit: &'static str,
    pub help: &'static str,
    /// Histogram only: the bucket limits.
    pub buckets: &'static [f64],
}

/// A series' attributes, sorted by name.
pub type Labels = Vec<(String, String)>;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Number(f64),
    Histogram(Histogram),
}

/// A metric with all its series, as the read sees them.
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub def: &'static Def,
    pub series: Vec<(Labels, Value)>,
}

#[derive(Default)]
pub struct Registry {
    families: RwLock<HashMap<&'static str, Family>>,
}

struct Family {
    def: &'static Def,
    /// Hash of the attributes → the series with that hash (more than one only on a collision).
    series: RwLock<HashMap<u64, Vec<Series>>>,
}

struct Series {
    labels: Labels,
    value: Cell,
}

/// A series' value, updated without a lock.
enum Cell {
    /// The bits of an `f64`.
    Number(AtomicU64),
    Histogram(AtomicHistogram),
}

struct AtomicHistogram {
    bounds: &'static [f64],
    /// One count per bucket (not cumulative); the last is `+Inf`.
    counts: Box<[AtomicU64]>,
    /// The bits of the `f64` sum.
    sum: AtomicU64,
}

impl AtomicHistogram {
    fn new(bounds: &'static [f64]) -> Self {
        Self {
            bounds,
            counts: (0..=bounds.len()).map(|_| AtomicU64::new(0)).collect(),
            sum: AtomicU64::new(0f64.to_bits()),
        }
    }

    fn from(histogram: &Histogram) -> Self {
        let this = Self::new(histogram.bounds());
        for (cell, n) in this.counts.iter().zip(histogram.buckets()) {
            cell.store(*n, Ordering::Relaxed);
        }
        this.sum.store(histogram.sum().to_bits(), Ordering::Relaxed);
        this
    }

    fn record(&self, value: f64) {
        let bucket = self
            .bounds
            .iter()
            .position(|bound| value <= *bound)
            .unwrap_or(self.bounds.len());
        self.counts[bucket].fetch_add(1, Ordering::Relaxed);
        add_f64(&self.sum, value);
    }

    fn load(&self) -> Histogram {
        let counts = self
            .counts
            .iter()
            .map(|n| n.load(Ordering::Relaxed))
            .collect();
        Histogram::from_parts(
            self.bounds,
            counts,
            f64::from_bits(self.sum.load(Ordering::Relaxed)),
        )
    }
}

fn add_f64(cell: &AtomicU64, delta: f64) {
    let mut current = cell.load(Ordering::Relaxed);
    loop {
        let next = (f64::from_bits(current) + delta).to_bits();
        match cell.compare_exchange_weak(current, next, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => return,
            Err(actual) => current = actual,
        }
    }
}

impl Cell {
    fn new(def: &'static Def) -> Self {
        match def.kind {
            Kind::Histogram => Cell::Histogram(AtomicHistogram::new(def.buckets)),
            _ => Cell::Number(AtomicU64::new(0f64.to_bits())),
        }
    }

    fn load(&self) -> Value {
        match self {
            Cell::Number(bits) => Value::Number(f64::from_bits(bits.load(Ordering::Relaxed))),
            Cell::Histogram(h) => Value::Histogram(h.load()),
        }
    }
}

/// Most series have few attributes: up to this many are sorted on the stack.
const INLINE_LABELS: usize = 8;

/// Calls `f` with the attributes sorted by name (and value), without allocating in the common case.
fn with_sorted<R>(labels: &[(&str, &str)], f: impl FnOnce(&[(&str, &str)]) -> R) -> R {
    if labels.len() <= INLINE_LABELS {
        let mut buffer = [("", ""); INLINE_LABELS];
        let sorted = &mut buffer[..labels.len()];
        sorted.copy_from_slice(labels);
        sorted.sort_unstable();
        f(sorted)
    } else {
        let mut sorted = labels.to_vec();
        sorted.sort_unstable();
        f(&sorted)
    }
}

/// FNV-1a of the sorted attributes.
fn hash_of(sorted: &[(&str, &str)]) -> u64 {
    const PRIME: u64 = 0x0100_0000_01b3;
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut feed = |bytes: &[u8]| {
        for byte in bytes {
            hash = (hash ^ u64::from(*byte)).wrapping_mul(PRIME);
        }
        // A separator, so ("ab", "c") and ("a", "bc") differ.
        hash = (hash ^ 0xff).wrapping_mul(PRIME);
    };
    for (key, value) in sorted {
        feed(key.as_bytes());
        feed(value.as_bytes());
    }
    hash
}

fn same_labels(labels: &Labels, sorted: &[(&str, &str)]) -> bool {
    labels.len() == sorted.len()
        && labels
            .iter()
            .zip(sorted)
            .all(|((k, v), (sk, sv))| k == sk && v == sv)
}

fn owned(sorted: &[(&str, &str)]) -> Labels {
    sorted
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect()
}

fn find<'a>(
    series: &'a HashMap<u64, Vec<Series>>,
    hash: u64,
    sorted: &[(&str, &str)],
) -> Option<&'a Series> {
    series
        .get(&hash)?
        .iter()
        .find(|s| same_labels(&s.labels, sorted))
}

impl Registry {
    fn read(&self) -> RwLockReadGuard<'_, HashMap<&'static str, Family>> {
        self.families.read().unwrap_or_else(|e| e.into_inner())
    }

    fn write(&self) -> RwLockWriteGuard<'_, HashMap<&'static str, Family>> {
        self.families.write().unwrap_or_else(|e| e.into_inner())
    }

    /// Runs `update` on the series, creating it if it does not exist yet.
    fn with_series(&self, def: &'static Def, labels: &[(&str, &str)], update: impl FnOnce(&Cell)) {
        with_sorted(labels, |sorted| {
            let hash = hash_of(sorted);
            {
                let families = self.read();
                if let Some(family) = families.get(def.name) {
                    let series = family.series.read().unwrap_or_else(|e| e.into_inner());
                    if let Some(found) = find(&series, hash, sorted) {
                        update(&found.value);
                        return;
                    }
                }
            }
            let mut families = self.write();
            let family = families.entry(def.name).or_insert_with(|| Family {
                def,
                series: RwLock::new(HashMap::new()),
            });
            let bucket = family
                .series
                .get_mut()
                .unwrap_or_else(|e| e.into_inner())
                .entry(hash)
                .or_default();
            let index = match bucket.iter().position(|s| same_labels(&s.labels, sorted)) {
                Some(index) => index,
                None => {
                    bucket.push(Series {
                        labels: owned(sorted),
                        value: Cell::new(def),
                    });
                    bucket.len() - 1
                }
            };
            update(&bucket[index].value);
        });
    }

    /// The value of a series, if it exists.
    fn load(&self, def: &'static Def, labels: &[(&str, &str)]) -> Option<Value> {
        with_sorted(labels, |sorted| {
            let families = self.read();
            let series = families
                .get(def.name)?
                .series
                .read()
                .unwrap_or_else(|e| e.into_inner());
            find(&series, hash_of(sorted), sorted).map(|s| s.value.load())
        })
    }

    /// Adds `delta` to the series (counter or up-down counter).
    pub fn add(&self, def: &'static Def, labels: &[(&str, &str)], delta: f64) {
        debug_assert!(matches!(def.kind, Kind::Counter | Kind::UpDownCounter));
        self.with_series(def, labels, |cell| {
            if let Cell::Number(bits) = cell {
                add_f64(bits, delta);
            }
        });
    }

    /// Sets the series' value. Serves the gauge and the counter that comes from an outside count.
    pub fn set(&self, def: &'static Def, labels: &[(&str, &str)], value: f64) {
        debug_assert!(def.kind != Kind::Histogram);
        self.with_series(def, labels, |cell| {
            if let Cell::Number(bits) = cell {
                bits.store(value.to_bits(), Ordering::Relaxed);
            }
        });
    }

    /// Adds an observation to the series' histogram.
    pub fn observe(&self, def: &'static Def, labels: &[(&str, &str)], value: f64) {
        debug_assert_eq!(def.kind, Kind::Histogram);
        self.with_series(def, labels, |cell| {
            if let Cell::Histogram(h) = cell {
                h.record(value);
            }
        });
    }

    /// Replaces the series' histogram with one that already comes counted.
    pub fn set_histogram(&self, def: &'static Def, labels: &[(&str, &str)], histogram: Histogram) {
        debug_assert_eq!(def.kind, Kind::Histogram);
        with_sorted(labels, |sorted| {
            let hash = hash_of(sorted);
            let mut families = self.write();
            let family = families.entry(def.name).or_insert_with(|| Family {
                def,
                series: RwLock::new(HashMap::new()),
            });
            let bucket = family
                .series
                .get_mut()
                .unwrap_or_else(|e| e.into_inner())
                .entry(hash)
                .or_default();
            let value = Cell::Histogram(AtomicHistogram::from(&histogram));
            match bucket.iter_mut().find(|s| same_labels(&s.labels, sorted)) {
                Some(found) => found.value = value,
                None => bucket.push(Series {
                    labels: owned(sorted),
                    value,
                }),
            }
        });
    }

    /// Deletes all the metric's series (for those rebuilt on every read).
    pub fn clear(&self, def: &'static Def) {
        self.write().remove(def.name);
    }

    /// The value of a numeric series (for the tests).
    pub fn get(&self, def: &'static Def, labels: &[(&str, &str)]) -> Option<f64> {
        match self.load(def, labels)? {
            Value::Number(n) => Some(n),
            Value::Histogram(_) => None,
        }
    }

    /// A series' histogram (for the tests).
    pub fn histogram(&self, def: &'static Def, labels: &[(&str, &str)]) -> Option<Histogram> {
        match self.load(def, labels)? {
            Value::Histogram(h) => Some(h),
            Value::Number(_) => None,
        }
    }

    /// A copy of everything there is, in name order (the series in attribute order), for whoever
    /// writes the output.
    pub fn snapshot(&self) -> Vec<Snapshot> {
        let families = self.read();
        let mut out: Vec<Snapshot> = families
            .values()
            .map(|family| {
                let series = family.series.read().unwrap_or_else(|e| e.into_inner());
                let mut series: Vec<(Labels, Value)> = series
                    .values()
                    .flatten()
                    .map(|s| (s.labels.clone(), s.value.load()))
                    .collect();
                series.sort_by(|a, b| a.0.cmp(&b.0));
                Snapshot {
                    def: family.def,
                    series,
                }
            })
            .collect();
        out.sort_by_key(|snapshot| snapshot.def.name);
        out
    }
}

#[cfg(test)]
mod tests;
