//! Regras que o registro de tópicos lê da configuração, e os pedidos que ele recebe.

use std::collections::BTreeMap;

use crate::config::Config;

/// Os valores de configuração de que os tópicos dependem (doc 10).
#[derive(Debug, Clone)]
pub struct TopicRules {
    /// `topics.dead-letter.suffix`.
    pub dead_letter_suffix: String,
    /// `message.max-bytes`: o padrão do `maxMessageBytes`.
    pub max_message_bytes_default: u64,
    /// `message.hard-limit`: sem ele, não há teto para o `maxMessageBytes`.
    pub hard_limit: Option<u64>,
    /// `retention.ttl.default`, em segundos.
    pub ttl_default: u64,
    /// `retention.max-deliveries.default`.
    pub max_deliveries_default: u64,
    /// `topics.volatile.idle-timeout.default`, em segundos.
    pub idle_timeout_default: u64,
    /// Fila FIFO exige `replication.factor` de pelo menos 2; em desenvolvimento não vale.
    pub fifo_needs_replication: bool,
    /// `api.pagination.default-limit` e `max-limit`.
    pub default_limit: u32,
    pub max_limit: u32,
}

impl TopicRules {
    pub fn from_config(config: &Config) -> Self {
        Self {
            dead_letter_suffix: config.topics.dead_letter_suffix.clone(),
            max_message_bytes_default: config.messages.max_bytes,
            hard_limit: config.messages.hard_limit,
            ttl_default: config.retention.ttl_default.as_secs(),
            max_deliveries_default: u64::from(config.retention.max_deliveries_default),
            idle_timeout_default: config.topics.volatile_idle_timeout_default.as_secs(),
            fifo_needs_replication: !config.is_development() && config.write.replication_factor < 2,
            default_limit: config.pagination.default_limit,
            max_limit: config.pagination.max_limit,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Queue,
    Distributor,
}

/// `POST /topics`. Os números chegam como `i64` para o `400` por faixa valer também
/// para valores negativos.
#[derive(Debug, Clone)]
pub struct CreateTopic {
    pub name: String,
    pub kind: Kind,
    pub tags: Option<BTreeMap<String, String>>,
    pub prefixes: Option<Vec<String>>,
    pub volatile: Option<bool>,
    pub idle_timeout: Option<i64>,
    pub fifo: Option<bool>,
    pub max_message_bytes: Option<i64>,
    pub ttl: Option<i64>,
    pub max_deliveries: Option<i64>,
}

/// Um campo de um `PATCH`: ausente (não mexe), `null` (volta ao padrão) ou um valor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Patch<T> {
    #[default]
    Absent,
    Null,
    Value(T),
}

/// `PATCH /topics/{topic}`: atualização parcial.
#[derive(Debug, Clone, Default)]
pub struct UpdateTopic {
    /// Campos imutáveis (`name`, `id`, `type`, `volatile`, `fifo`) que vieram no corpo.
    pub immutable_present: Vec<String>,
    pub tags: Option<BTreeMap<String, String>>,
    pub prefixes: Option<Vec<String>>,
    pub idle_timeout: Option<i64>,
    pub max_message_bytes: Patch<i64>,
    pub ttl: Patch<i64>,
    pub max_deliveries: Patch<i64>,
}

/// Os parâmetros de query de `GET /topics`, como vieram, para o `engine` validar.
#[derive(Debug, Clone, Default)]
pub struct ListQuery {
    pub limit: Option<String>,
    pub cursor: Option<String>,
    pub sort: Option<String>,
    pub name: Option<String>,
}
