//! The node clock, behind a trait so the tests control time
//! (invisibility, `ttl`, freezing, voting; doc 18, section 17.5).

use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub trait Clock: Send + Sync + 'static {
    fn now(&self) -> SystemTime;

    /// Milliseconds since the Unix epoch, the basis of the GUID v7 and of absolute deadlines.
    fn now_millis(&self) -> u64 {
        self.now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as u64)
    }
}

/// The system clock.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> SystemTime {
        SystemTime::now()
    }
}

/// Manual clock: only moves when the test says so.
#[derive(Debug)]
pub struct ManualClock {
    now: Mutex<SystemTime>,
}

impl ManualClock {
    pub fn new(start: SystemTime) -> Self {
        Self {
            now: Mutex::new(start),
        }
    }

    /// Starts at `2026-10-06T12:00:00Z`, the instant of the documentation's examples.
    pub fn at_doc_epoch() -> Self {
        Self::new(UNIX_EPOCH + Duration::from_secs(1_791_288_000))
    }

    pub fn advance(&self, by: Duration) {
        let mut now = self.now.lock().unwrap_or_else(|e| e.into_inner());
        *now += by;
    }
}

impl Clock for ManualClock {
    fn now(&self) -> SystemTime {
        *self.now.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[cfg(test)]
mod tests;
