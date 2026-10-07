//! Cursor opaco da paginação por chave (doc 06a, seção 8.1.2): o cliente nunca o monta.
//! O cursor guarda a última chave lida, mais o `sort` e os filtros em uso, e não tem
//! estado no servidor, então não expira.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::Serialize;
use serde::de::DeserializeOwned;

use super::error::{ErrorCode, TabaError};

pub fn encode_cursor<T: Serialize>(key: &T) -> String {
    let json = serde_json::to_vec(key).unwrap_or_default();
    URL_SAFE_NO_PAD.encode(json)
}

/// Cursor malformado: `400` `validation.invalid-cursor`.
pub fn decode_cursor<T: DeserializeOwned>(cursor: &str) -> Result<T, TabaError> {
    let invalid = || {
        TabaError::new(ErrorCode::ValidationInvalidCursor, "O cursor é inválido.").with_detail(
            "cursor",
            ErrorCode::ValidationInvalidCursor,
            "Use o `nextCursor` da página anterior.",
        )
    };
    let bytes = URL_SAFE_NO_PAD.decode(cursor).map_err(|_| invalid())?;
    serde_json::from_slice(&bytes).map_err(|_| invalid())
}

/// Valida o `limit` da query: de 1 até o máximo; sem ele, vale o padrão.
pub fn parse_limit(raw: Option<&str>, default: u32, max: u32) -> Result<usize, TabaError> {
    let Some(raw) = raw else {
        return Ok(default as usize);
    };
    let out_of_range = || {
        TabaError::new(
            ErrorCode::ValidationOutOfRange,
            format!("limit deve estar entre 1 e {max}."),
        )
        .with_detail(
            "query.limit",
            ErrorCode::ValidationOutOfRange,
            format!("Entre 1 e {max}."),
        )
    };
    match raw.trim().parse::<u32>() {
        Ok(n) if (1..=max).contains(&n) => Ok(n as usize),
        _ => Err(out_of_range()),
    }
}

#[cfg(test)]
mod tests;
