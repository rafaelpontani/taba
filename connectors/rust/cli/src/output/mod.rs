//! A saída: `json` (a resposta do servidor, indentada), `table` (colunas) e `raw`, e a forma dos
//! erros da API (doc 19b, seção 18.11.3).

use std::io::{self, Write};

use taba_client::json::{self, Fields};
use taba_client::Error as ApiError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Json,
    Table,
    Raw,
}

impl Format {
    pub fn parse(name: &str) -> Result<Format, String> {
        match name {
            "json" => Ok(Format::Json),
            "table" => Ok(Format::Table),
            "raw" => Ok(Format::Raw),
            other => Err(format!(
                "formato de saída `{other}` desconhecido (use json, table ou raw)"
            )),
        }
    }
}

/// O texto de uma resposta no formato pedido. `raw` devolve o texto como veio.
pub fn render(text: &str, format: Format) -> String {
    match format {
        Format::Json => json::pretty(text).unwrap_or_else(|_| text.trim_end().to_owned()),
        Format::Table => table(text).unwrap_or_else(|| render(text, Format::Json)),
        Format::Raw => text.trim_end().to_owned(),
    }
}

/// Uma tabela com os itens de uma lista (`items`, `messages` ou uma lista na raiz) ou, para um
/// objeto só, uma linha por campo. `None` se o texto não é JSON de um desses formatos.
pub fn table(text: &str) -> Option<String> {
    let text = text.trim();
    let rows: Vec<&str>;
    let mut single = false;
    if text.starts_with('[') {
        rows = json::array_items(text).ok()?;
    } else if text.starts_with('{') {
        let fields = Fields::parse(text).ok()?;
        match fields.raw("items").or_else(|| fields.raw("messages")) {
            Some(list) if list.starts_with('[') => rows = json::array_items(list).ok()?,
            _ => {
                rows = vec![text];
                single = true;
            }
        }
    } else {
        return None;
    }
    if rows.is_empty() {
        return Some("(vazio)".to_owned());
    }
    if single {
        let fields = json::object_fields(rows[0]).ok()?;
        let table: Vec<Vec<String>> = fields
            .iter()
            .map(|(k, v)| vec![k.clone(), cell(v)])
            .collect();
        return Some(layout(&["campo", "valor"], &table));
    }
    let mut columns: Vec<String> = Vec::new();
    let mut parsed = Vec::new();
    for row in &rows {
        let fields = json::object_fields(row).ok()?;
        for (k, _) in &fields {
            if !columns.contains(k) {
                columns.push(k.clone());
            }
        }
        parsed.push(fields);
    }
    let table: Vec<Vec<String>> = parsed
        .iter()
        .map(|fields| {
            columns
                .iter()
                .map(|c| {
                    fields
                        .iter()
                        .find(|(k, _)| k == c)
                        .map(|(_, v)| cell(v))
                        .unwrap_or_else(|| "-".to_owned())
                })
                .collect()
        })
        .collect();
    let header: Vec<&str> = columns.iter().map(String::as_str).collect();
    Some(layout(&header, &table))
}

/// O `ClusterStatus` em tabelas: os campos simples, uma linha por nó e uma por condição.
pub fn cluster_table(text: &str) -> Option<String> {
    let fields = Fields::parse(text.trim()).ok()?;
    let simple: Vec<Vec<String>> = [
        "scope",
        "partial",
        "worstCondition",
        "collectedAt",
        "summary",
    ]
    .iter()
    .filter_map(|name| fields.raw(name).map(|v| vec![(*name).to_owned(), cell(v)]))
    .collect();
    let mut out = layout(&["campo", "valor"], &simple);
    for (title, name) in [("nodes", "nodes"), ("conditions", "conditions")] {
        let list = fields.raw(name).filter(|l| l.starts_with('['))?;
        out.push_str(&format!("\n\n{title}\n"));
        out.push_str(&table(list)?);
    }
    Some(out)
}

/// O texto de uma célula: strings sem aspas, `null` como `-`, objetos e listas compactos.
fn cell(raw: &str) -> String {
    let raw = raw.trim();
    let text = if raw.starts_with('"') {
        json::decode_string(raw).unwrap_or_else(|_| raw.to_owned())
    } else if raw == "null" {
        "-".to_owned()
    } else if raw.starts_with('{') || raw.starts_with('[') {
        compact(raw)
    } else {
        raw.to_owned()
    };
    let text = text.replace(['\n', '\r', '\t'], " ");
    if text.chars().count() > 60 {
        format!("{}…", text.chars().take(59).collect::<String>())
    } else {
        text
    }
}

/// O JSON sem os espaços e quebras de linha fora das strings.
fn compact(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut in_string = false;
    let mut escaped = false;
    for c in raw.chars() {
        if in_string {
            out.push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
        } else if c == '"' {
            in_string = true;
            out.push(c);
        } else if !c.is_whitespace() {
            out.push(c);
        }
    }
    out
}

fn layout(header: &[&str], rows: &[Vec<String>]) -> String {
    let widths: Vec<usize> = (0..header.len())
        .map(|i| {
            rows.iter()
                .map(|r| r[i].chars().count())
                .chain(std::iter::once(header[i].chars().count()))
                .max()
                .unwrap_or(0)
        })
        .collect();
    let line = |cells: Vec<&str>| {
        let mut out = String::new();
        for (i, c) in cells.iter().enumerate() {
            if i + 1 == cells.len() {
                out.push_str(c);
            } else {
                out.push_str(&format!("{c:<width$}  ", width = widths[i]));
            }
        }
        out.trim_end().to_owned()
    };
    let mut lines = vec![line(header.to_vec())];
    for row in rows {
        lines.push(line(row.iter().map(String::as_str).collect()));
    }
    lines.join("\n")
}

/// Escreve um erro da API na saída de erro: `código: mensagem (requestId ...)`, os `details` e o
/// `Retry-After`. Com `json` e um `ErrorResponse` do servidor, escreve o corpo como veio (indentado).
pub fn write_api_error(w: &mut dyn Write, e: &ApiError, json: bool) -> io::Result<()> {
    if json && e.status > 0 {
        let body = String::from_utf8_lossy(&e.body);
        if Fields::parse(&body).is_ok_and(|f| f.string("code").is_some()) {
            writeln!(w, "{}", render(&body, Format::Json))?;
            if let Some(n) = e.retry_after {
                writeln!(w, "Retry-After: {n}")?;
            }
            return Ok(());
        }
    }
    writeln!(w, "{e}")?;
    if let Some(details) = &e.details {
        for item in json::array_items(details).unwrap_or_default() {
            if let Ok(f) = Fields::parse(item) {
                let field = f.string("field").unwrap_or_default();
                let message = f.string("message").unwrap_or_default();
                let code = f.string("code").unwrap_or_default();
                writeln!(w, "  - {field}: {message} ({code})")?;
            }
        }
    }
    if let Some(n) = e.retry_after {
        writeln!(w, "Retry-After: {n} s")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
