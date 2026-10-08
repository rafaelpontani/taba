//! O `Transport` real (doc 18, seção 17.5): as chamadas entre nós como requisições HTTP à porta do
//! cluster do outro nó (doc 06c), com TLS mútuo quando há pasta de certificados. A tradução entre
//! a chamada tipada e a requisição HTTP é a do `protocol::http`, a mesma que a porta do cluster
//! usa ao receber.
//!
//! Conexões saem e ficam abertas pelo `reqwest`; o certificado que o nó apresenta é escolhido a
//! cada handshake (`cluster::tls`), então a rotação vale para as conexões novas, e as abertas
//! seguem com o certificado antigo até caírem.

use std::net::{IpAddr, ToSocketAddrs, UdpSocket};
use std::sync::Arc;
use std::time::Duration;

use super::{BoxFuture, CallError, PeerAddr, Transport};
use crate::cluster::protocol::http::HttpReply;
use crate::cluster::protocol::{Request, Response};
use crate::cluster::tls::TlsStore;

/// O prazo de abrir a conexão (o prazo da chamada inteira é de quem chama).
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

pub struct HttpTransport {
    client: reqwest::Client,
    scheme: &'static str,
}

impl HttpTransport {
    /// Sem TLS: a porta do cluster atende em HTTP puro (sem pasta de certificados).
    pub fn plain() -> Result<Self, String> {
        let client = base_builder().build().map_err(|e| e.to_string())?;
        Ok(Self {
            client,
            scheme: "http",
        })
    }

    /// Com TLS mútuo, com os certificados da pasta.
    pub fn mutual_tls(store: &Arc<TlsStore>) -> Result<Self, String> {
        let config = store.client_config()?;
        let client = base_builder()
            .use_preconfigured_tls(config)
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Self {
            client,
            scheme: "https",
        })
    }

    /// `mutual_tls` se há pasta, senão `plain`.
    pub fn new(store: Option<&Arc<TlsStore>>) -> Result<Self, String> {
        Self::build(store, None)
    }

    /// Como `new`, mas as conexões saem do IP `local`. O cluster reconhece o chamador pelo IP
    /// de origem da conexão; em loopback o sistema escolheria `127.0.0.1` para todos, então os
    /// testes com vários nós no mesmo host fixam o IP de cada um.
    pub fn bound_to(store: Option<&Arc<TlsStore>>, local: IpAddr) -> Result<Self, String> {
        Self::build(store, Some(local))
    }

    fn build(store: Option<&Arc<TlsStore>>, local: Option<IpAddr>) -> Result<Self, String> {
        let mut builder = base_builder().local_address(local);
        let scheme = match store {
            Some(store) => {
                builder = builder.use_preconfigured_tls(store.client_config()?);
                "https"
            }
            None => "http",
        };
        Ok(Self {
            client: builder.build().map_err(|e| e.to_string())?,
            scheme,
        })
    }
}

fn base_builder() -> reqwest::ClientBuilder {
    reqwest::Client::builder()
        .no_proxy()
        .connect_timeout(CONNECT_TIMEOUT)
        .pool_idle_timeout(Duration::from_secs(60))
}

/// Uma falha de rede do `reqwest` como o `Transport` a conta.
fn network_error(error: &reqwest::Error) -> CallError {
    if error.is_timeout() {
        CallError::Timeout
    } else {
        // A cadeia de causas traz o motivo de verdade (conexão recusada, handshake recusado...).
        let mut why = error.to_string();
        let mut source = std::error::Error::source(error);
        while let Some(cause) = source {
            why.push_str(": ");
            why.push_str(&cause.to_string());
            source = cause.source();
        }
        CallError::Unreachable(why)
    }
}

fn authority(to: &PeerAddr) -> String {
    if to.host.contains(':') && !to.host.starts_with('[') {
        format!("[{}]:{}", to.host, to.port)
    } else {
        to.to_string()
    }
}

impl Transport for HttpTransport {
    fn call<'a>(
        &'a self,
        to: &'a PeerAddr,
        request: Request,
    ) -> BoxFuture<'a, Result<Response, CallError>> {
        Box::pin(async move {
            let mut call = request.to_http();
            // O contexto do trace segue com a chamada (doc 11, "Traces"; D119).
            crate::telemetry::trace::inject(&mut call.headers);
            let url = format!("{}://{}{}", self.scheme, authority(to), call.path);
            let method = reqwest::Method::from_bytes(call.method.as_bytes())
                .map_err(|e| CallError::Unreachable(e.to_string()))?;
            let mut builder = self.client.request(method, url);
            for (name, value) in &call.headers {
                builder = builder.header(name.as_str(), value.as_str());
            }
            if !call.body.is_empty() {
                builder = builder.body(call.body);
            }
            let response = builder.send().await.map_err(|e| network_error(&e))?;
            let status = response.status().as_u16();
            let headers = response
                .headers()
                .iter()
                .filter_map(|(k, v)| Some((k.as_str().to_owned(), v.to_str().ok()?.to_owned())))
                .collect();
            let body = response
                .bytes()
                .await
                .map_err(|e| network_error(&e))?
                .to_vec();
            request
                .parse_reply(HttpReply {
                    status,
                    headers,
                    body,
                })
                .map_err(CallError::Remote)
        })
    }
}

/// O IP que este nó usa para sair em direção a `entry` (o `cluster.entry-address`): o endereço
/// local de um soquete UDP "conectado" a ele, sem enviar nada (D95). Quem tem NAT ou várias
/// interfaces informa `cluster.advertise-addresses`.
pub fn discover_address(entry: &PeerAddr) -> Result<String, String> {
    let target = (entry.host.as_str(), entry.port)
        .to_socket_addrs()
        .map_err(|e| format!("não foi possível resolver `{entry}`: {e}"))?
        .next()
        .ok_or_else(|| format!("`{entry}` não resolve para nenhum endereço"))?;
    let bind = if target.is_ipv6() {
        "[::]:0"
    } else {
        "0.0.0.0:0"
    };
    let socket = UdpSocket::bind(bind).map_err(|e| e.to_string())?;
    socket
        .connect(target)
        .map_err(|e| format!("não foi possível escolher a interface para `{entry}`: {e}"))?;
    let local = socket.local_addr().map_err(|e| e.to_string())?;
    if local.ip().is_unspecified() {
        return Err(format!("sem interface de rede para chegar a `{entry}`"));
    }
    Ok(local.ip().to_string())
}

#[cfg(test)]
mod tests;
