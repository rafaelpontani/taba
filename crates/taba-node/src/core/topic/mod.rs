//! A topic's definition (doc 02, section 3.3): what is replicated to every node and
//! stored in the `topics` table of `_node.db`. The same JSON is the basis of the API response.

use std::collections::BTreeMap;

use serde_json::{Map, Value, json};

use super::time;

/// Queue settings. `None` in `max_message_bytes`, `ttl` and `max_deliveries`
/// means "the cluster default applies" (`null` in the JSON).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueSpec {
    pub volatile: bool,
    /// Only on a volatile queue.
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

    /// A distributor's prefixes; `None` on a queue.
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

    /// The definition's JSON, the same one the `topics` table stores. Absent field = does not apply to the type;
    /// `null` = it applies and the default holds (doc 06d, section 8.3.1).
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

    /// The inverse of [`to_json`](Self::to_json). The error is a text for the log.
    pub fn from_json(value: &Value) -> Result<Self, String> {
        let obj = value.as_object().ok_or("the definition is not an object")?;
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
                    .ok_or(format!("`{k}` is not an integer")),
            }
        };
        let tags = match obj.get("tags") {
            None => BTreeMap::new(),
            Some(Value::Object(t)) => t
                .iter()
                .map(|(k, v)| {
                    v.as_str()
                        .map(|v| (k.clone(), v.to_owned()))
                        .ok_or(format!("tag `{k}` is not text"))
                })
                .collect::<Result<_, _>>()?,
            Some(_) => return Err("`tags` is not an object".into()),
        };
        let created_ms = time::parse_ms(&text("createdAt")?).ok_or("`createdAt` is invalid")?;
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
            other => return Err(format!("unknown type `{other}`")),
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

/// The `id` of the dead-letter queue, derived from the source queue's `id` (it has no
/// definition of its own, doc 13, section 12.10): the last hexadecimal digit advances by one.
/// It lets the dead-letter have a unique key in ordering and pagination.
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
