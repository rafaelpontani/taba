//! The node metrics registry (doc 11): a single instrumentation, from which `GET /metrics` (and,
//! later, OTLP) reads. Each metric is a [`Def`] with a name, type and unit in the OpenTelemetry
//! vocabulary (the table is in `defs`); the registry keeps the series (one value per combination of
//! attributes).
//!
//! Two ways of feeding it: whoever is on the request path adds up (`add`, `observe`), and what the
//! `engine` already counts on its own is copied at read time (`set`, `set_histogram`). There is
//! no dependency on the rest of the node.

mod defs;

pub use defs::*;

use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard};

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
    families: Mutex<BTreeMap<&'static str, Family>>,
}

struct Family {
    def: &'static Def,
    series: BTreeMap<Labels, Value>,
}

fn labels_of(labels: &[(&str, &str)]) -> Labels {
    let mut out: Labels = labels
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect();
    out.sort();
    out
}

impl Registry {
    fn families(&self) -> MutexGuard<'_, BTreeMap<&'static str, Family>> {
        self.families.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn with_series(
        &self,
        def: &'static Def,
        labels: &[(&str, &str)],
        update: impl FnOnce(&mut Value),
        init: impl FnOnce() -> Value,
    ) {
        let mut families = self.families();
        let family = families.entry(def.name).or_insert_with(|| Family {
            def,
            series: BTreeMap::new(),
        });
        update(family.series.entry(labels_of(labels)).or_insert_with(init));
    }

    /// Adds `delta` to the series (counter or up-down counter).
    pub fn add(&self, def: &'static Def, labels: &[(&str, &str)], delta: f64) {
        debug_assert!(matches!(def.kind, Kind::Counter | Kind::UpDownCounter));
        self.with_series(
            def,
            labels,
            |value| {
                if let Value::Number(n) = value {
                    *n += delta;
                }
            },
            || Value::Number(0.0),
        );
    }

    /// Sets the series' value. Serves the gauge and the counter that comes from an outside count.
    pub fn set(&self, def: &'static Def, labels: &[(&str, &str)], value: f64) {
        debug_assert!(def.kind != Kind::Histogram);
        self.with_series(
            def,
            labels,
            |v| *v = Value::Number(value),
            || Value::Number(0.0),
        );
    }

    /// Adds an observation to the series' histogram.
    pub fn observe(&self, def: &'static Def, labels: &[(&str, &str)], value: f64) {
        debug_assert_eq!(def.kind, Kind::Histogram);
        self.with_series(
            def,
            labels,
            |v| {
                if let Value::Histogram(h) = v {
                    h.record(value);
                }
            },
            || Value::Histogram(Histogram::new(def.buckets)),
        );
    }

    /// Replaces the series' histogram with one that already comes counted.
    pub fn set_histogram(&self, def: &'static Def, labels: &[(&str, &str)], histogram: Histogram) {
        debug_assert_eq!(def.kind, Kind::Histogram);
        self.with_series(
            def,
            labels,
            |v| *v = Value::Histogram(histogram.clone()),
            || Value::Histogram(histogram.clone()),
        );
    }

    /// Deletes all the metric's series (for those rebuilt on every read).
    pub fn clear(&self, def: &'static Def) {
        self.families().remove(def.name);
    }

    /// The value of a numeric series (for the tests).
    pub fn get(&self, def: &'static Def, labels: &[(&str, &str)]) -> Option<f64> {
        match self
            .families()
            .get(def.name)?
            .series
            .get(&labels_of(labels))?
        {
            Value::Number(n) => Some(*n),
            Value::Histogram(_) => None,
        }
    }

    /// A series' histogram (for the tests).
    pub fn histogram(&self, def: &'static Def, labels: &[(&str, &str)]) -> Option<Histogram> {
        match self
            .families()
            .get(def.name)?
            .series
            .get(&labels_of(labels))?
        {
            Value::Histogram(h) => Some(h.clone()),
            Value::Number(_) => None,
        }
    }

    /// A copy of everything there is, in name order, for whoever writes the output.
    pub fn snapshot(&self) -> Vec<Snapshot> {
        self.families()
            .values()
            .map(|family| Snapshot {
                def: family.def,
                series: family
                    .series
                    .iter()
                    .map(|(labels, value)| (labels.clone(), value.clone()))
                    .collect(),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests;
