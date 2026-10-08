//! O `Transport` real (doc 18, seção 17.5): as chamadas entre nós como requisições HTTP à porta do
//! cluster do outro nó (doc 06c), com TLS mútuo quando há pasta de certificados. A tradução entre
//! a chamada tipada e a requisição HTTP é a do `protocol::http`, a mesma que a porta do cluster
//! usa ao receber. O **canal** entre dois nós é um WebSocket (`GET /cluster/channel` com upgrade)
//! aberto sobre a mesma conexão TCP e o mesmo TLS: cada mensagem é um texto JSON
//! ([`ChannelMessage`]).
//!
//! Conexões saem e ficam abertas pelo `reqwest`; o certificado que o nó apresenta é escolhido a
//! cada handshake (`cluster::tls`), então a rotação vale para as conexões novas, e as abertas
//! seguem com o certificado antigo até caírem.

use std::net::{IpAddr, SocketAddr, ToSocketAddrs, UdpSocket};
use std::sync::Arc;
use std::time::Duration;

use futures_util::{Sink, SinkExt, Stream, StreamExt};
use rustls::ClientConfig;
use rustls::pki_types::ServerName;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::{TcpSocket, TcpStream};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::{self, Message};

use super::{BoxFuture, CHANNEL_BUFFER, CallError, ChannelLink, ChannelOpen, PeerAddr, Transport};
use crate::cluster::protocol::http::{HttpReply, error_from_reply};
use crate::cluster::protocol::{ChannelMessage, Request, Response};
use crate::cluster::tls::TlsStore;

/// O prazo de abrir a conexão (o prazo da chamada inteira é de quem chama).
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

pub struct HttpTransport {
    client: reqwest::Client,
    scheme: &'static str,
    /// A configuração do cliente TLS (a mesma do `reqwest`), para o canal.
    tls: Option<Arc<ClientConfig>>,
    /// O IP de onde as conexões saem.
    local: Option<IpAddr>,
}

impl HttpTransport {
    /// Sem TLS: a porta do cluster atende em HTTP puro (sem pasta de certificados).
    pub fn plain() -> Result<Self, String> {
        Self::build(None, None)
    }

    /// Com TLS mútuo, com os certificados da pasta.
    pub fn mutual_tls(store: &Arc<TlsStore>) -> Result<Self, String> {
        Self::build(Some(store), None)
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
        let (scheme, tls) = match store {
            Some(store) => {
                builder = builder.use_preconfigured_tls(store.client_config()?);
                ("https", Some(Arc::new(store.client_config()?)))
            }
            None => ("http", None),
        };
        Ok(Self {
            client: builder.build().map_err(|e| e.to_string())?,
            scheme,
            tls,
            local,
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

    fn open_channel<'a>(
        &'a self,
        to: &'a PeerAddr,
        open: ChannelOpen,
    ) -> BoxFuture<'a, Result<ChannelLink, CallError>> {
        Box::pin(async move {
            let scheme = if self.tls.is_some() { "wss" } else { "ws" };
            let url = format!(
                "{scheme}://{}/cluster/channel?from={}&to={}",
                authority(to),
                open.from,
                open.to
            );
            let tcp = tokio::time::timeout(CONNECT_TIMEOUT, tcp_connect(to, self.local))
                .await
                .map_err(|_| CallError::Timeout)??;
            match &self.tls {
                Some(config) => {
                    let name = ServerName::try_from(to.host.clone())
                        .map_err(|e| CallError::Unreachable(e.to_string()))?;
                    let connector = tokio_rustls::TlsConnector::from(config.clone());
                    let stream =
                        tokio::time::timeout(CONNECT_TIMEOUT, connector.connect(name, tcp))
                            .await
                            .map_err(|_| CallError::Timeout)?
                            .map_err(|e| CallError::Unreachable(format!("handshake TLS: {e}")))?;
                    websocket(url, stream).await
                }
                None => websocket(url, tcp).await,
            }
        })
    }
}

/// Abre a conexão TCP com o par, saindo de `local` quando ele é fixado.
async fn tcp_connect(to: &PeerAddr, local: Option<IpAddr>) -> Result<TcpStream, CallError> {
    let unreachable = |e: std::io::Error| CallError::Unreachable(e.to_string());
    let mut addresses = tokio::net::lookup_host((to.host.as_str(), to.port))
        .await
        .map_err(unreachable)?;
    let target: SocketAddr = addresses
        .find(|a| local.is_none_or(|l| l.is_ipv4() == a.is_ipv4()))
        .ok_or_else(|| CallError::Unreachable(format!("`{to}` não resolve para um endereço")))?;
    let socket = if target.is_ipv4() {
        TcpSocket::new_v4()
    } else {
        TcpSocket::new_v6()
    }
    .map_err(unreachable)?;
    if let Some(local) = local {
        socket
            .bind(SocketAddr::new(local, 0))
            .map_err(unreachable)?;
    }
    socket.connect(target).await.map_err(unreachable)
}

/// O upgrade para WebSocket sobre uma conexão pronta (TCP ou TLS).
async fn websocket<S>(url: String, stream: S) -> Result<ChannelLink, CallError>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    match tokio_tungstenite::client_async(url, stream).await {
        Ok((socket, _)) => Ok(bridge(socket, Message::text, |message| match message {
            Message::Text(text) => Some(text.as_str().to_owned()),
            _ => None,
        })),
        // O nó recusou o upgrade com um erro (não é membro, outro processo no endereço...).
        Err(tungstenite::Error::Http(response)) => {
            let reply = HttpReply {
                status: response.status().as_u16(),
                headers: response
                    .headers()
                    .iter()
                    .filter_map(|(k, v)| Some((k.as_str().to_owned(), v.to_str().ok()?.to_owned())))
                    .collect(),
                body: response.body().clone().unwrap_or_default(),
            };
            Err(CallError::Remote(error_from_reply(&reply)))
        }
        Err(e) => Err(CallError::Unreachable(e.to_string())),
    }
}

/// Liga um WebSocket (de qualquer biblioteca) às duas pontas do canal: o que o nó manda vira uma
/// mensagem de texto, e o texto que chega vira um [`ChannelMessage`]. Cada lado que acaba fecha o
/// outro, e o fechamento chega ao nó como o fim da recepção.
pub fn bridge<S, M, E>(
    socket: S,
    text: fn(String) -> M,
    as_text: fn(M) -> Option<String>,
) -> ChannelLink
where
    S: Stream<Item = Result<M, E>> + Sink<M> + Send + Unpin + 'static,
    M: Send + 'static,
    E: Send + 'static,
{
    let (mut sink, mut stream) = socket.split();
    let (out_tx, mut out_rx) = mpsc::channel::<ChannelMessage>(CHANNEL_BUFFER);
    let (in_tx, in_rx) = mpsc::channel::<ChannelMessage>(CHANNEL_BUFFER);
    let done = tokio_util::sync::CancellationToken::new();
    {
        let done = done.clone();
        tokio::spawn(async move {
            loop {
                let message = tokio::select! {
                    () = done.cancelled() => break,
                    message = out_rx.recv() => message,
                };
                let Some(message) = message else { break };
                if sink.send(text(message.to_text())).await.is_err() {
                    break;
                }
            }
            done.cancel();
            let _ = sink.close().await;
        });
    }
    tokio::spawn(async move {
        loop {
            let frame = tokio::select! {
                () = done.cancelled() => break,
                frame = stream.next() => frame,
            };
            let Some(Ok(frame)) = frame else { break };
            // Só o texto importa: o sinal de vida é uma mensagem nossa, não o ping do WebSocket.
            let Some(body) = as_text(frame) else { continue };
            let Ok(message) = ChannelMessage::from_text(&body) else {
                break;
            };
            if in_tx.send(message).await.is_err() {
                break;
            }
        }
        done.cancel();
    });
    ChannelLink {
        tx: out_tx,
        rx: in_rx,
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
