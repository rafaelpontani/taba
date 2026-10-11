//! O cliente: a configuração, a chamada com a repetição do contrato comum (doc 19, seção 18.4) e as
//! nove operações da superfície (seção 18.3).

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use crate::base64;
use crate::http::{self, Request, Response, Url};
use crate::json::{self, Fields};
use crate::transport::{TcpTransport, Timeouts, Transport, TransportError};

/// A versão do contrato da API que este conector implementa (UC-CNT-028).
pub const CONTRACT_VERSION: &str = "1";

/// Acima disto o `consume` não espera mais que o `waitTime` somado à folga abaixo.
const LONG_POLL_SLACK: Duration = Duration::from_secs(10);

#[derive(Debug, Clone)]
pub struct Config {
    /// Endereço da API de cliente (`http://host:porta` ou `https://...`).
    pub base_url: String,
    pub token: Option<String>,
    pub connect_timeout: Duration,
    pub request_timeout: Duration,
    /// Tentativas, contando a primeira.
    pub max_attempts: u32,
    pub user_agent: String,
    /// Certificado de CA (PEM) a confiar, além das raízes do sistema.
    pub ca_pem: Option<Vec<u8>>,
}

impl Config {
    pub fn new(base_url: impl Into<String>) -> Config {
        Config {
            base_url: base_url.into(),
            token: None,
            connect_timeout: Duration::from_secs(5),
            request_timeout: Duration::from_secs(30),
            max_attempts: 3,
            user_agent: format!("taba-rust/{}", env!("CARGO_PKG_VERSION")),
            ca_pem: None,
        }
    }
}

/// A única forma de erro do conector: o `ErrorResponse` da API, ou uma falha do próprio cliente
/// (`status` zero e `code` `client.network`, `client.timeout` ou `client.invalid-config`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub status: u16,
    pub code: String,
    pub message: String,
    pub request_id: Option<String>,
    /// Segundos do `Retry-After`, se veio.
    pub retry_after: Option<u64>,
    /// A lista `details` do `ErrorResponse`, como texto JSON.
    pub details: Option<String>,
    /// O corpo da resposta, como veio (vazio em falha do cliente).
    pub body: Vec<u8>,
}

impl Error {
    /// Um erro do próprio cliente (`status` zero), para quem usa o conector montar o seu.
    pub fn from_message(code: &str, message: impl Into<String>) -> Error {
        Error::client(code, message)
    }

    fn client(code: &str, message: impl Into<String>) -> Error {
        Error {
            status: 0,
            code: code.to_owned(),
            message: message.into(),
            request_id: None,
            retry_after: None,
            details: None,
            body: Vec::new(),
        }
    }

    /// Falha do próprio cliente, e não uma resposta do servidor.
    pub fn is_client_side(&self) -> bool {
        self.status == 0
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)?;
        if let Some(id) = &self.request_id {
            write!(f, " (requestId {id})")?;
        }
        Ok(())
    }
}

impl std::error::Error for Error {}

/// Uma chamada à API, antes de virar HTTP.
#[derive(Debug, Clone)]
pub struct Call {
    pub method: &'static str,
    /// Caminho já com os segmentos codificados (`/topics/pedidos`).
    pub path: String,
    pub query: Vec<(String, String)>,
    pub body: Vec<u8>,
    /// Só vale com corpo; padrão `application/json`.
    /// Cabeçalhos a mais (um `Accept` próprio troca o padrão `application/json`).
    pub headers: Vec<(String, String)>,
    pub content_type: Option<String>,
    pub request_id: Option<String>,
    pub traceparent: Option<String>,
    /// Troca o tempo total da chamada (o `consume` usa `waitTime + 10 s`).
    pub timeout: Option<Duration>,
    /// Repete `502` e `504`: só a publicação em fila FIFO, em que o primário deduplica.
    pub retry_gateway: bool,
    /// Depois de um `503`, estas respostas contam como sucesso (a operação já tinha sido aplicada):
    /// criar tópico (`409 topic.already-exists`) e apagar (`404`).
    pub recover: Vec<(u16, &'static str)>,
}

impl Call {
    pub fn new(method: &'static str, path: impl Into<String>) -> Call {
        Call {
            method,
            path: path.into(),
            query: Vec::new(),
            body: Vec::new(),
            headers: Vec::new(),
            content_type: None,
            request_id: None,
            traceparent: None,
            timeout: None,
            retry_gateway: false,
            recover: Vec::new(),
        }
    }

    pub fn query(mut self, key: &str, value: impl ToString) -> Call {
        self.query.push((key.to_owned(), value.to_string()));
        self
    }

    pub fn json_body(mut self, body: String) -> Call {
        self.body = body.into_bytes();
        self
    }
}

/// A resposta de uma chamada que deu certo.
#[derive(Debug, Clone)]
pub struct Reply {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub request_id: Option<String>,
    /// `true` quando a resposta é um `409`/`404` aceito como sucesso depois de um `503` (a operação
    /// tinha sido aplicada na tentativa anterior).
    pub recovered: bool,
}

impl Reply {
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

type Sleeper = Arc<dyn Fn(Duration) + Send + Sync>;

pub struct Client {
    config: Config,
    url: Url,
    transport: Box<dyn Transport>,
    sleeper: Sleeper,
}

impl Client {
    pub fn new(config: Config) -> Result<Client, Error> {
        let transport = TcpTransport::new(config.ca_pem.clone());
        Client::with_transport(config, Box::new(transport), Arc::new(std::thread::sleep))
    }

    /// Para trocar a pilha de rede (e, nos testes, a espera entre tentativas).
    pub fn with_transport(
        config: Config,
        transport: Box<dyn Transport>,
        sleeper: Sleeper,
    ) -> Result<Client, Error> {
        let url =
            Url::parse(&config.base_url).map_err(|m| Error::client("client.invalid-config", m))?;
        Ok(Client {
            config,
            url,
            transport,
            sleeper,
        })
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    fn wire_request(&self, call: &Call) -> Request {
        let mut headers = Vec::new();
        if let Some(token) = &self.config.token {
            headers.push(("Authorization".to_owned(), format!("Bearer {token}")));
        }
        if !call
            .headers
            .iter()
            .any(|(k, _)| k.eq_ignore_ascii_case("accept"))
        {
            headers.push(("Accept".to_owned(), "application/json".to_owned()));
        }
        headers.extend(call.headers.iter().cloned());
        headers.push(("User-Agent".to_owned(), self.config.user_agent.clone()));
        if !call.body.is_empty() {
            headers.push((
                "Content-Type".to_owned(),
                call.content_type
                    .clone()
                    .unwrap_or_else(|| "application/json".to_owned()),
            ));
        }
        if let Some(id) = &call.request_id {
            headers.push(("X-Request-Id".to_owned(), id.clone()));
        }
        if let Some(tp) = &call.traceparent {
            headers.push(("traceparent".to_owned(), tp.clone()));
        }
        Request {
            method: call.method.to_owned(),
            target: http::build_target(&self.url.base_path, &call.path, &call.query),
            headers,
            body: call.body.clone(),
        }
    }

    /// Faz a chamada, repetindo só o que o contrato manda repetir (seção 18.4):
    /// `429` e `503` (esperando o `Retry-After`, ou 1 s, 2 s, 4 s), `502` e `504` só com
    /// `retry_gateway`, e falha de rede **antes** de enviar. Nunca um `4xx` (fora o `429`), nem
    /// uma falha depois de enviar o corpo, nem um estouro de tempo.
    pub fn call(&self, call: &Call) -> Result<Reply, Error> {
        let request = self.wire_request(call);
        let timeouts = Timeouts {
            connect: self.config.connect_timeout,
            total: call.timeout.unwrap_or(self.config.request_timeout),
        };
        let max = self.config.max_attempts.max(1);
        let mut attempt = 1;
        let mut after_503 = false;
        loop {
            let backoff = Duration::from_secs(1 << (attempt - 1).min(6));
            match self.transport.send(&self.url, &request, &timeouts) {
                Ok(response) => {
                    if response.status < 400 {
                        return Ok(reply(response, false));
                    }
                    if after_503
                        && call.recover.iter().any(|(status, code)| {
                            *status == response.status
                                && error_code(&response).as_deref() == Some(*code)
                        })
                    {
                        return Ok(reply(response, true));
                    }
                    let error = error_from(&response);
                    let retry = match response.status {
                        429 | 503 => true,
                        502 | 504 => call.retry_gateway,
                        _ => false,
                    };
                    if !retry || attempt >= max {
                        return Err(error);
                    }
                    after_503 |= response.status == 503;
                    (self.sleeper)(
                        error
                            .retry_after
                            .map(Duration::from_secs)
                            .unwrap_or(backoff),
                    );
                }
                Err(TransportError::Connect(message)) => {
                    if attempt >= max {
                        return Err(Error::client("client.network", message));
                    }
                    after_503 = true;
                    (self.sleeper)(backoff);
                }
                Err(TransportError::Timeout { sent: false }) => {
                    if attempt >= max {
                        return Err(Error::client(
                            "client.timeout",
                            "estourou o tempo ao abrir a conexão",
                        ));
                    }
                    after_503 = true;
                    (self.sleeper)(backoff);
                }
                Err(TransportError::Timeout { sent: true }) => {
                    return Err(Error::client(
                        "client.timeout",
                        "estourou o tempo esperando a resposta",
                    ));
                }
                Err(TransportError::Io(message)) => {
                    return Err(Error::client("client.network", message))
                }
                Err(TransportError::Protocol(message)) => {
                    return Err(Error::client(
                        "client.network",
                        format!("resposta inválida: {message}"),
                    ))
                }
            }
            attempt += 1;
        }
    }
}

fn reply(response: Response, recovered: bool) -> Reply {
    Reply {
        request_id: response.header("x-request-id").map(str::to_owned),
        status: response.status,
        headers: response.headers,
        body: response.body,
        recovered,
    }
}

fn error_code(response: &Response) -> Option<String> {
    let text = std::str::from_utf8(&response.body).ok()?;
    Fields::parse(text).ok()?.string("code")
}

/// O `ErrorResponse` da resposta; se o corpo não for um, o erro leva o código `http.<status>`.
fn error_from(response: &Response) -> Error {
    let text = String::from_utf8_lossy(&response.body);
    let fields = Fields::parse(&text).ok();
    let header_request_id = response.header("x-request-id").map(str::to_owned);
    let (code, message, request_id, details) = match &fields {
        Some(f) if f.string("code").is_some() => (
            f.string("code").unwrap_or_default(),
            f.string("message").unwrap_or_default(),
            f.string("requestId").or(header_request_id),
            f.raw("details").map(str::to_owned),
        ),
        _ => (
            format!("http.{}", response.status),
            if text.trim().is_empty() {
                format!("resposta {} sem corpo", response.status)
            } else {
                text.trim().chars().take(300).collect()
            },
            header_request_id,
            None,
        ),
    };
    Error {
        status: response.status,
        code,
        message,
        request_id,
        retry_after: response
            .header("retry-after")
            .and_then(|v| v.trim().parse().ok()),
        details,
        body: response.body.clone(),
    }
}

// ---------------------------------------------------------------------------------------------
// Modelos
// ---------------------------------------------------------------------------------------------

fn bad_response(what: &str, e: json::JsonError) -> Error {
    Error::client("client.invalid-response", format!("{what}: {e}"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Topic {
    pub id: String,
    pub name: String,
    /// `queue` ou `distributor`.
    pub topic_type: String,
    pub created_at: String,
    pub fifo: Option<bool>,
    pub volatile: Option<bool>,
    /// O JSON do tópico, como o servidor devolveu.
    pub raw: String,
}

impl Topic {
    pub fn parse(raw: &str) -> Result<Topic, Error> {
        let f = Fields::parse(raw).map_err(|e| bad_response("tópico", e))?;
        Ok(Topic {
            id: f.string("id").unwrap_or_default(),
            name: f.string("name").unwrap_or_default(),
            topic_type: f.string("type").unwrap_or_default(),
            created_at: f.string("createdAt").unwrap_or_default(),
            fifo: f.boolean("fifo"),
            volatile: f.boolean("volatile"),
            raw: raw.to_owned(),
        })
    }
}

/// Uma página (`Page<T>`): os itens e o cursor da próxima, se houver.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub next_cursor: Option<String>,
    pub raw: String,
}

fn parse_page<T>(raw: &str, item: impl Fn(&str) -> Result<T, Error>) -> Result<Page<T>, Error> {
    let f = Fields::parse(raw).map_err(|e| bad_response("página", e))?;
    let items_raw = f.raw("items").unwrap_or("[]");
    let items = json::array_items(items_raw)
        .map_err(|e| bad_response("itens da página", e))?
        .into_iter()
        .map(item)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Page {
        items,
        next_cursor: f.string("nextCursor"),
        raw: raw.to_owned(),
    })
}

/// Uma mensagem entregue. O `body` sai sempre como bytes (seção 18.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub id: String,
    pub body: Vec<u8>,
    pub content_type: String,
    pub receipt_token: String,
    pub delivery_count: u64,
    pub published_at: String,
    pub invisible_until: String,
    pub group_id: Option<String>,
    pub deduplication_id: Option<String>,
    pub dead_lettered_at: Option<String>,
    pub dead_letter_reason: Option<String>,
    /// O JSON da mensagem, como o servidor devolveu.
    pub raw: String,
}

fn is_json_type(content_type: &str) -> bool {
    let media = content_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    media == "application/json" || media.ends_with("+json")
}

impl Message {
    pub fn parse(raw: &str) -> Result<Message, Error> {
        let f = Fields::parse(raw).map_err(|e| bad_response("mensagem", e))?;
        let content_type = f
            .string("contentType")
            .unwrap_or_else(|| "application/json".to_owned());
        let body = match f.raw("body") {
            None => Vec::new(),
            Some(body) if f.string("bodyEncoding").as_deref() == Some("base64") => {
                let text =
                    json::decode_string(body).map_err(|e| bad_response("corpo da mensagem", e))?;
                base64::decode(&text).ok_or_else(|| {
                    Error::client("client.invalid-response", "corpo base64 inválido")
                })?
            }
            Some(body) if is_json_type(&content_type) => body.as_bytes().to_vec(),
            Some(body) => json::decode_string(body)
                .map_err(|e| bad_response("corpo da mensagem", e))?
                .into_bytes(),
        };
        Ok(Message {
            id: f.string("id").unwrap_or_default(),
            body,
            content_type,
            receipt_token: f.string("receiptToken").unwrap_or_default(),
            delivery_count: f.unsigned("deliveryCount").unwrap_or(0),
            published_at: f.string("publishedAt").unwrap_or_default(),
            invisible_until: f.string("invisibleUntil").unwrap_or_default(),
            group_id: f.string("groupId"),
            deduplication_id: f.string("deduplicationId"),
            dead_lettered_at: f.string("deadLetteredAt"),
            dead_letter_reason: f.string("deadLetterReason"),
            raw: raw.to_owned(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishResponse {
    /// O `id` da mensagem (fila). Ausente na resposta de um distribuidor.
    pub id: Option<String>,
    /// Só na fila FIFO.
    pub duplicate: Option<bool>,
    /// Só no distribuidor: quantas filas casaram.
    pub matched: Option<u64>,
    pub raw: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisibilityResponse {
    pub id: String,
    pub invisible_until: String,
    pub raw: String,
}

// ---------------------------------------------------------------------------------------------
// Requisições
// ---------------------------------------------------------------------------------------------

fn json_map(pairs: &[(String, String)]) -> String {
    let items: Vec<String> = pairs
        .iter()
        .map(|(k, v)| format!("{}:{}", json::quote(k), json::quote(v)))
        .collect();
    format!("{{{}}}", items.join(","))
}

fn json_list(items: &[String]) -> String {
    let items: Vec<String> = items.iter().map(|i| json::quote(i)).collect();
    format!("[{}]", items.join(","))
}

/// `POST /topics`. O conector manda só o que foi preenchido; o que não vale para o tipo, o servidor
/// recusa.
#[derive(Debug, Clone, Default)]
pub struct TopicRequest {
    pub name: String,
    /// `queue` ou `distributor`.
    pub topic_type: String,
    pub tags: Vec<(String, String)>,
    pub prefixes: Vec<String>,
    pub volatile: Option<bool>,
    pub idle_timeout: Option<u64>,
    pub fifo: Option<bool>,
    pub max_message_bytes: Option<u64>,
    pub ttl: Option<u64>,
    pub max_deliveries: Option<u64>,
}

impl TopicRequest {
    pub fn to_json(&self) -> String {
        let mut fields = vec![
            format!("\"name\":{}", json::quote(&self.name)),
            format!("\"type\":{}", json::quote(&self.topic_type)),
        ];
        if !self.tags.is_empty() {
            fields.push(format!("\"tags\":{}", json_map(&self.tags)));
        }
        if !self.prefixes.is_empty() {
            fields.push(format!("\"prefixes\":{}", json_list(&self.prefixes)));
        }
        if let Some(v) = self.volatile {
            fields.push(format!("\"volatile\":{v}"));
        }
        if let Some(v) = self.idle_timeout {
            fields.push(format!("\"idleTimeout\":{v}"));
        }
        if let Some(v) = self.fifo {
            fields.push(format!("\"fifo\":{v}"));
        }
        if let Some(v) = self.max_message_bytes {
            fields.push(format!("\"maxMessageBytes\":{v}"));
        }
        if let Some(v) = self.ttl {
            fields.push(format!("\"ttl\":{v}"));
        }
        if let Some(v) = self.max_deliveries {
            fields.push(format!("\"maxDeliveries\":{v}"));
        }
        format!("{{{}}}", fields.join(","))
    }
}

/// Um campo do `PATCH` que aceita voltar ao padrão do servidor (`null`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Nullable<T> {
    /// Não envia o campo.
    #[default]
    Keep,
    /// Envia `null`: volta ao padrão do servidor.
    Default,
    Set(T),
}

impl<T: fmt::Display> Nullable<T> {
    fn push(&self, name: &str, fields: &mut Vec<String>) {
        match self {
            Nullable::Keep => {}
            Nullable::Default => fields.push(format!("\"{name}\":null")),
            Nullable::Set(v) => fields.push(format!("\"{name}\":{v}")),
        }
    }
}

/// `PATCH /topics/{topic}`: só o que foi preenchido é enviado.
#[derive(Debug, Clone, Default)]
pub struct TopicUpdate {
    /// Substitui o mapa inteiro (`Some(vazio)` limpa as tags).
    pub tags: Option<Vec<(String, String)>>,
    pub prefixes: Option<Vec<String>>,
    pub idle_timeout: Option<u64>,
    pub max_message_bytes: Nullable<u64>,
    pub ttl: Nullable<u64>,
    pub max_deliveries: Nullable<u64>,
}

impl TopicUpdate {
    pub fn to_json(&self) -> String {
        let mut fields = Vec::new();
        if let Some(tags) = &self.tags {
            fields.push(format!("\"tags\":{}", json_map(tags)));
        }
        if let Some(prefixes) = &self.prefixes {
            fields.push(format!("\"prefixes\":{}", json_list(prefixes)));
        }
        if let Some(v) = self.idle_timeout {
            fields.push(format!("\"idleTimeout\":{v}"));
        }
        self.max_message_bytes.push("maxMessageBytes", &mut fields);
        self.ttl.push("ttl", &mut fields);
        self.max_deliveries.push("maxDeliveries", &mut fields);
        format!("{{{}}}", fields.join(","))
    }
}

#[derive(Debug, Clone, Default)]
pub struct PublishOptions {
    /// Padrão `application/json`.
    pub content_type: Option<String>,
    pub group_id: Option<String>,
    pub deduplication_id: Option<String>,
    pub request_id: Option<String>,
    /// Marque quando o tópico é uma fila FIFO: só nela o `502`/`504` pode ser repetido.
    pub fifo: bool,
}

#[derive(Debug, Clone, Default)]
pub struct ConsumeOptions {
    pub max_messages: Option<u32>,
    pub visibility_timeout: Option<u32>,
    pub wait_time: Option<u32>,
    pub auto_create: bool,
    /// Os quatro seguintes só valem com `auto_create`.
    pub volatile: Option<bool>,
    pub idle_timeout: Option<u32>,
    pub fifo: Option<bool>,
    pub tags: Vec<(String, String)>,
    pub request_id: Option<String>,
}

fn segment(s: &str) -> String {
    http::percent_encode(s)
}

// ---------------------------------------------------------------------------------------------
// As nove operações
// ---------------------------------------------------------------------------------------------

impl Client {
    /// `POST /topics`.
    pub fn create_topic(&self, request: &TopicRequest) -> Result<Topic, Error> {
        let mut call = Call::new("POST", "/topics").json_body(request.to_json());
        call.recover = vec![(409, "topic.already-exists")];
        let reply = self.call(&call)?;
        if reply.recovered {
            return self.get_topic(&request.name);
        }
        Topic::parse(&reply.text())
    }

    /// `GET /topics/{topic}`.
    pub fn get_topic(&self, name: &str) -> Result<Topic, Error> {
        let reply = self.call(&Call::new("GET", format!("/topics/{}", segment(name))))?;
        Topic::parse(&reply.text())
    }

    /// `GET /topics`. Os parâmetros que forem `None` não vão na query.
    pub fn list_topics(
        &self,
        limit: Option<u32>,
        cursor: Option<&str>,
        name: Option<&str>,
    ) -> Result<Page<Topic>, Error> {
        let mut call = Call::new("GET", "/topics");
        if let Some(limit) = limit {
            call = call.query("limit", limit);
        }
        if let Some(cursor) = cursor {
            call = call.query("cursor", cursor);
        }
        if let Some(name) = name {
            call = call.query("name", name);
        }
        parse_page(&self.call(&call)?.text(), Topic::parse)
    }

    /// Todos os tópicos, página a página.
    pub fn list_all_topics(&self, name: Option<&str>) -> Result<Vec<Topic>, Error> {
        let mut all = Vec::new();
        let mut cursor: Option<String> = None;
        loop {
            let page = self.list_topics(None, cursor.as_deref(), name)?;
            all.extend(page.items);
            match page.next_cursor {
                Some(next) => cursor = Some(next),
                None => return Ok(all),
            }
        }
    }

    /// `PATCH /topics/{topic}`.
    pub fn update_topic(&self, name: &str, update: &TopicUpdate) -> Result<Topic, Error> {
        let call =
            Call::new("PATCH", format!("/topics/{}", segment(name))).json_body(update.to_json());
        Topic::parse(&self.call(&call)?.text())
    }

    /// `DELETE /topics/{topic}`.
    pub fn delete_topic(&self, name: &str) -> Result<(), Error> {
        let mut call = Call::new("DELETE", format!("/topics/{}", segment(name)));
        call.recover = vec![(404, "topic.not-found")];
        self.call(&call).map(|_| ())
    }

    /// `POST /topics/{topic}/messages`. O corpo vai como bytes, sem validação.
    pub fn publish(
        &self,
        topic: &str,
        body: &[u8],
        options: &PublishOptions,
    ) -> Result<PublishResponse, Error> {
        let mut call = Call::new("POST", format!("/topics/{}/messages", segment(topic)));
        call.body = body.to_vec();
        call.content_type = Some(
            options
                .content_type
                .clone()
                .unwrap_or_else(|| "application/json".to_owned()),
        );
        if let Some(group) = &options.group_id {
            call = call.query("groupId", group);
        }
        if let Some(dedup) = &options.deduplication_id {
            call = call.query("deduplicationId", dedup);
        }
        call.request_id = options.request_id.clone();
        call.retry_gateway = options.fifo;
        // Uma publicação de corpo vazio ainda precisa de `Content-Type`: o corpo vai como veio.
        let raw = self.call(&call)?.text();
        let f = Fields::parse(&raw).map_err(|e| bad_response("publicação", e))?;
        Ok(PublishResponse {
            id: f.string("id"),
            duplicate: f.boolean("duplicate"),
            matched: f.unsigned("matched"),
            raw,
        })
    }

    pub fn publish_json(&self, topic: &str, json_text: &str) -> Result<PublishResponse, Error> {
        self.publish(topic, json_text.as_bytes(), &PublishOptions::default())
    }

    pub fn publish_text(&self, topic: &str, text: &str) -> Result<PublishResponse, Error> {
        let options = PublishOptions {
            content_type: Some("text/plain; charset=utf-8".to_owned()),
            ..PublishOptions::default()
        };
        self.publish(topic, text.as_bytes(), &options)
    }

    /// `GET /topics/{topic}/messages`. O tempo de leitura é `waitTime + 10 s`, e não o
    /// `requestTimeout`.
    pub fn consume(&self, topic: &str, options: &ConsumeOptions) -> Result<Vec<Message>, Error> {
        let call = self.consume_call(topic, options);
        let raw = self.call(&call)?.text();
        let f = Fields::parse(&raw).map_err(|e| bad_response("consumo", e))?;
        json::array_items(f.raw("messages").unwrap_or("[]"))
            .map_err(|e| bad_response("mensagens", e))?
            .into_iter()
            .map(Message::parse)
            .collect()
    }

    pub fn consume_call(&self, topic: &str, options: &ConsumeOptions) -> Call {
        let mut call = Call::new("GET", format!("/topics/{}/messages", segment(topic)));
        if let Some(v) = options.max_messages {
            call = call.query("maxMessages", v);
        }
        if let Some(v) = options.visibility_timeout {
            call = call.query("visibilityTimeout", v);
        }
        if let Some(v) = options.wait_time {
            call = call.query("waitTime", v);
        }
        if options.auto_create {
            call = call.query("autoCreate", true);
            if let Some(v) = options.volatile {
                call = call.query("volatile", v);
            }
            if let Some(v) = options.idle_timeout {
                call = call.query("idleTimeout", v);
            }
            if let Some(v) = options.fifo {
                call = call.query("fifo", v);
            }
            for (k, v) in &options.tags {
                call = call.query(&format!("tag.{k}"), v);
            }
        }
        call.request_id = options.request_id.clone();
        call.timeout =
            Some(Duration::from_secs(u64::from(options.wait_time.unwrap_or(0))) + LONG_POLL_SLACK);
        call
    }

    /// `DELETE /topics/{topic}/messages/{id}?receiptToken=`.
    pub fn ack(&self, topic: &str, message_id: &str, receipt_token: &str) -> Result<(), Error> {
        let call = Call::new(
            "DELETE",
            format!(
                "/topics/{}/messages/{}",
                segment(topic),
                segment(message_id)
            ),
        )
        .query("receiptToken", receipt_token);
        self.call(&call).map(|_| ())
    }

    /// `PUT /topics/{topic}/messages/{id}/visibility?receiptToken=`.
    pub fn change_visibility(
        &self,
        topic: &str,
        message_id: &str,
        receipt_token: &str,
        seconds: u32,
    ) -> Result<VisibilityResponse, Error> {
        let call = Call::new(
            "PUT",
            format!(
                "/topics/{}/messages/{}/visibility",
                segment(topic),
                segment(message_id)
            ),
        )
        .query("receiptToken", receipt_token)
        .json_body(format!("{{\"visibilityTimeout\":{seconds}}}"));
        let raw = self.call(&call)?.text();
        let f = Fields::parse(&raw).map_err(|e| bad_response("invisibilidade", e))?;
        Ok(VisibilityResponse {
            id: f.string("id").unwrap_or_default(),
            invisible_until: f.string("invisibleUntil").unwrap_or_default(),
            raw,
        })
    }

    /// O laço de consumo (D71.1): long polling (`waitTime` 20 e uma mensagem por vez, se não
    /// informados), chama o `handler` e confirma sozinho **só se ele devolver `Ok`**. `stop` é
    /// consultado antes de cada consumo e depois de cada mensagem; devolvendo `true`, o laço termina.
    /// Um erro do servidor ao consumir ou confirmar sobe.
    pub fn subscribe<H, S>(
        &self,
        topic: &str,
        options: &ConsumeOptions,
        mut stop: S,
        mut handler: H,
    ) -> Result<(), Error>
    where
        H: FnMut(&Message) -> Result<(), Box<dyn std::error::Error>>,
        S: FnMut() -> bool,
    {
        let mut options = options.clone();
        options.wait_time.get_or_insert(20);
        options.max_messages.get_or_insert(1);
        while !stop() {
            for message in self.consume(topic, &options)? {
                if handler(&message).is_ok() {
                    match self.ack(topic, &message.id, &message.receipt_token) {
                        // A mensagem já voltou à fila (prazo vencido, `409`) ou já foi embora (`404`):
                        // não há o que confirmar, e o laço segue.
                        Err(e) if e.status == 409 || e.status == 404 => {}
                        other => other?,
                    }
                }
                if stop() {
                    return Ok(());
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
