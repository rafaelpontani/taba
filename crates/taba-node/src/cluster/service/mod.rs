//! O nó dentro do cluster: atende as chamadas de outros nós (`Handler`), coordena o ingresso de
//! um nó novo (doc 04, seção 6.2) e entra num cluster existente (o bootstrap).
//!
//! Registro e desregistro de nó usam o protocolo em duas fases e exigem a confirmação de
//! **todos** os nós, sem quórum reduzido. Antes do commit a operação pode ser abortada (rollback);
//! depois de decidida não volta atrás, e o coordenador insiste até o teto `write.commit.timeout`.

use std::sync::{Arc, OnceLock, RwLock};
use std::time::{Duration, SystemTime};

use tokio::sync::Mutex;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use super::members::{Change, Member, MemberState, MemberTable, Membership, MembershipError};
use super::protocol::{
    Ack, JoinRequest, NodeRegisterRequest, NodeUnregisterRequest, OperationRef, Request, Response,
    UnregisterReason,
};
use super::replication::Replication;
use super::state::{NodeShared, NodeState};
use super::transport::{BoxFuture, CallError, Handler, PeerAddr, Transport};
use crate::config::Config;
use crate::core::clock::Clock;
use crate::core::error::{ErrorCode, TabaError};
use crate::core::id;
use crate::telemetry::Telemetry;
use crate::telemetry::metrics::{CLUSTER_BOOTSTRAP_FAILURES, CLUSTER_MEMBERSHIP_CHANGES};

/// O prazo de uma chamada a outro nó. Não é configurável por ora (D93).
const CALL_TIMEOUT: Duration = Duration::from_secs(5);
/// O prazo do `join`: o pedido espera a vez e coordena duas fases, então demora mais.
const JOIN_TIMEOUT: Duration = Duration::from_secs(60);
/// Segundos sugeridos em `Retry-After` quando o nó ainda não tem a tabela.
const SYNCING_RETRY_AFTER_SECS: u64 = 5;

/// Os parâmetros do protocolo de entrada (`write.*`, `cluster.*`).
#[derive(Debug, Clone)]
pub struct ClusterRules {
    /// `write.retry.attempts`
    pub retry_attempts: u32,
    /// `write.retry.backoff`
    pub retry_backoff: Duration,
    /// `write.commit.timeout`
    pub commit_timeout: Duration,
    /// `cluster.bootstrap.retry.interval`
    pub bootstrap_retry_interval: Duration,
    /// `cluster.bootstrap.retry.max-attempts`; `0` não limita.
    pub bootstrap_max_attempts: u32,
    /// `cluster.first-node`
    pub first_node: bool,
    /// `cluster.entry-address`
    pub entry: Option<PeerAddr>,
    /// `write.pending.ttl`
    pub pending_ttl: Duration,
    /// `ring.virtual-nodes`
    pub virtual_nodes: u32,
    /// `replication.factor`
    pub replication_factor: u32,
    /// `write.quorum`
    pub quorum: u32,
    /// `delivery.take.max-attempts` (a definir no doc 10: 3): quantos nós um `GET` com a lista
    /// local vazia consulta no máximo.
    pub take_max_attempts: usize,
    /// `cluster.channel.keepalive.interval` (a definir: 2 s): de quanto em quanto tempo o nó repete
    /// os sinais de estado, no lugar do sinal de vida do canal.
    pub keepalive_interval: Duration,
    pub call_timeout: Duration,
    pub join_timeout: Duration,
}

impl ClusterRules {
    pub fn from_config(config: &Config) -> Self {
        Self {
            retry_attempts: config.write.retry_attempts,
            retry_backoff: config.write.retry_backoff,
            commit_timeout: config.write.commit_timeout,
            bootstrap_retry_interval: config.cluster.bootstrap_retry_interval,
            bootstrap_max_attempts: config.cluster.bootstrap_retry_max_attempts,
            first_node: config.cluster.first_node,
            entry: config
                .cluster
                .entry_address
                .as_deref()
                .and_then(|a| a.parse().ok()),
            pending_ttl: config.write.pending_ttl,
            virtual_nodes: config.cluster.virtual_nodes,
            replication_factor: config.write.replication_factor,
            quorum: config.write.quorum,
            take_max_attempts: config.delivery.take_max_attempts.map_or(3, |n| n as usize),
            keepalive_interval: config
                .cluster
                .keepalive_interval
                .unwrap_or(Duration::from_secs(2)),
            call_timeout: CALL_TIMEOUT,
            join_timeout: JOIN_TIMEOUT,
        }
    }
}

/// O bootstrap esgotou as tentativas (`cluster.bootstrap.retry.max-attempts`) ou o nó parou.
#[derive(Debug, thiserror::Error)]
pub enum BootstrapError {
    #[error("o nó não conseguiu entrar no cluster em {attempts} tentativas: {last}")]
    Exhausted { attempts: u32, last: String },
    #[error("o nó parou antes de entrar no cluster")]
    Cancelled,
}

/// Uma tentativa de bootstrap que não deu certo.
struct Failure {
    reason: &'static str,
    message: String,
}

pub struct ClusterService {
    local: RwLock<Member>,
    membership: Membership,
    node: Arc<NodeShared>,
    transport: Arc<dyn Transport>,
    clock: Arc<dyn Clock>,
    telemetry: Arc<Telemetry>,
    rules: ClusterRules,
    /// Uma mudança de membros por vez neste nó (doc 04, seção 6.8): o `join` espera a vez.
    change_turn: Mutex<()>,
    cancel: CancellationToken,
    /// A replicação de definições e de mensagens, ligada depois da subida do nó.
    replication: OnceLock<Arc<Replication>>,
    /// Chamado quando a tabela de membros muda (o motor reavalia quem é o dono).
    on_change: OnceLock<Arc<dyn Fn() + Send + Sync>>,
}

/// O `Handler` do nó: o que as rotas `/cluster/*` e a rede em memória chamam.
pub struct ClusterHandler(pub Arc<ClusterService>);

impl Handler for ClusterHandler {
    fn handle<'a>(
        &'a self,
        caller: &'a str,
        request: Request,
    ) -> BoxFuture<'a, Result<Response, TabaError>> {
        Box::pin(async move { self.0.handle(caller, request).await })
    }
}

fn node_syncing() -> TabaError {
    TabaError::new(
        ErrorCode::ClusterNodeSyncing,
        "O nó ainda não tem a tabela de membros.",
    )
    .with_retry_after(SYNCING_RETRY_AFTER_SECS)
}

fn from_membership(error: MembershipError) -> TabaError {
    match error {
        MembershipError::NoTable => node_syncing(),
        MembershipError::Conflict(message) => TabaError::new(ErrorCode::ClusterConflict, message),
        MembershipError::UnknownOperation => TabaError::new(
            ErrorCode::ClusterOperationUnknown,
            "O nó não conhece essa operação (nunca chegou ou já expirou).",
        ),
    }
}

fn random_below(n: usize) -> usize {
    let mut bytes = [0u8; 8];
    getrandom::fill(&mut bytes).expect("o sistema não tem fonte de números aleatórios");
    (u64::from_le_bytes(bytes) % n.max(1) as u64) as usize
}

impl ClusterService {
    pub fn new(
        local: Member,
        node: Arc<NodeShared>,
        transport: Arc<dyn Transport>,
        clock: Arc<dyn Clock>,
        telemetry: Arc<Telemetry>,
        rules: ClusterRules,
        cancel: CancellationToken,
    ) -> Arc<Self> {
        Arc::new(Self {
            local: RwLock::new(local),
            membership: Membership::new(rules.pending_ttl, rules.virtual_nodes),
            node,
            transport,
            clock,
            telemetry,
            rules,
            change_turn: Mutex::new(()),
            cancel,
            replication: OnceLock::new(),
            on_change: OnceLock::new(),
        })
    }

    /// Liga a replicação (definições, mensagens, sinais e repasse) ao serviço.
    pub fn set_replication(&self, replication: Arc<Replication>) {
        let _ = self.replication.set(replication);
    }

    pub fn replication(&self) -> Option<&Arc<Replication>> {
        self.replication.get()
    }

    /// Registra quem é avisado quando a tabela de membros muda.
    pub fn set_on_change(&self, hook: Arc<dyn Fn() + Send + Sync>) {
        let _ = self.on_change.set(hook);
    }

    fn membership_changed(&self) {
        if let Some(hook) = self.on_change.get() {
            hook();
        }
    }

    pub fn cancel_token(&self) -> &CancellationToken {
        &self.cancel
    }

    pub fn clock(&self) -> &Arc<dyn Clock> {
        &self.clock
    }

    pub fn telemetry(&self) -> &Arc<Telemetry> {
        &self.telemetry
    }

    /// Os nós responsáveis pela posição, em ordem (primário primeiro): `replication.factor` nós
    /// distintos, ou menos se o cluster tem menos. `503` se o nó ainda não tem a tabela.
    pub fn owners_of(&self, position: u64) -> Result<Vec<Member>, TabaError> {
        let table = self.membership.table().ok_or_else(node_syncing)?;
        let ring = self.membership.ring();
        let wanted = self.rules.replication_factor.max(1) as usize;
        Ok(ring
            .owners(position, wanted)
            .into_iter()
            .filter_map(|id| table.member(id).cloned())
            .collect())
    }

    /// O IP com que este nó se apresenta a si mesmo (uma chamada local passa pela mesma
    /// conferência de membro que uma que veio da rede).
    pub fn self_caller(&self) -> String {
        self.local().addresses.first().cloned().unwrap_or_default()
    }

    pub fn is_self(&self, member: &Member) -> bool {
        member.node_id == self.node_id()
    }

    /// Faz a chamada a um membro: a si mesmo sem rede, aos outros pelo `Transport`.
    pub(crate) async fn call_member(
        self: &Arc<Self>,
        member: &Member,
        request: Request,
    ) -> Result<Response, CallError> {
        if self.is_self(member) {
            return self
                .handle(&self.self_caller(), request)
                .await
                .map_err(CallError::Remote);
        }
        self.call_peer(member, request, self.rules.call_timeout)
            .await
    }

    /// `call_member` com as tentativas e o intervalo de `write.retry.*` para falha de rede e
    /// erro `5xx` do par.
    pub(crate) async fn call_member_retrying(
        self: &Arc<Self>,
        member: &Member,
        request: Request,
    ) -> Result<Response, CallError> {
        let mut attempt = 0;
        loop {
            attempt += 1;
            let result = self.call_member(member, request.clone()).await;
            let retry = match &result {
                Err(e) if e.is_network() => true,
                Err(CallError::Remote(e)) => e.status() >= 500,
                _ => false,
            };
            if !retry || attempt >= self.rules.retry_attempts {
                return result;
            }
            tokio::select! {
                () = self.cancel.cancelled() => return result,
                () = tokio::time::sleep(self.rules.retry_backoff) => {}
            }
        }
    }

    pub fn local(&self) -> Member {
        self.local.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn node_id(&self) -> String {
        self.local
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .node_id
            .clone()
    }

    pub fn membership(&self) -> &Membership {
        &self.membership
    }

    pub fn rules(&self) -> &ClusterRules {
        &self.rules
    }

    pub(crate) fn now(&self) -> SystemTime {
        self.clock.now()
    }

    pub(crate) fn new_operation_id(&self) -> String {
        id::new_at(self.clock.now_millis())
    }

    fn count_change(&self, phase: &str, outcome: &str) {
        self.telemetry.registry.add(
            &CLUSTER_MEMBERSHIP_CHANGES,
            &[
                ("taba.operation", "register"),
                ("taba.phase", phase),
                ("taba.outcome", outcome),
            ],
            1.0,
        );
    }

    fn count_bootstrap_failure(&self, reason: &str) {
        self.telemetry
            .registry
            .add(&CLUSTER_BOOTSTRAP_FAILURES, &[("taba.reason", reason)], 1.0);
    }

    // ---- Quem atende ----

    /// Atende uma chamada de outro nó. `caller` é o IP de origem.
    pub async fn handle(
        self: &Arc<Self>,
        caller: &str,
        request: Request,
    ) -> Result<Response, TabaError> {
        match request {
            Request::ListNodes => self
                .membership
                .table()
                .map(Response::Members)
                .ok_or_else(node_syncing),
            Request::Join(request) => self.join(request).await,
            Request::Register(request) => {
                request.validate()?;
                self.require_member(caller)?;
                let id = request.operation_id.clone();
                let result = self
                    .membership
                    .register(&id, Change::Add(request.node), self.now());
                self.count_change("register", if result.is_ok() { "ok" } else { "failed" });
                result.map_err(from_membership)?;
                Ok(ack(id))
            }
            Request::Unregister(request) => {
                request.validate()?;
                self.require_member(caller)?;
                let id = request.operation_id.clone();
                self.membership
                    .register(
                        &id,
                        Change::Remove {
                            node_id: request.node_id,
                        },
                        self.now(),
                    )
                    .map_err(from_membership)?;
                Ok(ack(id))
            }
            Request::Commit(request) => {
                request.validate()?;
                self.require_member(caller)?;
                let result = self.membership.commit(&request.operation_id, self.now());
                self.count_change("commit", if result.is_ok() { "ok" } else { "failed" });
                result.map_err(from_membership)?;
                self.membership_changed();
                Ok(ack(request.operation_id))
            }
            Request::Rollback(request) => {
                request.validate()?;
                self.require_member(caller)?;
                let result = self.membership.rollback(&request.operation_id, self.now());
                self.count_change("rollback", if result.is_ok() { "ok" } else { "failed" });
                result.map_err(from_membership)?;
                Ok(ack(request.operation_id))
            }
            // O repasse de uma chamada de cliente também vale para quem saiu por drain (doc 04,
            // seção 6.9); quem o recebe nunca repassa de novo.
            Request::Forward(forward) => {
                let allowed = self.membership.is_member_address(caller)
                    || self.membership.is_departed_address(caller);
                if !allowed {
                    return Err(TabaError::new(
                        ErrorCode::ClusterNotMember,
                        "Quem chama não consta na tabela de membros.",
                    ));
                }
                if self.node.drain_state() != super::state::DrainState::Active {
                    return Err(TabaError::new(
                        ErrorCode::ClusterNodeDraining,
                        "O nó está em drain e não atende o repasse.",
                    )
                    .with_retry_after(SYNCING_RETRY_AFTER_SECS));
                }
                if self.node.state() != NodeState::Active {
                    return Err(node_syncing());
                }
                let Some(replication) = self.replication.get().cloned() else {
                    return Err(TabaError::route_not_found());
                };
                replication.forwarder.execute(forward).await
            }
            // Definições, mensagens, sinais: a replicação atende.
            other => {
                let Some(replication) = self.replication.get().cloned() else {
                    return Err(TabaError::route_not_found());
                };
                self.require_member(caller)?;
                replication.handle(caller, other).await
            }
        }
    }

    /// Só quem consta na tabela de membros chama as rotas de fase (doc 06c): o nó que ainda
    /// não tem a tabela não sabe quem é membro e responde `503`.
    pub(crate) fn require_member(&self, caller: &str) -> Result<(), TabaError> {
        if self.membership.table().is_none() {
            return Err(node_syncing());
        }
        if self.membership.is_member_address(caller) {
            Ok(())
        } else {
            Err(TabaError::new(
                ErrorCode::ClusterNotMember,
                "Quem chama não consta na tabela de membros.",
            ))
        }
    }

    // ---- Coordenar um ingresso ----

    /// `POST /cluster/nodes/join`: este nó coordena o registro do nó novo (doc 04, seção 6.2,
    /// passos 2 a 5) e só responde com a tabela depois do commit.
    async fn join(self: &Arc<Self>, request: JoinRequest) -> Result<Response, TabaError> {
        request.validate()?;
        if self.membership.table().is_none() {
            return Err(node_syncing());
        }
        // Uma mudança de membros por vez: outro ingresso em curso neste nó faz este esperar.
        let _turn = self.change_turn.lock().await;
        let table = self.membership.table().ok_or_else(node_syncing)?;
        if table.member(&request.node_id).is_some() {
            // O mesmo nó repetindo o pedido (o anterior foi aceito e a resposta se perdeu).
            return Ok(Response::Members(table));
        }

        self.probe(&request).await?;

        let node = Member {
            node_id: request.node_id.clone(),
            addresses: request.addresses.clone(),
            port: request.port,
            state: MemberState::Active,
        };
        let operation_id = self.new_operation_id();
        let peers: Vec<Member> = table
            .members
            .iter()
            .filter(|m| m.node_id != self.node_id())
            .cloned()
            .collect();

        // Fase 1: o registro em todos os nós, este inclusive.
        if let Err(e) =
            self.membership
                .register(&operation_id, Change::Add(node.clone()), self.now())
        {
            self.count_change("register", "failed");
            tracing::warn!(node_id = %node.node_id, error = ?e, "ingresso recusado neste nó");
            return Err(registration_failed());
        }
        let (confirmed, failures) = self
            .register_on_peers(
                &peers,
                Request::Register(NodeRegisterRequest {
                    operation_id: operation_id.clone(),
                    coordinator_id: self.node_id(),
                    node: node.clone(),
                }),
            )
            .await;
        if !failures.is_empty() {
            self.count_change("register", "failed");
            for (peer, error) in &failures {
                tracing::warn!(node_id = %node.node_id, peer = %peer, %error, "registro sem confirmação");
            }
            self.rollback_everywhere(&confirmed, &operation_id).await;
            tracing::error!(
                node_id = %node.node_id,
                "ingresso abortado (rollback): a fase de registro não foi confirmada por todos"
            );
            return Err(registration_failed());
        }
        self.count_change("register", "ok");

        // Fase 2: o quórum de todos foi atingido, a decisão está tomada e não volta atrás.
        let table = self
            .membership
            .commit(&operation_id, self.now())
            .map_err(from_membership)?;
        self.count_change("commit", "ok");
        self.membership_changed();
        if self.commit_on_peers(&peers, &operation_id).await {
            tracing::info!(
                node_id = %node.node_id,
                version = table.version,
                "nó novo registrado no cluster"
            );
            Ok(Response::Members(table))
        } else {
            self.count_change("commit", "timeout");
            tracing::warn!(
                node_id = %node.node_id,
                "o commit continua em segundo plano: o nó foi aceito"
            );
            Err(TabaError::new(
                ErrorCode::ClusterCommitTimeout,
                "O registro foi confirmado e o commit segue em segundo plano: o nó foi aceito. \
                 Consulte GET /cluster/nodes antes de repetir.",
            ))
        }
    }

    /// O teste de conexão (passo 2): conecta de volta ao nó novo. Uma resposta de erro do nó
    /// também prova que a conexão funciona (o nó novo ainda não tem a tabela e responde `503`).
    async fn probe(&self, request: &JoinRequest) -> Result<(), TabaError> {
        for address in &request.addresses {
            let to = PeerAddr::new(address.clone(), request.port);
            match self
                .call_addr(&to, Request::ListNodes, self.rules.call_timeout)
                .await
            {
                Ok(_) | Err(CallError::Remote(_)) => return Ok(()),
                Err(_) => {}
            }
        }
        tracing::warn!(node_id = %request.node_id, "o nó novo não é alcançável pelos IPs informados");
        Err(TabaError::new(
            ErrorCode::ClusterJoinUnreachable,
            "Não foi possível conectar de volta aos IPs informados.",
        ))
    }

    /// A fase 1 em todos os outros nós. Devolve quem confirmou e quem falhou.
    async fn register_on_peers(
        self: &Arc<Self>,
        peers: &[Member],
        request: Request,
    ) -> (Vec<Member>, Vec<(String, CallError)>) {
        let mut set = JoinSet::new();
        for peer in peers {
            let (me, peer, request) = (self.clone(), peer.clone(), request.clone());
            set.spawn(async move {
                // Outra mudança pendente no par (409) pode ser passageira: tenta de novo.
                let result = me.call_retrying(&peer, request, true).await;
                (peer, result)
            });
        }
        let (mut confirmed, mut failures) = (Vec::new(), Vec::new());
        while let Some(joined) = set.join_next().await {
            match joined {
                Ok((peer, Ok(_))) => confirmed.push(peer),
                Ok((peer, Err(error))) => failures.push((peer.node_id, error)),
                Err(e) => tracing::error!(error = %e, "task do registro falhou"),
            }
        }
        (confirmed, failures)
    }

    /// Descarta o registro pendente aqui e nos que já o tinham confirmado.
    async fn rollback_everywhere(self: &Arc<Self>, confirmed: &[Member], operation_id: &str) {
        let _ = self.membership.rollback(operation_id, self.now());
        self.count_change("rollback", "ok");
        let request = Request::Rollback(OperationRef {
            operation_id: operation_id.to_owned(),
        });
        let mut set = JoinSet::new();
        for peer in confirmed {
            let (me, peer, request) = (self.clone(), peer.clone(), request.clone());
            set.spawn(async move {
                if let Err(error) = me.call_retrying(&peer, request, false).await {
                    // O registro pendente expira sozinho em `write.pending.ttl`.
                    tracing::warn!(peer = %peer.node_id, %error, "rollback sem confirmação");
                }
            });
        }
        while set.join_next().await.is_some() {}
    }

    /// A fase 2 em todos os outros nós. `true` se todos confirmaram dentro de
    /// `write.commit.timeout`; senão os que faltam seguem sendo tentados em segundo plano.
    async fn commit_on_peers(self: &Arc<Self>, peers: &[Member], operation_id: &str) -> bool {
        let mut set = JoinSet::new();
        for peer in peers {
            let (me, peer, operation_id) = (self.clone(), peer.clone(), operation_id.to_owned());
            set.spawn(async move { me.commit_until_confirmed(peer, operation_id).await });
        }
        let all = tokio::time::timeout(self.rules.commit_timeout, async {
            while set.join_next().await.is_some() {}
        })
        .await
        .is_ok();
        if !all {
            set.detach_all();
        }
        all
    }

    /// Insiste no commit de um par até ele confirmar, sair da tabela ou o nó parar: depois da
    /// decisão não há rollback (doc 04, seção 6.2, passo 7).
    async fn commit_until_confirmed(self: Arc<Self>, peer: Member, operation_id: String) {
        let request = Request::Commit(OperationRef { operation_id });
        loop {
            match self
                .call_peer(&peer, request.clone(), self.rules.call_timeout)
                .await
            {
                Ok(_) => return,
                Err(CallError::Remote(error)) if error.status() < 500 => {
                    // O par não tem mais o registro (reiniciou ou expirou): não há o que insistir.
                    tracing::warn!(peer = %peer.node_id, code = %error.code, "commit recusado pelo par");
                    return;
                }
                Err(_) => {}
            }
            if !self.membership.is_member(&peer.node_id) {
                return;
            }
            tokio::select! {
                () = self.cancel.cancelled() => return,
                () = tokio::time::sleep(self.rules.retry_backoff) => {}
            }
        }
    }

    // ---- Chamadas a outros nós ----

    /// Chama um endereço, com prazo.
    async fn call_addr(
        &self,
        to: &PeerAddr,
        request: Request,
        timeout: Duration,
    ) -> Result<Response, CallError> {
        match tokio::time::timeout(timeout, self.transport.call(to, request)).await {
            Ok(result) => result,
            Err(_) => Err(CallError::Timeout),
        }
    }

    /// Chama um membro pelos IPs dele, na ordem, até um responder (mesmo que com erro).
    pub(crate) async fn call_peer(
        &self,
        peer: &Member,
        request: Request,
        timeout: Duration,
    ) -> Result<Response, CallError> {
        let mut last = CallError::Unreachable("o nó não tem endereço".into());
        for address in &peer.addresses {
            let to = PeerAddr::new(address.clone(), peer.port);
            match self.call_addr(&to, request.clone(), timeout).await {
                Err(e) if e.is_network() => last = e,
                other => return other,
            }
        }
        Err(last)
    }

    /// `call_peer` com `write.retry.attempts` tentativas e `write.retry.backoff` entre elas, para
    /// falha de rede e erro `5xx` do par (e, se pedido, o conflito `409`).
    async fn call_retrying(
        &self,
        peer: &Member,
        request: Request,
        retry_conflict: bool,
    ) -> Result<Response, CallError> {
        let mut attempt = 0;
        loop {
            attempt += 1;
            let result = self
                .call_peer(peer, request.clone(), self.rules.call_timeout)
                .await;
            let retry = match &result {
                Err(e) if e.is_network() => true,
                Err(CallError::Remote(e)) => {
                    e.status() >= 500 || (retry_conflict && e.code == ErrorCode::ClusterConflict)
                }
                _ => false,
            };
            if !retry || attempt >= self.rules.retry_attempts {
                return result;
            }
            tokio::select! {
                () = self.cancel.cancelled() => return result,
                () = tokio::time::sleep(self.rules.retry_backoff) => {}
            }
        }
    }

    // ---- Entrar num cluster ----

    /// O desregistro voluntário do drain (doc 04, seção 6.10, passo 5): este nó coordena a própria
    /// saída pelo protocolo de duas fases, **sem votação**, e espera a vez de outras mudanças de
    /// membros. Antes do commit, rollback; depois, o commit é insistido até `write.commit.timeout`.
    pub async fn leave(self: &Arc<Self>) -> Result<(), TabaError> {
        let _turn = self.change_turn.lock().await;
        let table = self.membership.table().ok_or_else(node_syncing)?;
        let me = self.node_id();
        if table.member(&me).is_none() {
            return Ok(());
        }
        let peers: Vec<Member> = table
            .members
            .iter()
            .filter(|m| m.node_id != me)
            .cloned()
            .collect();
        let operation_id = self.new_operation_id();
        let change = Change::Remove {
            node_id: me.clone(),
        };
        self.membership
            .register(&operation_id, change, self.now())
            .map_err(from_membership)?;
        let (confirmed, failures) = self
            .register_on_peers(
                &peers,
                Request::Unregister(NodeUnregisterRequest {
                    operation_id: operation_id.clone(),
                    coordinator_id: me.clone(),
                    node_id: me.clone(),
                    reason: UnregisterReason::Drain,
                }),
            )
            .await;
        if !failures.is_empty() {
            for (peer, error) in &failures {
                tracing::warn!(peer = %peer, %error, "desregistro sem confirmação");
            }
            self.rollback_everywhere(&confirmed, &operation_id).await;
            tracing::error!("saída do cluster abortada (rollback): a fase 1 não fechou em todos");
            // Outra mudança pendente num par: quem pede a saída espera a vez e tenta de novo.
            let contended = failures.iter().any(
                |(_, e)| matches!(e, CallError::Remote(r) if r.code == ErrorCode::ClusterConflict),
            );
            return Err(if contended {
                TabaError::new(
                    ErrorCode::ClusterConflict,
                    "Há outra mudança de membros em andamento.",
                )
            } else {
                registration_failed()
            });
        }
        self.membership
            .commit(&operation_id, self.now())
            .map_err(from_membership)?;
        self.membership_changed();
        if self.commit_on_peers(&peers, &operation_id).await {
            tracing::info!(node_id = %me, "nó fora do cluster (drain)");
            Ok(())
        } else {
            tracing::warn!("o commit da saída segue em segundo plano em algum nó");
            Err(TabaError::new(
                ErrorCode::ClusterCommitTimeout,
                "A saída foi decidida e o commit segue em segundo plano.",
            ))
        }
    }

    /// Volta do drain **como nó novo** (doc 04, seção 6.10): ID novo, estado local limpo e o
    /// bootstrap completo, com nova posição no anel. Não há freezing: o ID é outro e a saída foi
    /// voluntária.
    pub async fn rejoin_as_new(self: &Arc<Self>) {
        let new_id = id::new_at(self.clock.now_millis());
        if let Some(replication) = self.replication.get().cloned() {
            replication.reset_local().await;
        }
        self.local
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .node_id = new_id.clone();
        self.node.set_node_id(new_id.clone());
        self.membership.clear();
        self.node.set_state(NodeState::Joining);
        tracing::info!(node_id = %new_id, "nó voltou do drain como nó novo; refazendo o bootstrap");
        let me = self.clone();
        tokio::spawn(async move {
            if let Err(e) = me.bootstrap().await {
                tracing::error!(error = %e, "o nó não conseguiu voltar ao cluster");
            }
        });
    }

    /// O primeiro nó de um cluster novo: a tabela é só ele.
    pub fn start_alone(&self) {
        self.membership.install(MemberTable {
            version: 1,
            members: vec![self.local()],
        });
        self.node.set_state(NodeState::Active);
        self.membership_changed();
        tracing::info!(node_id = %self.node_id(), "primeiro nó: o cluster começa só com ele");
    }

    /// A tabela chegou: instala, puxa de um membro as definições de tópico e os tokens (doc 02,
    /// seções 3.3 e 3.4) e só então o nó fica ativo.
    async fn finish_join(&self, table: MemberTable, via: &Member) -> Result<(), Failure> {
        // O membro que coordenou o ingresso já tem o nó novo na tabela; os outros podem ainda não
        // ter (o commit chega a eles em seguida), então as definições se puxam dele primeiro.
        let mut sources = vec![via.clone()];
        sources.extend(
            table
                .members
                .iter()
                .filter(|m| m.node_id != self.node_id() && m.node_id != via.node_id)
                .cloned(),
        );
        self.membership.install(table.clone());
        if let Some(replication) = self.replication.get().cloned() {
            let mut last = None;
            for source in &sources {
                match replication.definitions.pull(source).await {
                    Ok(()) => {
                        last = None;
                        break;
                    }
                    Err(e) => last = Some((source.node_id.clone(), e)),
                }
            }
            if let Some((source, e)) = last {
                return Err(Failure {
                    reason: "sync",
                    message: format!("não foi possível puxar as definições de {source}: {e}"),
                });
            }
        }
        self.activate(&table);
        Ok(())
    }

    fn activate(&self, table: &MemberTable) {
        let version = table.version;
        let members = table.members.len();
        self.node.set_state(NodeState::Active);
        self.membership_changed();
        tracing::info!(node_id = %self.node_id(), version, members, "nó no cluster");
    }

    /// O bootstrap (doc 04, seção 6.2): lista os membros pelo balanceador, pede o ingresso a
    /// um deles e instala a tabela que volta. Repete a cada `cluster.bootstrap.retry.interval`
    /// até `cluster.bootstrap.retry.max-attempts` (`0` não limita).
    pub async fn bootstrap(self: &Arc<Self>) -> Result<(), BootstrapError> {
        let mut attempts = 0u32;
        loop {
            attempts += 1;
            let failure = match self.attempt_join().await {
                Ok(()) => return Ok(()),
                Err(failure) => failure,
            };
            self.count_bootstrap_failure(failure.reason);
            tracing::error!(
                attempt = attempts,
                reason = failure.reason,
                "falha no bootstrap: {}",
                failure.message
            );
            let max = self.rules.bootstrap_max_attempts;
            if max != 0 && attempts >= max {
                return Err(BootstrapError::Exhausted {
                    attempts,
                    last: failure.message,
                });
            }
            tokio::select! {
                () = self.cancel.cancelled() => return Err(BootstrapError::Cancelled),
                () = tokio::time::sleep(self.rules.bootstrap_retry_interval) => {}
            }
        }
    }

    async fn attempt_join(self: &Arc<Self>) -> Result<(), Failure> {
        let entry = self.rules.entry.clone().ok_or_else(|| Failure {
            reason: "config",
            message: "cluster.entry-address não é um host:porta".into(),
        })?;

        // 1. A listagem de membros, pelo balanceador.
        let listed = self
            .call_addr(&entry, Request::ListNodes, self.rules.call_timeout)
            .await;
        let table = match listed {
            Ok(Response::Members(table)) => table,
            Ok(_) => {
                return Err(Failure {
                    reason: "list",
                    message: "resposta inesperada à listagem de nós".into(),
                });
            }
            Err(e) if e.is_network() && self.rules.first_node => {
                // Só o primeiro nó de um cluster novo começa sozinho, e só quando a listagem
                // não encontra ninguém.
                self.start_alone();
                return Ok(());
            }
            Err(e) => {
                return Err(Failure {
                    reason: "list",
                    message: format!("a listagem de nós em {entry} falhou: {e}"),
                });
            }
        };

        // 2. Um membro ao acaso; se não responde, outro.
        let mut candidates = table.members;
        let mut shuffled = Vec::with_capacity(candidates.len());
        while !candidates.is_empty() {
            shuffled.push(candidates.swap_remove(random_below(candidates.len())));
        }
        let request = Request::Join(JoinRequest {
            node_id: self.local().node_id,
            addresses: self.local().addresses,
            port: self.local().port,
            version: env!("CARGO_PKG_VERSION").to_owned(),
        });
        let mut last = Failure {
            reason: "join",
            message: "nenhum membro respondeu ao pedido de ingresso".into(),
        };
        for member in shuffled {
            match self
                .call_peer(&member, request.clone(), self.rules.join_timeout)
                .await
            {
                Ok(Response::Members(table)) => {
                    self.finish_join(table, &member).await?;
                    return Ok(());
                }
                Ok(_) => {
                    last = Failure {
                        reason: "join",
                        message: "resposta inesperada ao pedido de ingresso".into(),
                    };
                }
                Err(CallError::Remote(error)) if error.code == ErrorCode::ClusterCommitTimeout => {
                    // O nó foi aceito: antes de repetir, a tabela diz se ele já consta.
                    if let Ok(Response::Members(table)) = self
                        .call_peer(&member, Request::ListNodes, self.rules.call_timeout)
                        .await
                        && table.member(&self.node_id()).is_some()
                    {
                        self.finish_join(table, &member).await?;
                        return Ok(());
                    }
                    return Err(Failure {
                        reason: "phase2",
                        message: error.message,
                    });
                }
                Err(CallError::Remote(error)) => {
                    let reason = match error.code {
                        ErrorCode::ClusterJoinUnreachable => "connectivity",
                        ErrorCode::ClusterRegistrationFailed => "phase1",
                        ErrorCode::ClusterNodeFrozen => "frozen",
                        _ => "join",
                    };
                    return Err(Failure {
                        reason,
                        message: format!("{}: {}", error.code, error.message),
                    });
                }
                Err(e) => {
                    last = Failure {
                        reason: "join",
                        message: format!("o membro {} não respondeu: {e}", member.node_id),
                    };
                }
            }
        }
        Err(last)
    }
}

fn ack(operation_id: String) -> Response {
    Response::Ack(Ack {
        operation_id,
        accepted: true,
    })
}

fn registration_failed() -> TabaError {
    TabaError::new(
        ErrorCode::ClusterRegistrationFailed,
        "A fase de registro não foi confirmada por todos os nós; o registro foi cancelado.",
    )
    .with_retry_after(SYNCING_RETRY_AFTER_SECS)
}
