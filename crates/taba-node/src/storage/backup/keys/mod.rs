//! Os nomes dos objetos no bucket (`backup.s3.prefix` já vem na frente, do `Prefixed`).
//!
//! ```text
//! topics/<tópico>.json                                  definição (ou lápide)
//! tokens/<id>.json                                      token
//! nodes/<id>.json                                       batimento do nó
//! messages/<tópico>/<topicId>/<primeiroId>_<escritor>_<seq>.seg
//! journal/<tópico>/<topicId>/<instante>_<escritor>_<seq>.jrn
//! restore/lock.json                                     trava de quem restaura
//! cluster/stop.json                                     lista da parada programada
//! ```
//!
//! O `topicId` no caminho impede que as mensagens de um tópico apagado ressuscitem num tópico novo
//! com o mesmo nome. O nome do segmento começa pelo `id` da primeira mensagem (GUID v7): os nomes
//! ordenam por tempo e dão a idade para o `ttl`.

pub const TOPICS_PREFIX: &str = "topics/";
pub const TOKENS_PREFIX: &str = "tokens/";
pub const NODES_PREFIX: &str = "nodes/";
pub const MESSAGES_PREFIX: &str = "messages/";
pub const JOURNAL_PREFIX: &str = "journal/";
pub const RESTORE_LOCK: &str = "restore/lock.json";
pub const STOP_LIST: &str = "cluster/stop.json";

pub fn topic(name: &str) -> String {
    format!("{TOPICS_PREFIX}{name}.json")
}

pub fn token(id: &str) -> String {
    format!("{TOKENS_PREFIX}{id}.json")
}

pub fn node(id: &str) -> String {
    format!("{NODES_PREFIX}{id}.json")
}

/// O `id` de um `nodes/<id>.json`.
pub fn node_id_of(key: &str) -> Option<&str> {
    key.strip_prefix(NODES_PREFIX)?.strip_suffix(".json")
}

/// Tudo o que é das mensagens de um tópico (todas as encarnações dele).
pub fn topic_messages_prefix(topic: &str) -> String {
    format!("{MESSAGES_PREFIX}{topic}/")
}

pub fn topic_journal_prefix(topic: &str) -> String {
    format!("{JOURNAL_PREFIX}{topic}/")
}

pub fn segments_prefix(topic: &str, topic_id: &str) -> String {
    format!("{MESSAGES_PREFIX}{topic}/{topic_id}/")
}

pub fn journals_prefix(topic: &str, topic_id: &str) -> String {
    format!("{JOURNAL_PREFIX}{topic}/{topic_id}/")
}

pub fn segment(topic: &str, topic_id: &str, first_id: &str, writer: &str, seq: u64) -> String {
    format!(
        "{}{first_id}_{writer}_{seq:06}.seg",
        segments_prefix(topic, topic_id)
    )
}

pub fn journal(topic: &str, topic_id: &str, at_ms: u64, writer: &str, seq: u64) -> String {
    format!(
        "{}{at_ms:016}_{writer}_{seq:06}.jrn",
        journals_prefix(topic, topic_id)
    )
}

/// O `(tópico, topicId)` de um segmento ou diário.
pub fn topic_of(key: &str) -> Option<(&str, &str)> {
    let rest = key
        .strip_prefix(MESSAGES_PREFIX)
        .or_else(|| key.strip_prefix(JOURNAL_PREFIX))?;
    let mut parts = rest.splitn(3, '/');
    Some((parts.next()?, parts.next()?))
}

/// O `id` da primeira mensagem de um segmento, pelo nome.
pub fn first_id_of(segment_key: &str) -> Option<&str> {
    let name = segment_key.rsplit('/').next()?;
    name.split('_').next()
}

/// Quem gravou o segmento (o `id` do nó), pelo nome.
pub fn writer_of(segment_key: &str) -> Option<&str> {
    let name = segment_key.rsplit('/').next()?.strip_suffix(".seg")?;
    name.split('_').nth(1)
}

#[cfg(test)]
mod tests;
