//! The engine's attachment point with the optional S3 backup (doc 23, "S3 backup").
//!
//! The engine only **notifies** what happened to each message, cheaply and without ever waiting for S3:
//! whoever implements it ([`BackupSink`], in `cluster::backup`) writes a row to the local `_backup.db`. Without a
//! backup configured, no notice is given.

/// What the engine tells the backup. Called with the topic lock: it must be fast and never fail
/// for the caller (a backup error becomes a log, never a publication error).
pub trait BackupSink: Send + Sync + 'static {
    /// This copy of the message was stored on the node (publication, restore, recovery). `owned`: the
    /// node is the owner (primary of the block), the only one that sends to S3.
    fn stored(&self, topic: &str, id: &str, owned: bool);

    /// The message died on the node: ack, `ttl` or exit to the dead-letter. `owned`: it was the owner.
    fn removed(&self, topic: &str, id: &str, owned: bool);

    /// The message's mutable state changed (delivery, invisibility).
    fn changed(&self, topic: &str, id: &str, owned: bool);

    /// The node became the message's owner (member change): if it is not in S3 yet,
    /// it enters the send queue.
    fn adopted(&self, topic: &str, id: &str);

    /// The node stopped being the owner: its send queue forgets the message (the new owner sends it).
    fn released(&self, topic: &str, id: &str);

    /// The message body is not on disk (the eviction deleted it): fetches from S3 by `Range`. `None`
    /// without backup, without a reference or with S3 down.
    fn fetch_body(&self, topic: &str, id: &str) -> Option<Vec<u8>>;

    /// Called before each memory-to-disk pass in the drain and flush: if the disk
    /// reached the pause limit, it waits for the eviction to free space (up to the backup's limit). Returns
    /// `false` when it gave up waiting.
    fn before_disk_write(&self) -> bool;
}
