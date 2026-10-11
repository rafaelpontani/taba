//! HTTP/1.1 como funções puras, sem entrada e saída (doc 19a, seção 18.10.3): montar a requisição,
//! interpretar a resposta (`Content-Length`, `chunked` ou até o fim da conexão) e ler o endereço.
//! O transporte (TCP, TLS) só leva e traz bytes.

use std::io::{self, BufRead, Read};

/// Teto do corpo de uma resposta: um lote de mensagens grandes em base64 cabe com folga.
pub const MAX_BODY: usize = 256 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub method: String,
    /// Caminho mais query, já codificados (`/topics?limit=5`).
    pub target: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Response {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

/// O endereço de um servidor, lido de `http://host:porta` ou `https://host:porta/prefixo`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Url {
    pub https: bool,
    pub host: String,
    pub port: u16,
    /// Prefixo de caminho, sem barra no fim (vazio na raiz).
    pub base_path: String,
}

impl Url {
    pub fn parse(text: &str) -> Result<Url, String> {
        let text = text.trim();
        let (https, rest) = if let Some(rest) = text.strip_prefix("https://") {
            (true, rest)
        } else if let Some(rest) = text.strip_prefix("http://") {
            (false, rest)
        } else {
            return Err(format!(
                "o endereço `{text}` precisa começar com http:// ou https://"
            ));
        };
        let (authority, path) = match rest.find('/') {
            Some(i) => (&rest[..i], &rest[i..]),
            None => (rest, ""),
        };
        if authority.is_empty() || authority.contains('@') {
            return Err(format!("o endereço `{text}` não tem um host válido"));
        }
        let default_port = if https { 443 } else { 80 };
        let (host, port) = if let Some(inner) = authority.strip_prefix('[') {
            // IPv6 literal: `[::1]` ou `[::1]:8080`.
            let end = inner
                .find(']')
                .ok_or_else(|| format!("o endereço `{text}` tem um IPv6 sem `]`"))?;
            let port = match &inner[end + 1..] {
                "" => default_port,
                p => parse_port(p.strip_prefix(':').ok_or("porta inválida")?, text)?,
            };
            (inner[..end].to_owned(), port)
        } else {
            match authority.rsplit_once(':') {
                Some((host, port)) => (host.to_owned(), parse_port(port, text)?),
                None => (authority.to_owned(), default_port),
            }
        };
        if host.is_empty() {
            return Err(format!("o endereço `{text}` não tem um host válido"));
        }
        Ok(Url {
            https,
            host,
            port,
            base_path: path.trim_end_matches('/').to_owned(),
        })
    }

    /// O valor do cabeçalho `Host`.
    pub fn host_header(&self) -> String {
        let host = if self.host.contains(':') {
            format!("[{}]", self.host)
        } else {
            self.host.clone()
        };
        let default_port = if self.https { 443 } else { 80 };
        if self.port == default_port {
            host
        } else {
            format!("{host}:{}", self.port)
        }
    }

    /// O mesmo endereço, com outra porta.
    pub fn with_port(&self, port: u16) -> Url {
        Url {
            port,
            ..self.clone()
        }
    }

    /// O endereço como texto (`http://host:porta`), sem o prefixo de caminho.
    pub fn origin(&self) -> String {
        format!(
            "{}://{}",
            if self.https { "https" } else { "http" },
            self.host_header()
        )
    }
}

fn parse_port(port: &str, text: &str) -> Result<u16, String> {
    port.parse::<u16>()
        .ok()
        .filter(|p| *p != 0)
        .ok_or_else(|| format!("o endereço `{text}` tem uma porta inválida"))
}

/// Codifica um componente de query: só `A-Za-z0-9-._~` passa como está.
pub fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Caminho mais query (`/topics/x/messages?maxMessages=2`). Os segmentos do caminho já vêm
/// codificados por quem chama.
pub fn build_target(base_path: &str, path: &str, query: &[(String, String)]) -> String {
    let mut target = format!("{base_path}{path}");
    for (n, (key, value)) in query.iter().enumerate() {
        target.push(if n == 0 { '?' } else { '&' });
        target.push_str(&percent_encode(key));
        target.push('=');
        target.push_str(&percent_encode(value));
    }
    target
}

/// Os bytes da requisição. Acrescenta `Host`, `Content-Length` e `Connection: close`.
pub fn encode_request(host: &str, req: &Request) -> Vec<u8> {
    let mut head = format!("{} {} HTTP/1.1\r\nHost: {host}\r\n", req.method, req.target);
    for (k, v) in &req.headers {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    if !req.body.is_empty() || matches!(req.method.as_str(), "POST" | "PUT" | "PATCH") {
        head.push_str(&format!("Content-Length: {}\r\n", req.body.len()));
    }
    head.push_str("Connection: close\r\n\r\n");
    let mut bytes = head.into_bytes();
    bytes.extend_from_slice(&req.body);
    bytes
}

#[derive(Debug)]
pub enum ReadError {
    /// Falha de leitura; `received` diz se algum byte da resposta já tinha chegado.
    Io {
        error: io::Error,
        received: bool,
    },
    Malformed(&'static str),
}

fn io_err(error: io::Error, received: bool) -> ReadError {
    ReadError::Io { error, received }
}

fn read_line<R: BufRead>(r: &mut R, received: bool) -> Result<String, ReadError> {
    let mut line = Vec::new();
    let n = r
        .take(64 * 1024)
        .read_until(b'\n', &mut line)
        .map_err(|e| io_err(e, received))?;
    if n == 0 {
        return Err(io_err(io::ErrorKind::UnexpectedEof.into(), received));
    }
    if line.last() != Some(&b'\n') {
        return Err(ReadError::Malformed("linha de cabeçalho sem fim"));
    }
    while matches!(line.last(), Some(b'\n' | b'\r')) {
        line.pop();
    }
    String::from_utf8(line).map_err(|_| ReadError::Malformed("cabeçalho que não é UTF-8"))
}

/// Lê uma resposta completa. As respostas `1xx` são puladas.
pub fn read_response<R: BufRead>(r: &mut R) -> Result<Response, ReadError> {
    let (status, headers) = loop {
        let line = read_line(r, false)?;
        let mut parts = line.splitn(3, ' ');
        let version = parts.next().unwrap_or_default();
        if !version.starts_with("HTTP/1.") {
            return Err(ReadError::Malformed("linha de status inválida"));
        }
        let status: u16 = parts
            .next()
            .and_then(|s| s.parse().ok())
            .ok_or(ReadError::Malformed("código de status inválido"))?;
        let mut headers = Vec::new();
        loop {
            let line = read_line(r, true)?;
            if line.is_empty() {
                break;
            }
            let (name, value) = line
                .split_once(':')
                .ok_or(ReadError::Malformed("cabeçalho sem ':'"))?;
            headers.push((name.trim().to_owned(), value.trim().to_owned()));
        }
        if !(100..200).contains(&status) {
            break (status, headers);
        }
    };

    let header = |name: &str| {
        headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    };
    let chunked =
        header("transfer-encoding").is_some_and(|v| v.to_ascii_lowercase().contains("chunked"));
    let length = match header("content-length") {
        Some(v) => Some(
            v.parse::<usize>()
                .map_err(|_| ReadError::Malformed("Content-Length inválido"))?,
        ),
        None => None,
    };

    let body = if matches!(status, 204 | 304) {
        Vec::new()
    } else if chunked {
        read_chunked(r)?
    } else if let Some(n) = length {
        if n > MAX_BODY {
            return Err(ReadError::Malformed("corpo grande demais"));
        }
        let mut body = vec![0; n];
        r.read_exact(&mut body).map_err(|e| io_err(e, true))?;
        body
    } else {
        // Sem tamanho: o corpo vai até o fim da conexão. Um TLS fechado sem `close_notify` chega
        // como `UnexpectedEof`, e aqui isso é o fim.
        let mut body = Vec::new();
        match r.take(MAX_BODY as u64 + 1).read_to_end(&mut body) {
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => {}
            Err(e) => return Err(io_err(e, true)),
        }
        if body.len() > MAX_BODY {
            return Err(ReadError::Malformed("corpo grande demais"));
        }
        body
    };
    Ok(Response {
        status,
        headers,
        body,
    })
}

fn read_chunked<R: BufRead>(r: &mut R) -> Result<Vec<u8>, ReadError> {
    let mut body = Vec::new();
    loop {
        let line = read_line(r, true)?;
        let size_text = line.split(';').next().unwrap_or_default().trim();
        let size = usize::from_str_radix(size_text, 16)
            .map_err(|_| ReadError::Malformed("tamanho de bloco inválido"))?;
        if size == 0 {
            // Trailers até a linha vazia.
            while !read_line(r, true)?.is_empty() {}
            return Ok(body);
        }
        if body.len() + size > MAX_BODY {
            return Err(ReadError::Malformed("corpo grande demais"));
        }
        let start = body.len();
        body.resize(start + size, 0);
        r.read_exact(&mut body[start..])
            .map_err(|e| io_err(e, true))?;
        if !read_line(r, true)?.is_empty() {
            return Err(ReadError::Malformed("bloco sem CRLF no fim"));
        }
    }
}

#[cfg(test)]
mod tests;
