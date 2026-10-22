//! The access token (doc 06a, section 8.1.3): the `tb_<id>.<secret>` format, the permissions, the
//! stored record and the salted hash. The full secret only exists in the creation response; the
//! node stores the secret's **salted hash**, never the secret.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use sha2::{Digest, Sha256};

const PREFIX: &str = "tb_";
/// 256 random bits (doc 06a).
const SECRET_BYTES: usize = 32;
const SALT_BYTES: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Permission {
    Publish,
    Consume,
    Create,
    /// Changes and deletes topics and empties queues, inside the token's prefixes. Includes `create`.
    /// Until the schema 2 of the permissions this was called `admin`.
    Manage,
    /// Manages the tokens and operates the nodes and the cluster (what only the root token did), with no prefixes.
    /// Includes `manage` and `create`.
    Admin,
}

/// The version of the permission names: schema 1 called the topic manager `admin`; schema 2 calls it `manage`
/// and `admin` became the general administrator. A stored or transmitted token without a version is schema 1.
pub const PERMISSIONS_SCHEMA: u64 = 2;

impl Permission {
    pub const ALL: [Permission; 5] = [
        Permission::Publish,
        Permission::Consume,
        Permission::Create,
        Permission::Manage,
        Permission::Admin,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Permission::Publish => "publish",
            Permission::Consume => "consume",
            Permission::Create => "create",
            Permission::Manage => "manage",
            Permission::Admin => "admin",
        }
    }

    /// Reads a name of schema 1: its `admin` is today's `manage`.
    pub fn parse_legacy(text: &str) -> Option<Self> {
        match text {
            "admin" => Some(Permission::Manage),
            other => Self::parse(other),
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.as_str() == text)
    }
}

/// A stored token: everything but the secret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenRecord {
    pub id: String,
    pub name: String,
    /// No repetition, in the order of [`Permission::ALL`].
    pub permissions: Vec<Permission>,
    pub topic_prefixes: Vec<String>,
    pub created_ms: u64,
    pub expires_ms: Option<u64>,
    /// A random 128-bit salt, in unpadded base64.
    pub salt: String,
    /// `SHA-256(salt ‖ secret)`, in unpadded base64.
    pub secret_hash: String,
}

impl TokenRecord {
    /// `manage` includes `create`; `admin` includes `manage` and `create` (doc 06a).
    pub fn allows(&self, permission: Permission) -> bool {
        let has = |p: Permission| self.permissions.contains(&p);
        has(permission)
            || (matches!(
                permission,
                Permission::Create | Permission::Publish | Permission::Consume
            ) && (has(Permission::Manage) || has(Permission::Admin)))
            || (permission == Permission::Manage && has(Permission::Admin))
    }

    /// The token reaches the topic: with no prefixes, all of them; otherwise the name **starts with** one of them.
    pub fn reaches(&self, topic: &str) -> bool {
        reaches(&self.topic_prefixes, topic)
    }

    pub fn expired_at(&self, now_ms: u64) -> bool {
        self.expires_ms.is_some_and(|at| at <= now_ms)
    }

    /// Is the presented secret this token's? Compares the hash in constant time.
    pub fn matches_secret(&self, secret: &str) -> bool {
        let Ok(salt) = URL_SAFE_NO_PAD.decode(&self.salt) else {
            return false;
        };
        let Ok(stored) = URL_SAFE_NO_PAD.decode(&self.secret_hash) else {
            return false;
        };
        constant_time_eq(&hash(&salt, secret), &stored)
    }
}

/// With no prefixes it reaches everything; otherwise the name starts with one.
pub fn reaches(prefixes: &[String], topic: &str) -> bool {
    prefixes.is_empty() || prefixes.iter().any(|p| topic.starts_with(p.as_str()))
}

fn hash(salt: &[u8], secret: &str) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(salt);
    hasher.update(secret.as_bytes());
    hasher.finalize().to_vec()
}

/// Comparison that does not exit at the first differing byte.
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn random<const N: usize>() -> [u8; N] {
    let mut bytes = [0u8; N];
    getrandom::fill(&mut bytes).expect("the system has no source of random numbers");
    bytes
}

/// What creating a token produces: the record (with salt and hash) and the full token, which the
/// caller hands over only once.
pub struct NewToken {
    pub record: TokenRecord,
    /// `tb_<id>.<segredo>`.
    pub token: String,
}

/// Generates the secret and the salt, and stores only the salted hash in the record.
pub fn generate(
    id: String,
    name: String,
    permissions: Vec<Permission>,
    topic_prefixes: Vec<String>,
    created_ms: u64,
    expires_ms: Option<u64>,
) -> NewToken {
    let secret = URL_SAFE_NO_PAD.encode(random::<SECRET_BYTES>());
    let salt = random::<SALT_BYTES>();
    let token = format!("{PREFIX}{id}.{secret}");
    let record = TokenRecord {
        id,
        name,
        permissions,
        topic_prefixes,
        created_ms,
        expires_ms,
        salt: URL_SAFE_NO_PAD.encode(salt),
        secret_hash: URL_SAFE_NO_PAD.encode(hash(&salt, &secret)),
    };
    NewToken { record, token }
}

/// A record for a secret that is not random: the `auth.dev-token` (doc 37, section 4.16). The presented token is
/// `tb_<id>.<secret>`; the record keeps only the salted hash, like any other.
pub fn with_secret(
    id: String,
    name: String,
    permissions: Vec<Permission>,
    created_ms: u64,
    secret: &str,
) -> TokenRecord {
    let salt = random::<SALT_BYTES>();
    TokenRecord {
        id,
        name,
        permissions,
        topic_prefixes: Vec::new(),
        created_ms,
        expires_ms: None,
        salt: URL_SAFE_NO_PAD.encode(salt),
        secret_hash: URL_SAFE_NO_PAD.encode(hash(&salt, secret)),
    }
}

/// Splits `tb_<id>.<secret>` into `(id, secret)`. The `id` is a GUID (no dot), so the
/// first dot separates. Malformed: `None`.
pub fn parse(token: &str) -> Option<(&str, &str)> {
    let rest = token.strip_prefix(PREFIX)?;
    let (id, secret) = rest.split_once('.')?;
    (!id.is_empty() && !secret.is_empty()).then_some((id, secret))
}

#[cfg(test)]
mod tests;
