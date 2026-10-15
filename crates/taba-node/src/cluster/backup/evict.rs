//! Full disk: the eviction (doc 23, "S3 backup", section 1.8).
//!
//! Above `backup.disk.evict-above` (85%) the node deletes the **body file** of messages that already
//! have a reference in the bucket, until usage falls to `backup.disk.evict-to` (70%). The row in `_topic.db`
//! stays, so the storage's "file first, row after" order still holds. Only what is
//! on disk is freed: a message only in memory frees nothing. On read, the missing
//! file becomes a `Range` on the segment (`fetch_body`).
//!
//! During the flush and the drain, on reaching `backup.disk.pause-at` (90%) the memory-to-disk
//! pass **pauses** until the eviction frees space, at most for `backup.drain.timeout`.

use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use super::{Backup, lock};

/// How many evictions between each new check of disk usage.
const CHECK_EVERY: u32 = 32;

impl Backup {
    /// Disk usage (%). The real reading holds for 200 ms: the drain asks for it on every message.
    fn usage(&self) -> u32 {
        if let Some(probe) = lock(&self.disk_probe).clone() {
            return probe();
        }
        let mut cached = lock(&self.disk_usage);
        if let Some((at, percent)) = *cached
            && at.elapsed() < Duration::from_millis(200)
        {
            return percent;
        }
        let percent = self.disk.disk_usage_percent().unwrap_or(0);
        *cached = Some((Instant::now(), percent));
        percent
    }

    /// One eviction pass: only acts above `evict-above`. Returns how many bodies it deleted.
    pub(crate) fn evict_pass(&self) -> u64 {
        if self.usage() <= self.rules.evict_above {
            return 0;
        }
        self.evict_down_to(self.rules.evict_to)
    }

    /// Deletes bodies of saved messages until disk usage reaches `target` (%).
    pub(crate) fn evict_down_to(&self, target: u32) -> u64 {
        let mut evicted = 0u64;
        let mut since_check = 0;
        for topic in self.messages.topic_names() {
            let Ok(saved) = self.db.saved_of_topic(&topic) else {
                continue;
            };
            for (id, _) in saved {
                match self.disk.evict_body(&topic, &id) {
                    Ok(freed) if freed > 0 => {
                        evicted += 1;
                        self.counters.bodies_evicted.fetch_add(1, Ordering::Relaxed);
                    }
                    Ok(_) => continue,
                    Err(e) => {
                        tracing::warn!(error = %e, topic = %topic, "backup: eviction failed");
                        continue;
                    }
                }
                since_check += 1;
                if since_check >= CHECK_EVERY {
                    since_check = 0;
                    if self.usage() <= target {
                        return evicted;
                    }
                }
            }
        }
        if evicted > 0 {
            tracing::info!(
                evicted,
                usage = self.usage(),
                "backup: bodies of messages already saved in S3 left the disk"
            );
        }
        evicted
    }

    /// Before moving a message from memory to disk (drain and flush): with the disk at
    /// `pause-at`, waits for the eviction to free space, up to `backup.drain.timeout`. Returns `false` if
    /// it gave up (the node follows the usual behavior: failure by full disk).
    pub(crate) fn wait_for_disk(&self) -> bool {
        if self.usage() < self.rules.pause_at {
            return true;
        }
        let deadline = Instant::now() + self.rules.drain_timeout;
        let mut announced = false;
        loop {
            self.evict_down_to(self.rules.evict_to);
            if self.usage() < self.rules.pause_at {
                return true;
            }
            if !announced {
                tracing::warn!(
                    usage = self.usage(),
                    "backup: disk at {}% or more; the memory-to-disk pass pauses until the eviction frees space",
                    self.rules.pause_at
                );
                announced = true;
            }
            if Instant::now() >= deadline || self.cancel.is_cancelled() {
                tracing::error!("backup: the disk did not free space in time; carrying on");
                return false;
            }
            // The upload to S3 is what creates the most evictable bodies.
            self.wake.notify_one();
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}
