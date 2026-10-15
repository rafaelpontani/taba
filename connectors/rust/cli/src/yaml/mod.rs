//! The subset of YAML that `taba-cli` reads and writes (doc 19b, section 18.11.2): maps nested by
//! indentation and texts (plain, in single or double quotes), with `#` comments. No lists,
//! anchors, multi-line blocks or flow maps (except an empty `{}`).

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    Map(Vec<(String, Node)>),
    Text(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YamlError {
    pub line: usize,
    pub message: String,
}

impl fmt::Display for YamlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for YamlError {}

fn fail<T>(line: usize, message: impl Into<String>) -> Result<T, YamlError> {
    Err(YamlError {
        line,
        message: message.into(),
    })
}

struct Line {
    number: usize,
    indent: usize,
    key: String,
    /// `None` when there is nothing after the colon (a child map follows, or an empty map).
    value: Option<String>,
}

/// Reads a document. The result is always a map (empty if the text only has comments).
pub fn parse(text: &str) -> Result<Vec<(String, Node)>, YamlError> {
    let mut lines = Vec::new();
    for (n, raw) in text.lines().enumerate() {
        let number = n + 1;
        let raw = if number == 1 {
            raw.trim_start_matches('\u{feff}')
        } else {
            raw
        };
        let content = strip_comment(raw, number)?;
        if content.trim().is_empty() {
            continue;
        }
        if content.starts_with('\t') || content.trim_start_matches(' ').starts_with('\t') {
            return fail(number, "tab in the indentation (use spaces)");
        }
        let indent = content.len() - content.trim_start_matches(' ').len();
        let body = content.trim();
        if body.starts_with("- ") || body == "-" {
            return fail(number, "lists are not supported");
        }
        if body == "---" || body == "..." {
            return fail(number, "document markers are not supported");
        }
        let (key, rest) = split_key(body, number)?;
        let value = match rest.trim() {
            "" => None,
            "{}" => Some("{}".to_owned()),
            r if r.starts_with('{') || r.starts_with('[') => {
                return fail(number, "flow maps and lists are not supported");
            }
            r if r.starts_with(['&', '*', '!']) => {
                return fail(number, "anchors, aliases and tags are not supported");
            }
            r if r.starts_with(['|', '>']) => {
                return fail(number, "multi-line blocks are not supported");
            }
            r => Some(r.to_owned()),
        };
        lines.push(Line {
            number,
            indent,
            key,
            value,
        });
    }
    let mut pos = 0;
    let map = parse_map(&lines, &mut pos, 0)?;
    if pos < lines.len() {
        return fail(lines[pos].number, "unexpected indentation");
    }
    Ok(map)
}

fn parse_map(
    lines: &[Line],
    pos: &mut usize,
    indent: usize,
) -> Result<Vec<(String, Node)>, YamlError> {
    let mut out: Vec<(String, Node)> = Vec::new();
    while *pos < lines.len() {
        let line = &lines[*pos];
        if line.indent < indent {
            break;
        }
        if line.indent > indent {
            return fail(line.number, "unexpected indentation");
        }
        if out.iter().any(|(k, _)| *k == line.key) {
            return fail(line.number, format!("the key `{}` appears twice", line.key));
        }
        *pos += 1;
        let node = match &line.value {
            Some(v) if v == "{}" => Node::Map(Vec::new()),
            Some(v) => Node::Text(scalar(v, line.number)?),
            None => match lines.get(*pos) {
                Some(next) if next.indent > indent => {
                    Node::Map(parse_map(lines, pos, next.indent)?)
                }
                _ => Node::Map(Vec::new()),
            },
        };
        out.push((line.key.clone(), node));
    }
    Ok(out)
}

/// Strips the comment (`#` at the start or after a space, outside quotes).
fn strip_comment(line: &str, number: usize) -> Result<String, YamlError> {
    let mut quote: Option<char> = None;
    let mut prev = ' ';
    let mut chars = line.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        match quote {
            Some('"') => {
                if c == '\\' {
                    chars.next();
                } else if c == '"' {
                    quote = None;
                }
            }
            Some(_) => {
                if c == '\'' {
                    if chars.peek().is_some_and(|(_, n)| *n == '\'') {
                        chars.next();
                    } else {
                        quote = None;
                    }
                }
            }
            None => {
                // Quotes only open a text at the start of the value (or the key); in the middle they are ordinary.
                let opens = {
                    let head = line[..i].trim_end();
                    head.is_empty() || head.ends_with(':')
                };
                if (c == '"' || c == '\'') && opens {
                    quote = Some(c);
                } else if c == '#' && (prev == ' ' || i == 0) {
                    return Ok(line[..i].trim_end().to_owned());
                }
            }
        }
        prev = c;
    }
    if quote.is_some() {
        return fail(number, "unterminated quoted text");
    }
    Ok(line.trim_end().to_owned())
}

/// Splits `key: rest`. The key can come in quotes.
fn split_key(body: &str, number: usize) -> Result<(String, &str), YamlError> {
    if body.starts_with('"') || body.starts_with('\'') {
        let q = body.chars().next().unwrap_or('"');
        let mut end = None;
        let mut chars = body.char_indices().skip(1);
        while let Some((i, c)) = chars.next() {
            if q == '"' && c == '\\' {
                chars.next();
            } else if c == q {
                end = Some(i);
                break;
            }
        }
        let end = end.ok_or_else(|| YamlError {
            line: number,
            message: "unterminated quoted key".into(),
        })?;
        let key = scalar(&body[..=end], number)?;
        let rest = body[end + 1..].trim_start();
        let rest = rest.strip_prefix(':').ok_or_else(|| YamlError {
            line: number,
            message: "expected `:` after the key".into(),
        })?;
        return Ok((key, rest));
    }
    // Plain key: goes up to the first `:` followed by a space or the end.
    let bytes = body.as_bytes();
    for (i, b) in bytes.iter().enumerate() {
        if *b == b':' && (i + 1 == bytes.len() || bytes[i + 1] == b' ') {
            let key = body[..i].trim();
            if key.is_empty() {
                return fail(number, "empty key");
            }
            return Ok((key.to_owned(), &body[i + 1..]));
        }
    }
    fail(number, "expected `key: value`")
}

fn scalar(text: &str, number: usize) -> Result<String, YamlError> {
    let text = text.trim();
    if let Some(inner) = text.strip_prefix('"') {
        let inner = inner
            .strip_suffix('"')
            .filter(|_| text.len() >= 2)
            .ok_or_else(|| YamlError {
                line: number,
                message: "unterminated double-quoted text".into(),
            })?;
        let mut out = String::new();
        let mut chars = inner.chars();
        while let Some(c) = chars.next() {
            if c != '\\' {
                out.push(c);
                continue;
            }
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some('0') => out.push('\0'),
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                Some('/') => out.push('/'),
                Some(' ') => out.push(' '),
                Some('u') => {
                    let hex: String = chars.by_ref().take(4).collect();
                    let c = u32::from_str_radix(&hex, 16)
                        .ok()
                        .filter(|_| hex.len() == 4)
                        .and_then(char::from_u32)
                        .ok_or_else(|| YamlError {
                            line: number,
                            message: "invalid \\u escape".into(),
                        })?;
                    out.push(c);
                }
                _ => return fail(number, "invalid escape in double-quoted text"),
            }
        }
        return Ok(out);
    }
    if let Some(inner) = text.strip_prefix('\'') {
        let inner = inner
            .strip_suffix('\'')
            .filter(|_| text.len() >= 2)
            .ok_or_else(|| YamlError {
                line: number,
                message: "unterminated single-quoted text".into(),
            })?;
        return Ok(inner.replace("''", "'"));
    }
    if text == "~" || text == "null" {
        return Ok(String::new());
    }
    Ok(text.to_owned())
}

/// The text of a value, in quotes only when needed.
pub fn render_scalar(value: &str) -> String {
    let lower = value.to_ascii_lowercase();
    let needs_quotes = value.is_empty()
        || value != value.trim()
        || value.contains(" #")
        || value.contains(": ")
        || value.ends_with(':')
        || value.chars().any(|c| c.is_control())
        || value.starts_with([
            '-', '?', ':', ',', '[', ']', '{', '}', '#', '&', '*', '!', '|', '>', '\'', '"', '%',
            '@', '`',
        ])
        || matches!(
            lower.as_str(),
            "null" | "~" | "true" | "false" | "yes" | "no" | "on" | "off"
        );
    if !needs_quotes {
        return value.to_owned();
    }
    let mut out = String::from("\"");
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Writes the map with two spaces per level.
pub fn render(map: &[(String, Node)]) -> String {
    let mut out = String::new();
    render_into(map, 0, &mut out);
    out
}

fn render_into(map: &[(String, Node)], indent: usize, out: &mut String) {
    for (key, node) in map {
        out.push_str(&" ".repeat(indent));
        out.push_str(&render_scalar(key));
        match node {
            Node::Text(t) => {
                out.push_str(": ");
                out.push_str(&render_scalar(t));
                out.push('\n');
            }
            Node::Map(children) if children.is_empty() => out.push_str(": {}\n"),
            Node::Map(children) => {
                out.push_str(":\n");
                render_into(children, indent + 2, out);
            }
        }
    }
}

#[cfg(test)]
mod tests;
