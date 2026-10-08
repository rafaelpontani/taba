//! O repasse das chamadas de cliente (doc 04, seção 6.9; doc 06c, `* /cluster/forward/{caminho}`).
//! Um nó em drain segue recebendo chamadas de cliente (o balanceador demora a perceber) e as
//! repassa **todas** ao nó mais livre, desde que o drain começa até o processo encerrar, mesmo
//! depois de ter saído do cluster.
//!
//! - **Destino:** a cada chamada, o membro pronto (fora de drain) com menos chamadas repassadas por
//!   este nó em andamento, desempatando ao acaso. O indicador de carga do sinal de vida entra com o
//!   canal (etapa 15).
//! - **Repasse transparente:** método, caminho, query, corpo e cabeçalhos seguem como vieram, e a
//!   resposta volta como o destino a deu (`409`, `413`, `429`...). Vale também para o `GET` com
//!   long polling.
//! - **Falha do destino:** antes de o corpo chegar (sem conexão, ou o destino em drain ou
//!   sincronizando) tenta outro, até um teto pequeno; depois de enviado **não repete** (repetir um
//!   `POST` duplicaria a mensagem): `504 forward.timeout`.
//! - **Sem laço:** quem recebe o repasse o executa e nunca repassa de novo.

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use super::signals::Signals;
use crate::cluster::members::Member;
use crate::cluster::protocol::{ForwardRequest, ForwardResponse, Request, Response};
use crate::cluster::service::ClusterService;
use crate::cluster::transport::CallError;
use crate::core::error::{ErrorCode, TabaError};
use crate::telemetry::metrics::CLUSTER_FORWARDED;

/// O prazo de um repasse: cobre um long polling (até 20 s) com folga.
const FORWARD_TIMEOUT: Duration = Duration::from_secs(60);
/// Quantos destinos, no máximo, uma chamada tenta antes de desistir.
const MAX_ATTEMPTS: usize = 3;

/// Executa uma chamada de cliente repassada contra a API de cliente deste nó (quem monta o nó
/// entrega: o `cluster` não conhece o HTTP).
pub type ForwardExecutor = Arc<
    dyn Fn(
            ForwardRequest,
        ) -> Pin<Box<dyn Future<Output = Result<ForwardResponse, TabaError>> + Send>>
        + Send
        + Sync,
>;

pub struct Forwarder {
    service: Arc<ClusterService>,
    signals: Arc<Signals>,
    executor: OnceLock<ForwardExecutor>,
    /// Chamadas repassadas por este nó e ainda em andamento, por destino.
    in_flight: Mutex<HashMap<String, usize>>,
}

/// Subtrai a chamada em andamento ao sair, mesmo que o futuro seja descartado.
struct InFlight<'a> {
    forwarder: &'a Forwarder,
    node_id: String,
}

impl<'a> InFlight<'a> {
    fn new(forwarder: &'a Forwarder, node_id: &str) -> Self {
        *forwarder.lock().entry(node_id.to_owned()).or_default() += 1;
        Self {
            forwarder,
            node_id: node_id.to_owned(),
        }
    }
}

impl Drop for InFlight<'_> {
    fn drop(&mut self) {
        let mut map = self.forwarder.lock();
        if let Some(count) = map.get_mut(&self.node_id) {
            *count = count.saturating_sub(1);
        }
    }
}

fn no_destination(state: crate::cluster::state::NodeState) -> TabaError {
    if state == crate::cluster::state::NodeState::Failed {
        return TabaError::new(
            ErrorCode::NodeFailed,
            "O nó está em falha e nenhum outro nó pôde atender a chamada.",
        )
        .with_retry_after(5);
    }
    if state == crate::cluster::state::NodeState::Syncing {
        return TabaError::new(
            ErrorCode::ClusterNodeSyncing,
            "O nó está em sincronização e nenhum outro nó pôde atender a chamada.",
        )
        .with_retry_after(5);
    }
    TabaError::new(
        ErrorCode::ClusterNodeDraining,
        "O nó está em drain e nenhum outro nó pôde atender a chamada.",
    )
    .with_retry_after(5)
}

fn random_index(n: usize) -> usize {
    let mut bytes = [0u8; 8];
    getrandom::fill(&mut bytes).expect("o sistema não tem fonte de números aleatórios");
    (u64::from_le_bytes(bytes) % n.max(1) as u64) as usize
}

impl Forwarder {
    pub fn new(service: Arc<ClusterService>, signals: Arc<Signals>) -> Arc<Self> {
        Arc::new(Self {
            service,
            signals,
            executor: OnceLock::new(),
            in_flight: Mutex::new(HashMap::new()),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, usize>> {
        self.in_flight.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn set_executor(&self, executor: ForwardExecutor) {
        let _ = self.executor.set(executor);
    }

    /// O destino desta chamada: entre os membros prontos e fora de drain que ainda não foram
    /// tentados, o de menos chamadas em andamento; empate, sorteio.
    fn pick(&self, tried: &HashSet<String>) -> Option<Member> {
        let table = self.service.membership().table()?;
        let me = self.service.node_id();
        let in_flight = self.lock();
        let candidates: Vec<&Member> = table
            .members
            .iter()
            .filter(|m| {
                m.node_id != me
                    && !tried.contains(&m.node_id)
                    && m.state == crate::cluster::members::MemberState::Active
                    && !self.signals.is_draining(&m.node_id)
            })
            .collect();
        let least = candidates
            .iter()
            .map(|m| in_flight.get(&m.node_id).copied().unwrap_or(0))
            .min()?;
        let tied: Vec<&&Member> = candidates
            .iter()
            .filter(|m| in_flight.get(&m.node_id).copied().unwrap_or(0) == least)
            .collect();
        Some((**tied[random_index(tied.len())]).clone())
    }

    fn count(&self, outcome: &str) {
        self.service.telemetry().registry.add(
            &CLUSTER_FORWARDED,
            &[("taba.outcome", outcome)],
            1.0,
        );
    }

    /// Repassa a chamada ao nó mais livre.
    pub async fn forward(&self, request: ForwardRequest) -> Result<ForwardResponse, TabaError> {
        let mut tried = HashSet::new();
        for _ in 0..MAX_ATTEMPTS {
            let Some(member) = self.pick(&tried) else {
                break;
            };
            tried.insert(member.node_id.clone());
            let _busy = InFlight::new(self, &member.node_id);
            match self
                .service
                .call_peer(&member, Request::Forward(request.clone()), FORWARD_TIMEOUT)
                .await
            {
                Ok(Response::Forwarded(response)) => {
                    self.count("ok");
                    return Ok(response);
                }
                Ok(_) => {
                    self.count("error");
                    return Err(TabaError::new(
                        ErrorCode::ForwardBadGateway,
                        "Resposta inesperada do nó que recebeu o repasse.",
                    ));
                }
                // Sem resposta no prazo: o corpo já foi enviado, então não se repete.
                Err(CallError::Timeout) => {
                    self.count("timeout");
                    return Err(TabaError::new(
                        ErrorCode::ForwardTimeout,
                        "O nó que recebeu o repasse não respondeu no prazo.",
                    ));
                }
                // O destino não pôde atender (sem conexão, em drain, sincronizando): outro.
                Err(CallError::Unreachable(_)) => {}
                Err(CallError::Remote(error)) if error.status() == 503 => {}
                Err(CallError::Remote(error)) => {
                    self.count("error");
                    tracing::warn!(peer = %member.node_id, %error, "o repasse foi recusado");
                    return Err(TabaError::new(
                        ErrorCode::ForwardBadGateway,
                        format!(
                            "O nó {} recusou o repasse: {}",
                            member.node_id, error.message
                        ),
                    ));
                }
            }
        }
        self.count("no-destination");
        Err(no_destination(self.service.node_state()))
    }

    /// O lado que recebe o repasse: executa a chamada contra a API de cliente deste nó.
    pub async fn execute(&self, request: ForwardRequest) -> Result<Response, TabaError> {
        let executor = self.executor.get().ok_or_else(TabaError::route_not_found)?;
        executor(request).await.map(Response::Forwarded)
    }
}
