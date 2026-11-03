//! Identifiers: lowercase GUID v7, in the canonical hyphenated form
//! (doc 06d, section 8.3.1). Applies to topic, message, token and node.
//!
//! The random bits come from the thread's generator (`rand::rng()`, a CSPRNG seeded once by the
//! operating system): drawing them costs no system call, which matters on the publish path.

use std::sync::atomic::{AtomicU64, Ordering};

use uuid::{Builder, Uuid};

/// Generates a GUID v7 with the given instant (milliseconds since the Unix epoch).
pub fn new_at(unix_millis: u64) -> String {
    let random: [u8; 10] = rand::random();
    format_id(Builder::from_unix_timestamp_millis(unix_millis, &random).into_uuid())
}

fn format_id(uuid: Uuid) -> String {
    uuid.hyphenated()
        .encode_lower(&mut Uuid::encode_buffer())
        .to_owned()
}

/// Bits of the counter inside the same millisecond (the low bits of `IdGenerator::state`).
const COUNTER_BITS: u32 = 22;
const COUNTER_MASK: u64 = (1 << COUNTER_BITS) - 1;

/// ID generator that **never repeats or goes backwards**, not even within the same millisecond: two
/// IDs generated in sequence come out in increasing order (UC-MSG-013). `new_at` alone draws
/// the final bits at random, and two IDs of the same millisecond would end up in random order.
///
/// Like `uuid::ContextV7`, the ID carries a counter right after the instant (`rand_a` and the start of
/// `rand_b`) that starts at a random value in each millisecond and goes up by one at each ID; when it
/// runs out, the instant moves one millisecond ahead. The state (instant and counter) is a single
/// atomic, so generating takes no lock.
#[derive(Debug)]
pub struct IdGenerator {
    /// The instant (ms) of the last ID in the high bits and its counter in the low `COUNTER_BITS`.
    state: AtomicU64,
    /// No ID is generated below this instant (ms): the generator of a new life starts above the watermark of the
    /// old ones (doc 07, section 10.9).
    floor_ms: AtomicU64,
    /// The latest instant (ms) an ID was generated for.
    last_ms: AtomicU64,
}

impl Default for IdGenerator {
    fn default() -> Self {
        Self::new()
    }
}

impl IdGenerator {
    pub fn new() -> Self {
        Self {
            state: AtomicU64::new(0),
            floor_ms: AtomicU64::new(0),
            last_ms: AtomicU64::new(0),
        }
    }

    /// From now on no ID is generated for an instant before `unix_millis`. It only goes up.
    pub fn raise_floor(&self, unix_millis: u64) {
        self.floor_ms.fetch_max(unix_millis, Ordering::SeqCst);
        self.last_ms
            .fetch_max(unix_millis.saturating_sub(1), Ordering::SeqCst);
    }

    /// The largest ID this generator produced or was told to stay above (the instant of it; the rest of the
    /// ID is not meaningful): the watermark the manifest of the life records. `None` if there is none.
    pub fn watermark(&self) -> Option<String> {
        match self.last_ms.load(Ordering::SeqCst) {
            0 => None,
            ms => Some(new_at(ms)),
        }
    }

    pub fn new_at(&self, unix_millis: u64) -> String {
        let unix_millis = unix_millis.max(self.floor_ms.load(Ordering::SeqCst));
        let (ms, counter) = self.next(unix_millis);
        self.last_ms.fetch_max(ms, Ordering::SeqCst);
        let random: [u8; 7] = rand::random();
        // 48 bits of instant, version 7, 12 bits of counter (`rand_a`), variant, 10 more bits of
        // counter and 52 random bits.
        let mut bytes = [0u8; 16];
        bytes[..6].copy_from_slice(&ms.to_be_bytes()[2..]);
        bytes[6] = 0x70 | ((counter >> 18) & 0x0F) as u8;
        bytes[7] = (counter >> 10) as u8;
        bytes[8] = 0x80 | ((counter >> 4) & 0x3F) as u8;
        bytes[9] = (((counter & 0x0F) as u8) << 4) | (random[0] & 0x0F);
        bytes[10..].copy_from_slice(&random[1..]);
        format_id(Uuid::from_bytes(bytes))
    }

    /// The instant and counter of the next ID: never below the last one.
    fn next(&self, unix_millis: u64) -> (u64, u64) {
        let mut current = self.state.load(Ordering::Acquire);
        loop {
            let (last_ms, last_counter) = (current >> COUNTER_BITS, current & COUNTER_MASK);
            let (ms, counter) = if unix_millis > last_ms {
                // A new millisecond: the counter starts at random in the lower half, so there is room to grow.
                (unix_millis, rand::random::<u64>() & (COUNTER_MASK >> 1))
            } else if last_counter < COUNTER_MASK {
                (last_ms, last_counter + 1)
            } else {
                (last_ms + 1, 0)
            };
            match self.state.compare_exchange_weak(
                current,
                (ms << COUNTER_BITS) | counter,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return (ms, counter),
                Err(actual) => current = actual,
            }
        }
    }
}

/// `true` if `s` is a GUID v7 in the canonical form, lowercase.
pub fn is_guid_v7(s: &str) -> bool {
    // Without building a lowercase copy: it is checked for every message of every request.
    s.len() == 36
        && !s.bytes().any(|b| b.is_ascii_uppercase())
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
