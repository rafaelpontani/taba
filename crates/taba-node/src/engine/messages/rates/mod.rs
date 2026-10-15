//! The publish and read rate of a topic (doc 02, section 3.2; doc 11): the moving average
//! of messages per second in the `memory.rate.window` window (5 minutes). The read rate
//! decides which topics lose memory first when it fills up.
//!
//! It keeps one bucket per second and discards what left the window, so the cost is fixed per topic.

use std::collections::VecDeque;

/// Counts per second within a window.
#[derive(Debug, Clone, Default)]
pub struct RateWindow {
    /// `(second, amount)`, in second order.
    buckets: VecDeque<(u64, u64)>,
}

impl RateWindow {
    pub fn record(&mut self, now_ms: u64, count: u64, window_secs: u64) {
        let second = now_ms / 1000;
        self.prune(second, window_secs);
        match self.buckets.back_mut() {
            Some((s, n)) if *s == second => *n += count,
            _ => self.buckets.push_back((second, count)),
        }
    }

    fn prune(&mut self, second: u64, window_secs: u64) {
        let oldest = second.saturating_sub(window_secs.saturating_sub(1));
        while self.buckets.front().is_some_and(|(s, _)| *s < oldest) {
            self.buckets.pop_front();
        }
    }

    /// Messages per second in the window ending at `now_ms`.
    pub fn per_second(&self, now_ms: u64, window_secs: u64) -> f64 {
        let second = now_ms / 1000;
        let oldest = second.saturating_sub(window_secs.saturating_sub(1));
        let total: u64 = self
            .buckets
            .iter()
            .filter(|(s, _)| *s >= oldest && *s <= second)
            .map(|(_, n)| *n)
            .sum();
        total as f64 / window_secs.max(1) as f64
    }
}

/// A topic's rates.
#[derive(Debug, Clone, Default)]
pub struct Rates {
    pub published: RateWindow,
    pub consumed: RateWindow,
    pub acked: RateWindow,
}

#[cfg(test)]
mod tests;
