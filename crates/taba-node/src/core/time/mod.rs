//! Datas da API: RFC 3339 em UTC, com milissegundos (`2026-10-06T12:00:00.000Z`),
//! guardadas no nó como milissegundos desde a época Unix (doc 06d, seção 8.3.1).

use time::format_description::FormatItem;
use time::macros::format_description;
use time::{OffsetDateTime, PrimitiveDateTime};

const FORMAT: &[FormatItem<'static>] =
    format_description!("[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]Z");

pub fn format_ms(unix_millis: u64) -> String {
    OffsetDateTime::from_unix_timestamp_nanos(i128::from(unix_millis) * 1_000_000)
        .ok()
        .and_then(|dt| dt.format(FORMAT).ok())
        .unwrap_or_default()
}

/// O inverso de [`format_ms`]; só aceita exatamente esse formato.
pub fn parse_ms(text: &str) -> Option<u64> {
    let nanos = PrimitiveDateTime::parse(text, FORMAT)
        .ok()?
        .assume_utc()
        .unix_timestamp_nanos();
    u64::try_from(nanos / 1_000_000).ok()
}

#[cfg(test)]
mod tests;
