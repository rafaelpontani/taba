//! Identifiers: lowercase GUID v7, in the canonical hyphenated form
//! (doc 06d, section 8.3.1). Applies to topic, message, token and node.

use uuid::{Timestamp, Uuid};

/// Generates a GUID v7 with the given instant (milliseconds since the Unix epoch).
pub fn new_at(unix_millis: u64) -> String {
    let secs = unix_millis / 1000;
    let nanos = ((unix_millis % 1000) * 1_000_000) as u32;
    Uuid::new_v7(Timestamp::from_unix(uuid::NoContext, secs, nanos)).to_string()
}

/// ID generator that **never repeats or goes backwards**, not even within the same millisecond: two
/// IDs generated in sequence come out in increasing order (UC-MSG-013). `new_at` alone draws
/// the final bits at random, and two IDs of the same millisecond would end up in random order.
#[derive(Debug)]
pub struct IdGenerator {
    context: std::sync::Mutex<uuid::ContextV7>,
    /// No ID is generated below this instant (ms): the generator of a new life starts above the watermark of the
    /// old ones (doc 07, section 10.9).
    floor_ms: std::sync::atomic::AtomicU64,
    /// The latest instant (ms) an ID was generated for.
    last_ms: std::sync::atomic::AtomicU64,
}

impl Default for IdGenerator {
    fn default() -> Self {
        Self::new()
    }
}

impl IdGenerator {
    pub fn new() -> Self {
        Self {
            context: std::sync::Mutex::new(uuid::ContextV7::new()),
            floor_ms: std::sync::atomic::AtomicU64::new(0),
            last_ms: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// From now on no ID is generated for an instant before `unix_millis`. It only goes up.
    pub fn raise_floor(&self, unix_millis: u64) {
        self.floor_ms
            .fetch_max(unix_millis, std::sync::atomic::Ordering::SeqCst);
        self.last_ms.fetch_max(
            unix_millis.saturating_sub(1),
            std::sync::atomic::Ordering::SeqCst,
        );
    }

    /// The largest ID this generator produced or was told to stay above (the instant of it; the rest of the
    /// ID is not meaningful): the watermark the manifest of the life records. `None` if there is none.
    pub fn watermark(&self) -> Option<String> {
        match self.last_ms.load(std::sync::atomic::Ordering::SeqCst) {
            0 => None,
            ms => Some(new_at(ms)),
        }
    }

    pub fn new_at(&self, unix_millis: u64) -> String {
        let unix_millis = unix_millis.max(self.floor_ms.load(std::sync::atomic::Ordering::SeqCst));
        self.last_ms
            .fetch_max(unix_millis, std::sync::atomic::Ordering::SeqCst);
        let secs = unix_millis / 1000;
        let nanos = ((unix_millis % 1000) * 1_000_000) as u32;
        let context = self.context.lock().unwrap_or_else(|e| e.into_inner());
        Uuid::new_v7(Timestamp::from_unix(&*context, secs, nanos)).to_string()
    }
}

/// `true` if `s` is a GUID v7 in the canonical form, lowercase.
pub fn is_guid_v7(s: &str) -> bool {
    s == s.to_ascii_lowercase()
        && s.len() == 36
        && Uuid::parse_str(s).is_ok_and(|u| u.get_version_num() == 7)
}

/// The instant (ms since the Unix epoch) recorded in a GUID v7. A message's
/// age, for the `ttl`, comes from here (doc 13, section 12.10).
pub fn unix_millis_of(s: &str) -> Option<u64> {
    let uuid = Uuid::parse_str(s).ok()?;
    if uuid.get_version_num() != 7 {
        return None;
    }
    let (secs, nanos) = uuid.get_timestamp()?.to_unix();
    Some(secs * 1000 + u64::from(nanos) / 1_000_000)
}

#[cfg(test)]
mod tests;
