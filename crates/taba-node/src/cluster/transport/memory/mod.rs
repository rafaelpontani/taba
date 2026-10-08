//! O `Transport` de teste (doc 18, seção 17.5): uma rede em memória que liga vários nós do
//! mesmo processo, sem soquete. Cada nó registra o seu `Handler` num endereço (`host:porta`)
//! e chama os outros por um `MemoryTransport` que sabe quem é o chamador.
//!
//! O teste programa as falhas: **partição** entre dois nós ou isolamento de um, **perda** das
//! próximas chamadas a um nó, **atraso** em todas as chamadas (ou só nas de um tipo), um nó **mudo** (aceita e não
//! responde, para estourar o prazo de quem chama) e um nó que **cai** (some da rede). Um
//! **balanceador** reparte as chamadas entre nós, como o `cluster.entry-address` real.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::{
    BoxFuture, CHANNEL_BUFFER, CallError, ChannelLink, ChannelOpen, Handler, PeerAddr, Transport,
};
use crate::cluster::protocol::{ChannelMessage, Request, Response};

/// Uma chamada que passou pela rede, para o teste conferir quem falou com quem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Call {
    /// O endereço do chamador.
    pub from: String,
    /// O endereço que o chamador usou (o do balanceador, se foi por ele).
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
    calls: Vec<Call>,
    /// Os canais abertos (os dois hosts e como fechá-los): uma partição os derruba, como a queda de
    /// uma conexão TCP.
    links: Vec<(String, String, CancellationToken)>,
}

/// A rede compartilhada entre os nós de um teste.
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

    /// Põe um nó na rede: as chamadas a `addr` chegam ao `handler`.
    pub fn register(&self, addr: PeerAddr, handler: Arc<dyn Handler>) {
        self.lock().handlers.insert(addr, handler);
    }

    /// O nó cai: as chamadas a ele deixam de ser atendidas (conexão recusada).
    pub fn unregister(&self, addr: &PeerAddr) {
        let mut state = self.lock();
        state.handlers.remove(addr);
        Self::cut(&mut state, |a, b| a == addr.host || b == addr.host);
    }

    /// Fecha os canais abertos entre os hosts que a condição escolhe.
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

    /// Um balanceador em `addr`: cada chamada vai a um dos `backends` que estejam no ar e
    /// alcançáveis pelo chamador, em rodízio.
    pub fn add_balancer(&self, addr: PeerAddr, backends: Vec<PeerAddr>) {
        self.lock().balancers.insert(addr, backends);
    }

    /// O `Transport` de um nó, que se identifica por `local` (o IP dele) nas chamadas.
    pub fn transport(&self, local: impl Into<String>) -> Arc<MemoryTransport> {
        Arc::new(MemoryTransport {
            network: self.clone(),
            local: local.into(),
        })
    }

    /// Corta a comunicação entre dois nós, nos dois sentidos.
    pub fn partition(&self, a: &str, b: &str) {
        let mut state = self.lock();
        state.partitions.insert(pair(a, b));
        Self::cut(&mut state, |x, y| pair(x, y) == pair(a, b));
    }

    pub fn heal(&self, a: &str, b: &str) {
        self.lock().partitions.remove(&pair(a, b));
    }

    /// Isola um nó de todos os outros (ele ainda fala consigo mesmo).
    pub fn isolate(&self, host: &str) {
        let mut state = self.lock();
        state.isolated.insert(host.to_owned());
        Self::cut(&mut state, |x, y| x != y && (x == host || y == host));
    }

    pub fn restore(&self, host: &str) {
        self.lock().isolated.remove(host);
    }

    /// Perde as próximas `count` chamadas ao nó (falha de rede pontual).
    pub fn lose_next(&self, host: &str, count: usize) {
        self.lock().lose_next.insert(host.to_owned(), count);
    }

    /// Perde as próximas `count` chamadas **de um tipo** (`Request::name()`) ao nó, e só elas:
    /// para falhar uma fase do protocolo sem derrubar as outras.
    pub fn lose_kind(&self, host: &str, request: &str, count: usize) {
        self.lock()
            .lose_kind
            .insert((host.to_owned(), request.to_owned()), count);
    }

    /// O nó recebe as chamadas e não responde: quem chama só sai pelo prazo.
    pub fn set_silent(&self, host: &str, silent: bool) {
        let mut state = self.lock();
        if silent {
            state.silent.insert(host.to_owned());
        } else {
            state.silent.remove(host);
        }
    }

    /// Atraso somado a toda chamada.
    pub fn set_delay(&self, delay: Duration) {
        self.lock().delay = delay;
    }

    /// Atraso somado só às chamadas de um tipo (`Request::name()`): para alongar uma fase do
    /// protocolo (por exemplo, a votação) sem estourar o prazo do canal. `Duration::ZERO` o tira.
    pub fn set_delay_kind(&self, request: &str, delay: Duration) {
        self.lock().delay_kind.insert(request.to_owned(), delay);
    }

    /// As chamadas feitas até agora, na ordem.
    pub fn calls(&self) -> Vec<Call> {
        self.lock().calls.clone()
    }

    pub fn clear_calls(&self) {
        self.lock().calls.clear();
    }

    /// Quantas chamadas daquele tipo foram feitas.
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

    /// Escolhe o destino, aplica as falhas programadas e devolve quem atende.
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
                        "o balanceador {to} não tem nó disponível"
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
                "sem rota de {from} para {target}"
            )));
        }
        if let Some(left) = state
            .lose_kind
            .get_mut(&(target.host.clone(), request.to_owned()))
            && *left > 0
        {
            *left -= 1;
            return Err(CallError::Unreachable(format!(
                "chamada {request} a {target} perdida"
            )));
        }
        if let Some(left) = state.lose_next.get_mut(&target.host)
            && *left > 0
        {
            *left -= 1;
            return Err(CallError::Unreachable(format!(
                "chamada a {target} perdida"
            )));
        }
        let Some(handler) = state.handlers.get(&target).cloned() else {
            return Err(CallError::Unreachable(format!(
                "conexão recusada por {target}"
            )));
        };
        let delay = state.delay + state.delay_kind.get(request).copied().unwrap_or_default();
        Ok((handler, delay, state.silent.contains(&target.host)))
    }
}

impl MemoryNetwork {
    /// Aplica as falhas programadas a uma mensagem do canal de `from` para `to`: `Err` derruba o
    /// canal (partição, nó isolado, mensagem perdida), `Ok(None)` descarta a mensagem em silêncio (nó
    /// mudo) e `Ok(Some(atraso))` a entrega depois do atraso.
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

    /// Um sentido do canal: leva as mensagens de `from` a `to` até um lado fechar, a rede cortar ou
    /// o canal ser cancelado (o fim de um sentido fecha o outro).
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

/// O `Transport` de um nó na rede em memória.
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
            // Um sentido de cada vez, pelas regras da rede.
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
