//! The history of the session (doc 34, D-TUI-5 and section 7): circular buffers, one per series, filled by the
//! sampler from the cluster reads. Nothing goes to disk; closing the app forgets everything.

use std::collections::{HashMap, VecDeque};

use crate::tui::api::ClusterStatus;
use crate::tui::prom;

/// The longest retention accepted and the shortest sampling interval (section 7).
pub const MAX_RETENTION_SECS: u64 = 6 * 3600;
pub const MIN_INTERVAL_SECS: u64 = 1;
pub const DEFAULT_RETENTION_SECS: u64 = 3600;
pub const DEFAULT_INTERVAL_SECS: u64 = 5;

/// Checks the two settings of `[history]`.
pub fn validate(retention_secs: u64, interval_secs: u64) -> Result<(), String> {
    if interval_secs < MIN_INTERVAL_SECS {
        return Err("history.interval: use 1s or more".to_owned());
    }
    if retention_secs > MAX_RETENTION_SECS {
        return Err("history.retention: use 6h or less".to_owned());
    }
    if retention_secs < interval_secs {
        return Err("history.retention: it must hold at least one sample".to_owned());
    }
    Ok(())
}

/// What is measured for a node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Series {
    MemoryUsed,
    MemoryLimit,
    MemoryMessages,
    DiskMessages,
    ActiveRequests,
    /// Requests refused with `429` per second, from the counter.
    RateLimited,
    /// The worst `p95` of the channels' round trip, in seconds.
    ChannelRtt,
}

/// One sample: when (ms since the app opened), the value, and whether the read it came from was incomplete.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    pub at: u64,
    pub value: f64,
    pub partial: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Ring {
    cap: usize,
    points: VecDeque<Point>,
}

impl Ring {
    pub fn new(cap: usize) -> Ring {
        Ring {
            cap: cap.max(1),
            points: VecDeque::new(),
        }
    }

    pub fn push(&mut self, point: Point) {
        if self.points.len() == self.cap {
            self.points.pop_front();
        }
        self.points.push_back(point);
    }

    pub fn len(&self) -> usize {
        self.points.len()
    }

    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    pub fn last(&self) -> Option<Point> {
        self.points.back().copied()
    }

    /// The points of the `window_ms` that ends at `now`, oldest first (a frozen chart passes the instant it froze).
    pub fn window(&self, now: u64, window_ms: u64) -> Vec<Point> {
        let from = now.saturating_sub(window_ms);
        self.points
            .iter()
            .copied()
            .filter(|p| p.at >= from && p.at <= now)
            .collect()
    }

    /// How long ago the oldest sample was taken (what the chart can honestly claim to show).
    pub fn collected_ms(&self, now: u64) -> u64 {
        self.points.front().map_or(0, |p| now.saturating_sub(p.at))
    }
}

/// Smallest, largest and last value of some points, for the summary line of a chart.
pub fn summary(points: &[Point]) -> Option<(f64, f64, f64)> {
    let last = points.last()?.value;
    let min = points.iter().map(|p| p.value).fold(f64::INFINITY, f64::min);
    let max = points
        .iter()
        .map(|p| p.value)
        .fold(f64::NEG_INFINITY, f64::max);
    Some((min, max, last))
}

#[derive(Debug, Clone)]
pub struct History {
    cap: usize,
    interval_ms: u64,
    rings: HashMap<(String, Series), Ring>,
    /// The last reading of each counter, for its rate.
    counters: HashMap<String, (u64, f64)>,
    last_sample: Option<u64>,
}

impl History {
    pub fn new(retention_secs: u64, interval_secs: u64) -> History {
        let interval = interval_secs.max(MIN_INTERVAL_SECS);
        History {
            cap: (retention_secs / interval).max(1) as usize,
            interval_ms: interval * 1000,
            rings: HashMap::new(),
            counters: HashMap::new(),
            last_sample: None,
        }
    }

    pub fn capacity(&self) -> usize {
        self.cap
    }

    pub fn interval_ms(&self) -> u64 {
        self.interval_ms
    }

    /// The retention in milliseconds: no chart window may go beyond it.
    pub fn retention_ms(&self) -> u64 {
        self.cap as u64 * self.interval_ms
    }

    /// Forgets everything (a new profile).
    pub fn clear(&mut self) {
        self.rings.clear();
        self.counters.clear();
        self.last_sample = None;
    }

    pub fn series(&self, node: &str, series: Series) -> Option<&Ring> {
        self.rings.get(&(node.to_owned(), series))
    }

    fn push(&mut self, node: &str, series: Series, point: Point) {
        let cap = self.cap;
        self.rings
            .entry((node.to_owned(), series))
            .or_insert_with(|| Ring::new(cap))
            .push(point);
    }

    /// Takes a sample of every node of a status read, if the interval has passed since the last one. The
    /// sample of an incomplete read is kept, marked as such.
    pub fn sample(&mut self, now: u64, status: &ClusterStatus) -> bool {
        if self.last_sample.is_some_and(|t| now < t + self.interval_ms) {
            return false;
        }
        self.last_sample = Some(now);
        for node in status.nodes.iter().filter(|n| n.reachable) {
            let mk = |value: f64| Point {
                at: now,
                value,
                partial: status.partial,
            };
            let id = node.node_id.as_str();
            self.push(id, Series::MemoryUsed, mk(node.memory_used as f64));
            self.push(id, Series::MemoryLimit, mk(node.memory_limit as f64));
            self.push(id, Series::MemoryMessages, mk(node.memory_messages as f64));
            self.push(id, Series::DiskMessages, mk(node.disk_messages as f64));
            self.push(id, Series::ActiveRequests, mk(node.active_requests));
            if let Some(rtt) = node.rtt_p95 {
                self.push(id, Series::ChannelRtt, mk(rtt));
            }
            let previous = self
                .counters
                .insert(id.to_owned(), (now, node.rate_limited));
            if let Some((then, was)) = previous {
                let seconds = (now - then) as f64 / 1000.0;
                // A counter that went down is a restart: no sample, never a negative rate.
                if let Some(per_second) = prom::rate(was, node.rate_limited, seconds) {
                    self.push(id, Series::RateLimited, mk(per_second));
                }
            }
        }
        true
    }
}

#[cfg(test)]
mod tests;
