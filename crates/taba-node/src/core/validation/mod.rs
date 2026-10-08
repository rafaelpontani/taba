//! Limites fixos da API de cliente (doc 12 e 10, "Limites fixos da API"): regras de
//! formato aplicadas na criação e na alteração de recursos.

use std::collections::BTreeMap;

use super::error::{ErrorCode, TabaError};

pub const TOPIC_NAME_MAX: usize = 100;
pub const TAGS_MAX: usize = 20;
pub const TAG_KEY_MAX: usize = 64;
pub const TAG_VALUE_MAX: usize = 128;
pub const PREFIXES_MAX: usize = 20;
pub const PREFIX_MAX: usize = 100;
pub const KEY_MAX: usize = 128;
pub const IDLE_TIMEOUT_SECS: (u64, u64) = (60, 604_800);
pub const TTL_SECS: (u64, u64) = (60, 31_536_000);
pub const MAX_DELIVERIES: (u64, u64) = (1, 1000);

/// `A-Za-z0-9_-`: o conjunto do nome de tópico e dos prefixos.
fn is_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '-'
}

/// `A-Za-z0-9_-.:`: o conjunto de tags, `groupId` e `deduplicationId`.
fn is_tag_char(c: char) -> bool {
    is_name_char(c) || c == '.' || c == ':'
}

/// Nome de tópico criado: `^[A-Za-z0-9_-]{1,100}$`, e não terminado no sufixo de
/// dead-letter, que é reservado.
pub fn topic_name(name: &str, dead_letter_suffix: &str) -> Result<(), TabaError> {
    if name.is_empty() || name.chars().count() > TOPIC_NAME_MAX || !name.chars().all(is_name_char) {
        return Err(TabaError::new(
            ErrorCode::ValidationInvalidName,
            "Use de 1 a 100 caracteres entre A-Za-z0-9_-.",
        ));
    }
    if name.ends_with(dead_letter_suffix) {
        return Err(TabaError::new(
            ErrorCode::ValidationReservedName,
            format!("Nomes terminados em `{dead_letter_suffix}` são reservados à dead-letter."),
        ));
    }
    Ok(())
}

/// Nome do token: de 1 a 100 caracteres de `A-Za-z0-9_-.:` (não precisa ser único).
pub fn token_name(name: &str) -> Result<(), TabaError> {
    if !name.is_empty() && name.chars().count() <= TOPIC_NAME_MAX && name.chars().all(is_tag_char) {
        return Ok(());
    }
    Err(TabaError::new(
        ErrorCode::ValidationInvalidName,
        "Use de 1 a 100 caracteres entre A-Za-z0-9_-.:",
    )
    .with_detail(
        "name",
        ErrorCode::ValidationInvalidName,
        "Use de 1 a 100 caracteres entre A-Za-z0-9_-.:",
    ))
}

/// Tags do tópico: até 20; chave de 1 a 64 e valor de 0 a 128 caracteres de `A-Za-z0-9_-.:`.
pub fn tags(tags: &BTreeMap<String, String>) -> Result<(), TabaError> {
    let mut err = TabaError::new(
        ErrorCode::ValidationFailed,
        "A requisição tem campos inválidos.",
    );
    if tags.len() > TAGS_MAX {
        err = err.with_detail(
            "tags",
            ErrorCode::ValidationOutOfRange,
            "No máximo 20 tags por tópico.",
        );
    }
    for (key, value) in tags {
        let field = format!("tags.{key}");
        if key.is_empty() || key.chars().count() > TAG_KEY_MAX || !key.chars().all(is_tag_char) {
            err = err.with_detail(
                &field,
                ErrorCode::ValidationFailed,
                "Chave de 1 a 64 caracteres de A-Za-z0-9_-.:",
            );
        }
        if value.chars().count() > TAG_VALUE_MAX || !value.chars().all(is_tag_char) {
            err = err.with_detail(
                &field,
                ErrorCode::ValidationFailed,
                "Valor de 0 a 128 caracteres de A-Za-z0-9_-.:",
            );
        }
    }
    if err.details.is_empty() {
        Ok(())
    } else {
        Err(err)
    }
}

/// Prefixos de um distribuidor: até 20, de 1 a 100 caracteres de `A-Za-z0-9_-`.
/// Prefixo vazio é recusado (casaria com tudo).
pub fn prefixes(prefixes: &[String]) -> Result<(), TabaError> {
    prefixes_of("prefixes", prefixes)
}

/// A mesma regra para o campo `field` (`topicPrefixes` do token).
pub fn prefixes_of(field: &str, prefixes: &[String]) -> Result<(), TabaError> {
    let mut err = TabaError::new(
        ErrorCode::ValidationFailed,
        "A requisição tem campos inválidos.",
    );
    if prefixes.len() > PREFIXES_MAX {
        err = err.with_detail(
            field,
            ErrorCode::ValidationOutOfRange,
            "No máximo 20 prefixos.",
        );
    }
    for (i, prefix) in prefixes.iter().enumerate() {
        if prefix.is_empty()
            || prefix.chars().count() > PREFIX_MAX
            || !prefix.chars().all(is_name_char)
        {
            err = err.with_detail(
                format!("{field}[{i}]"),
                ErrorCode::ValidationInvalidName,
                "Use de 1 a 100 caracteres entre A-Za-z0-9_-.",
            );
        }
    }
    if err.details.is_empty() {
        Ok(())
    } else {
        Err(err)
    }
}

/// `groupId` e `deduplicationId` da mensagem: de 1 a 128 caracteres de `A-Za-z0-9_-.:`.
/// Inválido: `400` `validation.failed`, com o campo em `details`.
pub fn message_key(field: &str, value: &str) -> Result<(), TabaError> {
    if !value.is_empty() && value.chars().count() <= KEY_MAX && value.chars().all(is_tag_char) {
        return Ok(());
    }
    Err(TabaError::new(
        ErrorCode::ValidationFailed,
        "A requisição tem campos inválidos.",
    )
    .with_detail(
        field,
        ErrorCode::ValidationFailed,
        "Use de 1 a 128 caracteres entre A-Za-z0-9_-.:",
    ))
}

/// Número dentro de uma faixa inclusiva (`ttl`, `idleTimeout`, `maxDeliveries`...).
pub fn in_range(field: &str, value: u64, (min, max): (u64, u64)) -> Result<(), TabaError> {
    if (min..=max).contains(&value) {
        return Ok(());
    }
    Err(out_of_range(field, (min, max)))
}

/// Como `in_range`, para um número que pode vir negativo (negativo está fora de qualquer
/// faixa). Devolve o valor já como `u64`.
pub fn in_range_signed(field: &str, value: i64, range: (u64, u64)) -> Result<u64, TabaError> {
    match u64::try_from(value) {
        Ok(v) => in_range(field, v, range).map(|()| v),
        Err(_) => Err(out_of_range(field, range)),
    }
}

fn out_of_range(field: &str, (min, max): (u64, u64)) -> TabaError {
    TabaError::new(
        ErrorCode::ValidationOutOfRange,
        format!("{field} deve estar entre {min} e {max}."),
    )
    .with_detail(
        field,
        ErrorCode::ValidationOutOfRange,
        format!("Entre {min} e {max}."),
    )
}

#[cfg(test)]
mod tests;
