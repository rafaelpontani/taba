//! O que toca a rede entre nós fica atrás de um trait (doc 18, seção 17.5): o `Transport`.
//! A implementação real (`http`: HTTP e WebSocket, com mTLS) e a de teste (`memory`, que liga vários
//! nós do mesmo processo por canais em memória, com perda, atraso e partição programáveis) levam
//! as mesmas duas coisas: chamadas de pedido e resposta e o **canal** permanente entre dois nós.
//!
//! O trait fala em tipos do protocolo (`Request`, `Response`), não em bytes: cada
//! implementação sabe como levá-los. A parte do nó que **recebe** a chamada é um `Handler`, o
//! mesmo que as rotas `/cluster/*` usam.

pub mod http;
pub mod memory;

use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::str::FromStr;

use tokio::sync::mpsc;

use super::protocol::{ChannelMessage, Request, Response};
use crate::core::error::TabaError;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// O endereço de um nó para as chamadas entre nós: um IP (ou nome) e a porta do cluster.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PeerAddr {
    pub host: String,
    pub port: u16,
}

impl PeerAddr {
    pub fn new(host: impl Into<String>, port: u16) -> Self {
        Self {
            host: host.into(),
            port,
        }
    }
}

impl fmt::Display for PeerAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.host, self.port)
    }
}

impl FromStr for PeerAddr {
    type Err = String;

    /// `host:porta` (o formato de `cluster.entry-address`).
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (host, port) = s
            .trim()
            .rsplit_once(':')
            .ok_or_else(|| format!("`{s}` não é host:porta"))?;
        let port = port
            .parse::<u16>()
            .map_err(|_| format!("`{s}` não tem uma porta válida"))?;
        if host.is_empty() {
            return Err(format!("`{s}` não tem host"));
        }
        Ok(Self::new(host, port))
    }
}

/// Por que uma chamada a outro nó não deu certo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallError {
    /// Não houve resposta: conexão recusada, nó fora do ar, partição de rede.
    Unreachable(String),
    /// A resposta não veio no prazo.
    Timeout,
    /// O nó respondeu com um erro (status 4xx ou 5xx e o `ErrorResponse`).
    Remote(TabaError),
}

impl CallError {
    /// Falhas de rede: vale tentar de novo (ou outro nó). O erro que o par devolveu não.
    pub fn is_network(&self) -> bool {
        matches!(self, CallError::Unreachable(_) | CallError::Timeout)
    }
}

impl fmt::Display for CallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CallError::Unreachable(why) => write!(f, "sem resposta: {why}"),
            CallError::Timeout => f.write_str("prazo esgotado"),
            CallError::Remote(error) => write!(f, "{}: {}", error.code, error.message),
        }
    }
}

/// Quem abre o canal e com quem quer falar: os `nodeId` dos dois lados. O nó que atende confere que
/// o `to` é ele (um processo novo no mesmo IP e porta tem outro ID: `cluster.wrong-node`, D100).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelOpen {
    pub from: String,
    pub to: String,
}

/// Uma ponta do canal: o que se manda ao outro nó e o que ele manda. Soltar a ponta fecha o canal
/// (e o outro lado vê a recepção terminar).
pub struct ChannelLink {
    pub tx: mpsc::Sender<ChannelMessage>,
    pub rx: mpsc::Receiver<ChannelMessage>,
}

impl ChannelLink {
    /// As duas pontas de um canal em memória.
    pub fn pair() -> (ChannelLink, ChannelLink) {
        let (a_tx, b_rx) = mpsc::channel(CHANNEL_BUFFER);
        let (b_tx, a_rx) = mpsc::channel(CHANNEL_BUFFER);
        (
            ChannelLink { tx: a_tx, rx: a_rx },
            ChannelLink { tx: b_tx, rx: b_rx },
        )
    }
}

/// Quantas mensagens cabem em cada sentido antes de quem envia esperar.
pub const CHANNEL_BUFFER: usize = 256;

/// As chamadas entre nós.
pub trait Transport: Send + Sync + 'static {
    fn call<'a>(
        &'a self,
        to: &'a PeerAddr,
        request: Request,
    ) -> BoxFuture<'a, Result<Response, CallError>>;

    /// Abre o canal permanente com o nó em `to` (`GET /cluster/channel` com o upgrade para
    /// WebSocket, na rede real). O erro do nó (não é membro, outro processo no endereço) chega como
    /// `CallError::Remote`, antes de o canal existir.
    fn open_channel<'a>(
        &'a self,
        to: &'a PeerAddr,
        open: ChannelOpen,
    ) -> BoxFuture<'a, Result<ChannelLink, CallError>>;
}

/// Quem atende as chamadas que chegam a um nó pela porta do cluster.
pub trait Handler: Send + Sync + 'static {
    /// `caller` é o endereço de quem chama (o IP de origem da conexão). O membro é reconhecido
    /// por ele: a conexão não diz mais nada (todos usam o mesmo certificado do cluster).
    fn handle<'a>(
        &'a self,
        caller: &'a str,
        request: Request,
    ) -> BoxFuture<'a, Result<Response, TabaError>>;

    /// Confere se o canal que `caller` (o IP de origem) quer abrir é aceito: é membro e o `to` é
    /// este nó. A rota espera esta resposta antes de fazer o upgrade, para o erro virar status HTTP.
    fn admit_channel(&self, _caller: &str, _open: &ChannelOpen) -> Result<(), TabaError> {
        Ok(())
    }

    /// O canal foi aberto por `open.from`: passa a usar a ponta `link`.
    fn accept_channel(&self, _open: ChannelOpen, _link: ChannelLink) {}
}

#[cfg(test)]
mod tests;
