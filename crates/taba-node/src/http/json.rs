//! Leitura do corpo JSON das rotas (doc 06d, seção 8.3.1): `415` para outro
//! `Content-Type`, `validation.unknown-field` para campo que a rota não define e
//! `validation.required` para o que falta. Os erros de tipo viram `validation.failed`
//! com o campo em `details`.

use std::collections::BTreeMap;

use axum::http::{HeaderMap, header};
use serde_json::{Map, Value};

use crate::core::error::{ErrorCode, TabaError};
use crate::engine::topics::Patch;

pub struct Fields(Map<String, Value>);

fn wrong_type(field: &str, expected: &str) -> TabaError {
    TabaError::new(
        ErrorCode::ValidationFailed,
        "A requisição tem campos inválidos.",
    )
    .with_detail(
        field,
        ErrorCode::ValidationFailed,
        format!("Esperado {expected}."),
    )
}

/// `415` se o `Content-Type` não é `application/json`.
fn require_json(headers: &HeaderMap) -> Result<(), TabaError> {
    let media_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(';').next())
        .map(|v| v.trim().to_ascii_lowercase());
    if media_type.as_deref() == Some("application/json") {
        Ok(())
    } else {
        Err(TabaError::new(
            ErrorCode::MediaUnsupported,
            "Use `Content-Type: application/json`.",
        ))
    }
}

/// O corpo como objeto JSON. Corpo vazio vale como `{}`: quem exige campos reclama deles.
pub fn object(headers: &HeaderMap, body: &[u8]) -> Result<Fields, TabaError> {
    if body.iter().all(u8::is_ascii_whitespace) {
        return Ok(Fields(Map::new()));
    }
    require_json(headers)?;
    match serde_json::from_slice::<Value>(body) {
        Ok(Value::Object(map)) => Ok(Fields(map)),
        Ok(_) => Err(TabaError::new(
            ErrorCode::ValidationFailed,
            "O corpo deve ser um objeto JSON.",
        )),
        Err(e) => Err(TabaError::new(
            ErrorCode::ValidationFailed,
            format!("O corpo não é um JSON válido: {e}."),
        )),
    }
}

impl Fields {
    /// O primeiro campo que a rota não define.
    pub fn reject_unknown(&self, allowed: &[&str]) -> Result<(), TabaError> {
        match self.0.keys().find(|k| !allowed.contains(&k.as_str())) {
            None => Ok(()),
            Some(field) => Err(TabaError::new(
                ErrorCode::ValidationUnknownField,
                format!("O campo `{field}` não existe nesta rota."),
            )
            .with_detail(
                field,
                ErrorCode::ValidationUnknownField,
                "Campo desconhecido.",
            )),
        }
    }

    /// Todos os campos que faltam (ausentes ou `null`), cada um em `details`.
    pub fn require(&self, fields: &[&str]) -> Result<(), TabaError> {
        let mut error =
            TabaError::new(ErrorCode::ValidationRequired, "Faltam campos obrigatórios.");
        for field in fields {
            if self.0.get(*field).is_none_or(Value::is_null) {
                error =
                    error.with_detail(*field, ErrorCode::ValidationRequired, "Campo obrigatório.");
            }
        }
        if error.details.is_empty() {
            Ok(())
        } else {
            Err(error)
        }
    }

    /// O campo veio, mesmo que `null`.
    pub fn has(&self, field: &str) -> bool {
        self.0.contains_key(field)
    }

    fn present(&self, field: &str) -> Option<&Value> {
        self.0.get(field).filter(|v| !v.is_null())
    }

    pub fn string(&self, field: &str) -> Result<Option<String>, TabaError> {
        match self.present(field) {
            None => Ok(None),
            Some(Value::String(s)) => Ok(Some(s.clone())),
            Some(_) => Err(wrong_type(field, "um texto")),
        }
    }

    pub fn boolean(&self, field: &str) -> Result<Option<bool>, TabaError> {
        match self.present(field) {
            None => Ok(None),
            Some(Value::Bool(b)) => Ok(Some(*b)),
            Some(_) => Err(wrong_type(field, "`true` ou `false`")),
        }
    }

    /// Inteiro. Um número que não cabe em `i64` é fora da faixa; um decimal é erro de tipo.
    pub fn integer(&self, field: &str) -> Result<Option<i64>, TabaError> {
        match self.present(field) {
            None => Ok(None),
            Some(Value::Number(n)) => match n.as_i64() {
                Some(v) => Ok(Some(v)),
                None if n.is_u64() => Err(TabaError::new(
                    ErrorCode::ValidationOutOfRange,
                    format!("{field} está fora da faixa."),
                )
                .with_detail(
                    field,
                    ErrorCode::ValidationOutOfRange,
                    "Fora da faixa.",
                )),
                None => Err(wrong_type(field, "um número inteiro")),
            },
            Some(_) => Err(wrong_type(field, "um número inteiro")),
        }
    }

    pub fn tags(&self, field: &str) -> Result<Option<BTreeMap<String, String>>, TabaError> {
        match self.present(field) {
            None => Ok(None),
            Some(Value::Object(map)) => map
                .iter()
                .map(|(k, v)| match v {
                    Value::String(s) => Ok((k.clone(), s.clone())),
                    _ => Err(wrong_type(&format!("{field}.{k}"), "um texto")),
                })
                .collect::<Result<_, _>>()
                .map(Some),
            Some(_) => Err(wrong_type(field, "um objeto de texto para texto")),
        }
    }

    pub fn string_list(&self, field: &str) -> Result<Option<Vec<String>>, TabaError> {
        match self.present(field) {
            None => Ok(None),
            Some(Value::Array(items)) => items
                .iter()
                .enumerate()
                .map(|(i, v)| {
                    v.as_str()
                        .map(str::to_owned)
                        .ok_or_else(|| wrong_type(&format!("{field}[{i}]"), "um texto"))
                })
                .collect::<Result<_, _>>()
                .map(Some),
            Some(_) => Err(wrong_type(field, "uma lista de textos")),
        }
    }

    /// Data no formato da API (`2026-10-06T12:00:00.000Z`), em milissegundos desde a época.
    pub fn date(&self, field: &str) -> Result<Option<u64>, TabaError> {
        match self.string(field)? {
            None => Ok(None),
            Some(text) => crate::core::time::parse_ms(&text)
                .map(Some)
                .ok_or_else(|| wrong_type(field, "uma data como 2026-10-06T12:00:00.000Z")),
        }
    }

    /// Data num `PATCH`: ausente, `null` (remove) ou uma data.
    pub fn patch_date(&self, field: &str) -> Result<Patch<u64>, TabaError> {
        match self.0.get(field) {
            None => Ok(Patch::Absent),
            Some(Value::Null) => Ok(Patch::Null),
            Some(_) => Ok(self.date(field)?.map_or(Patch::Absent, Patch::Value)),
        }
    }

    /// Campo de um `PATCH`: ausente, `null` (volta ao padrão) ou um inteiro.
    pub fn patch_integer(&self, field: &str) -> Result<Patch<i64>, TabaError> {
        match self.0.get(field) {
            None => Ok(Patch::Absent),
            Some(Value::Null) => Ok(Patch::Null),
            Some(_) => Ok(self.integer(field)?.map_or(Patch::Absent, Patch::Value)),
        }
    }
}
