//! O modo drain (doc 04, seção 6.10; doc 06b, "Drain"). Hoje vale a parte **local**: sair da
//! rotação, gravar tudo em disco (o save do desenvolvimento) e voltar. O aviso aos pares e o
//! desregistro do nó entram com o protocolo de cluster.
//!
//! Estados: `active` → `draining` → `drained`. Entra por sinal (`drain.on-stop`) ou pela API.
//! O `drain.timeout` cobre o drain inteiro; estourado, o nó registra o que sobrou em memória
//! e, se foi por sinal, encerra mesmo assim.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant, SystemTime};

use super::backup::Backup;
use super::service::{ClusterService, conflict_wait};
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
    /// Em `mode=production`: o nó no cluster, que avisa os pares, se desregistra e volta.
    cluster: OnceLock<Arc<ClusterService>>,
    /// O desregistro (passo 5) está em andamento: `DELETE /node/drain` responde `409`.
    leaving: AtomicBool,
    /// O backup opcional em S3: o drain manda tudo ao bucket antes de sair (doc 23, seção 1.10).
    backup: OnceLock<Arc<Backup>>,
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
            cluster: OnceLock::new(),
            leaving: AtomicBool::new(false),
            backup: OnceLock::new(),
        })
    }

    /// Liga o backup em S3 ao drain.
    pub fn set_backup(&self, backup: Arc<Backup>) {
        let _ = self.backup.set(backup);
    }

    /// O drain pedido por quem para o cluster inteiro (`POST /cluster/stop`): começa, espera
    /// concluir (memória em disco, envio ao S3, saída do cluster) e devolve se o nó chegou a
    /// `drained`. Dentro do prazo do drain mais o do backup.
    pub async fn drain_and_wait(self: &Arc<Self>) -> bool {
        let _ = self.start_api();
        let backup_timeout = self
            .backup
            .get()
            .map_or(Duration::ZERO, |b| b.rules().drain_timeout);
        let deadline = Instant::now() + self.timeout + backup_timeout + Duration::from_secs(5);
        while self.shared.drain_state() != DrainState::Drained && Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        self.shared.drain_state() == DrainState::Drained
    }

    /// Liga o drain ao cluster: o aviso aos pares, o desregistro e a volta como nó novo.
    pub fn set_cluster(&self, cluster: Arc<ClusterService>) {
        let _ = self.cluster.set(cluster);
    }

    fn signals(&self) -> Option<Arc<crate::cluster::replication::Signals>> {
        self.cluster
            .get()
            .and_then(|c| c.replication())
            .map(|r| r.signals.clone())
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
        // Os pares deixam de pedir `take` a este nó (doc 04, seção 6.10, passo 2).
        if let Some(signals) = self.signals() {
            signals.announce_draining();
        }
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
        // Com a memória toda em disco, tudo vai ao S3 **antes** de o nó sair do cluster: depois da
        // saída ele deixa de ser dono do que enviaria. Não espera para sempre: passado
        // `backup.drain.timeout` o nó segue com o que tem (doc 23, seção 1.10, cenário 18).
        if let Some(backup) = self.backup.get().cloned()
            && report.remaining == 0
            && !stop.load(Ordering::Relaxed)
        {
            let sent = backup.drain_upload().await;
            if sent.complete() {
                tracing::info!(
                    segments = sent.segments_uploaded,
                    messages = sent.messages_uploaded,
                    "drain: tudo foi para o S3"
                );
            } else {
                tracing::error!(
                    outbox = sent.outbox_pending,
                    journal = sent.journal_pending,
                    "drain: o envio ao S3 não terminou no prazo (backup.drain.timeout); o nó sai assim mesmo e o que sobrou depende das réplicas"
                );
            }
        }
        // Com a memória toda em disco, o nó pede a própria saída do cluster, sem votação.
        if let Some(cluster) = self.cluster.get().cloned()
            && report.remaining == 0
            && !stop.load(Ordering::Relaxed)
        {
            self.leaving.store(true, Ordering::Relaxed);
            // Outra mudança de membros em curso: o pedido espera a vez, dentro de `drain.timeout`
            // (doc 04, seção 6.8).
            let left = tokio::time::timeout_at(deadline.into(), async {
                loop {
                    match cluster.leave().await {
                        Err(e) if e.code == ErrorCode::ClusterConflict => {
                            tokio::time::sleep(conflict_wait()).await;
                        }
                        other => return other,
                    }
                }
            })
            .await;
            self.leaving.store(false, Ordering::Relaxed);
            match left {
                Ok(Ok(())) => {}
                // Decidido, mas o commit ainda corre em algum par: o nó já saiu da tabela.
                Ok(Err(e)) if e.code == ErrorCode::ClusterCommitTimeout => {}
                Ok(Err(e)) => {
                    tracing::error!(error = %e, "o drain não conseguiu sair do cluster: o nó segue em draining");
                    return;
                }
                Err(_) => {
                    tracing::error!(
                        "drain.timeout estourou antes de o nó sair do cluster: o nó segue em draining"
                    );
                    return;
                }
            }
        }
        self.finish(&stop, report.moved, report.failed, report.remaining);
        // Saiu limpo: o batimento no bucket some junto (quem observa não espera o prazo de morte).
        if self.shared.drain_state() == DrainState::Drained
            && let Some(backup) = self.backup.get().cloned()
        {
            backup.leave().await;
        }
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
        if self.leaving.load(Ordering::Relaxed) {
            return Err(TabaError::new(
                ErrorCode::DrainConflict,
                "O nó está se desregistrando do cluster; tente de novo quando estiver drained.",
            ));
        }
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
        if let Some(backup) = self.backup.get() {
            backup.resume_beating();
        }
        let status = self.status_of(None);
        Ok(match (before, self.shared.mode) {
            (DrainState::Drained, Mode::Production) => {
                // Já fora do cluster: volta como nó novo, com ID novo e bootstrap completo.
                if let Some(cluster) = self.cluster.get().cloned() {
                    tokio::spawn(async move { cluster.rejoin_as_new().await });
                }
                Cancelled::Returned(status)
            }
            _ => {
                if let Some(signals) = self.signals() {
                    signals.announce_active();
                }
                Cancelled::Cancelled(status)
            }
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
