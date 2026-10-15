//! Memory, disk overflow, per-topic SQLite, `sync.db` and `_node.db` (doc 02, section 3.2): the
//! node database (`db`), the topic definitions (`topics`) and the messages on disk (`messages`).
//!
//! The traits live here, in the lowest layer the `engine` needs.

pub mod backup;
pub mod db;
pub mod losses;
pub mod messages;
pub mod state;
pub mod synclog;
pub mod tokens;
pub mod topics;

use std::io;
use std::path::Path;

/// `true` for the disk error that brings the node down (doc 05, section 7.3): full disk or
/// corrupt database. The others (a file that vanished, for example) only apply to the operation.
pub fn is_fatal(e: &io::Error) -> bool {
    use rusqlite::ErrorCode;
    if e.kind() == io::ErrorKind::StorageFull {
        return true;
    }
    e.get_ref()
        .and_then(|inner| inner.downcast_ref::<rusqlite::Error>())
        .is_some_and(|inner| {
            matches!(
                inner.sqlite_error_code(),
                Some(ErrorCode::DiskFull | ErrorCode::DatabaseCorrupt | ErrorCode::NotADatabase)
            )
        })
}

/// Deletes what is **inside** `dir`, keeping the folder. It is `development.resume-on-start=false`
/// (doc 10): the node starts clean.
pub fn wipe_dir_contents(dir: &Path) -> io::Result<()> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    for entry in entries {
        let path = entry?.path();
        if path.is_dir() {
            std::fs::remove_dir_all(&path)?;
        } else {
            std::fs::remove_file(&path)?;
        }
    }
    Ok(())
}

/// Creates `storage.dir` if it does not exist and checks that it can be written to. If not,
/// the node does not start (doc 10, `storage.dir`).
pub fn prepare_dir(dir: &Path) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let probe = dir.join(".taba-write-probe");
    std::fs::write(&probe, b"")?;
    std::fs::remove_file(&probe)
}

#[cfg(test)]
mod tests;
