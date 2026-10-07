//! Do corpo JSON para os pedidos do `engine`.

use crate::core::error::{ErrorCode, TabaError};
use crate::engine::topics::{CreateTopic, Kind, UpdateTopic};
use crate::http::json::Fields;

const CREATE_FIELDS: &[&str] = &[
    "name",
    "type",
    "tags",
    "prefixes",
    "volatile",
    "idleTimeout",
    "fifo",
    "maxMessageBytes",
    "ttl",
    "maxDeliveries",
];

/// Campos que o `PATCH` reconhece só para recusá-los: nome, `id`, tipo, `volatile` e `fifo`.
const IMMUTABLE_FIELDS: &[&str] = &["name", "id", "type", "volatile", "fifo"];

const PATCH_FIELDS: &[&str] = &[
    "tags",
    "prefixes",
    "idleTimeout",
    "maxMessageBytes",
    "ttl",
    "maxDeliveries",
];

pub fn create(fields: &Fields) -> Result<CreateTopic, TabaError> {
    fields.reject_unknown(CREATE_FIELDS)?;
    fields.require(&["name", "type"])?;
    let kind = match fields.string("type")?.as_deref() {
        Some("queue") => Kind::Queue,
        Some("distributor") => Kind::Distributor,
        _ => {
            return Err(TabaError::new(
                ErrorCode::ValidationFailed,
                "A requisição tem campos inválidos.",
            )
            .with_detail(
                "type",
                ErrorCode::ValidationFailed,
                "Use `queue` ou `distributor`.",
            ));
        }
    };
    Ok(CreateTopic {
        name: fields.string("name")?.unwrap_or_default(),
        kind,
        tags: fields.tags("tags")?,
        prefixes: fields.string_list("prefixes")?,
        volatile: fields.boolean("volatile")?,
        idle_timeout: fields.integer("idleTimeout")?,
        fifo: fields.boolean("fifo")?,
        max_message_bytes: fields.integer("maxMessageBytes")?,
        ttl: fields.integer("ttl")?,
        max_deliveries: fields.integer("maxDeliveries")?,
    })
}

pub fn update(fields: &Fields) -> Result<UpdateTopic, TabaError> {
    let known: Vec<&str> = PATCH_FIELDS
        .iter()
        .chain(IMMUTABLE_FIELDS)
        .copied()
        .collect();
    fields.reject_unknown(&known)?;
    Ok(UpdateTopic {
        immutable_present: IMMUTABLE_FIELDS
            .iter()
            .filter(|f| fields.has(f))
            .map(|f| (*f).to_owned())
            .collect(),
        tags: fields.tags("tags")?,
        prefixes: fields.string_list("prefixes")?,
        idle_timeout: fields.integer("idleTimeout")?,
        max_message_bytes: fields.patch_integer("maxMessageBytes")?,
        ttl: fields.patch_integer("ttl")?,
        max_deliveries: fields.patch_integer("maxDeliveries")?,
    })
}
