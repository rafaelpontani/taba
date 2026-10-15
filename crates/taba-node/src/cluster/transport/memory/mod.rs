//! The test `Transport` (doc 18, section 17.5): an in-memory network that links several nodes of the
//! same process, without a socket. Each node registers its `Handler` at an address (`host:port`)
//! and calls the others through a `MemoryTransport` that knows who the caller is.
//!
//! The test programs the failures: **partition** between two nodes or the isolation of one, **loss** of the
//! next calls to a node, **delay** on all calls (or only those of one type), a **mute** node (accepts and does not
//! answer, so the caller's deadline runs out) and a node that **falls** (vanishes from the network). A
//! **load balancer** splits the calls among nodes, like the real `cluster.entry-address`.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::{
    BoxFuture, CHANNEL_BUFFER, CallError, ChannelLink, ChannelOpen, Handler, PeerAddr, Transport,
};
use crate::cluster::protocol::{ChannelMessage, Request, Response};

/// A call that went through the network, for the test to check who talked to whom.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Call {
    /// The caller's address.
    pub from: String,
    /// The address the caller used (the load balancer's, if it went through it).
    pub to: PeerAddr,
    /// `Request::name()`.
    pub request: &'static str,
}

#[derive(Default)]
struct State {
    handlers: HashMap<PeerAddr, Arc<dyn Handler>>,
    balancers: HashMap<PeerAddr, Vec<PeerAddr>>,
    next_backend: usize,
    partitions: HashSet<(String, String)>,
    isolated: HashSet<String>,
    lose_next: HashMap<String, usize>,
    lose_kind: HashMap<(String, String), usize>,
    silent: HashSet<String>,
    delay: Duration,
    delay_kind: HashMap<String, Duration>,
    delay_host_kind: HashMap<(String, String), Duration>,
    calls: Vec<Call>,
    /// The open channels (the two hosts and how to close them): a partition brings them down, like the drop of
    /// a TCP connection.
    links: Vec<(String, String, CancellationToken)>,
}

/// The network shared among a test's nodes.
#[derive(Clone, Default)]
pub struct MemoryNetwork {
    state: Arc<Mutex<State>>,
}

fn pair(a: &str, b: &str) -> (String, String) {
    if a <= b {
        (a.to_owned(), b.to_owned())
    } else {
        (b.to_owned(), a.to_owned())
    }
}

impl MemoryNetwork {
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Puts a node on the network: calls to `addr` reach the `handler`.
    pub fn register(&self, addr: PeerAddr, handler: Arc<dyn Handler>) {
        self.lock().handlers.insert(addr, handler);
    }

    /// The node falls: calls to it stop being served (connection refused).
    pub fn unregister(&self, addr: &PeerAddr) {
        let mut state = self.lock();
        state.handlers.remove(addr);
        Self::cut(&mut state, |a, b| a == addr.host || b == addr.host);
    }

    /// Closes the open channels between the hosts the condition chooses.
    fn cut(state: &mut State, mut which: impl FnMut(&str, &str) -> bool) {
        state.links.retain(|(a, b, token)| {
            if which(a, b) {
                token.cancel();
                false
            } else {
                !token.is_cancelled()
            }
        });
    }

    /// A load balancer at `addr`: each call goes to one of the `backends` that are up and
    /// reachable by the caller, round-robin.
    pub fn add_balancer(&self, addr: PeerAddr, backends: Vec<PeerAddr>) {
        self.lock().balancers.insert(addr, backends);
    }

    /// A node's `Transport`, which identifies itself by `local` (its IP) on the calls.
    pub fn transport(&self, local: impl Into<String>) -> Arc<MemoryTransport> {
        Arc::new(MemoryTransport {
            network: self.clone(),
            local: local.into(),
        })
    }

    /// Cuts communication between two nodes, in both directions.
    pub fn partition(&self, a: &str, b: &str) {
        let mut state = self.lock();
        state.partitions.insert(pair(a, b));
        Self::cut(&mut state, |x, y| pair(x, y) == pair(a, b));
    }

    pub fn heal(&self, a: &str, b: &str) {
        self.lock().partitions.remove(&pair(a, b));
    }

    /// Isolates a node from all the others (it still talks to itself).
    pub fn isolate(&self, host: &str) {
        let mut state = self.lock();
        state.isolated.insert(host.to_owned());
        Self::cut(&mut state, |x, y| x != y && (x == host || y == host));
    }

    pub fn restore(&self, host: &str) {
        self.lock().isolated.remove(host);
    }

    /// Loses the next `count` calls to the node (a one-off network failure).
    pub fn lose_next(&self, host: &str, count: usize) {
        self.lock().lose_next.insert(host.to_owned(), count);
    }

    /// Loses the next `count` calls **of one type** (`Request::name()`) to the node, and only those:
    /// to fail a phase of the protocol without bringing the others down.
    pub fn lose_kind(&self, host: &str, request: &str, count: usize) {
        self.lock()
            .lose_kind
            .insert((host.to_owned(), request.to_owned()), count);
    }

    /// The node receives the calls and does not answer: whoever calls only leaves by the deadline.
    pub fn set_silent(&self, host: &str, silent: bool) {
        let mut state = self.lock();
        if silent {
            state.silent.insert(host.to_owned());
        } else {
            state.silent.remove(host);
        }
    }

    /// Delay added to every call.
    pub fn set_delay(&self, delay: Duration) {
        self.lock().delay = delay;
    }

    /// Delay added only to the calls of one type (`Request::name()`): to lengthen a phase of the
    /// protocol (for example, the vote) without blowing the channel's deadline. `Duration::ZERO` removes it.
    pub fn set_delay_kind(&self, request: &str, delay: Duration) {
        self.lock().delay_kind.insert(request.to_owned(), delay);
    }

    /// Delay added only to the calls of one type (`Request::name()`) **to a node**: to make a
    /// node's response arrive after the others'. `Duration::ZERO` removes it.
    pub fn set_delay_host_kind(&self, host: &str, request: &str, delay: Duration) {
        self.lock()
            .delay_host_kind
            .insert((host.to_owned(), request.to_owned()), delay);
    }

    /// The calls made so far, in order.
    pub fn calls(&self) -> Vec<Call> {
        self.lock().calls.clone()
    }

    pub fn clear_calls(&self) {
        self.lock().calls.clear();
    }

    /// How many calls of that type were made.
    pub fn count(&self, request: &str) -> usize {
        self.lock()
            .calls
            .iter()
            .filter(|c| c.request == request)
            .count()
    }

    fn blocked(state: &State, from: &str, to: &str) -> bool {
        from != to
            && (state.isolated.contains(from)
                || state.isolated.contains(to)
                || state.partitions.contains(&pair(from, to)))
    }

    /// Chooses the destination, applies the programmed failures and returns who serves.
    fn route(
        &self,
        from: &str,
        to: &PeerAddr,
        request: &'static str,
    ) -> Result<(Arc<dyn Handler>, Duration, bool), CallError> {
        let mut state = self.lock();
        state.calls.push(Call {
            from: from.to_owned(),
            to: to.clone(),
            request,
        });
        let target = match state.balancers.get(to).cloned() {
            Some(backends) => {
                let live: Vec<&PeerAddr> = backends
                    .iter()
                    .filter(|b| {
                        state.handlers.contains_key(*b) && !Self::blocked(&state, from, &b.host)
                    })
                    .collect();
                if live.is_empty() {
                    return Err(CallError::Unreachable(format!(
                        "the load balancer {to} has no available node"
                    )));
                }
                let chosen = live[state.next_backend % live.len()].clone();
                state.next_backend = state.next_backend.wrapping_add(1);
                chosen
            }
            None => to.clone(),
        };
        if Self::blocked(&state, from, &target.host) {
            return Err(CallError::Unreachable(format!(
                "no route from {from} to {target}"
            )));
        }
        if let Some(left) = state
            .lose_kind
            .get_mut(&(target.host.clone(), request.to_owned()))
            && *left > 0
        {
            *left -= 1;
            return Err(CallError::Unreachable(format!(
                "call {request} to {target} lost"
            )));
        }
        if let Some(left) = state.lose_next.get_mut(&target.host)
            && *left > 0
        {
            *left -= 1;
            return Err(CallError::Unreachable(format!("call to {target} lost")));
        }
        let Some(handler) = state.handlers.get(&target).cloned() else {
            return Err(CallError::Unreachable(format!(
                "connection refused by {target}"
            )));
        };
        let delay = state.delay
            + state.delay_kind.get(request).copied().unwrap_or_default()
            + state
                .delay_host_kind
                .get(&(target.host.clone(), request.to_owned()))
                .copied()
                .unwrap_or_default();
        Ok((handler, delay, state.silent.contains(&target.host)))
    }
}

impl MemoryNetwork {
    /// Applies the programmed failures to a channel message from `from` to `to`: `Err` brings the
    /// channel down (partition, isolated node, lost message), `Ok(None)` silently drops the message (mute
    /// node) and `Ok(Some(delay))` delivers it after the delay.
    fn gate(&self, from: &str, to: &str) -> Result<Option<Duration>, ()> {
        let mut state = self.lock();
        let request = "channel";
        state.calls.push(Call {
            from: from.to_owned(),
            to: PeerAddr::new(to, 0),
            request,
        });
        if Self::blocked(&state, from, to) {
            return Err(());
        }
        if let Some(left) = state
            .lose_kind
            .get_mut(&(to.to_owned(), request.to_owned()))
            && *left > 0
        {
            *left -= 1;
            return Err(());
        }
        if let Some(left) = state.lose_next.get_mut(to)
            && *left > 0
        {
            *left -= 1;
            return Err(());
        }
        if state.silent.contains(to) || state.silent.contains(from) {
            return Ok(None);
        }
        Ok(Some(
            state.delay + state.delay_kind.get(request).copied().unwrap_or_default(),
        ))
    }

    /// One direction of the channel: carries the messages from `from` to `to` until a side closes, the network cuts or
    /// the channel is cancelled (the end of one direction closes the other).
    fn pump(
        &self,
        from: String,
        to: String,
        mut source: mpsc::Receiver<ChannelMessage>,
        sink: mpsc::Sender<ChannelMessage>,
        cancel: CancellationToken,
    ) {
        let network = self.clone();
        tokio::spawn(async move {
            loop {
                let message = tokio::select! {
                    () = cancel.cancelled() => break,
                    message = source.recv() => match message {
                        Some(message) => message,
                        None => break,
                    },
                };
                match network.gate(&from, &to) {
                    Err(()) => break,
                    Ok(None) => continue,
                    Ok(Some(delay)) => {
                        if !delay.is_zero() {
                            tokio::time::sleep(delay).await;
                        }
                    }
                }
                if sink.send(message).await.is_err() {
                    break;
                }
            }
            cancel.cancel();
        });
    }
}

/// A node's `Transport` on the in-memory network.
pub struct MemoryTransport {
    network: MemoryNetwork,
    local: String,
}

impl Transport for MemoryTransport {
    fn call<'a>(
        &'a self,
        to: &'a PeerAddr,
        request: Request,
    ) -> BoxFuture<'a, Result<Response, CallError>> {
        Box::pin(async move {
            let (handler, delay, silent) = self.network.route(&self.local, to, request.name())?;
            if !delay.is_zero() {
                tokio::time::sleep(delay).await;
            }
            if silent {
                std::future::pending::<()>().await;
            }
            handler
                .handle(&self.local, request)
                .await
                .map_err(CallError::Remote)
        })
    }

    fn open_channel<'a>(
        &'a self,
        to: &'a PeerAddr,
        open: ChannelOpen,
    ) -> BoxFuture<'a, Result<ChannelLink, CallError>> {
        Box::pin(async move {
            let (handler, delay, silent) = self.network.route(&self.local, to, "channel")?;
            if !delay.is_zero() {
                tokio::time::sleep(delay).await;
            }
            if silent {
                std::future::pending::<()>().await;
            }
            handler
                .admit_channel(&self.local, &open)
                .map_err(CallError::Remote)?;
            // One direction at a time, by the network's rules.
            let cancel = CancellationToken::new();
            let server_host = to.host.clone();
            self.network.lock().links.push((
                self.local.clone(),
                server_host.clone(),
                cancel.clone(),
            ));
            let (client_tx, client_out) = mpsc::channel(CHANNEL_BUFFER);
            let (server_tx, server_out) = mpsc::channel(CHANNEL_BUFFER);
            let (client_in_tx, client_in) = mpsc::channel(CHANNEL_BUFFER);
            let (server_in_tx, server_in) = mpsc::channel(CHANNEL_BUFFER);
            self.network.pump(
                self.local.clone(),
                server_host.clone(),
                client_out,
                server_in_tx,
                cancel.clone(),
            );
            self.network.pump(
                server_host,
                self.local.clone(),
                server_out,
                client_in_tx,
                cancel,
            );
            handler.accept_channel(
                open,
                ChannelLink {
                    tx: server_tx,
                    rx: server_in,
                },
            );
            Ok(ChannelLink {
                tx: client_tx,
                rx: client_in,
            })
        })
    }
}

#[cfg(test)]
mod tests;
