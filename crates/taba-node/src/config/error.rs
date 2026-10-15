use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("could not read the configuration file `{}`: {source}", .path.display())]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("configuration file `{path}` is invalid: {reason}")]
    File { path: String, reason: String },

    #[error("unknown parameter `{0}` in the configuration file")]
    UnknownKey(String),

    #[error("parameter `{key}` ({env}): invalid value `{value}`: {reason}")]
    InvalidValue {
        key: &'static str,
        env: String,
        value: String,
        reason: String,
    },

    /// A program defect, not the user's: `Config` read a key outside the table.
    #[error("parameter `{0}` is not in the parameter table")]
    NotRegistered(&'static str),

    /// A program defect: a required parameter without a default in the table.
    #[error("parameter `{0}` has neither a value nor a default")]
    Missing(&'static str),

    /// Values that are valid one by one, but together prevent the start.
    #[error("invalid configuration:\n  - {}", .0.join("\n  - "))]
    Validation(Vec<String>),
}
