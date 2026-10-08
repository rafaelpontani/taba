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

use super::{BoxFuture, CallError, Handler, PeerAddr, Transport};
use crate::cluster::protocol::{Request, Response};

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
        self.lock().handlers.remove(addr);
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
        self.lock().partitions.insert(pair(a, b));
    }

    pub fn heal(&self, a: &str, b: &str) {
        self.lock().partitions.remove(&pair(a, b));
    }

    /// Isola um nó de todos os outros (ele ainda fala consigo mesmo).
    pub fn isolate(&self, host: &str) {
        self.lock().isolated.insert(host.to_owned());
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
}

#[cfg(test)]
mod tests;
