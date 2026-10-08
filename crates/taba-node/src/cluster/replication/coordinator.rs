//! Quem **coordena** uma operação em duas fases sobre os nós responsáveis por uma mensagem (doc 05,
//! seção 7.2): registro em todos, commit decidido por quórum, rollback antes da decisão e
//! insistência depois dela.
//!
//! 1. A fase 1 vai a **todos** os nós responsáveis ao mesmo tempo, cada um com as tentativas de
//!    `write.retry.*`. O próprio nó, se for um deles, é chamado sem rede.
//! 2. Assim que o **quórum** confirma, **primário incluso**, o coordenador decide o commit. Antes da
//!    decisão, se o primário falha ou o quórum não tem mais como fechar, decide o **rollback**
//!    (`503 cluster.quorum-unavailable`, ou o erro que o primário devolveu).
//! 3. Decidido o commit, a chamada só responde depois de o commit estar confirmado no quórum
//!    (o primário e mais `write.quorum − 1` nós). Os demais nós seguem em segundo plano: registram
//!    e fazem o commit pelo tempo que for preciso, sem rollback, e `write.commit.timeout` esgotado
//!    sem confirmação só gera o aviso e a métrica (o bloco fica abaixo do fator pleno).

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Instant;

use tokio::sync::{mpsc, watch};

use crate::cluster::members::Member;
use crate::cluster::protocol::Request;
use crate::cluster::service::ClusterService;
use crate::cluster::transport::CallError;
use crate::core::error::{ErrorCode, TabaError};
use crate::telemetry::metrics::{
    WRITE_COMMIT_TIMEOUTS, WRITE_COORDINATED, WRITE_RETRIES, WRITE_ROLLBACKS,
};

/// O que o coordenador faz nos nós responsáveis.
pub struct Plan {
    /// Os nós responsáveis, o primário primeiro.
    pub owners: Vec<Member>,
    /// `write.quorum`: o primário e mais `quorum − 1` nós.
    pub quorum: usize,
    /// Quantos dos primeiros responsáveis o quórum precisa incluir: 1 (o primário) na fila comum;
    /// 2 (o primário e o secundário) na fila FIFO (doc 17, seção 16.5).
    pub required: usize,
    pub register: Request,
    pub commit: Request,
    pub rollback: Request,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Decision {
    Pending,
    Commit,
    Abort,
}

enum Event {
    Registered(usize),
    RegisterFailed(usize, CallError),
    Committed(usize),
    CommitFailed(usize, CallError),
    CommitSlow(usize),
}

fn quorum_unavailable() -> TabaError {
    TabaError::new(
        ErrorCode::ClusterQuorumUnavailable,
        "Não há nós responsáveis suficientes para fechar o quórum.",
    )
    .with_retry_after(5)
}

fn commit_timeout() -> TabaError {
    TabaError::new(
        ErrorCode::ClusterCommitTimeout,
        "O quórum decidiu, mas o commit não foi confirmado no prazo.",
    )
    .with_retry_after(5)
}

/// Roda a operação. `Ok` quando o commit está confirmado no quórum.
pub async fn run(service: &Arc<ClusterService>, plan: Plan) -> Result<(), TabaError> {
    let result = run_inner(service, plan).await;
    service.telemetry().registry.add(
        &WRITE_COORDINATED,
        &[(
            "taba.outcome",
            if result.is_ok() {
                "committed"
            } else {
                "failed"
            },
        )],
        1.0,
    );
    result
}

async fn run_inner(service: &Arc<ClusterService>, plan: Plan) -> Result<(), TabaError> {
    let total = plan.owners.len();
    let required = plan.required.max(1);
    let quorum = plan.quorum.max(required);
    if total == 0 || total < quorum {
        return Err(quorum_unavailable());
    }
    let plan = Arc::new(plan);
    let (events_tx, mut events) = mpsc::unbounded_channel();
    let (decision_tx, decision_rx) = watch::channel(Decision::Pending);
    for index in 0..total {
        tokio::spawn(owner_task(
            service.clone(),
            plan.clone(),
            index,
            decision_rx.clone(),
            events_tx.clone(),
        ));
    }
    drop(events_tx);

    let mut confirmed: Vec<usize> = Vec::new();
    let mut remaining = total;
    let mut primary_error: Option<CallError> = None;
    let mut required_failed = false;
    let mut committed: Vec<usize> = Vec::new();
    let mut quorum_set: Vec<usize> = Vec::new();
    let mut decided = false;
    while let Some(event) = events.recv().await {
        match event {
            Event::Registered(index) => {
                confirmed.push(index);
                remaining -= 1;
            }
            Event::RegisterFailed(index, error) => {
                remaining -= 1;
                if index == 0 {
                    primary_error = Some(error);
                } else if index < required {
                    required_failed = true;
                }
            }
            Event::Committed(index) => committed.push(index),
            Event::CommitFailed(index, error) => {
                if decided && quorum_set.contains(&index) {
                    tracing::error!(%error, "commit recusado por um nó do quórum depois da decisão");
                    return Err(commit_timeout());
                }
            }
            Event::CommitSlow(index) => {
                if decided && quorum_set.contains(&index) {
                    return Err(commit_timeout());
                }
            }
        }
        if !decided {
            if (0..required).all(|i| confirmed.contains(&i)) && confirmed.len() >= quorum {
                decided = true;
                quorum_set.extend(0..required);
                quorum_set.extend(
                    confirmed
                        .iter()
                        .copied()
                        .filter(|i| *i >= required)
                        .take(quorum - required),
                );
                let _ = decision_tx.send(Decision::Commit);
            } else if primary_error.is_some()
                || required_failed
                || confirmed.len() + remaining < quorum
            {
                let _ = decision_tx.send(Decision::Abort);
                service.telemetry().registry.add(&WRITE_ROLLBACKS, &[], 1.0);
                tracing::error!(
                    confirmed = confirmed.len(),
                    quorum,
                    "operação abortada (rollback): o quórum não fechou"
                );
                return Err(match primary_error {
                    // O que o primário diz vale: mensagem que não existe, token vencido...
                    Some(CallError::Remote(error)) if error.status() < 500 => error,
                    _ => quorum_unavailable(),
                });
            }
        }
        if decided && quorum_set.iter().all(|i| committed.contains(i)) {
            return Ok(());
        }
    }
    Err(commit_timeout())
}

/// O trabalho de um nó responsável: registrar, esperar a decisão e fazer o commit (ou o rollback).
/// O futuro é apagado para `dyn`: sem isso o compilador não fecha o ciclo `run` → `owner_task` →
/// `ClusterService::handle` → `ClusterMessages` → `run`.
fn owner_task(
    service: Arc<ClusterService>,
    plan: Arc<Plan>,
    index: usize,
    decision: watch::Receiver<Decision>,
    events: mpsc::UnboundedSender<Event>,
) -> Pin<Box<dyn Future<Output = ()> + Send>> {
    Box::pin(owner_work(service, plan, index, decision, events))
}

async fn owner_work(
    service: Arc<ClusterService>,
    plan: Arc<Plan>,
    index: usize,
    mut decision: watch::Receiver<Decision>,
    events: mpsc::UnboundedSender<Event>,
) {
    let owner = &plan.owners[index];
    let registered = match service
        .call_member_retrying(owner, plan.register.clone())
        .await
    {
        Ok(_) => {
            let _ = events.send(Event::Registered(index));
            true
        }
        Err(error) => {
            let semantic = matches!(&error, CallError::Remote(e) if e.status() < 500);
            if error.is_network() {
                count_retry(&service, "register", owner);
            }
            let _ = events.send(Event::RegisterFailed(index, error));
            if semantic {
                return;
            }
            // Falha de rede: se o quórum fechar sem este nó, o registro e o commit seguem aqui
            // em segundo plano, até ele voltar (doc 05, seção 7.2, passo 7).
            false
        }
    };

    let decided = loop {
        let current = *decision.borrow_and_update();
        if current != Decision::Pending {
            break current;
        }
        if decision.changed().await.is_err() {
            // O coordenador terminou sem decidir: o registro expira sozinho.
            return;
        }
    };
    if decided == Decision::Abort {
        if !registered {
            return;
        }
        if let Err(error) = service
            .call_member_retrying(owner, plan.rollback.clone())
            .await
        {
            tracing::warn!(node_id = %owner.node_id, %error, "rollback sem confirmação: o registro expira sozinho");
        }
        return;
    }
    commit_until_done(&service, &plan, index, &events, registered).await;
}

fn count_retry(service: &ClusterService, phase: &str, owner: &Member) {
    service.telemetry().registry.add(
        &WRITE_RETRIES,
        &[("taba.peer", &owner.node_id), ("taba.phase", phase)],
        1.0,
    );
}

/// Insiste no commit até o nó confirmar (depois da decisão não há rollback). Se o nó perdeu o
/// registro (expirou ou reiniciou), registra de novo e repete o commit.
async fn commit_until_done(
    service: &Arc<ClusterService>,
    plan: &Plan,
    index: usize,
    events: &mpsc::UnboundedSender<Event>,
    mut registered: bool,
) {
    let owner = &plan.owners[index];
    let started = Instant::now();
    let timeout = service.rules().commit_timeout;
    let mut slow = false;
    loop {
        // O nó que não chegou a registrar (ou perdeu o registro) registra antes do commit.
        let step = if registered {
            service.call_member(owner, plan.commit.clone()).await
        } else {
            match service.call_member(owner, plan.register.clone()).await {
                Ok(_) => {
                    registered = true;
                    continue;
                }
                Err(error) => Err(error),
            }
        };
        match step {
            Ok(_) => {
                let _ = events.send(Event::Committed(index));
                return;
            }
            Err(CallError::Remote(error)) if error.code == ErrorCode::ClusterOperationUnknown => {
                // Registra de novo e tenta o commit na volta.
                registered = false;
                continue;
            }
            Err(CallError::Remote(error)) if error.status() < 500 => {
                let _ = events.send(Event::CommitFailed(index, CallError::Remote(error)));
                return;
            }
            Err(_) => count_retry(service, "commit", owner),
        }
        if !slow && started.elapsed() >= timeout {
            slow = true;
            service
                .telemetry()
                .registry
                .add(&WRITE_COMMIT_TIMEOUTS, &[], 1.0);
            tracing::warn!(
                node_id = %owner.node_id,
                "write.commit.timeout esgotado sem commit confirmado: o bloco fica abaixo do fator pleno e a tentativa segue em segundo plano"
            );
            let _ = events.send(Event::CommitSlow(index));
        }
        if service.cancel_token().is_cancelled() || !service.membership().is_member(&owner.node_id)
        {
            let _ = events.send(Event::CommitFailed(
                index,
                CallError::Unreachable("o nó saiu do cluster".into()),
            ));
            return;
        }
        tokio::select! {
            () = service.cancel_token().cancelled() => {}
            () = tokio::time::sleep(service.rules().retry_backoff) => {}
        }
    }
}
