//! A recuperação de perdas (doc 04, seção 6.6; doc 06c, "Sincronização e recuperação").
//!
//! Dois lados:
//!
//! - **Quem serve:** a tabela `losses` do nó (`GET /cluster/message-loss`), o registro e a baixa de
//!   uma perda (`POST`/`DELETE /cluster/message-loss`) e a aceitação das mensagens que um nó que
//!   volta encontrou (`POST /cluster/messages/recover`). As duas primeiras rotas e a última
//!   aceitam quem ainda não é membro.
//! - **Quem volta:** o nó que descobre ter sido excluído, ou que volta do drain, consulta a lista de
//!   perdas de todos os nós que alcança, procura cada mensagem no que ainda guarda e a envia ao
//!   responsável pelo bloco; só então limpa o estado local e refaz o ingresso.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use tokio::task::JoinSet;
use tokio::time::Instant;

use super::members::{Member, MemberTable};
use super::protocol::{
    LossAck, LossRecord, MessageRecord, RecoverResult, Request, Response, StreamPage, StreamQuery,
};
use super::service::ClusterService;
use super::state::NodeState;
use super::transport::CallError;
use crate::core::error::{ErrorCode, TabaError};
use crate::core::ring::Ring;
use crate::core::time;
use crate::engine::messages::{Loaded, Messages};
use crate::storage::losses::{Loss, LossStore};
use crate::telemetry::metrics::{CLUSTER_RECOVER, RECOVER_MESSAGES};

/// O que a volta recuperou, para o log e para os testes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Summary {
    /// Perdas distintas que os nós consultados listaram.
    pub losses: usize,
    /// Quantas dessas o nó ainda tinha.
    pub found: usize,
    /// Quantas o responsável aplicou (quórum de gravação atingido).
    pub recovered: usize,
    /// Todos os nós responderam à consulta antes de `cluster.recovery.timeout`.
    pub complete: bool,
}

pub struct Recovery {
    service: Arc<ClusterService>,
    messages: Arc<Messages>,
    losses: LossStore,
    /// O teto de linhas de uma página da lista de perdas e de um envio (`cluster.sync.stream.max-items`).
    max_items: usize,
}

fn disk(error: &std::io::Error) -> TabaError {
    tracing::error!(error = %error, "falha ao acessar a tabela de perdas");
    TabaError::internal()
}

fn node_syncing() -> TabaError {
    TabaError::new(
        ErrorCode::ClusterNodeSyncing,
        "O nó ainda está em sincronização e não aceita mensagens recuperadas.",
    )
    .with_retry_after(5)
}

impl Recovery {
    pub fn new(
        service: Arc<ClusterService>,
        messages: Arc<Messages>,
        losses: LossStore,
        max_items: usize,
    ) -> Arc<Self> {
        Arc::new(Self {
            service,
            messages,
            losses,
            max_items: max_items.max(2),
        })
    }

    /// A tabela de perdas deste nó.
    pub fn losses(&self) -> &LossStore {
        &self.losses
    }

    fn count(&self, def: &'static crate::telemetry::metrics::Def, label: (&str, &str), n: usize) {
        if n > 0 {
            self.service
                .telemetry()
                .registry
                .add(def, &[label], n as f64);
        }
    }

    // ---- Quem serve ----

    /// Apaga os registros mais velhos que `cluster.message-loss.ttl`.
    fn expire(&self) {
        let ttl =
            u64::try_from(self.service.rules().message_loss_ttl.as_millis()).unwrap_or(u64::MAX);
        let cutoff = self.service.clock().now_millis().saturating_sub(ttl);
        if let Err(e) = self.losses.expire(cutoff) {
            tracing::warn!(error = %e, "falha ao expirar a tabela de perdas");
        }
    }

    /// `GET /cluster/message-loss`: a tabela `losses`, em ordem de ID.
    pub fn serve_losses(&self, query: &StreamQuery) -> Result<Response, TabaError> {
        self.expire();
        let limit = query
            .limit
            .map_or(self.max_items, |n| (n as usize).clamp(2, self.max_items));
        let rows = self
            .losses
            .list(query.from.as_deref(), limit + 1)
            .map_err(|e| disk(&e))?;
        let more = rows.len() > limit;
        let items = rows
            .into_iter()
            .take(limit)
            .filter(|loss| query.to.as_deref().is_none_or(|to| loss.id.as_str() <= to))
            .map(|loss| json!({ "id": loss.id, "topic": loss.topic }))
            .collect();
        Ok(Response::Stream(StreamPage::new(items, more)))
    }

    /// `POST /cluster/message-loss`: outro nó declarou uma perda num bloco que este também guarda.
    pub fn register_loss(&self, record: LossRecord) -> Result<Response, TabaError> {
        record.validate()?;
        let detected_ms = time::parse_ms(&record.detected_at).unwrap_or(0);
        let changed = self
            .losses
            .record(&Loss {
                topic: record.topic,
                id: record.id,
                block: record.block,
                detected_ms,
            })
            .map_err(|e| disk(&e))?;
        Ok(Response::Loss(LossAck { changed }))
    }

    /// `DELETE /cluster/message-loss/{topic}/{id}`: a mensagem foi recuperada em outro nó.
    pub fn remove_loss(&self, topic: &str, id: &str) -> Result<Response, TabaError> {
        super::protocol::validate_message_path(topic, id)?;
        let changed = self.losses.remove(topic, id).map_err(|e| disk(&e))?;
        Ok(Response::Loss(LossAck { changed }))
    }

    /// Grava a mensagem recuperada neste nó, sem as duas fases: a mensagem não existe nos donos,
    /// ninguém concorre por ela. A invisibilidade é zerada e o recibo de uma entrega antiga some;
    /// o `deliveryCount` fica.
    async fn apply_local(&self, record: &MessageRecord) -> Result<Loaded, TabaError> {
        let messages = self.messages.clone();
        let topic = record.topic.clone();
        let mut meta = record.to_meta();
        meta.invisible_until_ms = 0;
        meta.receipt_token = None;
        let body = record.body.clone();
        tokio::task::spawn_blocking(move || messages.apply_load(&topic, meta, body))
            .await
            .map_err(|_| TabaError::internal())?
    }

    /// `POST /cluster/messages/recover`. Cada registro vai aos nós responsáveis pelo bloco; com
    /// `replicate` este nó grava o seu e manda aos outros, e a linha só conta como aplicada quando
    /// `write.quorum` nós gravaram. Sem `replicate` só grava aqui (é o pedido de outro nó
    /// responsável). Quem aceita dá baixa na perda e avisa os demais (melhor esforço).
    pub async fn accept(
        self: &Arc<Self>,
        records: Vec<MessageRecord>,
        replicate: bool,
    ) -> Result<Response, TabaError> {
        if self.service.node_state() != NodeState::Active {
            return Err(node_syncing());
        }
        let me = self.service.node_id();
        let quorum = self.service.rules().quorum.max(1) as usize;
        // Gravações confirmadas por registro, e os responsáveis de cada um.
        let mut confirmed = vec![0usize; records.len()];
        let mut needed = vec![quorum; records.len()];
        let mut remote: BTreeMap<String, (Member, Vec<usize>)> = BTreeMap::new();
        let mut applied = 0usize;
        let mut skipped = 0usize;
        let mut local_ok = vec![false; records.len()];
        for (index, record) in records.iter().enumerate() {
            let position =
                self.messages
                    .position_for(&record.topic, &record.id, record.group_id.as_deref());
            let owners = self.service.owners_of(position)?;
            needed[index] = quorum.min(owners.len()).max(1);
            for owner in owners {
                if owner.node_id == me {
                    match self.apply_local(record).await {
                        Ok(Loaded::Stored) => {
                            applied += 1;
                            confirmed[index] += 1;
                            local_ok[index] = true;
                        }
                        // Já existia, venceu por `ttl` ou o tópico deixou de existir: não há mais o
                        // que gravar e a linha está resolvida.
                        Ok(_) => {
                            skipped += 1;
                            confirmed[index] += 1;
                            local_ok[index] = true;
                        }
                        Err(e) => {
                            tracing::warn!(topic = %record.topic, id = %record.id, error = %e.message, "não foi possível gravar a mensagem recuperada");
                        }
                    }
                } else if replicate {
                    remote
                        .entry(owner.node_id.clone())
                        .or_insert_with(|| (owner.clone(), Vec::new()))
                        .1
                        .push(index);
                }
            }
        }
        self.count(&RECOVER_MESSAGES, ("taba.outcome", "applied"), applied);
        self.count(&RECOVER_MESSAGES, ("taba.outcome", "skipped"), skipped);
        self.count(
            &CLUSTER_RECOVER,
            ("taba.direction", "received"),
            applied + skipped,
        );

        if !replicate {
            let count = local_ok.iter().filter(|ok| **ok).count();
            return Ok(Response::Recovered(RecoverResult::new(count as u64)));
        }
        let mut set = JoinSet::new();
        for (member, indexes) in remote.into_values() {
            let service = self.service.clone();
            let batch: Vec<MessageRecord> = indexes.iter().map(|i| records[*i].clone()).collect();
            set.spawn(async move {
                let stored = Self::send_to_owner(&service, &member, batch).await;
                (indexes, stored)
            });
        }
        while let Some(joined) = set.join_next().await {
            if let Ok((indexes, stored)) = joined {
                for (index, ok) in indexes.into_iter().zip(stored) {
                    if ok {
                        confirmed[index] += 1;
                    }
                }
            }
        }
        let mut count = 0u64;
        for (index, record) in records.iter().enumerate() {
            if confirmed[index] >= needed[index] {
                count += 1;
                self.settle(&record.topic, &record.id);
            }
        }
        Ok(Response::Recovered(RecoverResult::new(count)))
    }

    /// Manda o lote a um responsável (`replicate=false`) e diz, por registro, se ele gravou. Se o
    /// responsável aplicou só parte, cada registro é pedido de novo sozinho para saber quais.
    async fn send_to_owner(
        service: &Arc<ClusterService>,
        owner: &Member,
        batch: Vec<MessageRecord>,
    ) -> Vec<bool> {
        let call = |records: Vec<MessageRecord>| {
            let service = service.clone();
            let owner = owner.clone();
            async move {
                let request = Request::Recover {
                    records,
                    replicate: false,
                };
                // Chama pelo `Transport` (nunca o próprio nó, que não é responsável aqui): evita
                // que `accept` apareça dentro do próprio tipo de futuro.
                let rules = service.rules();
                for attempt in 1..=rules.retry_attempts.max(1) {
                    match service
                        .call_peer(&owner, request.clone(), rules.call_timeout)
                        .await
                    {
                        Ok(Response::Recovered(result)) => return Some(result.count as usize),
                        Err(CallError::Remote(e)) if e.status() < 500 => return None,
                        _ if attempt < rules.retry_attempts => {
                            tokio::time::sleep(rules.retry_backoff).await;
                        }
                        _ => {}
                    }
                }
                None
            }
        };
        match call(batch.clone()).await {
            Some(count) if count == batch.len() => vec![true; batch.len()],
            Some(0) | None if batch.len() == 1 => vec![false],
            _ => {
                let mut out = Vec::with_capacity(batch.len());
                for record in batch {
                    out.push(call(vec![record]).await == Some(1));
                }
                out
            }
        }
    }

    /// Dá baixa na perda aqui e avisa os outros membros, sem esperar.
    fn settle(self: &Arc<Self>, topic: &str, id: &str) {
        if let Err(e) = self.losses.remove(topic, id) {
            tracing::warn!(error = %e, "falha ao dar baixa na perda");
        }
        tracing::info!(
            event.name = "taba.message.recovered",
            messaging.destination.name = %topic,
            message.id = %id,
            "mensagem perdida recuperada"
        );
        let Some(table) = self.service.membership().table() else {
            return;
        };
        let me = self.service.node_id();
        for member in table.members.into_iter().filter(|m| m.node_id != me) {
            let service = self.service.clone();
            let request = Request::LossRemove {
                topic: topic.to_owned(),
                id: id.to_owned(),
            };
            tokio::spawn(async move {
                let _ = service.call_member(&member, request).await;
            });
        }
    }

    // ---- Quem volta ----

    /// O procedimento do doc 04, seção 6.6, passos 1 a 5: devolve o resumo quando a lista de
    /// perdas foi tratada ou o prazo `cluster.recovery.timeout` acabou. A limpeza do estado local e
    /// o novo ingresso (passo 6) são de quem chama.
    pub async fn run(self: &Arc<Self>, peers: Vec<Member>) -> Summary {
        let rules = self.service.rules().clone();
        let deadline = Instant::now() + rules.recovery_timeout;
        let mut summary = Summary::default();
        if peers.is_empty() {
            summary.complete = true;
            return summary;
        }

        // Passo 1: a lista de perdas de todos os nós que respondem. Quem não respondeu é
        // consultado de novo a cada `cluster.bootstrap.retry.interval`, até o prazo.
        let mut pending = peers;
        let mut lost: BTreeSet<(String, String)> = BTreeSet::new();
        let mut newest: Option<MemberTable> = None;
        loop {
            let mut failed = Vec::new();
            for peer in pending {
                match self.fetch_losses(&peer).await {
                    Some(found) => lost.extend(found),
                    None => {
                        failed.push(peer);
                        continue;
                    }
                }
                if let Some(table) = self.fetch_table(&peer).await
                    && newest.as_ref().is_none_or(|n| table.version > n.version)
                {
                    newest = Some(table);
                }
            }
            pending = failed;
            if pending.is_empty() {
                summary.complete = true;
                break;
            }
            let now = Instant::now();
            if now >= deadline || self.service.cancel_token().is_cancelled() {
                tracing::warn!(
                    missing = pending.len(),
                    "a recuperação seguiu sem a lista de perdas de todos os nós"
                );
                break;
            }
            let wait = rules.bootstrap_retry_interval.min(deadline - now);
            tokio::select! {
                () = self.service.cancel_token().cancelled() => break,
                () = tokio::time::sleep(wait) => {}
            }
        }
        summary.losses = lost.len();
        if lost.is_empty() {
            return summary;
        }

        // Passo 2: o que o nó ainda tem. Passo 3: o envio ao responsável pelo bloco.
        let table = newest.or_else(|| self.service.membership().table());
        let Some(table) = table else {
            return summary;
        };
        // O destino nunca é o próprio nó: a tabela de um par pode ainda listá-lo (o commit da
        // saída não chegou a todos), mas ele não atende mais ninguém. No reinício do processo, a
        // tabela ainda tem o ID antigo, no mesmo endereço (D111).
        let me = self.service.node_id();
        let local = self.service.local();
        let is_me = |m: &&Member| {
            m.node_id == me
                || (m.port == local.port && m.addresses.iter().any(|a| local.addresses.contains(a)))
        };
        let ring = Ring::new(
            table
                .members
                .iter()
                .filter(|m| !is_me(m))
                .map(|m| m.node_id.as_str()),
            rules.virtual_nodes,
        );
        let mut by_owner: BTreeMap<String, Vec<MessageRecord>> = BTreeMap::new();
        for (topic, id) in lost {
            let messages = self.messages.clone();
            let (t, i) = (topic.clone(), id.clone());
            let held = tokio::task::spawn_blocking(move || messages.held_message(&t, &i)).await;
            let Ok(Ok(Some(found))) = held else {
                continue;
            };
            summary.found += 1;
            let position = self
                .messages
                .position_for(&topic, &id, found.meta.group_id.as_deref());
            let Some(primary) = ring.primary(position) else {
                continue;
            };
            by_owner
                .entry(primary.to_owned())
                .or_default()
                .push(MessageRecord::from_stored(&topic, found.meta, found.body));
        }
        let sent: usize = by_owner.values().map(Vec::len).sum();
        self.count(&RECOVER_MESSAGES, ("taba.outcome", "sent"), sent);
        self.count(&CLUSTER_RECOVER, ("taba.direction", "sent"), sent);
        for (owner, records) in by_owner {
            let Some(member) = table.member(&owner).cloned() else {
                continue;
            };
            for chunk in records.chunks(self.max_items) {
                summary.recovered += self.send_chunk(&member, chunk, deadline).await;
            }
        }
        tracing::info!(
            losses = summary.losses,
            found = summary.found,
            recovered = summary.recovered,
            complete = summary.complete,
            "recuperação de perdas concluída"
        );
        summary
    }

    /// Uma passada completa pela lista de perdas de um nó, página a página. `None` se o nó não
    /// respondeu.
    async fn fetch_losses(&self, peer: &Member) -> Option<Vec<(String, String)>> {
        let mut found = Vec::new();
        let mut from: Option<String> = None;
        loop {
            let request = Request::MessageLoss(StreamQuery {
                from: from.clone(),
                to: None,
                limit: None,
            });
            let page = match self
                .service
                .call_peer(peer, request, self.service.rules().call_timeout)
                .await
            {
                Ok(Response::Stream(page)) => page,
                _ => return None,
            };
            for item in &page.items {
                if let (Some(id), Some(topic)) = (
                    item.get("id").and_then(|v| v.as_str()),
                    item.get("topic").and_then(|v| v.as_str()),
                ) {
                    found.push((topic.to_owned(), id.to_owned()));
                }
            }
            match (page.end.has_more, page.end.last) {
                (true, Some(last)) if from.as_deref() != Some(last.as_str()) => from = Some(last),
                _ => return Some(found),
            }
        }
    }

    /// A tabela de membros que o nó enxerga (para achar o responsável de cada bloco hoje).
    async fn fetch_table(&self, peer: &Member) -> Option<MemberTable> {
        match self
            .service
            .call_peer(peer, Request::ListNodes, self.service.rules().call_timeout)
            .await
        {
            Ok(Response::Members(table)) => Some(table),
            _ => None,
        }
    }

    /// Envia um lote ao responsável e repete tudo (o reenvio é idempotente) a cada
    /// `cluster.bootstrap.retry.interval` até o prazo. Devolve quantas linhas foram aplicadas.
    async fn send_chunk(
        &self,
        owner: &Member,
        chunk: &[MessageRecord],
        deadline: Instant,
    ) -> usize {
        let interval: Duration = self.service.rules().bootstrap_retry_interval;
        loop {
            let request = Request::Recover {
                records: chunk.to_vec(),
                replicate: true,
            };
            match self
                .service
                .call_peer(owner, request, self.service.rules().call_timeout * 4)
                .await
            {
                Ok(Response::Recovered(result)) if result.count as usize == chunk.len() => {
                    return chunk.len();
                }
                outcome => {
                    let now = Instant::now();
                    if now >= deadline || self.service.cancel_token().is_cancelled() {
                        tracing::warn!(owner = %owner.node_id, ?outcome, "o lote recuperado não foi aceito por inteiro");
                        return match outcome {
                            Ok(Response::Recovered(result)) => result.count as usize,
                            _ => 0,
                        };
                    }
                    tokio::select! {
                        () = self.service.cancel_token().cancelled() => {}
                        () = tokio::time::sleep(interval.min(deadline - now)) => {}
                    }
                }
            }
        }
    }
}
