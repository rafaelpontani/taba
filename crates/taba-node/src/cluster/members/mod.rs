//! A tabela de membros e o registro em duas fases de uma entrada ou saída de nó (doc 04,
//! seções 6.2 e 6.3; doc 06c, "Membros e canal").
//!
//! Uma mudança de membros é decidida só com a confirmação de **todos** os nós (sem quórum
//! reduzido): a fase 1 (`register`) deixa a mudança pendente, e a fase 2 (`commit`) a efetiva e
//! sobe a `version` da tabela. O que é pendente não muda a tabela, e há **uma mudança por vez**
//! (seção 6.8): um segundo `register` com outro `operationId` é um conflito. Os três passos são
//! idempotentes pelo `operationId`.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::core::ring::Ring;

/// Quantas operações já decididas o nó lembra, para repetir a resposta de um `commit` ou
/// `rollback` repetido em vez de dizer que a operação é desconhecida.
const FINISHED_MEMORY: usize = 256;

/// Milissegundos desde 1970.
fn millis(at: SystemTime) -> u64 {
    at.duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MemberState {
    Active,
    Syncing,
    Draining,
    Suspect,
}

impl MemberState {
    pub const fn as_str(self) -> &'static str {
        match self {
            MemberState::Active => "active",
            MemberState::Syncing => "syncing",
            MemberState::Draining => "draining",
            MemberState::Suspect => "suspect",
        }
    }
}

/// Um nó da tabela (`Member`, doc 06c).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Member {
    pub node_id: String,
    /// Os IPs internos do nó.
    pub addresses: Vec<String>,
    /// A porta do cluster.
    pub port: u16,
    pub state: MemberState,
}

/// A tabela de membros (`MemberTable`, doc 06c). A `version` é a época: sobe a cada mudança
/// efetivada, e versões diferentes entre nós indicam propagação incompleta.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MemberTable {
    pub version: u64,
    pub members: Vec<Member>,
    /// Os nós em freezing que quem responde conhece (D106). Só viaja nas respostas que levam a
    /// tabela a um nó novo; a tabela guardada nunca o tem (o freezing vive à parte, sem subir a
    /// `version`), e ele não conta na igualdade das tabelas.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub frozen: Vec<FrozenNode>,
}

impl PartialEq for MemberTable {
    fn eq(&self, other: &Self) -> bool {
        self.version == other.version && self.members == other.members
    }
}

impl Eq for MemberTable {}

/// Um nó excluído por votação e o instante da exclusão (doc 04, seção 6.7).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FrozenNode {
    pub node_id: String,
    /// Milissegundos desde 1970.
    pub frozen_at: u64,
}

impl MemberTable {
    pub fn member(&self, node_id: &str) -> Option<&Member> {
        self.members.iter().find(|m| m.node_id == node_id)
    }

    fn sort(&mut self) {
        self.members.sort_by(|a, b| a.node_id.cmp(&b.node_id));
    }
}

/// O que uma operação pendente faz com a tabela.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    Add(Member),
    /// A saída voluntária (drain): sem freezing.
    Remove {
        node_id: String,
    },
    /// A exclusão por votação: o nó entra em freezing (doc 04, seção 6.7).
    Exclude {
        node_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MembershipError {
    /// O nó ainda não tem a tabela (está entrando).
    NoTable,
    /// Há outra mudança pendente, ou o estado atual impede esta (já é membro, não é membro).
    Conflict(String),
    /// O `commit` ou `rollback` de uma operação que o nó não conhece (nunca chegou ou expirou).
    UnknownOperation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Finished {
    Committed,
    RolledBack,
}

#[derive(Debug)]
struct Pending {
    operation_id: String,
    change: Change,
    registered_at: SystemTime,
    /// Este nó é quem coordena a mudança (e não só um participante dela).
    coordinated: bool,
}

#[derive(Debug, Default)]
struct Inner {
    table: Option<MemberTable>,
    ring: Arc<Ring>,
    pending: Option<Pending>,
    finished: VecDeque<(String, Finished)>,
    /// Os nós que saíram por drain: ainda podem repassar chamadas de cliente a quem está no
    /// cluster (doc 04, seção 6.9). Só em memória; sai quando um nó novo entra no mesmo endereço.
    departed: Vec<Member>,
    /// Os nós excluídos por votação e quando, para o período de freezing (doc 04, seção 6.7).
    frozen: HashMap<String, SystemTime>,
    /// Os nós excluídos por votação, com os endereços: depois de uma partição em que os dois lados
    /// se excluem, é por eles que um lado procura o outro (D101). Só em memória, até 64.
    excluded: Vec<Member>,
}

/// A tabela de membros de um nó, com a mudança pendente e o anel derivado dela.
#[derive(Debug)]
pub struct Membership {
    inner: Mutex<Inner>,
    /// `write.pending.ttl`: depois dele um registro sem commit nem rollback some sozinho.
    pending_ttl: Duration,
    virtual_nodes: u32,
}

impl Membership {
    pub fn new(pending_ttl: Duration, virtual_nodes: u32) -> Self {
        Self {
            inner: Mutex::new(Inner::default()),
            pending_ttl,
            virtual_nodes,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn rebuild_ring(&self, inner: &mut Inner) {
        inner.ring = Arc::new(match &inner.table {
            Some(table) => Ring::new(
                table.members.iter().map(|m| m.node_id.as_str()),
                self.virtual_nodes,
            ),
            None => Ring::default(),
        });
    }

    /// Instala a tabela: a do primeiro nó (ele sozinho) ou a que o `join` devolveu.
    pub fn install(&self, mut table: MemberTable) {
        table.sort();
        let mut inner = self.lock();
        // O freezing que veio com a tabela é herdado; o que o nó já conhece vale mais.
        for node in std::mem::take(&mut table.frozen) {
            inner
                .frozen
                .entry(node.node_id)
                .or_insert(UNIX_EPOCH + Duration::from_millis(node.frozen_at));
        }
        inner.table = Some(table);
        inner.pending = None;
        self.rebuild_ring(&mut inner);
    }

    /// Esquece a tabela (o nó volta a ser novo).
    pub fn clear(&self) {
        let mut inner = self.lock();
        *inner = Inner::default();
    }

    pub fn table(&self) -> Option<MemberTable> {
        self.lock().table.clone()
    }

    /// A tabela com os nós em freezing que ainda valem, para quem está entrando (D106).
    pub fn table_with_frozen(&self, now: SystemTime, duration: Duration) -> Option<MemberTable> {
        let mut inner = self.lock();
        inner
            .frozen
            .retain(|_, at| now.duration_since(*at).unwrap_or_default() < duration);
        let mut table = inner.table.clone()?;
        table.frozen = inner
            .frozen
            .iter()
            .map(|(node_id, at)| FrozenNode {
                node_id: node_id.clone(),
                frozen_at: at
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64,
            })
            .collect();
        table.frozen.sort_by(|a, b| a.node_id.cmp(&b.node_id));
        Some(table)
    }

    pub fn version(&self) -> Option<u64> {
        self.lock().table.as_ref().map(|t| t.version)
    }

    /// O anel dos membros atuais (vazio enquanto o nó não tem a tabela).
    pub fn ring(&self) -> Arc<Ring> {
        self.lock().ring.clone()
    }

    pub fn is_member(&self, node_id: &str) -> bool {
        self.lock()
            .table
            .as_ref()
            .is_some_and(|t| t.member(node_id).is_some())
    }

    /// O membro está na tabela e `active`? (Um nó em sincronização ainda não atende nada.)
    pub fn is_active(&self, node_id: &str) -> bool {
        self.lock().table.as_ref().is_some_and(|t| {
            t.member(node_id)
                .is_some_and(|m| m.state == MemberState::Active)
        })
    }

    /// Algum membro está em sincronização? Enquanto houver, nenhuma outra entrada, saída ou
    /// rebalanceamento é efetivada (doc 04, seção 6.8).
    pub fn syncing(&self) -> Vec<String> {
        self.lock()
            .table
            .as_ref()
            .map(|t| {
                t.members
                    .iter()
                    .filter(|m| m.state == MemberState::Syncing)
                    .map(|m| m.node_id.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Muda o estado de um membro na tabela, sem subir a `version` (não é uma mudança de membros:
    /// é o nó dizendo em que pé está). `true` se mudou.
    pub fn set_state(&self, node_id: &str, state: MemberState) -> bool {
        let mut inner = self.lock();
        let Some(member) = inner
            .table
            .as_mut()
            .and_then(|t| t.members.iter_mut().find(|m| m.node_id == node_id))
        else {
            return false;
        };
        let changed = member.state != state;
        member.state = state;
        changed
    }

    /// Algum membro tem este endereço? É como a porta do cluster reconhece quem chama.
    pub fn is_member_address(&self, address: &str) -> bool {
        self.lock().table.as_ref().is_some_and(|t| {
            t.members
                .iter()
                .any(|m| m.addresses.iter().any(|a| a == address))
        })
    }

    /// O endereço é de um nó que saiu por drain e pode ainda repassar chamadas?
    pub fn is_departed_address(&self, address: &str) -> bool {
        self.lock()
            .departed
            .iter()
            .any(|m| m.addresses.iter().any(|a| a == address))
    }

    /// Os nós que este nó viu serem excluídos por votação e que não voltaram à tabela (D101).
    pub fn excluded_members(&self) -> Vec<Member> {
        let inner = self.lock();
        inner
            .excluded
            .iter()
            .filter(|m| {
                inner
                    .table
                    .as_ref()
                    .is_none_or(|t| t.member(&m.node_id).is_none())
            })
            .cloned()
            .collect()
    }

    /// Quanto falta do freezing do nó (excluído por votação), ou `None` se ele não está em freezing.
    pub fn frozen_remaining(
        &self,
        node_id: &str,
        now: SystemTime,
        duration: Duration,
    ) -> Option<Duration> {
        let inner = self.lock();
        let at = inner.frozen.get(node_id)?;
        let elapsed = now.duration_since(*at).unwrap_or_default();
        duration.checked_sub(elapsed).filter(|left| !left.is_zero())
    }

    /// Guarda o freezing de um nó que outro nó viu ser excluído (o quadro `frozen` do canal).
    /// Devolve `true` se era novidade: o que o nó já conhece vale mais (o eco não muda o instante),
    /// e um freezing que já acabou não é guardado.
    pub fn learn_frozen(
        &self,
        node_id: &str,
        frozen_at: u64,
        now: SystemTime,
        duration: Duration,
    ) -> bool {
        let at = UNIX_EPOCH + Duration::from_millis(frozen_at);
        if now.duration_since(at).unwrap_or_default() >= duration {
            return false;
        }
        let mut inner = self.lock();
        inner
            .frozen
            .retain(|_, known| now.duration_since(*known).unwrap_or_default() < duration);
        if inner.frozen.contains_key(node_id) {
            return false;
        }
        inner.frozen.insert(node_id.to_owned(), at);
        true
    }

    /// O instante (ms desde 1970) em que o nó entrou em freezing, se ele está guardado.
    pub fn frozen_at(&self, node_id: &str) -> Option<u64> {
        self.lock().frozen.get(node_id).map(|at| millis(*at))
    }

    /// Os nós ainda em freezing, em ordem de ID (os vencidos são esquecidos).
    pub fn frozen_nodes(&self, now: SystemTime, duration: Duration) -> Vec<FrozenNode> {
        let mut inner = self.lock();
        inner
            .frozen
            .retain(|_, at| now.duration_since(*at).unwrap_or_default() < duration);
        let mut nodes: Vec<FrozenNode> = inner
            .frozen
            .iter()
            .map(|(node_id, at)| FrozenNode {
                node_id: node_id.clone(),
                frozen_at: millis(*at),
            })
            .collect();
        nodes.sort_by(|a, b| a.node_id.cmp(&b.node_id));
        nodes
    }
    /// Quantos nós estão em freezing agora (os vencidos são esquecidos).
    pub fn frozen_count(&self, now: SystemTime, duration: Duration) -> usize {
        let mut inner = self.lock();
        inner
            .frozen
            .retain(|_, at| now.duration_since(*at).unwrap_or_default() < duration);
        inner.frozen.len()
    }

    /// Há uma mudança pendente (e ainda não vencida)?
    pub fn has_pending(&self, now: SystemTime) -> bool {
        let mut inner = self.lock();
        self.drop_expired(&mut inner, now);
        inner.pending.is_some()
    }

    fn drop_expired(&self, inner: &mut Inner, now: SystemTime) {
        let expired = inner.pending.as_ref().is_some_and(|p| {
            now.duration_since(p.registered_at)
                .is_ok_and(|age| age >= self.pending_ttl)
        });
        if expired {
            inner.pending = None;
        }
    }

    fn remember(inner: &mut Inner, operation_id: String, outcome: Finished) {
        inner.finished.retain(|(id, _)| *id != operation_id);
        inner.finished.push_back((operation_id, outcome));
        while inner.finished.len() > FINISHED_MEMORY {
            inner.finished.pop_front();
        }
    }

    fn finished(inner: &Inner, operation_id: &str) -> Option<Finished> {
        inner
            .finished
            .iter()
            .find(|(id, _)| id == operation_id)
            .map(|(_, f)| *f)
    }

    /// Fase 1 como participante: deixa a mudança pendente. Repetir o mesmo `operationId` não muda
    /// nada. Se a pendente é uma que **este nó coordena** e a que chega tem o `operationId` menor (a
    /// mais antiga), este nó cede: descarta a sua e aceita a outra, de modo que dois coordenadores
    /// simultâneos sempre têm um vencedor (D94).
    pub fn register(
        &self,
        operation_id: &str,
        change: Change,
        now: SystemTime,
    ) -> Result<(), MembershipError> {
        self.register_as(operation_id, change, now, false)
    }

    /// Fase 1 de quem coordena a mudança.
    pub fn register_coordinating(
        &self,
        operation_id: &str,
        change: Change,
        now: SystemTime,
    ) -> Result<(), MembershipError> {
        self.register_as(operation_id, change, now, true)
    }

    /// A mudança com esse `operationId` ainda está pendente aqui? Quem coordena confere antes do
    /// commit: um participante mais antigo pode tê-la deslocado.
    pub fn is_pending(&self, operation_id: &str) -> bool {
        self.lock()
            .pending
            .as_ref()
            .is_some_and(|p| p.operation_id == operation_id)
    }

    fn register_as(
        &self,
        operation_id: &str,
        change: Change,
        now: SystemTime,
        coordinated: bool,
    ) -> Result<(), MembershipError> {
        let mut inner = self.lock();
        self.drop_expired(&mut inner, now);
        if let Some(pending) = &inner.pending {
            if pending.operation_id == operation_id {
                return Ok(());
            }
            let yields =
                !coordinated && pending.coordinated && operation_id < pending.operation_id.as_str();
            if !yields {
                return Err(MembershipError::Conflict(
                    "Há outra mudança de membros em andamento.".into(),
                ));
            }
            let displaced = inner.pending.take().expect("conferido acima");
            Self::remember(&mut inner, displaced.operation_id, Finished::RolledBack);
        }
        if Self::finished(&inner, operation_id).is_some() {
            return Ok(());
        }
        let Some(table) = &inner.table else {
            return Err(MembershipError::NoTable);
        };
        match &change {
            Change::Add(member) if table.member(&member.node_id).is_some() => {
                return Err(MembershipError::Conflict(format!(
                    "O nó {} já é membro.",
                    member.node_id
                )));
            }
            Change::Remove { node_id } | Change::Exclude { node_id }
                if table.member(node_id).is_none() =>
            {
                return Err(MembershipError::Conflict(format!(
                    "O nó {node_id} não é membro."
                )));
            }
            _ => {}
        }
        inner.pending = Some(Pending {
            operation_id: operation_id.to_owned(),
            change,
            registered_at: now,
            coordinated,
        });
        Ok(())
    }

    /// A mudança pendente com esse `operationId` (para o commit e o rollback saberem o que contam).
    pub fn pending_change(&self, operation_id: &str) -> Option<Change> {
        let inner = self.lock();
        inner
            .pending
            .as_ref()
            .filter(|p| p.operation_id == operation_id)
            .map(|p| p.change.clone())
    }

    /// Fase 2: efetiva a mudança pendente e devolve a tabela nova.
    pub fn commit(
        &self,
        operation_id: &str,
        now: SystemTime,
    ) -> Result<MemberTable, MembershipError> {
        let mut inner = self.lock();
        self.drop_expired(&mut inner, now);
        let matches = inner
            .pending
            .as_ref()
            .is_some_and(|p| p.operation_id == operation_id);
        if !matches {
            return match Self::finished(&inner, operation_id) {
                Some(Finished::Committed) => {
                    inner.table.clone().ok_or(MembershipError::UnknownOperation)
                }
                Some(Finished::RolledBack) => Err(MembershipError::Conflict(
                    "A operação foi cancelada (rollback).".into(),
                )),
                None => Err(MembershipError::UnknownOperation),
            };
        }
        let pending = inner.pending.take().expect("conferido acima");
        let excluded = matches!(pending.change, Change::Exclude { .. });
        let mut frozen = std::mem::take(&mut inner.frozen);
        let mut inner_departed = std::mem::take(&mut inner.departed);
        let mut excluded_members = std::mem::take(&mut inner.excluded);
        let Some(table) = inner.table.as_mut() else {
            inner.excluded = excluded_members;
            inner.departed = inner_departed;
            inner.frozen = frozen;
            return Err(MembershipError::NoTable);
        };
        match pending.change {
            Change::Add(member) => {
                inner_departed.retain(|d| d.addresses != member.addresses || d.port != member.port);
                excluded_members
                    .retain(|d| d.addresses != member.addresses || d.port != member.port);
                table.members.push(member);
            }
            Change::Remove { node_id } | Change::Exclude { node_id } => {
                if excluded {
                    frozen.insert(node_id.clone(), now);
                }
                if let Some(left) = table.members.iter().find(|m| m.node_id == node_id) {
                    if excluded {
                        excluded_members.push(left.clone());
                        if excluded_members.len() > 64 {
                            excluded_members.remove(0);
                        }
                    }
                    inner_departed.push(left.clone());
                    if inner_departed.len() > 64 {
                        inner_departed.remove(0);
                    }
                }
                table.members.retain(|m| m.node_id != node_id);
            }
        }
        table.version += 1;
        table.sort();
        let table = table.clone();
        inner.excluded = excluded_members;
        inner.departed = inner_departed;
        inner.frozen = frozen;
        Self::remember(&mut inner, operation_id.to_owned(), Finished::Committed);
        self.rebuild_ring(&mut inner);
        Ok(table)
    }

    /// Descarta a mudança pendente (só antes do commit).
    pub fn rollback(&self, operation_id: &str, now: SystemTime) -> Result<(), MembershipError> {
        let mut inner = self.lock();
        self.drop_expired(&mut inner, now);
        let matches = inner
            .pending
            .as_ref()
            .is_some_and(|p| p.operation_id == operation_id);
        if matches {
            inner.pending = None;
            Self::remember(&mut inner, operation_id.to_owned(), Finished::RolledBack);
            return Ok(());
        }
        match Self::finished(&inner, operation_id) {
            Some(Finished::RolledBack) => Ok(()),
            Some(Finished::Committed) => Err(MembershipError::Conflict(
                "A operação já foi efetivada: não há rollback depois do commit.".into(),
            )),
            None => Err(MembershipError::UnknownOperation),
        }
    }
}

#[cfg(test)]
mod tests;
