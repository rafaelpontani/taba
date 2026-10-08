//! O que toca a rede entre nós fica atrás de um trait (doc 18, seção 17.5): o `Transport`.
//! A implementação real (HTTP com mTLS) ainda não existe; a de teste (`memory`) liga vários
//! nós do mesmo processo por canais em memória, com perda, atraso e partição programáveis.
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

use super::protocol::{Request, Response};
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

/// As chamadas entre nós.
pub trait Transport: Send + Sync + 'static {
    fn call<'a>(
        &'a self,
        to: &'a PeerAddr,
        request: Request,
    ) -> BoxFuture<'a, Result<Response, CallError>>;
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
}

#[cfg(test)]
mod tests;
