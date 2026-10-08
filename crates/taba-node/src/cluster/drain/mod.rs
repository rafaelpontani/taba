//! O modo drain (doc 04, seção 6.10; doc 06b, "Drain"). Hoje vale a parte **local**: sair da
//! rotação, gravar tudo em disco (o save do desenvolvimento) e voltar. O aviso aos pares e o
//! desregistro do nó entram com o protocolo de cluster.
//!
//! Estados: `active` → `draining` → `drained`. Entra por sinal (`drain.on-stop`) ou pela API.
//! O `drain.timeout` cobre o drain inteiro; estourado, o nó registra o que sobrou em memória
//! e, se foi por sinal, encerra mesmo assim.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime};

use super::state::{DrainState, NodeShared};
use crate::config::Mode;
use crate::core::clock::Clock;
use crate::core::error::{ErrorCode, TabaError};
use crate::engine::messages::Messages;
use crate::storage::state::NodeStateStore;

/// Quem pediu o drain (`DrainStatus.trigger`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    Signal,
    Api,
}

impl Trigger {
    pub const fn as_str(self) -> &'static str {
        match self {
            Trigger::Signal => "signal",
            Trigger::Api => "api",
        }
    }
}

/// O `DrainStatus` do doc 06b.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrainStatus {
    pub state: DrainState,
    /// `None` em `active`.
    pub trigger: Option<Trigger>,
    pub started_at: Option<SystemTime>,
    /// Mensagens ainda em memória.
    pub pending_messages: u64,
}

/// Resposta do `POST /node/drain`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Started {
    /// `202`: o drain começou agora.
    Started(DrainStatus),
    /// `200`: o nó já estava em drain.
    AlreadyDraining(DrainStatus),
}

/// Resposta do `DELETE /node/drain`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cancelled {
    /// `200`: não havia drain.
    Nothing(DrainStatus),
    /// `200`: o drain foi cancelado e o nó voltou a gravar em memória.
    Cancelled(DrainStatus),
    /// `202`: o nó que já tinha saído volta como nó novo (em produção).
    Returned(DrainStatus),
}

/// O drain em andamento ou concluído.
struct Run {
    trigger: Trigger,
    started_at: SystemTime,
    /// Cancelamento: o `DELETE` liga, e a tarefa do drain para na próxima mensagem.
    stop: Arc<AtomicBool>,
}

pub struct DrainController {
    shared: Arc<NodeShared>,
    messages: Arc<Messages>,
    saves: NodeStateStore,
    clock: Arc<dyn Clock>,
    timeout: Duration,
    run: Mutex<Option<Run>>,
    /// O processo está parando por sinal: `DELETE /node/drain` responde `409`.
    stopping: AtomicBool,
}

impl DrainController {
    pub fn new(
        shared: Arc<NodeShared>,
        messages: Arc<Messages>,
        saves: NodeStateStore,
        clock: Arc<dyn Clock>,
        timeout: Duration,
    ) -> Arc<Self> {
        Arc::new(Self {
            shared,
            messages,
            saves,
            clock,
            timeout,
            run: Mutex::new(None),
            stopping: AtomicBool::new(false),
        })
    }

    fn run(&self) -> MutexGuard<'_, Option<Run>> {
        self.run.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn status(&self) -> DrainStatus {
        let run = self.run();
        self.status_of(run.as_ref())
    }

    fn status_of(&self, run: Option<&Run>) -> DrainStatus {
        DrainStatus {
            state: self.shared.drain_state(),
            trigger: run.map(|r| r.trigger),
            started_at: run.map(|r| r.started_at),
            pending_messages: self.messages.pending(),
        }
    }

    /// `POST /node/drain`: põe o nó em drain sem encerrar o processo. O andamento fica em
    /// `GET /node/drain`.
    pub fn start_api(self: &Arc<Self>) -> Started {
        let stop = match self.begin(Trigger::Api) {
            Some(stop) => stop,
            None => return Started::AlreadyDraining(self.status()),
        };
        let this = self.clone();
        tokio::spawn(async move { this.run_drain(stop).await });
        Started::Started(self.status())
    }

    /// Entra em `draining` se o nó estava `active`. Devolve o sinal de cancelamento do drain.
    fn begin(&self, trigger: Trigger) -> Option<Arc<AtomicBool>> {
        let mut run = self.run();
        if !self
            .shared
            .transition_drain(DrainState::Active, DrainState::Draining)
        {
            return None;
        }
        let stop = Arc::new(AtomicBool::new(false));
        *run = Some(Run {
            trigger,
            started_at: self.clock.now(),
            stop: stop.clone(),
        });
        self.messages.begin_drain();
        tracing::info!(trigger = trigger.as_str(), "drain iniciado");
        Some(stop)
    }

    /// Esvazia a memória e, se deu certo, marca o save e passa a `drained`.
    async fn run_drain(self: Arc<Self>, stop: Arc<AtomicBool>) {
        let deadline = Instant::now() + self.timeout;
        let messages = self.messages.clone();
        let flag = stop.clone();
        let report = tokio::task::spawn_blocking(move || {
            messages.drain_until(|| flag.load(Ordering::Relaxed) || Instant::now() >= deadline)
        })
        .await;
        let report = match report {
            Ok(report) => report,
            Err(e) => {
                tracing::error!(error = %e, "o drain falhou");
                return;
            }
        };
        self.finish(&stop, report.moved, report.failed, report.remaining);
    }

    fn finish(&self, stop: &Arc<AtomicBool>, moved: u64, failed: u64, remaining: u64) {
        let run = self.run();
        // Cancelado (ou substituído por outro drain) enquanto esvaziava: não há o que concluir.
        let current = run.as_ref().is_some_and(|r| Arc::ptr_eq(&r.stop, stop));
        if !current || stop.load(Ordering::Relaxed) {
            return;
        }
        if remaining > 0 {
            tracing::error!(
                moved,
                failed,
                remaining,
                timeout = ?self.timeout,
                "drain incompleto: sobraram mensagens em memória (etapa: esvaziar a memória)"
            );
            return;
        }
        if let Err(e) = self.saves.mark_save_completed(self.clock.now_millis()) {
            tracing::error!(error = %e, "não foi possível gravar a marca do save");
            return;
        }
        self.shared
            .transition_drain(DrainState::Draining, DrainState::Drained);
        tracing::info!(moved, "drain concluído: a memória está toda em disco");
    }

    /// `DELETE /node/drain`: cancela o drain (ou, depois da saída, volta). Sem cluster não há
    /// desregistro, então nunca há `409` por esse motivo; só com o processo parando por sinal.
    pub fn cancel(&self) -> Result<Cancelled, TabaError> {
        if self.stopping.load(Ordering::Relaxed) {
            return Err(TabaError::new(
                ErrorCode::DrainConflict,
                "O processo está parando por sinal; o drain não pode ser cancelado.",
            ));
        }
        let mut run = self.run();
        let before = self.shared.drain_state();
        if before == DrainState::Active {
            return Ok(Cancelled::Nothing(self.status_of(run.as_ref())));
        }
        if let Some(r) = run.take() {
            r.stop.store(true, Ordering::Relaxed);
        }
        self.messages.cancel_drain();
        if let Err(e) = self.saves.clear_save_mark() {
            tracing::warn!(error = %e, "não foi possível apagar a marca do save");
        }
        self.shared.set_drain_state(DrainState::Active);
        tracing::info!(
            from = before.as_str(),
            "drain cancelado: o nó voltou a gravar em memória"
        );
        let status = self.status_of(None);
        Ok(match (before, self.shared.mode) {
            (DrainState::Drained, Mode::Production) => Cancelled::Returned(status),
            _ => Cancelled::Cancelled(status),
        })
    }

    /// A parada por sinal (`drain.on-stop`): faz o drain e espera terminar, dentro de
    /// `drain.timeout`. Devolve se a memória ficou toda em disco. Se a API já tinha começado o
    /// drain, espera por ele.
    pub async fn stop_by_signal(self: &Arc<Self>) -> bool {
        self.stopping.store(true, Ordering::Relaxed);
        match self.begin(Trigger::Signal) {
            Some(stop) => {
                self.clone().run_drain(stop).await;
            }
            None => self.wait_for_drained().await,
        }
        let done = self.shared.drain_state() == DrainState::Drained;
        if !done {
            tracing::error!(
                pending = self.messages.pending(),
                "o drain não terminou: encerrando mesmo assim, o que só estava em memória se perde"
            );
        }
        done
    }

    async fn wait_for_drained(&self) {
        let deadline = Instant::now() + self.timeout;
        while self.shared.drain_state() == DrainState::Draining && Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
}

#[cfg(test)]
mod tests;
