//! Os corpos das rotas de definição entre nós (doc 06c, "Tópicos e tokens") e o contrato dos
//! streams de sincronização (seção 8.2.1): a proposta ao dono, a mudança decidida e a página de um
//! stream NDJSON.
//!
//! A definição do tópico e o registro do token viajam como JSON (`TopicDefinition` e `TokenRecord`
//! do doc 06c): o nó que recebe converte para os tipos do `core`.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::core::error::{ErrorCode, TabaError};
use crate::core::time;
use crate::core::token::{Permission, TokenRecord};

/// O tipo de uma definição compartilhada por todos os nós.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DefinitionKind {
    Topic,
    Token,
}

impl DefinitionKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            DefinitionKind::Topic => "topic",
            DefinitionKind::Token => "token",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DefinitionOperation {
    Create,
    Update,
    /// Remove o tópico ou revoga o token.
    Delete,
}

/// A proposta de uma mudança, de quem recebeu a chamada do cliente ao dono da definição.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DefinitionProposal {
    pub kind: DefinitionKind,
    pub operation: DefinitionOperation,
    /// O nome do tópico ou o `id` do token.
    pub key: String,
    /// A `TopicDefinition` ou o `TokenRecord` completos (o do token leva o sal e o hash salgado,
    /// **nunca o segredo**); em `delete`, o tópico leva `name` e `id`, e o token, `id`.
    pub body: Value,
    /// Em `delete` pelo nó gestor de uma fila volátil: o `id` que o tópico deve ter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_id: Option<String>,
}

/// A mudança já decidida pelo dono, com a versão que ele deu, como cada nó a aplica. Só vale se a
/// versão não é menor que a que o nó já tem; repetir é inofensivo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DefinitionApply {
    pub kind: DefinitionKind,
    pub key: String,
    pub version: u64,
    /// `true`: o tópico foi removido ou o token revogado (a versão fica como lápide).
    pub deleted: bool,
    #[serde(default)]
    pub body: Value,
    /// `true` na cópia que o dono grava no **herdeiro**: quem a recebe, além de aplicá-la, a enfileira
    /// para todos os pares no quadro `definition` do canal (D127). O dono faz o mesmo.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub relay: bool,
}

/// O `TokenRecord` do doc 06c, como JSON.
pub fn token_to_json(token: &TokenRecord) -> Value {
    json!({
        "id": token.id,
        "name": token.name,
        "permissions": token.permissions.iter().map(|p| p.as_str()).collect::<Vec<_>>(),
        "topicPrefixes": token.topic_prefixes,
        "createdAt": time::format_ms(token.created_ms),
        "expiresAt": token.expires_ms.map(time::format_ms),
        "salt": token.salt,
        "secretHash": token.secret_hash,
    })
}

/// O inverso de [`token_to_json`].
pub fn token_from_json(value: &Value) -> Result<TokenRecord, TabaError> {
    let bad = |what: &str| {
        TabaError::new(
            ErrorCode::ValidationFailed,
            format!("O token entre nós é inválido: {what}."),
        )
    };
    let object = value.as_object().ok_or_else(|| bad("não é um objeto"))?;
    let text = |key: &str| {
        object
            .get(key)
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| bad(&format!("falta `{key}`")))
    };
    let date = |key: &str| -> Result<Option<u64>, TabaError> {
        match object.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(s)) => time::parse_ms(s)
                .map(Some)
                .ok_or_else(|| bad(&format!("`{key}` inválido"))),
            Some(_) => Err(bad(&format!("`{key}` inválido"))),
        }
    };
    let list = |key: &str| -> Result<Vec<String>, TabaError> {
        object
            .get(key)
            .and_then(Value::as_array)
            .ok_or_else(|| bad(&format!("falta `{key}`")))?
            .iter()
            .map(|v| {
                v.as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| bad(&format!("`{key}` tem um item que não é texto")))
            })
            .collect()
    };
    let mut permissions = Vec::new();
    for name in list("permissions")? {
        let permission = Permission::ALL
            .into_iter()
            .find(|p| p.as_str() == name)
            .ok_or_else(|| bad(&format!("permissão `{name}` desconhecida")))?;
        permissions.push(permission);
    }
    Ok(TokenRecord {
        id: text("id")?,
        name: text("name")?,
        permissions,
        topic_prefixes: list("topicPrefixes")?,
        created_ms: date("createdAt")?.ok_or_else(|| bad("falta `createdAt`"))?,
        expires_ms: date("expiresAt")?,
        salt: text("salt")?,
        secret_hash: text("secretHash")?,
    })
}

/// A consulta de um stream de sincronização: `from` (inclusivo) e `limit` (reduz o teto).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StreamQuery {
    pub from: Option<String>,
    pub to: Option<String>,
    pub limit: Option<u32>,
}

/// `StreamEnd` (doc 06d, seção 8.3.7): a linha final de todo stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StreamEnd {
    pub end: bool,
    pub count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last: Option<String>,
    pub has_more: bool,
}

/// Um trecho de um stream: as linhas de dados e a linha final. Quem pede o próximo trecho repete a
/// consulta com `from = último` (a linha repetida é ignorada).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamPage {
    pub items: Vec<Value>,
    pub end: StreamEnd,
}

impl StreamPage {
    /// Monta o trecho com o `id` de cada linha como `last`.
    pub fn new(items: Vec<Value>, has_more: bool) -> Self {
        let last = items
            .last()
            .and_then(|item| item.get("id"))
            .and_then(Value::as_str)
            .map(str::to_owned);
        Self {
            end: StreamEnd {
                end: true,
                count: items.len() as u64,
                last,
                has_more,
            },
            items,
        }
    }

    /// NDJSON: um objeto por linha e, por último, o `StreamEnd`.
    pub fn to_ndjson(&self) -> String {
        let mut out = String::new();
        for item in &self.items {
            out.push_str(&item.to_string());
            out.push('\n');
        }
        out.push_str(&serde_json::to_string(&self.end).unwrap_or_default());
        out.push('\n');
        out
    }

    /// O inverso. Sem a linha final o trecho é **falho**, mesmo que a conexão tenha fechado
    /// limpa (doc 06c, seção 8.2.1).
    pub fn from_ndjson(text: &str) -> Result<Self, TabaError> {
        let bad = |what: &str| {
            TabaError::new(
                ErrorCode::ValidationFailed,
                format!("Stream incompleto ou inválido: {what}."),
            )
        };
        let mut items = Vec::new();
        for line in text.lines().filter(|l| !l.trim().is_empty()) {
            let value: Value = serde_json::from_str(line).map_err(|_| bad("linha ilegível"))?;
            if value.get("error").is_some() {
                return Err(bad("o emissor reportou um erro"));
            }
            if value.get("end").is_some() {
                let end: StreamEnd =
                    serde_json::from_value(value).map_err(|_| bad("linha final inválida"))?;
                if end.count != items.len() as u64 {
                    return Err(bad("a contagem da linha final não confere"));
                }
                return Ok(Self { items, end });
            }
            items.push(value);
        }
        Err(bad("faltou a linha final"))
    }
}
