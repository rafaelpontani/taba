//! The credentials file of the AWS gateways (`gateway.aws.credentials-file`, doc 39).
//!
//! SigV4 never sends the secret, so the node can only check a signature if it knows it. The Taba tokens keep a salted
//! hash of the secret and cannot be used for that: the file maps an `accessKeyId` to a `secret` (what the AWS SDK
//! signs with) and to the Taba `token` whose permissions and prefixes apply to the calls of that key.
//!
//! ```json
//! { "AKIAEXAMPLE": { "secret": "wJalrXUtnFEMI...", "token": "tb_1a2b.xxxxxxxx" } }
//! ```
//!
//! The file is read again every `auth.reload-interval`; if the new content is invalid, the previous one stays.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use serde::Deserialize;

use crate::config::Secret;

/// What the file says about one access key.
#[derive(Clone)]
pub struct Entry {
    pub secret: Secret,
    /// The Taba token the gateway presents to the client API for this key.
    pub token: Secret,
}

#[derive(Deserialize)]
struct RawEntry {
    secret: String,
    token: String,
}

/// The access keys the gateway knows.
pub struct Credentials {
    file: Option<PathBuf>,
    entries: RwLock<HashMap<String, Entry>>,
}

/// Reads the text of the file. Every entry needs a non-empty key, secret and token.
pub fn parse(text: &str) -> Result<HashMap<String, Entry>, String> {
    let raw: HashMap<String, RawEntry> =
        serde_json::from_str(text).map_err(|e| format!("not a valid credentials file: {e}"))?;
    let mut entries = HashMap::with_capacity(raw.len());
    for (key, entry) in raw {
        if key.trim().is_empty() || entry.secret.is_empty() || entry.token.is_empty() {
            return Err(format!(
                "the access key `{key}` needs a non-empty key, secret and token"
            ));
        }
        entries.insert(
            key,
            Entry {
                secret: Secret::new(entry.secret),
                token: Secret::new(entry.token),
            },
        );
    }
    Ok(entries)
}

impl Credentials {
    /// No file: no key is known (with the Taba authentication on, every call is refused).
    pub fn none() -> Self {
        Self {
            file: None,
            entries: RwLock::new(HashMap::new()),
        }
    }

    /// Opens the file; an unreadable or invalid one is an error at the start, never a silent empty set.
    pub fn open(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("could not read `{}`: {e}", path.display()))?;
        let entries = parse(&text).map_err(|e| format!("`{}`: {e}", path.display()))?;
        Ok(Self {
            file: Some(path.to_owned()),
            entries: RwLock::new(entries),
        })
    }

    pub fn has_file(&self) -> bool {
        self.file.is_some()
    }

    /// Reads the file again. If it vanished or became invalid, the previous keys keep applying.
    pub fn reload(&self) {
        let Some(path) = &self.file else { return };
        let loaded = std::fs::read_to_string(path)
            .map_err(|e| e.to_string())
            .and_then(|text| parse(&text));
        match loaded {
            Ok(entries) => {
                *self.entries.write().unwrap_or_else(|e| e.into_inner()) = entries;
            }
            Err(why) => tracing::warn!(
                file = %path.display(),
                "could not re-read gateway.aws.credentials-file ({why}); the previous keys keep applying"
            ),
        }
    }

    pub fn lookup(&self, access_key: &str) -> Option<Entry> {
        self.entries
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(access_key)
            .cloned()
    }

    pub fn len(&self) -> usize {
        self.entries.read().unwrap_or_else(|e| e.into_inner()).len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests;
