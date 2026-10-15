//! What the optional S3 backup needs from storage (doc 23, "S3 backup"): the object
//! store (`store`), the names in the bucket (`keys`), the segment (`segment`) and journal
//! (`journal`) format, and the local `_backup.db` (`db`).
//!
//! Nothing here talks to the cluster or to the message engine: coordination lives in
//! `cluster::backup`. S3 is only a backup for serious failure; SQLite and memory remain the
//! primary storage, and no publication waits for S3.

pub mod db;
pub mod journal;
pub mod keys;
pub mod segment;
pub mod store;

pub use db::{BackupDb, Committed, Dirty, SavedRef, SegmentRow};
pub use store::{
    MemoryStore, Object, ObjectInfo, ObjectStore, Prefixed, S3Settings, S3Store, StoreError,
    StoreResult,
};
