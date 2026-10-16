//! How the body of a message is shown in its dialog: exactly as sent, formatted (JSON, or the simple YAML the
//! program reads) or as base64. Formatting is an option, never the default: whoever looks at a message may need to
//! see exactly what was sent.

use taba_client::base64;
use taba_client::json;

use crate::tui::model::BodyView;
use crate::yaml;

/// The body formatted, when it is valid JSON or simple YAML (maps and texts, as `config.yml`); `None` otherwise.
pub fn formatted(bytes: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(bytes).ok()?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        return json::validate(trimmed)
            .ok()
            .and_then(|()| json::pretty(trimmed).ok());
    }
    let map = yaml::parse(text).ok()?;
    // A single line of plain text is a valid YAML map only by accident of its colon: it needs a key.
    if map.is_empty() || !trimmed.contains(':') {
        return None;
    }
    Some(yaml::render(&map))
}

/// The bytes as base64, in lines of 76 characters.
pub fn base64_lines(bytes: &[u8]) -> Vec<String> {
    let encoded = base64::encode(bytes);
    encoded
        .as_bytes()
        .chunks(76)
        .map(|chunk| String::from_utf8_lossy(chunk).into_owned())
        .collect()
}

/// The lines of the body for a way of seeing it, and whether the way asked for could be used (a body that is not
/// JSON or YAML is shown as sent).
pub fn lines(bytes: &[u8], view: BodyView) -> (Vec<String>, bool) {
    let as_sent = || {
        String::from_utf8_lossy(bytes)
            .lines()
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };
    match view {
        BodyView::Sent => (as_sent(), true),
        BodyView::Formatted => match formatted(bytes) {
            Some(text) => (text.lines().map(str::to_owned).collect(), true),
            None => (as_sent(), false),
        },
        BodyView::Base64 => (base64_lines(bytes), true),
    }
}

#[cfg(test)]
mod tests;
