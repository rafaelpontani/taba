//! O subconjunto de YAML que o `taba-cli` lê e escreve (doc 19b, seção 18.11.2): mapas aninhados por
//! indentação e textos (simples, entre aspas simples ou duplas), com comentários `#`. Nada de listas,
//! âncoras, blocos de várias linhas nem mapas de fluxo (exceto `{}` vazio).

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
        write!(f, "linha {}: {}", self.line, self.message)
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
    /// `None` quando depois dos dois-pontos não há nada (vem um mapa filho, ou um mapa vazio).
    value: Option<String>,
}

/// Lê um documento. O resultado é sempre um mapa (vazio se o texto só tem comentários).
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
            return fail(number, "tabulação na indentação (use espaços)");
        }
        let indent = content.len() - content.trim_start_matches(' ').len();
        let body = content.trim();
        if body.starts_with("- ") || body == "-" {
            return fail(number, "listas não são suportadas");
        }
        if body == "---" || body == "..." {
            return fail(number, "marcadores de documento não são suportados");
        }
        let (key, rest) = split_key(body, number)?;
        let value = match rest.trim() {
            "" => None,
            "{}" => Some("{}".to_owned()),
            r if r.starts_with('{') || r.starts_with('[') => {
                return fail(number, "mapas e listas de fluxo não são suportados");
            }
            r if r.starts_with(['&', '*', '!']) => {
                return fail(number, "âncoras, aliases e tags não são suportados");
            }
            r if r.starts_with(['|', '>']) => {
                return fail(number, "blocos de várias linhas não são suportados");
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
        return fail(lines[pos].number, "indentação inesperada");
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
            return fail(line.number, "indentação inesperada");
        }
        if out.iter().any(|(k, _)| *k == line.key) {
            return fail(
                line.number,
                format!("a chave `{}` aparece duas vezes", line.key),
            );
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

/// Tira o comentário (`#` no começo ou depois de um espaço, fora de aspas).
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
                // Aspas só abrem um texto no começo do valor (ou da chave); no meio são comuns.
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
        return fail(number, "texto entre aspas sem fechar");
    }
    Ok(line.trim_end().to_owned())
}

/// Separa `chave: resto`. A chave pode vir entre aspas.
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
            message: "chave entre aspas sem fechar".into(),
        })?;
        let key = scalar(&body[..=end], number)?;
        let rest = body[end + 1..].trim_start();
        let rest = rest.strip_prefix(':').ok_or_else(|| YamlError {
            line: number,
            message: "esperava `:` depois da chave".into(),
        })?;
        return Ok((key, rest));
    }
    // Chave simples: vai até o primeiro `:` seguido de espaço ou do fim.
    let bytes = body.as_bytes();
    for (i, b) in bytes.iter().enumerate() {
        if *b == b':' && (i + 1 == bytes.len() || bytes[i + 1] == b' ') {
            let key = body[..i].trim();
            if key.is_empty() {
                return fail(number, "chave vazia");
            }
            return Ok((key.to_owned(), &body[i + 1..]));
        }
    }
    fail(number, "esperava `chave: valor`")
}

fn scalar(text: &str, number: usize) -> Result<String, YamlError> {
    let text = text.trim();
    if let Some(inner) = text.strip_prefix('"') {
        let inner = inner
            .strip_suffix('"')
            .filter(|_| text.len() >= 2)
            .ok_or_else(|| YamlError {
                line: number,
                message: "texto entre aspas duplas sem fechar".into(),
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
                            message: "escape \\u inválido".into(),
                        })?;
                    out.push(c);
                }
                _ => return fail(number, "escape inválido no texto entre aspas duplas"),
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
                message: "texto entre aspas simples sem fechar".into(),
            })?;
        return Ok(inner.replace("''", "'"));
    }
    if text == "~" || text == "null" {
        return Ok(String::new());
    }
    Ok(text.to_owned())
}

/// O texto de um valor, entre aspas só quando precisa.
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

/// Escreve o mapa com dois espaços por nível.
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
