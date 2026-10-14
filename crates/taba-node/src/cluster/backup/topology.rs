//! O que o backup precisa saber do cluster, atrás de um trait: quem é dono de quê e quem está no ar.
//! Um nó sem cluster (`mode=development`) é dono de tudo ([`Solo`]).

use std::sync::Arc;

use crate::cluster::members::MemberState;
use crate::cluster::protocol::{BACKUP_SAVED_MAX_ITEMS, BackupSaved, BackupSavedItem, Request};
use crate::cluster::service::ClusterService;
use crate::cluster::state::{NodeShared, NodeState};
use crate::core::ring::definition_position;
use crate::storage::backup::SavedRef;

pub trait Topology: Send + Sync + 'static {
    fn node_id(&self) -> String;

    /// Os IPs com que o nó se apresenta aos outros.
    fn addresses(&self) -> Vec<String>;

    /// O nó está ativo (já sincronizou): só então ele é dono de algo.
    fn is_active(&self) -> bool;

    /// A porta do cluster do nó.
    fn port(&self) -> u16;

    /// O nó é o primário da posição no anel (o dono de uma mensagem).
    fn owns_position(&self, position: u64) -> bool;

    /// Os nós responsáveis pela posição, o primário primeiro.
    fn owners(&self, position: u64) -> Vec<String>;

    /// O nó é o **gestor** da definição (o dono dela, como em `09-topicos.md`): `kind` é `topic` ou
    /// `token`.
    fn manages_definition(&self, kind: &str, key: &str) -> bool;

    /// Os `id` dos membros da tabela, o próprio incluído.
    fn members(&self) -> Vec<String>;

    /// O nó fala com `node_id` agora (o canal está aberto e ouviu o par há pouco)?
    fn reaches(&self, node_id: &str) -> bool;

    /// Avisa `node_id`, uma réplica, de quais mensagens do tópico já estão no bucket (sem esperar
    /// resposta: o aviso é melhor esforço, a réplica só ganha a evicção com ele).
    fn send_saved(&self, _node_id: &str, _topic: &str, _items: Vec<(String, SavedRef)>) {}
}

/// Um cluster de um nó só.
pub struct Solo {
    shared: Arc<NodeShared>,
}

impl Solo {
    pub fn new(shared: Arc<NodeShared>) -> Self {
        Self { shared }
    }
}

impl Topology for Solo {
    fn node_id(&self) -> String {
        self.shared.node_id()
    }

    fn addresses(&self) -> Vec<String> {
        Vec::new()
    }

    fn port(&self) -> u16 {
        0
    }

    fn is_active(&self) -> bool {
        true
    }

    fn owns_position(&self, _position: u64) -> bool {
        true
    }

    fn owners(&self, _position: u64) -> Vec<String> {
        vec![self.shared.node_id()]
    }

    fn manages_definition(&self, _kind: &str, _key: &str) -> bool {
        true
    }

    fn members(&self) -> Vec<String> {
        vec![self.shared.node_id()]
    }

    fn reaches(&self, _node_id: &str) -> bool {
        false
    }
}

/// O cluster de verdade, pelo `ClusterService`.
pub struct ClusterTopology(pub Arc<ClusterService>);

impl Topology for ClusterTopology {
    fn node_id(&self) -> String {
        self.0.node_id()
    }

    fn addresses(&self) -> Vec<String> {
        self.0.local().addresses
    }

    fn port(&self) -> u16 {
        self.0.local().port
    }

    fn is_active(&self) -> bool {
        self.0.node_state() == NodeState::Active
    }

    fn owns_position(&self, position: u64) -> bool {
        self.is_active()
            && self
                .0
                .membership()
                .ring()
                .primary(position)
                .is_some_and(|primary| primary == self.0.node_id())
    }

    fn owners(&self, position: u64) -> Vec<String> {
        let factor = self.0.rules().replication_factor.max(1) as usize;
        self.0
            .membership()
            .ring()
            .owners(position, factor)
            .into_iter()
            .map(str::to_owned)
            .collect()
    }

    fn manages_definition(&self, kind: &str, key: &str) -> bool {
        if !self.is_active() {
            return false;
        }
        // O dono é o primeiro nó **ativo** da posição: um nó que ainda sincroniza não tem a cópia
        // completa (o mesmo critério de `Definitions::owners`).
        let Ok(owners) = self.0.owners_of(definition_position(kind, key)) else {
            return false;
        };
        owners
            .iter()
            .find(|m| m.state == MemberState::Active)
            .is_some_and(|m| m.node_id == self.0.node_id())
    }

    fn members(&self) -> Vec<String> {
        self.0
            .membership()
            .table()
            .map(|t| t.members.into_iter().map(|m| m.node_id).collect())
            .unwrap_or_default()
    }

    fn reaches(&self, node_id: &str) -> bool {
        self.0
            .replication()
            .is_some_and(|r| r.channels.is_live(node_id))
    }

    fn send_saved(&self, node_id: &str, topic: &str, items: Vec<(String, SavedRef)>) {
        let Some(member) = self
            .0
            .membership()
            .table()
            .and_then(|t| t.member(node_id).cloned())
        else {
            return;
        };
        for chunk in items.chunks(BACKUP_SAVED_MAX_ITEMS) {
            let request = Request::BackupSaved(BackupSaved {
                topic: topic.to_owned(),
                items: chunk
                    .iter()
                    .map(|(id, saved)| BackupSavedItem {
                        id: id.clone(),
                        segment: saved.segment.clone(),
                        offset: saved.offset,
                        len: saved.len,
                        saved_at_ms: saved.saved_at_ms,
                    })
                    .collect(),
            });
            let (service, member) = (self.0.clone(), member.clone());
            tokio::spawn(async move {
                // Melhor esforço: sem o aviso a réplica só deixa de poder fazer a evicção.
                let _ = service.call_member(&member, request).await;
            });
        }
    }
}
