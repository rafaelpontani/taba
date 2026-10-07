//! O `Content-Type` da mensagem e a forma do corpo na entrega (doc 06a, "O corpo da
//! mensagem"). O Taba guarda os bytes como vieram; só a resposta do `GET` decide se o
//! `body` sai como JSON nativo, texto ou base64.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde::Serialize;
use serde_json::value::RawValue;

use super::error::{ErrorCode, TabaError};

/// `contentType` quando a publicação não traz o cabeçalho.
pub const DEFAULT_CONTENT_TYPE: &str = "application/json";
const MAX_LEN: usize = 255;

/// Como o corpo é validado e mostrado.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyKind {
    /// `application/json` ou `*/*+json`.
    Json,
    /// `text/*` em UTF-8 (ou sem `charset`).
    Text,
    /// Qualquer outro, incluindo `text/*` com outro `charset`.
    Binary,
}

/// Um media type válido, guardado como veio (com os parâmetros).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentType {
    text: String,
    kind: BodyKind,
}

fn invalid(message: &str) -> TabaError {
    TabaError::new(ErrorCode::ValidationInvalidContentType, message)
}

fn is_token(s: &str) -> bool {
    const SYMBOLS: &[u8] = b"!#$%&'*+-.^_`|~";
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || SYMBOLS.contains(&b))
}

/// Separa os parâmetros em `;`, sem quebrar dentro de aspas.
fn split_params(rest: &str) -> Vec<&str> {
    let (mut parts, mut start, mut quoted, mut escaped) = (Vec::new(), 0, false, false);
    for (i, c) in rest.char_indices() {
        match c {
            _ if escaped => escaped = false,
            '\\' if quoted => escaped = true,
            '"' => quoted = !quoted,
            ';' if !quoted => {
                parts.push(&rest[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&rest[start..]);
    parts
}

/// O valor de um parâmetro: um token ou uma string entre aspas.
fn param_value(value: &str) -> Option<String> {
    if let Some(inner) = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) {
        let mut out = String::new();
        let mut chars = inner.chars();
        while let Some(c) = chars.next() {
            match c {
                '\\' => out.push(chars.next()?),
                '"' => return None,
                c => out.push(c),
            }
        }
        return Some(out);
    }
    is_token(value).then(|| value.to_owned())
}

impl ContentType {
    /// Confere o media type (`tipo/subtipo` e parâmetros `; chave=valor`), de 1 a 255
    /// caracteres. Inválido: `400` `validation.invalid-content-type`.
    pub fn parse(raw: &str) -> Result<Self, TabaError> {
        if raw.is_empty() || raw.chars().count() > MAX_LEN {
            return Err(invalid("O Content-Type deve ter de 1 a 255 caracteres."));
        }
        let (essence, rest) = match raw.split_once(';') {
            Some((essence, rest)) => (essence, Some(rest)),
            None => (raw, None),
        };
        let essence = essence.trim().to_ascii_lowercase();
        let Some((main, sub)) = essence.split_once('/') else {
            return Err(invalid("O Content-Type deve ser `tipo/subtipo`."));
        };
        if !is_token(main) || !is_token(sub) {
            return Err(invalid("O Content-Type deve ser `tipo/subtipo`."));
        }
        let mut charset = None;
        for param in rest.map(split_params).unwrap_or_default() {
            let Some((key, value)) = param.split_once('=') else {
                return Err(invalid("Parâmetro do Content-Type sem `=`."));
            };
            let (key, value) = (key.trim(), value.trim());
            let value = param_value(value)
                .filter(|_| is_token(key))
                .ok_or_else(|| invalid("Parâmetro do Content-Type inválido."))?;
            if key.eq_ignore_ascii_case("charset") {
                charset = Some(value);
            }
        }
        let utf8 = charset.is_none_or(|c| c.eq_ignore_ascii_case("utf-8"));
        let kind = if essence == "application/json" || sub.ends_with("+json") {
            BodyKind::Json
        } else if main == "text" && utf8 {
            BodyKind::Text
        } else {
            BodyKind::Binary
        };
        Ok(Self {
            text: raw.to_owned(),
            kind,
        })
    }

    pub fn as_str(&self) -> &str {
        &self.text
    }

    pub fn kind(&self) -> BodyKind {
        self.kind
    }

    /// Corpo `application/json` precisa ser JSON, e `text/*` precisa ser UTF-8; os demais
    /// tipos não são validados. Inválido: `400` `validation.invalid-body`.
    pub fn validate_body(&self, body: &[u8]) -> Result<(), TabaError> {
        let bad = |message: &str| TabaError::new(ErrorCode::ValidationInvalidBody, message);
        match self.kind {
            BodyKind::Json => serde_json::from_slice::<serde::de::IgnoredAny>(body)
                .map(|_| ())
                .map_err(|e| bad(&format!("O corpo não é um JSON válido: {e}."))),
            BodyKind::Text => std::str::from_utf8(body)
                .map(|_| ())
                .map_err(|_| bad("O corpo não é um texto UTF-8 válido.")),
            BodyKind::Binary => Ok(()),
        }
    }
}

/// O `body` (e o `bodyEncoding`) de uma mensagem entregue.
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum Body {
    /// O JSON como foi enviado, sem reserializar.
    Json(Box<RawValue>),
    Text(String),
    Base64(String),
}

impl Body {
    /// Monta o `body` a partir dos bytes guardados, na forma que o `contentType` manda.
    /// Os bytes já passaram por `validate_body` na publicação.
    pub fn render(content_type: &ContentType, bytes: &[u8]) -> Self {
        let as_text = || String::from_utf8_lossy(bytes).into_owned();
        match content_type.kind {
            BodyKind::Json => RawValue::from_string(as_text())
                .map(Self::Json)
                .unwrap_or_else(|_| Self::Base64(STANDARD.encode(bytes))),
            BodyKind::Text => Self::Text(as_text()),
            BodyKind::Binary => Self::Base64(STANDARD.encode(bytes)),
        }
    }

    /// `Some("base64")` só para o binário.
    pub fn encoding(&self) -> Option<&'static str> {
        matches!(self, Self::Base64(_)).then_some("base64")
    }
}

#[cfg(test)]
mod tests;
