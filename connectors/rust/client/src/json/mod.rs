//! Minimal JSON reader (doc 19, section 18.5).
//!
//! Walks the text without building a tree: it can skip any value, hands out an object's fields
//! as name → text-span pairs and decodes a JSON string. A message's `body` is
//! any JSON, so the connector returns the text span without interpreting it.

use std::fmt;

/// Maximum nesting depth accepted; above it the document is refused instead of overflowing
/// the stack.
pub const MAX_DEPTH: usize = 512;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonError {
    pub position: usize,
    pub message: &'static str,
}

impl fmt::Display for JsonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "invalid JSON at position {}: {}",
            self.position, self.message
        )
    }
}

impl std::error::Error for JsonError {}

type Result<T> = std::result::Result<T, JsonError>;

fn err<T>(position: usize, message: &'static str) -> Result<T> {
    Err(JsonError { position, message })
}

fn ws(b: &[u8], mut i: usize) -> usize {
    while matches!(b.get(i), Some(b' ' | b'\t' | b'\n' | b'\r')) {
        i += 1;
    }
    i
}

/// Checks that the whole of `s` is a single valid JSON value (surrounding spaces are accepted).
pub fn validate(s: &str) -> Result<()> {
    let end = skip(s.as_bytes(), 0, 0)?;
    finish(s.as_bytes(), end, Vec::new()).map(|_| ())
}

/// Skips the value that starts at `start` (after spaces) and returns the index right after it.
pub fn skip_value(s: &str, start: usize) -> Result<usize> {
    skip(s.as_bytes(), start, 0)
}

fn skip(b: &[u8], i: usize, depth: usize) -> Result<usize> {
    if depth > MAX_DEPTH {
        return err(i, "nesting too deep");
    }
    let i = ws(b, i);
    match b.get(i).copied() {
        None => err(i, "unexpected end"),
        Some(b'{') => {
            let mut i = ws(b, i + 1);
            if b.get(i) == Some(&b'}') {
                return Ok(i + 1);
            }
            loop {
                i = skip_string(b, i)?;
                i = ws(b, i);
                if b.get(i) != Some(&b':') {
                    return err(i, "esperava ':'");
                }
                i = skip(b, i + 1, depth + 1)?;
                i = ws(b, i);
                match b.get(i) {
                    Some(b',') => i = ws(b, i + 1),
                    Some(b'}') => return Ok(i + 1),
                    _ => return err(i, "expected ',' or '}'"),
                }
            }
        }
        Some(b'[') => {
            let mut i = ws(b, i + 1);
            if b.get(i) == Some(&b']') {
                return Ok(i + 1);
            }
            loop {
                i = skip(b, i, depth + 1)?;
                i = ws(b, i);
                match b.get(i) {
                    Some(b',') => i += 1,
                    Some(b']') => return Ok(i + 1),
                    _ => return err(i, "expected ',' or ']'"),
                }
            }
        }
        Some(b'"') => skip_string(b, i),
        Some(b't') => literal(b, i, b"true"),
        Some(b'f') => literal(b, i, b"false"),
        Some(b'n') => literal(b, i, b"null"),
        Some(b'-' | b'0'..=b'9') => skip_number(b, i),
        Some(_) => err(i, "unexpected value"),
    }
}

fn literal(b: &[u8], i: usize, word: &[u8]) -> Result<usize> {
    if b[i..].starts_with(word) {
        Ok(i + word.len())
    } else {
        err(i, "invalid literal")
    }
}

fn skip_string(b: &[u8], i: usize) -> Result<usize> {
    if b.get(i) != Some(&b'"') {
        return err(i, "expected a string");
    }
    let mut i = i + 1;
    loop {
        match b.get(i).copied() {
            None => return err(i, "unterminated string"),
            Some(b'"') => return Ok(i + 1),
            Some(b'\\') => match b.get(i + 1).copied() {
                Some(b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't') => i += 2,
                Some(b'u') => {
                    let hex = b.get(i + 2..i + 6);
                    if hex.is_some_and(|h| h.iter().all(u8::is_ascii_hexdigit)) {
                        i += 6;
                    } else {
                        return err(i, "invalid \\u escape");
                    }
                }
                _ => return err(i, "invalid escape"),
            },
            Some(c) if c < 0x20 => return err(i, "control character in string"),
            Some(_) => i += 1,
        }
    }
}

fn skip_number(b: &[u8], mut i: usize) -> Result<usize> {
    let digits = |b: &[u8], mut i: usize| -> (usize, usize) {
        let start = i;
        while matches!(b.get(i), Some(b'0'..=b'9')) {
            i += 1;
        }
        (i, i - start)
    };
    if b.get(i) == Some(&b'-') {
        i += 1;
    }
    match b.get(i) {
        Some(b'0') => i += 1,
        Some(b'1'..=b'9') => i = digits(b, i).0,
        _ => return err(i, "invalid number"),
    }
    if b.get(i) == Some(&b'.') {
        let (next, count) = digits(b, i + 1);
        if count == 0 {
            return err(i, "invalid number");
        }
        i = next;
    }
    if matches!(b.get(i), Some(b'e' | b'E')) {
        i += 1;
        if matches!(b.get(i), Some(b'+' | b'-')) {
            i += 1;
        }
        let (next, count) = digits(b, i);
        if count == 0 {
            return err(i, "invalid number");
        }
        i = next;
    }
    Ok(i)
}

/// The fields of an object, with the name still quoted and the value as a text span.
fn raw_fields(s: &str) -> Result<Vec<(&str, &str)>> {
    let b = s.as_bytes();
    let mut i = ws(b, 0);
    if b.get(i) != Some(&b'{') {
        return err(i, "expected an object");
    }
    i = ws(b, i + 1);
    let mut out = Vec::new();
    if b.get(i) == Some(&b'}') {
        return finish(b, i + 1, out);
    }
    loop {
        let key_end = skip_string(b, i)?;
        let key = &s[i..key_end];
        i = ws(b, key_end);
        if b.get(i) != Some(&b':') {
            return err(i, "esperava ':'");
        }
        let value_start = ws(b, i + 1);
        let value_end = skip(b, value_start, 1)?;
        out.push((key, &s[value_start..value_end]));
        i = ws(b, value_end);
        match b.get(i) {
            Some(b',') => i = ws(b, i + 1),
            Some(b'}') => return finish(b, i + 1, out),
            _ => return err(i, "expected ',' or '}'"),
        }
    }
}

fn finish<'a>(b: &[u8], i: usize, out: Vec<(&'a str, &'a str)>) -> Result<Vec<(&'a str, &'a str)>> {
    let i = ws(b, i);
    if i != b.len() {
        return err(i, "text after the document");
    }
    Ok(out)
}

/// The fields of an object: decoded name and the text span of each value.
pub fn object_fields(s: &str) -> Result<Vec<(String, &str)>> {
    raw_fields(s)?
        .into_iter()
        .map(|(k, v)| Ok((decode_string(k)?, v)))
        .collect()
}

/// The items of a list, each one as a text span.
pub fn array_items(s: &str) -> Result<Vec<&str>> {
    let b = s.as_bytes();
    let mut i = ws(b, 0);
    if b.get(i) != Some(&b'[') {
        return err(i, "expected a list");
    }
    i = ws(b, i + 1);
    let mut out = Vec::new();
    if b.get(i) == Some(&b']') {
        finish(b, i + 1, Vec::new())?;
        return Ok(out);
    }
    loop {
        let end = skip(b, i, 1)?;
        out.push(s[i..end].trim());
        i = ws(b, end);
        match b.get(i) {
            Some(b',') => i = ws(b, i + 1),
            Some(b']') => {
                finish(b, i + 1, Vec::new())?;
                return Ok(out);
            }
            _ => return err(i, "expected ',' or ']'"),
        }
    }
}

/// Decodes a JSON string (`raw` includes the quotes): escapes, `\uXXXX` and surrogate pairs.
pub fn decode_string(raw: &str) -> Result<String> {
    let raw = raw.trim();
    let b = raw.as_bytes();
    if b.len() < 2 || b[0] != b'"' || b[b.len() - 1] != b'"' {
        return err(0, "expected a string");
    }
    let inner = &raw[1..raw.len() - 1];
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.char_indices();
    while let Some((at, c)) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        let Some((_, e)) = chars.next() else {
            return err(at, "escape incompleto");
        };
        match e {
            '"' => out.push('"'),
            '\\' => out.push('\\'),
            '/' => out.push('/'),
            'b' => out.push('\u{8}'),
            'f' => out.push('\u{c}'),
            'n' => out.push('\n'),
            'r' => out.push('\r'),
            't' => out.push('\t'),
            'u' => {
                let first = hex4(&mut chars, at)?;
                if (0xD800..0xDC00).contains(&first) {
                    // Surrogate pair: the second escape must follow right away.
                    let mut ahead = chars.clone();
                    let paired = match (ahead.next(), ahead.next()) {
                        (Some((_, '\\')), Some((_, 'u'))) => {
                            let second = hex4(&mut ahead, at)?;
                            if (0xDC00..0xE000).contains(&second) {
                                chars = ahead;
                                Some(0x10000 + ((first - 0xD800) << 10) + (second - 0xDC00))
                            } else {
                                None
                            }
                        }
                        _ => None,
                    };
                    out.push(paired.and_then(char::from_u32).unwrap_or('\u{FFFD}'));
                } else {
                    out.push(char::from_u32(first).unwrap_or('\u{FFFD}'));
                }
            }
            _ => return err(at, "invalid escape"),
        }
    }
    Ok(out)
}

fn hex4(chars: &mut std::str::CharIndices<'_>, at: usize) -> Result<u32> {
    let mut value = 0;
    for _ in 0..4 {
        let digit = chars.next().and_then(|(_, c)| c.to_digit(16));
        match digit {
            Some(d) => value = value * 16 + d,
            None => return err(at, "invalid \\u escape"),
        }
    }
    Ok(value)
}

/// Writes `s` as a JSON string, in quotes.
pub fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The fields of an object, searchable by name.
pub struct Fields<'a>(Vec<(String, &'a str)>);

impl<'a> Fields<'a> {
    pub fn parse(s: &'a str) -> Result<Self> {
        object_fields(s).map(Fields)
    }

    /// The field's text span, if it exists.
    pub fn raw(&self, name: &str) -> Option<&'a str> {
        self.0.iter().find(|(k, _)| k == name).map(|(_, v)| *v)
    }

    /// The field as text, if it exists and is a string.
    pub fn string(&self, name: &str) -> Option<String> {
        let raw = self.raw(name)?;
        if raw.starts_with('"') {
            decode_string(raw).ok()
        } else {
            None
        }
    }

    /// The field as an unsigned integer, if it exists and is such a number.
    pub fn unsigned(&self, name: &str) -> Option<u64> {
        self.raw(name)?.parse().ok()
    }

    /// The field as a boolean, if it exists and is `true` or `false`.
    pub fn boolean(&self, name: &str) -> Option<bool> {
        match self.raw(name)? {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        }
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.0.iter().map(|(k, _)| k.as_str())
    }
}

/// Re-indents a JSON document with two spaces, without touching the text of strings and numbers.
pub fn pretty(s: &str) -> Result<String> {
    let mut out = String::with_capacity(s.len() * 2);
    pretty_into(s.trim(), &mut out, 0, 0)?;
    Ok(out)
}

fn pretty_into(s: &str, out: &mut String, indent: usize, depth: usize) -> Result<()> {
    if depth > MAX_DEPTH {
        return err(0, "nesting too deep");
    }
    match s.as_bytes().first() {
        Some(b'{') => {
            let fields = raw_fields(s)?;
            if fields.is_empty() {
                out.push_str("{}");
                return Ok(());
            }
            out.push_str("{\n");
            for (n, (key, value)) in fields.iter().enumerate() {
                out.push_str(&" ".repeat(indent + 2));
                out.push_str(key);
                out.push_str(": ");
                pretty_into(value, out, indent + 2, depth + 1)?;
                out.push_str(if n + 1 < fields.len() { ",\n" } else { "\n" });
            }
            out.push_str(&" ".repeat(indent));
            out.push('}');
        }
        Some(b'[') => {
            let items = array_items(s)?;
            if items.is_empty() {
                out.push_str("[]");
                return Ok(());
            }
            out.push_str("[\n");
            for (n, item) in items.iter().enumerate() {
                out.push_str(&" ".repeat(indent + 2));
                pretty_into(item, out, indent + 2, depth + 1)?;
                out.push_str(if n + 1 < items.len() { ",\n" } else { "\n" });
            }
            out.push_str(&" ".repeat(indent));
            out.push(']');
        }
        _ => {
            let end = skip_value(s, 0)?;
            if ws(s.as_bytes(), end) != s.len() {
                return err(end, "text after the document");
            }
            out.push_str(s.trim());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
