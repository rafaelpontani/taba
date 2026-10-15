//! Fixed limits of the client API (doc 12 and 10, "Fixed API limits"): format rules
//! applied when creating and changing resources.

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

/// `A-Za-z0-9_-`: the set for topic names and prefixes.
fn is_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '-'
}

/// `A-Za-z0-9_-.:`: the set for tags, `groupId` and `deduplicationId`.
fn is_tag_char(c: char) -> bool {
    is_name_char(c) || c == '.' || c == ':'
}

/// A created topic name: `^[A-Za-z0-9_-]{1,100}$`, and not ending in the
/// dead-letter suffix, which is reserved.
pub fn topic_name(name: &str, dead_letter_suffix: &str) -> Result<(), TabaError> {
    if name.is_empty() || name.chars().count() > TOPIC_NAME_MAX || !name.chars().all(is_name_char) {
        return Err(TabaError::new(
            ErrorCode::ValidationInvalidName,
            "Use 1 to 100 characters from A-Za-z0-9_-.",
        ));
    }
    if name.ends_with(dead_letter_suffix) {
        return Err(TabaError::new(
            ErrorCode::ValidationReservedName,
            format!("Names ending in `{dead_letter_suffix}` are reserved for the dead-letter."),
        ));
    }
    Ok(())
}

/// Token name: 1 to 100 characters of `A-Za-z0-9_-.:` (it need not be unique).
pub fn token_name(name: &str) -> Result<(), TabaError> {
    if !name.is_empty() && name.chars().count() <= TOPIC_NAME_MAX && name.chars().all(is_tag_char) {
        return Ok(());
    }
    Err(TabaError::new(
        ErrorCode::ValidationInvalidName,
        "Use 1 to 100 characters from A-Za-z0-9_-.:",
    )
    .with_detail(
        "name",
        ErrorCode::ValidationInvalidName,
        "Use 1 to 100 characters from A-Za-z0-9_-.:",
    ))
}

/// Topic tags: up to 20; key of 1 to 64 and value of 0 to 128 characters of `A-Za-z0-9_-.:`.
pub fn tags(tags: &BTreeMap<String, String>) -> Result<(), TabaError> {
    let mut err = TabaError::new(
        ErrorCode::ValidationFailed,
        "The request has invalid fields.",
    );
    if tags.len() > TAGS_MAX {
        err = err.with_detail(
            "tags",
            ErrorCode::ValidationOutOfRange,
            "At most 20 tags per topic.",
        );
    }
    for (key, value) in tags {
        let field = format!("tags.{key}");
        if key.is_empty() || key.chars().count() > TAG_KEY_MAX || !key.chars().all(is_tag_char) {
            err = err.with_detail(
                &field,
                ErrorCode::ValidationFailed,
                "Key of 1 to 64 characters from A-Za-z0-9_-.:",
            );
        }
        if value.chars().count() > TAG_VALUE_MAX || !value.chars().all(is_tag_char) {
            err = err.with_detail(
                &field,
                ErrorCode::ValidationFailed,
                "Value of 0 to 128 characters from A-Za-z0-9_-.:",
            );
        }
    }
    if err.details.is_empty() {
        Ok(())
    } else {
        Err(err)
    }
}

/// A distributor's prefixes: up to 20, of 1 to 100 characters of `A-Za-z0-9_-`.
/// An empty prefix is refused (it would match everything).
pub fn prefixes(prefixes: &[String]) -> Result<(), TabaError> {
    prefixes_of("prefixes", prefixes)
}

/// The same rule for the `field` field (the token's `topicPrefixes`).
pub fn prefixes_of(field: &str, prefixes: &[String]) -> Result<(), TabaError> {
    let mut err = TabaError::new(
        ErrorCode::ValidationFailed,
        "The request has invalid fields.",
    );
    if prefixes.len() > PREFIXES_MAX {
        err = err.with_detail(
            field,
            ErrorCode::ValidationOutOfRange,
            "At most 20 prefixes.",
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
                "Use 1 to 100 characters from A-Za-z0-9_-.",
            );
        }
    }
    if err.details.is_empty() {
        Ok(())
    } else {
        Err(err)
    }
}

/// A message's `groupId` and `deduplicationId`: 1 to 128 characters of `A-Za-z0-9_-.:`.
/// Invalid: `400` `validation.failed`, with the field in `details`.
pub fn message_key(field: &str, value: &str) -> Result<(), TabaError> {
    if !value.is_empty() && value.chars().count() <= KEY_MAX && value.chars().all(is_tag_char) {
        return Ok(());
    }
    Err(TabaError::new(
        ErrorCode::ValidationFailed,
        "The request has invalid fields.",
    )
    .with_detail(
        field,
        ErrorCode::ValidationFailed,
        "Use 1 to 128 characters from A-Za-z0-9_-.:",
    ))
}

/// A number within an inclusive range (`ttl`, `idleTimeout`, `maxDeliveries`...).
pub fn in_range(field: &str, value: u64, (min, max): (u64, u64)) -> Result<(), TabaError> {
    if (min..=max).contains(&value) {
        return Ok(());
    }
    Err(out_of_range(field, (min, max)))
}

/// Like `in_range`, for a number that may come in negative (negative is outside any
/// range). Returns the value already as `u64`.
pub fn in_range_signed(field: &str, value: i64, range: (u64, u64)) -> Result<u64, TabaError> {
    match u64::try_from(value) {
        Ok(v) => in_range(field, v, range).map(|()| v),
        Err(_) => Err(out_of_range(field, range)),
    }
}

fn out_of_range(field: &str, (min, max): (u64, u64)) -> TabaError {
    TabaError::new(
        ErrorCode::ValidationOutOfRange,
        format!("{field} must be between {min} and {max}."),
    )
    .with_detail(
        field,
        ErrorCode::ValidationOutOfRange,
        format!("Between {min} and {max}."),
    )
}

#[cfg(test)]
mod tests;
