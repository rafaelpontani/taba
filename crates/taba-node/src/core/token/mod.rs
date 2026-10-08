//! O token de acesso (doc 06a, seção 8.1.3): o formato `tb_<id>.<segredo>`, as permissões, o
//! registro guardado e o hash com sal. O segredo completo só existe na resposta da criação; o
//! nó guarda o **hash salgado** do segredo, nunca o segredo.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use sha2::{Digest, Sha256};

const PREFIX: &str = "tb_";
/// 256 bits aleatórios (doc 06a).
const SECRET_BYTES: usize = 32;
const SALT_BYTES: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Permission {
    Publish,
    Consume,
    Create,
    Admin,
}

impl Permission {
    pub const ALL: [Permission; 4] = [
        Permission::Publish,
        Permission::Consume,
        Permission::Create,
        Permission::Admin,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Permission::Publish => "publish",
            Permission::Consume => "consume",
            Permission::Create => "create",
            Permission::Admin => "admin",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.as_str() == text)
    }
}

/// Um token guardado: tudo menos o segredo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenRecord {
    pub id: String,
    pub name: String,
    /// Sem repetição, na ordem de [`Permission::ALL`].
    pub permissions: Vec<Permission>,
    pub topic_prefixes: Vec<String>,
    pub created_ms: u64,
    pub expires_ms: Option<u64>,
    /// Sal aleatório de 128 bits, em base64 sem preenchimento.
    pub salt: String,
    /// `SHA-256(sal ‖ segredo)`, em base64 sem preenchimento.
    pub secret_hash: String,
}

impl TokenRecord {
    /// `admin` inclui `create` (doc 06a).
    pub fn allows(&self, permission: Permission) -> bool {
        self.permissions.contains(&permission)
            || (permission == Permission::Create && self.permissions.contains(&Permission::Admin))
    }

    /// O token alcança o tópico: sem prefixos, todos; senão o nome **começa com** algum deles.
    pub fn reaches(&self, topic: &str) -> bool {
        reaches(&self.topic_prefixes, topic)
    }

    pub fn expired_at(&self, now_ms: u64) -> bool {
        self.expires_ms.is_some_and(|at| at <= now_ms)
    }

    /// O segredo apresentado é o deste token? Compara o hash em tempo constante.
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

/// Sem prefixos alcança tudo; senão o nome começa com algum.
pub fn reaches(prefixes: &[String], topic: &str) -> bool {
    prefixes.is_empty() || prefixes.iter().any(|p| topic.starts_with(p.as_str()))
}

fn hash(salt: &[u8], secret: &str) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(salt);
    hasher.update(secret.as_bytes());
    hasher.finalize().to_vec()
}

/// Comparação sem sair no primeiro byte diferente.
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn random<const N: usize>() -> [u8; N] {
    let mut bytes = [0u8; N];
    getrandom::fill(&mut bytes).expect("o sistema não tem fonte de números aleatórios");
    bytes
}

/// O que a criação de um token produz: o registro (com sal e hash) e o token completo, que o
/// chamador entrega uma única vez.
pub struct NewToken {
    pub record: TokenRecord,
    /// `tb_<id>.<segredo>`.
    pub token: String,
}

/// Gera o segredo e o sal, e guarda só o hash salgado no registro.
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

/// Separa `tb_<id>.<segredo>` em `(id, segredo)`. O `id` é um GUID (sem ponto), então o
/// primeiro ponto separa. Mal formado: `None`.
pub fn parse(token: &str) -> Option<(&str, &str)> {
    let rest = token.strip_prefix(PREFIX)?;
    let (id, secret) = rest.split_once('.')?;
    (!id.is_empty() && !secret.is_empty()).then_some((id, secret))
}

#[cfg(test)]
mod tests;
