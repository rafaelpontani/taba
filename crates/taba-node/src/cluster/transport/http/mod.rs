//! The real `Transport` (doc 18, section 17.5): the calls between nodes as HTTP requests to the other node's cluster
//! port (doc 06c), with mutual TLS when there is a certificates folder. The translation between
//! the typed call and the HTTP request is that of `protocol::http`, the same the cluster port
//! uses on receiving. The **channel** between two nodes is a WebSocket (`GET /cluster/channel` with upgrade)
//! opened over the same TCP connection and the same TLS: each message is a JSON text
//! ([`ChannelMessage`]).
//!
//! Connections go out and stay open through `reqwest`; the certificate the node presents is chosen on
//! each handshake (`cluster::tls`), so rotation applies to new connections, and open ones
//! carry on with the old certificate until they drop.

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

/// The deadline for opening the connection (the deadline of the whole call is the caller's).
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

pub struct HttpTransport {
    client: reqwest::Client,
    scheme: &'static str,
    /// The TLS client configuration (the same as `reqwest`'s), for the channel.
    tls: Option<Arc<ClientConfig>>,
    /// The IP the connections go out from.
    local: Option<IpAddr>,
}

impl HttpTransport {
    /// Without TLS: the cluster port serves plain HTTP (no certificates folder).
    pub fn plain() -> Result<Self, String> {
        Self::build(None, None)
    }

    /// With mutual TLS, with the folder's certificates.
    pub fn mutual_tls(store: &Arc<TlsStore>) -> Result<Self, String> {
        Self::build(Some(store), None)
    }

    /// `mutual_tls` if there is a folder, otherwise `plain`.
    pub fn new(store: Option<&Arc<TlsStore>>) -> Result<Self, String> {
        Self::build(store, None)
    }

    /// Like `new`, but the connections go out from IP `local`. The cluster recognizes the caller by the
    /// connection's source IP; on loopback the system would choose `127.0.0.1` for all, so the
    /// tests with several nodes on the same host pin each one's IP.
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

/// A `reqwest` network failure as the `Transport` counts it.
fn network_error(error: &reqwest::Error) -> CallError {
    if error.is_timeout() {
        CallError::Timeout
    } else {
        // The cause chain brings the real reason (connection refused, handshake refused...).
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
            // The entry listing goes through a load balancer (`cluster.entry-address`) that decides the node
            // per connection: reusing the connection from one attempt in the next would pin the bootstrap to the
            // same node (on restart, the node itself, which does not have the table yet). Each listing opens its own.
            let fresh_connection = matches!(request, Request::ListNodes);
            let mut call = request.to_http();
            // The trace context follows the call (doc 11, "Traces"; D119).
            crate::telemetry::trace::inject(&mut call.headers);
            let url = format!("{}://{}{}", self.scheme, authority(to), call.path);
            let method = reqwest::Method::from_bytes(call.method.as_bytes())
                .map_err(|e| CallError::Unreachable(e.to_string()))?;
            let mut builder = self.client.request(method, url);
            if fresh_connection {
                builder = builder.header("connection", "close");
            }
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

/// Opens the TCP connection with the peer, going out from `local` when it is pinned.
async fn tcp_connect(to: &PeerAddr, local: Option<IpAddr>) -> Result<TcpStream, CallError> {
    let unreachable = |e: std::io::Error| CallError::Unreachable(e.to_string());
    let mut addresses = tokio::net::lookup_host((to.host.as_str(), to.port))
        .await
        .map_err(unreachable)?;
    let target: SocketAddr = addresses
        .find(|a| local.is_none_or(|l| l.is_ipv4() == a.is_ipv4()))
        .ok_or_else(|| CallError::Unreachable(format!("`{to}` does not resolve to an address")))?;
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

/// The upgrade to WebSocket over a ready connection (TCP or TLS).
async fn websocket<S>(url: String, stream: S) -> Result<ChannelLink, CallError>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    match tokio_tungstenite::client_async(url, stream).await {
        Ok((socket, _)) => Ok(bridge(socket, Message::text, |message| match message {
            Message::Text(text) => Some(text.as_str().to_owned()),
            _ => None,
        })),
        // The node refused the upgrade with an error (not a member, another process at the address...).
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

/// Attaches a WebSocket (from any library) to the two ends of the channel: what the node sends becomes a
/// text message, and the text that arrives becomes a [`ChannelMessage`]. Each side that ends closes the
/// other, and the closing reaches the node as the end of reception.
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
            // Only text matters: the heartbeat is a message of ours, not the WebSocket ping.
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

/// The IP this node uses to go out toward `entry` (the `cluster.entry-address`): the local
/// address of a UDP socket "connected" to it, without sending anything (D95). Whoever has NAT or several
/// interfaces sets `cluster.advertise-addresses`.
pub fn discover_address(entry: &PeerAddr) -> Result<String, String> {
    let target = (entry.host.as_str(), entry.port)
        .to_socket_addrs()
        .map_err(|e| format!("could not resolve `{entry}`: {e}"))?
        .next()
        .ok_or_else(|| format!("`{entry}` does not resolve to any address"))?;
    let bind = if target.is_ipv6() {
        "[::]:0"
    } else {
        "0.0.0.0:0"
    };
    let socket = UdpSocket::bind(bind).map_err(|e| e.to_string())?;
    socket
        .connect(target)
        .map_err(|e| format!("could not choose the interface for `{entry}`: {e}"))?;
    let local = socket.local_addr().map_err(|e| e.to_string())?;
    if local.ip().is_unspecified() {
        return Err(format!("no network interface to reach `{entry}`"));
    }
    Ok(local.ip().to_string())
}

#[cfg(test)]
mod tests;
