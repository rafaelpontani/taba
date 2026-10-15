//! The in-memory store of the tests: what S3 does, without a network, and with programmable failures.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};

use super::{Object, ObjectInfo, ObjectStore, StoreError, StoreFuture, StoreResult};

#[derive(Default)]
pub struct MemoryStore {
    objects: Mutex<BTreeMap<String, Object>>,
    /// Down: every call fails.
    down: AtomicBool,
    /// No conditional write (an old S3): `put_if_*` fail.
    no_conditional: AtomicBool,
    version: AtomicU64,
    puts: AtomicU64,
    gets: AtomicU64,
}

impl MemoryStore {
    pub fn new() -> Self {
        Self::default()
    }

    fn objects(&self) -> MutexGuard<'_, BTreeMap<String, Object>> {
        self.objects.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Turns the failure of all calls on or off.
    pub fn set_down(&self, down: bool) {
        self.down.store(down, Ordering::SeqCst);
    }

    /// Turns conditional write support on or off.
    pub fn set_conditional(&self, supported: bool) {
        self.no_conditional.store(!supported, Ordering::SeqCst);
    }

    /// How many writes (`put*`) succeeded.
    pub fn puts(&self) -> u64 {
        self.puts.load(Ordering::SeqCst)
    }

    /// How many reads (`get*`) were made.
    pub fn gets(&self) -> u64 {
        self.gets.load(Ordering::SeqCst)
    }

    /// The names of the objects that start with `prefix`.
    pub fn keys(&self, prefix: &str) -> Vec<String> {
        self.objects()
            .keys()
            .filter(|k| k.starts_with(prefix))
            .cloned()
            .collect()
    }

    /// An object's content, without counting as a read.
    pub fn peek(&self, key: &str) -> Option<Vec<u8>> {
        self.objects().get(key).map(|o| o.body.clone())
    }

    fn check(&self) -> StoreResult<()> {
        if self.down.load(Ordering::SeqCst) {
            Err(StoreError::Unavailable("down (test)".into()))
        } else {
            Ok(())
        }
    }

    fn check_conditional(&self) -> StoreResult<()> {
        if self.no_conditional.load(Ordering::SeqCst) {
            Err(StoreError::Unsupported(
                "no conditional write (test)".into(),
            ))
        } else {
            Ok(())
        }
    }

    fn store(&self, map: &mut BTreeMap<String, Object>, key: &str, body: Vec<u8>) -> String {
        let etag = format!("v{}", self.version.fetch_add(1, Ordering::SeqCst) + 1);
        map.insert(
            key.to_owned(),
            Object {
                body,
                etag: etag.clone(),
            },
        );
        self.puts.fetch_add(1, Ordering::SeqCst);
        etag
    }
}

impl ObjectStore for MemoryStore {
    fn put(&self, key: &str, body: Vec<u8>) -> StoreFuture<'_, String> {
        let key = key.to_owned();
        Box::pin(async move {
            self.check()?;
            Ok(self.store(&mut self.objects(), &key, body))
        })
    }

    fn put_if_absent(&self, key: &str, body: Vec<u8>) -> StoreFuture<'_, bool> {
        let key = key.to_owned();
        Box::pin(async move {
            self.check()?;
            self.check_conditional()?;
            let mut map = self.objects();
            if map.contains_key(&key) {
                return Ok(false);
            }
            self.store(&mut map, &key, body);
            Ok(true)
        })
    }

    fn put_if_match(&self, key: &str, body: Vec<u8>, etag: &str) -> StoreFuture<'_, bool> {
        let (key, etag) = (key.to_owned(), etag.to_owned());
        Box::pin(async move {
            self.check()?;
            self.check_conditional()?;
            let mut map = self.objects();
            if map.get(&key).is_none_or(|o| o.etag != etag) {
                return Ok(false);
            }
            self.store(&mut map, &key, body);
            Ok(true)
        })
    }

    fn get(&self, key: &str) -> StoreFuture<'_, Option<Object>> {
        let key = key.to_owned();
        Box::pin(async move {
            self.check()?;
            self.gets.fetch_add(1, Ordering::SeqCst);
            Ok(self.objects().get(&key).cloned())
        })
    }

    fn get_range(&self, key: &str, start: u64, len: u64) -> StoreFuture<'_, Option<Vec<u8>>> {
        let key = key.to_owned();
        Box::pin(async move {
            self.check()?;
            self.gets.fetch_add(1, Ordering::SeqCst);
            Ok(self.objects().get(&key).map(|o| {
                let from = (start as usize).min(o.body.len());
                let to = (start.saturating_add(len) as usize).min(o.body.len());
                o.body[from..to].to_vec()
            }))
        })
    }

    fn get_suffix(&self, key: &str, len: u64) -> StoreFuture<'_, Option<Vec<u8>>> {
        let key = key.to_owned();
        Box::pin(async move {
            self.check()?;
            self.gets.fetch_add(1, Ordering::SeqCst);
            Ok(self.objects().get(&key).map(|o| {
                let from = o.body.len().saturating_sub(len as usize);
                o.body[from..].to_vec()
            }))
        })
    }

    fn list(&self, prefix: &str) -> StoreFuture<'_, Vec<ObjectInfo>> {
        let prefix = prefix.to_owned();
        Box::pin(async move {
            self.check()?;
            Ok(self
                .objects()
                .iter()
                .filter(|(k, _)| k.starts_with(&prefix))
                .map(|(k, o)| ObjectInfo {
                    key: k.clone(),
                    size: o.body.len() as u64,
                    etag: o.etag.clone(),
                })
                .collect())
        })
    }

    fn delete(&self, key: &str) -> StoreFuture<'_, ()> {
        let key = key.to_owned();
        Box::pin(async move {
            self.check()?;
            self.objects().remove(&key);
            Ok(())
        })
    }
}
