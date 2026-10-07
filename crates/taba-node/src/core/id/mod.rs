//! Identificadores: GUID v7 em minúsculas, na forma canônica com hífens
//! (doc 06d, seção 8.3.1). Vale para tópico, mensagem, token e nó.

use uuid::{Timestamp, Uuid};

/// Gera um GUID v7 com o instante dado (milissegundos desde a época Unix).
pub fn new_at(unix_millis: u64) -> String {
    let secs = unix_millis / 1000;
    let nanos = ((unix_millis % 1000) * 1_000_000) as u32;
    Uuid::new_v7(Timestamp::from_unix(uuid::NoContext, secs, nanos)).to_string()
}

/// `true` se `s` é um GUID v7 na forma canônica, em minúsculas.
pub fn is_guid_v7(s: &str) -> bool {
    s == s.to_ascii_lowercase()
        && s.len() == 36
        && Uuid::parse_str(s).is_ok_and(|u| u.get_version_num() == 7)
}

/// O instante (ms desde a época Unix) gravado num GUID v7. A idade de uma
/// mensagem, para o `ttl`, vem daqui (doc 13, seção 12.10).
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
