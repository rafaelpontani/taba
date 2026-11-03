//! A fixed-bucket histogram (doc 11): the data the metrics registry keeps and the
//! `engine` uses for topic latencies (p50, p95 and p99) and for fair delivery wait.
//! It depends on nothing: it only counts and sums.

/// Upper limits, in seconds, of publish and consume latencies.
pub const LATENCY_BOUNDS: &[f64] = &[
    0.0001, 0.00025, 0.0005, 0.001, 0.0025, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5,
    5.0, 10.0,
];

#[derive(Debug, Clone, PartialEq)]
pub struct Histogram {
    bounds: &'static [f64],
    /// One count per bucket (not cumulative); the last is the `+Inf` bucket.
    counts: Vec<u64>,
    sum: f64,
    count: u64,
}

impl Histogram {
    pub fn new(bounds: &'static [f64]) -> Self {
        Self {
            bounds,
            counts: vec![0; bounds.len() + 1],
            sum: 0.0,
            count: 0,
        }
    }

    /// A histogram already counted elsewhere: `counts` per bucket (not cumulative, `+Inf` last).
    pub fn from_parts(bounds: &'static [f64], counts: Vec<u64>, sum: f64) -> Self {
        debug_assert_eq!(counts.len(), bounds.len() + 1);
        Self {
            bounds,
            count: counts.iter().sum(),
            counts,
            sum,
        }
    }

    pub fn record(&mut self, value: f64) {
        let bucket = self
            .bounds
            .iter()
            .position(|bound| value <= *bound)
            .unwrap_or(self.bounds.len());
        self.counts[bucket] += 1;
        self.sum += value;
        self.count += 1;
    }

    pub fn bounds(&self) -> &'static [f64] {
        self.bounds
    }

    pub fn count(&self) -> u64 {
        self.count
    }

    pub fn sum(&self) -> f64 {
        self.sum
    }

    /// The count of each bucket (not cumulative), `+Inf` last (the OTLP format).
    pub fn buckets(&self) -> &[u64] {
        &self.counts
    }

    /// The cumulative counts per bucket, `+Inf` last (the Prometheus format).
    pub fn cumulative(&self) -> Vec<u64> {
        let mut total = 0;
        self.counts
            .iter()
            .map(|n| {
                total += n;
                total
            })
            .collect()
    }

    /// The limit of the bucket where percentile `p` (from 0 to 1) falls: an approximation from above, at the
    /// size of the bucket. `None` with no observation. Whatever went past the last limit gets the
    /// last limit.
    pub fn percentile(&self, p: f64) -> Option<f64> {
        if self.count == 0 {
            return None;
        }
        let rank = ((p * self.count as f64).ceil() as u64).clamp(1, self.count);
        let mut total = 0;
        for (index, n) in self.counts.iter().enumerate() {
            total += n;
            if total >= rank {
                return Some(self.bounds[index.min(self.bounds.len() - 1)]);
            }
        }
        None
    }

    /// Adds up the observations of `other`, which must have the same limits.
    pub fn merge(&mut self, other: &Histogram) {
        debug_assert_eq!(self.bounds, other.bounds);
        for (mine, theirs) in self.counts.iter_mut().zip(&other.counts) {
            *mine += theirs;
        }
        self.sum += other.sum;
        self.count += other.count;
    }
}

#[cfg(test)]
mod tests;
