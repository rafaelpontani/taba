//! From the JSON body to the `engine`'s requests.

use crate::core::error::{ErrorCode, TabaError};
use crate::engine::topics::{CreateTopic, Kind, Recreate, UpdateTopic};
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

/// Fields the `PATCH` recognizes only to refuse them: name, `id`, type, `volatile` and `fifo`.
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
    topic(fields, fields.string("name")?.unwrap_or_default())
}

/// `PUT /topics/{topic}`: the creation body without `name` (the path's).
pub fn put(fields: &Fields, name: &str) -> Result<CreateTopic, TabaError> {
    let allowed: Vec<&str> = CREATE_FIELDS
        .iter()
        .copied()
        .filter(|f| *f != "name")
        .collect();
    fields.reject_unknown(&allowed)?;
    fields.require(&["type"])?;
    topic(fields, name.to_owned())
}

/// What `recreate` asks for (D216): absent, `if-empty` or `always`.
pub fn recreate(value: Option<&str>) -> Result<Recreate, TabaError> {
    match value {
        None => Ok(Recreate::Never),
        Some("if-empty") => Ok(Recreate::IfEmpty),
        Some("always") => Ok(Recreate::Always),
        Some(_) => Err(query_error("recreate", "Use `if-empty` or `always`.")),
    }
}

/// `ifEmpty` of the `DELETE`: absent, `true` or `false`.
pub fn if_empty(value: Option<&str>) -> Result<bool, TabaError> {
    match value {
        None | Some("false") => Ok(false),
        Some("true") => Ok(true),
        Some(_) => Err(query_error("ifEmpty", "Use `true` or `false`.")),
    }
}

fn query_error(field: &str, message: &str) -> TabaError {
    TabaError::new(ErrorCode::ValidationFailed, message).with_detail(
        format!("query.{field}"),
        ErrorCode::ValidationFailed,
        message,
    )
}

fn topic(fields: &Fields, name: String) -> Result<CreateTopic, TabaError> {
    let kind = match fields.string("type")?.as_deref() {
        Some("queue") => Kind::Queue,
        Some("distributor") => Kind::Distributor,
        _ => {
            return Err(TabaError::new(
                ErrorCode::ValidationFailed,
                "The request has invalid fields.",
            )
            .with_detail(
                "type",
                ErrorCode::ValidationFailed,
                "Use `queue` or `distributor`.",
            ));
        }
    };
    Ok(CreateTopic {
        name,
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
