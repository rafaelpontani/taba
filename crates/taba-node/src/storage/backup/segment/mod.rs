//! O formato do segmento (`.seg`) do backup (doc 23, "Backup em S3").
//!
//! ```text
//! "TABASEG1"
//! entrada*   = u32 (LE) tamanho dos metadados | metadados (JSON) | corpo
//! rodapé     = JSON { topic, topicId, writer, entries: [{ id, offset, len }] }
//! u32 (LE) tamanho do rodapé | "TABAFTR1"
//! ```
//!
//! O rodapé-índice deixa listar um segmento sem baixá-lo (`get_suffix`) e ler uma só mensagem por
//! `Range` (`offset` e `len` cobrem a entrada inteira: metadados e corpo).

use std::io;

use serde::{Deserialize, Serialize};

use crate::storage::messages::MessageMeta;

const HEADER: &[u8; 8] = b"TABASEG1";
const TRAILER: &[u8; 8] = b"TABAFTR1";
/// O que se lê do fim do objeto na primeira tentativa de achar o rodapé.
pub const TAIL_GUESS: u64 = 16 * 1024;

/// Os metadados de uma mensagem como o backup os guarda: os mesmos de [`MessageMeta`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupMessage {
    pub id: String,
    pub content_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deduplication_id: Option<String>,
    pub published_ms: u64,
    #[serde(default)]
    pub delivery_count: u32,
    #[serde(default)]
    pub invisible_until_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dead_lettered_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dead_letter_reason: Option<String>,
    #[serde(default)]
    pub position: u64,
}

impl From<&MessageMeta> for BackupMessage {
    fn from(meta: &MessageMeta) -> Self {
        Self {
            id: meta.id.clone(),
            content_type: meta.content_type.clone(),
            group_id: meta.group_id.clone(),
            deduplication_id: meta.deduplication_id.clone(),
            published_ms: meta.published_ms,
            delivery_count: meta.delivery_count,
            invisible_until_ms: meta.invisible_until_ms,
            receipt_token: meta.receipt_token.clone(),
            dead_lettered_ms: meta.dead_lettered_ms,
            dead_letter_reason: meta.dead_letter_reason.clone(),
            position: meta.position,
        }
    }
}

impl From<BackupMessage> for MessageMeta {
    fn from(m: BackupMessage) -> Self {
        Self {
            id: m.id,
            content_type: m.content_type,
            group_id: m.group_id,
            deduplication_id: m.deduplication_id,
            published_ms: m.published_ms,
            delivery_count: m.delivery_count,
            invisible_until_ms: m.invisible_until_ms,
            receipt_token: m.receipt_token,
            dead_lettered_ms: m.dead_lettered_ms,
            dead_letter_reason: m.dead_letter_reason,
            position: m.position,
        }
    }
}

/// Onde uma mensagem está dentro do segmento.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexEntry {
    pub id: String,
    pub offset: u64,
    pub len: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Footer {
    pub topic: String,
    #[serde(rename = "topicId")]
    pub topic_id: String,
    pub writer: String,
    pub entries: Vec<IndexEntry>,
}

/// Monta um segmento, mensagem por mensagem.
pub struct SegmentBuilder {
    footer: Footer,
    buf: Vec<u8>,
}

impl SegmentBuilder {
    pub fn new(topic: &str, topic_id: &str, writer: &str) -> Self {
        Self {
            footer: Footer {
                topic: topic.to_owned(),
                topic_id: topic_id.to_owned(),
                writer: writer.to_owned(),
                entries: Vec::new(),
            },
            buf: HEADER.to_vec(),
        }
    }

    /// Acrescenta a mensagem e devolve onde ela ficou.
    pub fn push(&mut self, meta: &MessageMeta, body: &[u8]) -> IndexEntry {
        let head = serde_json::to_vec(&BackupMessage::from(meta)).expect("metadados serializam");
        let offset = self.buf.len() as u64;
        self.buf
            .extend_from_slice(&(head.len() as u32).to_le_bytes());
        self.buf.extend_from_slice(&head);
        self.buf.extend_from_slice(body);
        let entry = IndexEntry {
            id: meta.id.clone(),
            offset,
            len: self.buf.len() as u64 - offset,
        };
        self.footer.entries.push(entry.clone());
        entry
    }

    pub fn count(&self) -> usize {
        self.footer.entries.len()
    }

    /// Os bytes das entradas até agora (sem o rodapé).
    pub fn bytes(&self) -> usize {
        self.buf.len()
    }

    pub fn first_id(&self) -> Option<&str> {
        self.footer.entries.first().map(|e| e.id.as_str())
    }

    pub fn entries(&self) -> &[IndexEntry] {
        &self.footer.entries
    }

    /// Fecha o segmento: as entradas, o rodapé e o tamanho do rodapé.
    pub fn finish(mut self) -> Vec<u8> {
        let footer = serde_json::to_vec(&self.footer).expect("rodapé serializa");
        self.buf.extend_from_slice(&footer);
        self.buf
            .extend_from_slice(&(footer.len() as u32).to_le_bytes());
        self.buf.extend_from_slice(TRAILER);
        self.buf
    }
}

fn invalid(what: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("segmento inválido: {what}"),
    )
}

/// O que `read_footer` decide a partir do fim do objeto.
#[derive(Debug, PartialEq, Eq)]
pub enum FooterRead {
    Done(Footer),
    /// O trecho lido é curto: o rodapé inteiro tem este tamanho (com o fim), leia de novo.
    NeedMore(u64),
}

/// Lê o rodapé do fim do segmento (`tail` são os últimos bytes do objeto).
pub fn read_footer(tail: &[u8]) -> io::Result<FooterRead> {
    if tail.len() < 12 || &tail[tail.len() - 8..] != TRAILER {
        return Err(invalid("sem o fim esperado"));
    }
    let size_at = tail.len() - 12;
    let footer_len = u32::from_le_bytes(tail[size_at..size_at + 4].try_into().expect("4 bytes"));
    let needed = u64::from(footer_len) + 12;
    if needed > tail.len() as u64 {
        return Ok(FooterRead::NeedMore(needed));
    }
    let body = &tail[size_at - footer_len as usize..size_at];
    serde_json::from_slice(body)
        .map(FooterRead::Done)
        .map_err(|e| invalid(&format!("rodapé: {e}")))
}

/// Lê uma entrada (metadados e corpo), como `offset` e `len` do rodapé a delimitam.
pub fn decode_entry(entry: &[u8]) -> io::Result<(BackupMessage, Vec<u8>)> {
    if entry.len() < 4 {
        return Err(invalid("entrada curta"));
    }
    let head_len = u32::from_le_bytes(entry[..4].try_into().expect("4 bytes")) as usize;
    if entry.len() < 4 + head_len {
        return Err(invalid("entrada truncada"));
    }
    let meta = serde_json::from_slice(&entry[4..4 + head_len])
        .map_err(|e| invalid(&format!("metadados: {e}")))?;
    Ok((meta, entry[4 + head_len..].to_vec()))
}

/// Uma entrada lida do segmento: os metadados e o corpo.
pub type Entry = (BackupMessage, Vec<u8>);

/// Lê o segmento inteiro: o rodapé e todas as entradas.
pub fn decode_all(whole: &[u8]) -> io::Result<(Footer, Vec<Entry>)> {
    if whole.len() < HEADER.len() || &whole[..HEADER.len()] != HEADER {
        return Err(invalid("sem o começo esperado"));
    }
    let footer = match read_footer(whole)? {
        FooterRead::Done(footer) => footer,
        FooterRead::NeedMore(_) => return Err(invalid("rodapé truncado")),
    };
    let mut messages = Vec::with_capacity(footer.entries.len());
    for entry in &footer.entries {
        let (from, to) = (entry.offset as usize, (entry.offset + entry.len) as usize);
        if to > whole.len() {
            return Err(invalid("entrada fora do objeto"));
        }
        messages.push(decode_entry(&whole[from..to])?);
    }
    Ok((footer, messages))
}

#[cfg(test)]
mod tests;
