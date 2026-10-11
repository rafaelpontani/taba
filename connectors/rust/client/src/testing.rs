//! Um transporte de mentira para os testes de quem usa o conector (o `taba-cli` o usa): devolve
//! respostas combinadas e guarda o que foi enviado.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::client::{Client, Config};
use crate::http::{Request, Response, Url};
use crate::transport::{Timeouts, Transport, TransportError};

/// O que o transporte recebeu numa chamada.
#[derive(Debug, Clone)]
pub struct Seen {
    /// `http://host:porta` do servidor chamado.
    pub origin: String,
    pub request: Request,
    pub timeouts: Timeouts,
}

impl Seen {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.request
            .headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    pub fn body_text(&self) -> String {
        String::from_utf8_lossy(&self.request.body).into_owned()
    }
}

#[derive(Default)]
pub struct MockTransport {
    replies: Mutex<VecDeque<Result<Response, TransportError>>>,
    seen: Mutex<Vec<Seen>>,
}

impl MockTransport {
    pub fn new() -> Arc<MockTransport> {
        Arc::new(MockTransport::default())
    }

    /// Combina a próxima resposta.
    pub fn reply(&self, status: u16, headers: &[(&str, &str)], body: &str) {
        self.replies.lock().unwrap().push_back(Ok(Response {
            status,
            headers: headers
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            body: body.as_bytes().to_vec(),
        }));
    }

    /// Combina uma falha de transporte como próxima resposta.
    pub fn fail(&self, error: TransportError) {
        self.replies.lock().unwrap().push_back(Err(error));
    }

    pub fn seen(&self) -> Vec<Seen> {
        self.seen.lock().unwrap().clone()
    }

    /// Respostas combinadas que ninguém usou.
    pub fn pending(&self) -> usize {
        self.replies.lock().unwrap().len()
    }
}

impl Transport for Arc<MockTransport> {
    fn send(
        &self,
        url: &Url,
        request: &Request,
        timeouts: &Timeouts,
    ) -> Result<Response, TransportError> {
        self.seen.lock().unwrap().push(Seen {
            origin: url.origin(),
            request: request.clone(),
            timeouts: *timeouts,
        });
        self.replies
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(Err(TransportError::Connect(
                "sem resposta combinada no teste".into(),
            )))
    }
}

/// Um cliente sobre o transporte de mentira, sem esperar de verdade entre as tentativas.
pub fn client(config: Config, mock: &Arc<MockTransport>) -> Client {
    Client::with_transport(config, Box::new(mock.clone()), Arc::new(|_: Duration| {}))
        .expect("endereço de teste inválido")
}
