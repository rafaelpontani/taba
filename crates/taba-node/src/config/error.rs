use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("não foi possível ler o arquivo de configuração `{}`: {source}", .path.display())]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("arquivo de configuração `{path}` inválido: {reason}")]
    File { path: String, reason: String },

    #[error("parâmetro desconhecido `{0}` no arquivo de configuração")]
    UnknownKey(String),

    #[error("parâmetro `{key}` ({env}): valor `{value}` inválido: {reason}")]
    InvalidValue {
        key: &'static str,
        env: String,
        value: String,
        reason: String,
    },

    /// Defeito do programa, não do usuário: `Config` leu uma chave fora da tabela.
    #[error("parâmetro `{0}` não está na tabela de parâmetros")]
    NotRegistered(&'static str),

    /// Defeito do programa: parâmetro obrigatório sem padrão na tabela.
    #[error("parâmetro `{0}` não tem valor nem padrão")]
    Missing(&'static str),

    /// Valores válidos um a um, mas que juntos impedem a partida.
    #[error("configuração inválida:\n  - {}", .0.join("\n  - "))]
    Validation(Vec<String>),
}
