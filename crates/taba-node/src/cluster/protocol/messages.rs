//! Os corpos das rotas de mensagem entre nós (doc 06c, "Mensagens"): a escrita, a
//! invisibilidade, o ack e o `take`, mais o `MessageRecord`, que é a mensagem como os nós a
//! trocam.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::value::RawValue;

use crate::core::content::{BodyKind, ContentType};
use crate::core::error::{ErrorCode, TabaError};
use crate::core::id;
use crate::core::time;
use crate::storage::messages::MessageMeta;

/// A mensagem **completa** entre nós: o conteúdo e os metadados (`MessageRecord`, doc 06c).
///
/// O corpo guarda os bytes. Na rede ele sai em JSON nativo quando o `contentType` é JSON
/// (`application/json` ou `*/*+json`) e, nos demais casos (texto e binário), em base64 numa string
/// com `bodyEncoding: "base64"`: entre nós não há a forma de texto da API de cliente, para o
/// limite de linha ser previsível.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageRecord {
    pub id: String,
    pub topic: String,
    pub body: Vec<u8>,
    pub content_type: String,
    pub group_id: Option<String>,
    pub deduplication_id: Option<String>,
    pub delivery_count: u32,
    /// `None` ou no passado: pronta.
    pub invisible_until_ms: Option<u64>,
    pub receipt_token: Option<String>,
    pub dead_lettered_ms: Option<u64>,
    pub dead_letter_reason: Option<String>,
    /// Só na fila FIFO: a linha curta de uma mensagem confirmada (sem corpo), que vale para a
    /// deduplicação até esta data (doc 17, seção 16.7).
    pub delete_at_ms: Option<u64>,
}

impl MessageRecord {
    /// O instante da publicação, que o `id` (GUID v7) carrega.
    pub fn published_ms(&self) -> u64 {
        id::unix_millis_of(&self.id).unwrap_or(0)
    }

    /// A mensagem como o nó a guarda. A posição no anel fica `0`: quem grava a calcula.
    pub fn to_meta(&self) -> MessageMeta {
        MessageMeta {
            id: self.id.clone(),
            content_type: self.content_type.clone(),
            group_id: self.group_id.clone(),
            deduplication_id: self.deduplication_id.clone(),
            published_ms: self.published_ms(),
            delivery_count: self.delivery_count,
            invisible_until_ms: self.invisible_until_ms.unwrap_or(0),
            receipt_token: self.receipt_token.clone(),
            dead_lettered_ms: self.dead_lettered_ms,
            dead_letter_reason: self.dead_letter_reason.clone(),
            position: 0,
        }
    }

    /// O inverso de [`to_meta`](Self::to_meta): a mensagem de um tópico com o corpo.
    pub fn from_stored(topic: &str, meta: MessageMeta, body: Vec<u8>) -> Self {
        Self {
            id: meta.id,
            topic: topic.to_owned(),
            body,
            content_type: meta.content_type,
            group_id: meta.group_id,
            deduplication_id: meta.deduplication_id,
            delivery_count: meta.delivery_count,
            invisible_until_ms: (meta.invisible_until_ms > 0).then_some(meta.invisible_until_ms),
            receipt_token: meta.receipt_token,
            dead_lettered_ms: meta.dead_lettered_ms,
            dead_letter_reason: meta.dead_letter_reason,
            delete_at_ms: None,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RecordWire {
    id: String,
    topic: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    body: Option<Box<RawValue>>,
    content_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    body_encoding: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    group_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    deduplication_id: Option<String>,
    delivery_count: u32,
    #[serde(default)]
    invisible_until: Option<String>,
    #[serde(default)]
    receipt_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    dead_lettered_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    dead_letter_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    delete_at: Option<String>,
}

impl Serialize for MessageRecord {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let json_body = ContentType::parse(&self.content_type)
            .is_ok_and(|ct| ct.kind() == BodyKind::Json)
            .then(|| std::str::from_utf8(&self.body).ok())
            .flatten()
            // Só vai como JSON nativo se o texto sobrevive intacto (sem mexer nos bytes).
            .and_then(|text| RawValue::from_string(text.to_owned()).ok())
            .filter(|raw| raw.get().as_bytes() == self.body.as_slice());
        let (body, body_encoding) = match json_body {
            // A linha curta da FIFO não tem corpo.
            _ if self.delete_at_ms.is_some() => (None, None),
            Some(raw) => (Some(raw), None),
            None => {
                let text = serde_json::to_string(&STANDARD.encode(&self.body))
                    .map_err(serde::ser::Error::custom)?;
                (
                    Some(RawValue::from_string(text).map_err(serde::ser::Error::custom)?),
                    Some("base64".to_owned()),
                )
            }
        };
        RecordWire {
            id: self.id.clone(),
            topic: self.topic.clone(),
            body,
            content_type: self.content_type.clone(),
            body_encoding,
            group_id: self.group_id.clone(),
            deduplication_id: self.deduplication_id.clone(),
            delivery_count: self.delivery_count,
            invisible_until: self.invisible_until_ms.map(time::format_ms),
            receipt_token: self.receipt_token.clone(),
            dead_lettered_at: self.dead_lettered_ms.map(time::format_ms),
            dead_letter_reason: self.dead_letter_reason.clone(),
            delete_at: self.delete_at_ms.map(time::format_ms),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for MessageRecord {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = RecordWire::deserialize(deserializer)?;
        let date = |field: &str, text: Option<String>| -> Result<Option<u64>, D::Error> {
            text.map(|t| {
                time::parse_ms(&t).ok_or_else(|| D::Error::custom(format!("`{field}` inválido")))
            })
            .transpose()
        };
        let body = match (wire.body, wire.body_encoding.as_deref()) {
            (None, _) => Vec::new(),
            (Some(raw), Some("base64")) => {
                let text: String = serde_json::from_str(raw.get())
                    .map_err(|_| D::Error::custom("`body` em base64 deve ser uma string"))?;
                STANDARD
                    .decode(text)
                    .map_err(|_| D::Error::custom("`body` não é base64 válido"))?
            }
            (Some(_), Some(other)) => {
                return Err(D::Error::custom(format!(
                    "bodyEncoding `{other}` desconhecido"
                )));
            }
            (Some(raw), None) => raw.get().as_bytes().to_vec(),
        };
        Ok(Self {
            id: wire.id,
            topic: wire.topic,
            body,
            content_type: wire.content_type,
            group_id: wire.group_id,
            deduplication_id: wire.deduplication_id,
            delivery_count: wire.delivery_count,
            invisible_until_ms: date("invisibleUntil", wire.invisible_until)?,
            receipt_token: wire.receipt_token,
            dead_lettered_ms: date("deadLetteredAt", wire.dead_lettered_at)?,
            dead_letter_reason: wire.dead_letter_reason,
            delete_at_ms: date("deleteAt", wire.delete_at)?,
        })
    }
}

/// `kind` do `register` da escrita.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WriteKind {
    Publish,
    DeadLetter,
}

/// `MessageRegisterRequest` (doc 06c): a fase 1 da publicação e do movimento para a dead-letter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MessageRegisterRequest {
    pub operation_id: String,
    pub coordinator_id: String,
    pub kind: WriteKind,
    pub message: MessageRecord,
    /// Só em `dead-letter`: a fila de origem, de onde a mensagem sai no commit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_topic: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VisibilityKind {
    Delivery,
    Change,
}

/// `VisibilityRegisterRequest` (doc 06c): o prazo é um instante **absoluto** calculado pelo dono.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisibilityRegisterRequest {
    pub operation_id: String,
    pub coordinator_id: String,
    pub kind: VisibilityKind,
    pub invisible_until_ms: u64,
    /// Em `delivery`, o token **novo**; em `change`, o que o cliente enviou.
    pub receipt_token: String,
    /// Só em `delivery`: o contador novo.
    pub delivery_count: Option<u32>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct VisibilityWire {
    operation_id: String,
    coordinator_id: String,
    kind: VisibilityKind,
    invisible_until: String,
    receipt_token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    delivery_count: Option<u32>,
}

impl Serialize for VisibilityRegisterRequest {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        VisibilityWire {
            operation_id: self.operation_id.clone(),
            coordinator_id: self.coordinator_id.clone(),
            kind: self.kind,
            invisible_until: time::format_ms(self.invisible_until_ms),
            receipt_token: self.receipt_token.clone(),
            delivery_count: self.delivery_count,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for VisibilityRegisterRequest {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = VisibilityWire::deserialize(deserializer)?;
        Ok(Self {
            operation_id: wire.operation_id,
            coordinator_id: wire.coordinator_id,
            kind: wire.kind,
            invisible_until_ms: time::parse_ms(&wire.invisible_until)
                .ok_or_else(|| D::Error::custom("`invisibleUntil` inválido"))?,
            receipt_token: wire.receipt_token,
            delivery_count: wire.delivery_count,
        })
    }
}

/// `AckRegisterRequest` (doc 06c).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AckRegisterRequest {
    pub operation_id: String,
    pub coordinator_id: String,
    pub receipt_token: String,
    /// Só na fila FIFO.
    pub delete_at_ms: Option<u64>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AckWire {
    operation_id: String,
    coordinator_id: String,
    receipt_token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    delete_at: Option<String>,
}

impl Serialize for AckRegisterRequest {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        AckWire {
            operation_id: self.operation_id.clone(),
            coordinator_id: self.coordinator_id.clone(),
            receipt_token: self.receipt_token.clone(),
            delete_at: self.delete_at_ms.map(time::format_ms),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for AckRegisterRequest {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = AckWire::deserialize(deserializer)?;
        Ok(Self {
            operation_id: wire.operation_id,
            coordinator_id: wire.coordinator_id,
            receipt_token: wire.receipt_token,
            delete_at_ms: wire
                .delete_at
                .map(|t| time::parse_ms(&t).ok_or_else(|| D::Error::custom("`deleteAt` inválido")))
                .transpose()?,
        })
    }
}

/// `TakeRequest` (doc 06c): a quantidade que ainda falta ao `GET` do cliente e o
/// `visibilityTimeout` que ele pediu.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TakeRequest {
    pub max_messages: u32,
    pub visibility_timeout: u64,
}

/// `TakeResponse` (doc 06c). Lista vazia: a dica de que havia mensagens estava velha, ou o nó está
/// em drain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TakeResponse {
    pub messages: Vec<MessageRecord>,
}

/// `FifoPublished` (doc 06c, `POST /cluster/topics/{topic}/fifo/publish`): o primário do grupo
/// responde com o `id` da mensagem criada, ou o da original numa duplicata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FifoPublished {
    pub id: String,
    pub duplicate: bool,
}

/// O `id` do `MessageRecord` de uma publicação FIFO repassada: o primário é quem gera o verdadeiro.
pub const FIFO_PLACEHOLDER_ID: &str = "00000000-0000-7000-8000-000000000000";

/// Os campos obrigatórios de um GUID v7 do `id` da mensagem na URL.
pub fn validate_message_path(topic: &str, id: &str) -> Result<(), TabaError> {
    if topic.is_empty() || !crate::core::id::is_guid_v7(id) {
        return Err(TabaError::new(
            ErrorCode::ValidationFailed,
            "O caminho tem um tópico ou um id de mensagem inválido.",
        ));
    }
    Ok(())
}
