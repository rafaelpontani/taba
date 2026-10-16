//! Whatever touches the network between nodes sits behind a trait (doc 18, section 17.5): the `Transport`.
//! The real implementation (`http`: HTTP and WebSocket, with mTLS) and the test one (`memory`, which links several
//! nodes of the same process through in-memory channels, with programmable loss, delay and partition) carry
//! the same two things: request-and-response calls and the permanent **channel** between two nodes.
//!
//! The trait speaks in protocol types (`Request`, `Response`), not bytes: each
//! implementation knows how to carry them. The part of the node that **receives** the call is a `Handler`, the
//! same one the `/cluster/*` routes use.

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

/// A node's address for calls between nodes: an IP (or name) and the cluster port.
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

    /// `host:port` (the format of `cluster.entry-address`).
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (host, port) = s
            .trim()
            .rsplit_once(':')
            .ok_or_else(|| format!("`{s}` is not host:port"))?;
        let port = port
            .parse::<u16>()
            .map_err(|_| format!("`{s}` does not have a valid port"))?;
        if host.is_empty() {
            return Err(format!("`{s}` has no host"));
        }
        Ok(Self::new(host, port))
    }
}

/// Why a call to another node did not work out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallError {
    /// There was no response: connection refused, node down, network partition.
    Unreachable(String),
    /// The response did not come in time.
    Timeout,
    /// The node answered with an error (status 4xx or 5xx and the `ErrorResponse`).
    Remote(TabaError),
}

impl CallError {
    /// The listing found no cluster to join: nobody answered, or the node that answered has no member table yet
    /// (through a load balancer that only reaches this very node, the answer is its own `cluster.node-syncing`).
    pub fn found_no_cluster(&self) -> bool {
        self.is_network()
            || matches!(self, CallError::Remote(e) if e.code == crate::core::error::ErrorCode::ClusterNodeSyncing)
    }

    /// Network failures: worth trying again (or another node). The error the peer returned is not.
    pub fn is_network(&self) -> bool {
        matches!(self, CallError::Unreachable(_) | CallError::Timeout)
    }
}

impl fmt::Display for CallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CallError::Unreachable(why) => write!(f, "no response: {why}"),
            CallError::Timeout => f.write_str("prazo esgotado"),
            CallError::Remote(error) => write!(f, "{}: {}", error.code, error.message),
        }
    }
}

/// Who opens the channel and whom it wants to talk to: the `nodeId`s of both sides. The node that serves checks that
/// the `to` is itself (a new process on the same IP and port has another ID: `cluster.wrong-node`, D100).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelOpen {
    pub from: String,
    pub to: String,
}

/// One end of the channel: what is sent to the other node and what it sends. Dropping the end closes the channel
/// (and the other side sees reception end).
pub struct ChannelLink {
    pub tx: mpsc::Sender<ChannelMessage>,
    pub rx: mpsc::Receiver<ChannelMessage>,
}

impl ChannelLink {
    /// The two ends of an in-memory channel.
    pub fn pair() -> (ChannelLink, ChannelLink) {
        let (a_tx, b_rx) = mpsc::channel(CHANNEL_BUFFER);
        let (b_tx, a_rx) = mpsc::channel(CHANNEL_BUFFER);
        (
            ChannelLink { tx: a_tx, rx: a_rx },
            ChannelLink { tx: b_tx, rx: b_rx },
        )
    }
}

/// How many messages fit in each direction before the sender waits.
pub const CHANNEL_BUFFER: usize = 256;

/// The calls between nodes.
pub trait Transport: Send + Sync + 'static {
    fn call<'a>(
        &'a self,
        to: &'a PeerAddr,
        request: Request,
    ) -> BoxFuture<'a, Result<Response, CallError>>;

    /// Opens the permanent channel with the node at `to` (`GET /cluster/channel` with the upgrade to
    /// WebSocket, on the real network). The node's error (not a member, another process at the address) arrives as
    /// `CallError::Remote`, before the channel exists.
    fn open_channel<'a>(
        &'a self,
        to: &'a PeerAddr,
        open: ChannelOpen,
    ) -> BoxFuture<'a, Result<ChannelLink, CallError>>;
}

/// Who serves the calls that arrive at a node through the cluster port.
pub trait Handler: Send + Sync + 'static {
    /// `caller` is the address of whoever calls (the connection's source IP). The member is recognized
    /// by it: the connection says nothing more (everyone uses the same cluster certificate).
    fn handle<'a>(
        &'a self,
        caller: &'a str,
        request: Request,
    ) -> BoxFuture<'a, Result<Response, TabaError>>;

    /// Checks whether the channel `caller` (the source IP) wants to open is accepted: it is a member and the `to` is
    /// this node. The route waits for this answer before doing the upgrade, so the error becomes an HTTP status.
    fn admit_channel(&self, _caller: &str, _open: &ChannelOpen) -> Result<(), TabaError> {
        Ok(())
    }

    /// The channel was opened by `open.from`: starts using the `link` end.
    fn accept_channel(&self, _open: ChannelOpen, _link: ChannelLink) {}
}

#[cfg(test)]
mod tests;
