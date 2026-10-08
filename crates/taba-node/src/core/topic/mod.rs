//! A definição de um tópico (doc 02, seção 3.3): o que é replicado a todos os nós e
//! gravado na tabela `topics` do `_node.db`. O mesmo JSON é a base da resposta da API.

use std::collections::BTreeMap;

use serde_json::{Map, Value, json};

use super::time;

/// Configurações de uma fila. `None` em `max_message_bytes`, `ttl` e `max_deliveries`
/// quer dizer "vale o padrão do cluster" (`null` no JSON).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueSpec {
    pub volatile: bool,
    /// Só em fila volátil.
    pub idle_timeout: Option<u64>,
    pub fifo: bool,
    pub max_message_bytes: Option<u64>,
    pub ttl: Option<u64>,
    pub max_deliveries: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Spec {
    Queue(QueueSpec),
    Distributor { prefixes: Vec<String> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TopicDefinition {
    pub id: String,
    pub name: String,
    pub created_ms: u64,
    pub tags: BTreeMap<String, String>,
    pub spec: Spec,
}

impl TopicDefinition {
    pub fn queue(&self) -> Option<&QueueSpec> {
        match &self.spec {
            Spec::Queue(q) => Some(q),
            Spec::Distributor { .. } => None,
        }
    }

    /// Os prefixos de um distribuidor; `None` numa fila.
    pub fn prefixes(&self) -> Option<&[String]> {
        match &self.spec {
            Spec::Distributor { prefixes } => Some(prefixes),
            Spec::Queue(_) => None,
        }
    }

    pub fn type_name(&self) -> &'static str {
        match self.spec {
            Spec::Queue(_) => "queue",
            Spec::Distributor { .. } => "distributor",
        }
    }

    /// O JSON da definição, o mesmo que a tabela `topics` guarda. Campo ausente = não se aplica ao tipo;
    /// `null` = aplica-se e vale o padrão (doc 06d, seção 8.3.1).
    pub fn to_json(&self) -> Value {
        let mut m = Map::new();
        m.insert("id".into(), json!(self.id));
        m.insert("name".into(), json!(self.name));
        m.insert("type".into(), json!(self.type_name()));
        m.insert("tags".into(), json!(self.tags));
        m.insert("createdAt".into(), json!(time::format_ms(self.created_ms)));
        match &self.spec {
            Spec::Distributor { prefixes } => {
                m.insert("prefixes".into(), json!(prefixes));
            }
            Spec::Queue(q) => {
                m.insert("volatile".into(), json!(q.volatile));
                if q.volatile {
                    m.insert("idleTimeout".into(), json!(q.idle_timeout));
                }
                m.insert("fifo".into(), json!(q.fifo));
                m.insert("maxMessageBytes".into(), json!(q.max_message_bytes));
                m.insert("ttl".into(), json!(q.ttl));
                m.insert("maxDeliveries".into(), json!(q.max_deliveries));
            }
        }
        Value::Object(m)
    }

    /// O inverso de [`to_json`](Self::to_json). O erro é um texto para o log.
    pub fn from_json(value: &Value) -> Result<Self, String> {
        let obj = value.as_object().ok_or("a definição não é um objeto")?;
        let text = |k: &str| {
            obj.get(k)
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or(format!("falta `{k}`"))
        };
        let opt_u64 = |k: &str| -> Result<Option<u64>, String> {
            match obj.get(k) {
                None | Some(Value::Null) => Ok(None),
                Some(v) => v
                    .as_u64()
                    .map(Some)
                    .ok_or(format!("`{k}` não é um inteiro")),
            }
        };
        let tags = match obj.get("tags") {
            None => BTreeMap::new(),
            Some(Value::Object(t)) => t
                .iter()
                .map(|(k, v)| {
                    v.as_str()
                        .map(|v| (k.clone(), v.to_owned()))
                        .ok_or(format!("tag `{k}` não é texto"))
                })
                .collect::<Result<_, _>>()?,
            Some(_) => return Err("`tags` não é um objeto".into()),
        };
        let created_ms = time::parse_ms(&text("createdAt")?).ok_or("`createdAt` inválido")?;
        let spec = match text("type")?.as_str() {
            "queue" => Spec::Queue(QueueSpec {
                volatile: obj
                    .get("volatile")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                idle_timeout: opt_u64("idleTimeout")?,
                fifo: obj.get("fifo").and_then(Value::as_bool).unwrap_or(false),
                max_message_bytes: opt_u64("maxMessageBytes")?,
                ttl: opt_u64("ttl")?,
                max_deliveries: opt_u64("maxDeliveries")?,
            }),
            "distributor" => Spec::Distributor {
                prefixes: obj
                    .get("prefixes")
                    .and_then(Value::as_array)
                    .map(|a| {
                        a.iter()
                            .filter_map(|p| p.as_str().map(str::to_owned))
                            .collect()
                    })
                    .unwrap_or_default(),
            },
            other => return Err(format!("tipo `{other}` desconhecido")),
        };
        Ok(Self {
            id: text("id")?,
            name: text("name")?,
            created_ms,
            tags,
            spec,
        })
    }
}

/// O `id` da fila de dead-letter, derivado do `id` da fila de origem (ela não tem
/// definição própria, doc 13, seção 12.10): o último dígito hexadecimal avança um.
/// Serve para a dead-letter ter uma chave única na ordenação e na paginação.
pub fn dead_letter_id(source_id: &str) -> String {
    let mut chars: Vec<char> = source_id.chars().collect();
    if let Some(last) = chars.last_mut() {
        let next = last.to_digit(16).map_or(0, |d| (d + 1) % 16);
        *last = char::from_digit(next, 16).unwrap_or('0');
    }
    chars.into_iter().collect()
}

#[cfg(test)]
mod tests;
