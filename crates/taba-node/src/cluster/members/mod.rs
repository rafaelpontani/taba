//! A tabela de membros e o registro em duas fases de uma entrada ou saída de nó (doc 04,
//! seções 6.2 e 6.3; doc 06c, "Membros e canal").
//!
//! Uma mudança de membros é decidida só com a confirmação de **todos** os nós (sem quórum
//! reduzido): a fase 1 (`register`) deixa a mudança pendente, e a fase 2 (`commit`) a efetiva e
//! sobe a `version` da tabela. O que é pendente não muda a tabela, e há **uma mudança por vez**
//! (seção 6.8): um segundo `register` com outro `operationId` é um conflito. Os três passos são
//! idempotentes pelo `operationId`.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

use crate::core::ring::Ring;

/// Quantas operações já decididas o nó lembra, para repetir a resposta de um `commit` ou
/// `rollback` repetido em vez de dizer que a operação é desconhecida.
const FINISHED_MEMORY: usize = 256;

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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MemberTable {
    pub version: u64,
    pub members: Vec<Member>,
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
    Remove { node_id: String },
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
}

#[derive(Debug, Default)]
struct Inner {
    table: Option<MemberTable>,
    ring: Arc<Ring>,
    pending: Option<Pending>,
    finished: VecDeque<(String, Finished)>,
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

    /// Algum membro tem este endereço? É como a porta do cluster reconhece quem chama.
    pub fn is_member_address(&self, address: &str) -> bool {
        self.lock().table.as_ref().is_some_and(|t| {
            t.members
                .iter()
                .any(|m| m.addresses.iter().any(|a| a == address))
        })
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

    /// Fase 1: deixa a mudança pendente. Repetir o mesmo `operationId` não muda nada.
    pub fn register(
        &self,
        operation_id: &str,
        change: Change,
        now: SystemTime,
    ) -> Result<(), MembershipError> {
        let mut inner = self.lock();
        self.drop_expired(&mut inner, now);
        if let Some(pending) = &inner.pending {
            return if pending.operation_id == operation_id {
                Ok(())
            } else {
                Err(MembershipError::Conflict(
                    "Há outra mudança de membros em andamento.".into(),
                ))
            };
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
            Change::Remove { node_id } if table.member(node_id).is_none() => {
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
        });
        Ok(())
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
        let table = inner.table.as_mut().ok_or(MembershipError::NoTable)?;
        match pending.change {
            Change::Add(member) => table.members.push(member),
            Change::Remove { node_id } => table.members.retain(|m| m.node_id != node_id),
        }
        table.version += 1;
        table.sort();
        let table = table.clone();
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
