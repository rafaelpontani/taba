//! The backup's object store (doc 23, "S3 backup"): the [`ObjectStore`] trait, the in-memory
//! store of the tests ([`MemoryStore`]), the S3 client ([`S3Store`]) and the base prefix
//! ([`Prefixed`]).
//!
//! S3 is **optional** and never enters the publication path: whoever uses the store treats each error
//! as "the backup is down" and carries on.

mod memory;
mod s3;

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

pub use memory::MemoryStore;
pub use s3::{S3Settings, S3Store};

/// The error of a call to the store.
#[derive(Debug, Clone, thiserror::Error)]
pub enum StoreError {
    /// The store did not answer (network, credentials, service down).
    #[error("the object store did not answer: {0}")]
    Unavailable(String),
    /// The store does not support conditional writes (`If-None-Match`, `If-Match`).
    #[error("the store does not support conditional writes: {0}")]
    Unsupported(String),
}

pub type StoreResult<T> = Result<T, StoreError>;
pub type StoreFuture<'a, T> = Pin<Box<dyn Future<Output = StoreResult<T>> + Send + 'a>>;

/// What is known about an object without downloading it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectInfo {
    pub key: String,
    pub size: u64,
    /// Changes on every write with different content: the nodes' heartbeat depends on this.
    pub etag: String,
}

/// A downloaded object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Object {
    pub body: Vec<u8>,
    pub etag: String,
}

/// What the backup asks of S3 (or MinIO). All the keys are relative to the base prefix.
pub trait ObjectStore: Send + Sync + 'static {
    /// Writes (or replaces) the object and returns the new `etag`.
    fn put(&self, key: &str, body: Vec<u8>) -> StoreFuture<'_, String>;

    /// Creates the object only if it does not exist yet (`If-None-Match: *`). `false`: it already existed.
    fn put_if_absent(&self, key: &str, body: Vec<u8>) -> StoreFuture<'_, bool>;

    /// Replaces the object only if the current `etag` is `etag` (`If-Match`). `false`: it changed or vanished.
    fn put_if_match(&self, key: &str, body: Vec<u8>, etag: &str) -> StoreFuture<'_, bool>;

    fn get(&self, key: &str) -> StoreFuture<'_, Option<Object>>;

    /// `len` bytes from `start` (`Range`). `None`: the object does not exist. The end past the
    /// size is no error.
    fn get_range(&self, key: &str, start: u64, len: u64) -> StoreFuture<'_, Option<Vec<u8>>>;

    /// The last `len` bytes of the object (to read a segment's footer without downloading it).
    fn get_suffix(&self, key: &str, len: u64) -> StoreFuture<'_, Option<Vec<u8>>>;

    /// The objects whose name starts with `prefix`, in name order.
    fn list(&self, prefix: &str) -> StoreFuture<'_, Vec<ObjectInfo>>;

    /// Deletes the object. Deleting what does not exist is no error.
    fn delete(&self, key: &str) -> StoreFuture<'_, ()>;
}

/// Puts a base prefix (`backup.s3.prefix`) in front of every key.
pub struct Prefixed {
    inner: Arc<dyn ObjectStore>,
    prefix: String,
}

impl Prefixed {
    /// `prefix` without a slash at the start and end; empty, it changes nothing.
    pub fn new(inner: Arc<dyn ObjectStore>, prefix: &str) -> Self {
        let prefix = prefix.trim_matches('/');
        Self {
            inner,
            prefix: if prefix.is_empty() {
                String::new()
            } else {
                format!("{prefix}/")
            },
        }
    }

    fn full(&self, key: &str) -> String {
        format!("{}{key}", self.prefix)
    }
}

impl ObjectStore for Prefixed {
    fn put(&self, key: &str, body: Vec<u8>) -> StoreFuture<'_, String> {
        self.inner.put(&self.full(key), body)
    }

    fn put_if_absent(&self, key: &str, body: Vec<u8>) -> StoreFuture<'_, bool> {
        self.inner.put_if_absent(&self.full(key), body)
    }

    fn put_if_match(&self, key: &str, body: Vec<u8>, etag: &str) -> StoreFuture<'_, bool> {
        self.inner.put_if_match(&self.full(key), body, etag)
    }

    fn get(&self, key: &str) -> StoreFuture<'_, Option<Object>> {
        self.inner.get(&self.full(key))
    }

    fn get_range(&self, key: &str, start: u64, len: u64) -> StoreFuture<'_, Option<Vec<u8>>> {
        self.inner.get_range(&self.full(key), start, len)
    }

    fn get_suffix(&self, key: &str, len: u64) -> StoreFuture<'_, Option<Vec<u8>>> {
        self.inner.get_suffix(&self.full(key), len)
    }

    fn list(&self, prefix: &str) -> StoreFuture<'_, Vec<ObjectInfo>> {
        let strip = self.prefix.len();
        let fut = self.inner.list(&self.full(prefix));
        Box::pin(async move {
            let mut items = fut.await?;
            for item in &mut items {
                item.key = item.key[strip..].to_owned();
            }
            Ok(items)
        })
    }

    fn delete(&self, key: &str) -> StoreFuture<'_, ()> {
        self.inner.delete(&self.full(key))
    }
}

#[cfg(test)]
mod tests;
