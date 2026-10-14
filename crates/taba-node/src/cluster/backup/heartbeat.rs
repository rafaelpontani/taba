//! O batimento dos nós no bucket e a detecção de morte (doc 23, "Backup em S3", seção 1.6).
//!
//! Cada nó grava `nodes/<id>.json` com o **id**, os **ips de acesso**, o estado e um **número de
//! sequência** que cresce a cada batimento. O `seq` é obrigatório: sem ele o conteúdo se repete, o
//! ETag não muda, e quem observa não percebe o batimento.
//!
//! **Sem relógio sincronizado:** quem observa não compara horários. Guarda o ETag de cada arquivo e
//! marca, no próprio relógio monotônico, quando o ETag mudou pela última vez. Se não mudou por
//! `backup.dead-after`, o nó está morto **para quem observa**. Um arquivo visto pela primeira vez
//! conta como fresco até completar `backup.dead-after` sem mudar (sem histórico não há como saber).
//!
//! **Vivo não é o mesmo que alcançável.** O batimento mostra que o nó está vivo, seja qual for a
//! rede entre os nós; a rede só responde "consigo falar com ele agora?":
//!
//! | Rede até Y | Batimento de Y | Conclusão | Ação de X |
//! |---|---|---|---|
//! | alcançável | fresco | vivo | normal |
//! | inalcançável | fresco | vivo em outra partição | **não assume** os blocos de Y |
//! | inalcançável | vencido | morto | pode assumir os blocos |

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::{Backup, lock};
use crate::storage::backup::keys;

/// O conteúdo de `nodes/<id>.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Heartbeat {
    pub id: String,
    pub addresses: Vec<String>,
    /// A porta do cluster: com ela e os `addresses` outro nó entra pelo caminho normal.
    #[serde(default)]
    pub port: u16,
    /// Cresce a cada batimento.
    pub seq: u64,
    /// `member` (no cluster), `joining` (ainda entrando) ou `draining`.
    pub state: String,
}

/// O que quem observa conclui de um nó.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Liveness {
    /// O batimento mudou há menos que `backup.dead-after` (ou o arquivo é novo demais para saber).
    Fresh,
    /// O arquivo não mudou por `backup.dead-after`, ou o nó saiu limpo e removeu o arquivo.
    Dead,
    /// O S3 não pôde ser lido: nada a dizer. Quem decide cai no comportamento sem o backup.
    Unknown,
}

struct Seen {
    etag: String,
    changed_at: Instant,
    /// O ETag já mudou desde que o vimos: o nó bate de verdade.
    proven: bool,
    body: Option<Heartbeat>,
}

#[derive(Default)]
struct BeatState {
    seen: HashMap<String, Seen>,
    seq: u64,
    /// A última listagem de `nodes/` funcionou.
    listed_ok: bool,
}

#[derive(Default)]
pub(crate) struct Beats {
    state: Mutex<BeatState>,
}

/// O que `wait_for_verdict` conclui da observação do bucket na partida.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Há ao menos um nó batendo: o cluster está vivo, o nó entra pelo caminho normal.
    Alive(Vec<Heartbeat>),
    /// Existem arquivos de nós e nenhum bate mais.
    AllDead,
    /// Nenhum arquivo de nó no bucket.
    Empty,
    /// O S3 não respondeu.
    Unavailable,
}

impl Backup {
    fn me(&self, seq: u64) -> Heartbeat {
        use crate::cluster::state::DrainState;
        let state = match (self.topology.is_active(), self.shared.drain_state()) {
            (true, DrainState::Active) => "member",
            (_, DrainState::Draining | DrainState::Drained) => "draining",
            _ => "joining",
        };
        Heartbeat {
            id: self.node_id(),
            addresses: self.topology.addresses(),
            port: self.topology.port(),
            seq,
            state: state.to_owned(),
        }
    }

    /// Um batimento: grava o próprio arquivo e observa os dos outros.
    pub(crate) async fn heartbeat_pass(&self) {
        if self.beating.load(Ordering::SeqCst) && !self.drained() {
            let seq = {
                let mut state = lock_state(&self.beats.state);
                state.seq += 1;
                state.seq
            };
            let beat = self.me(seq);
            let body = serde_json::to_vec(&beat).expect("o batimento serializa");
            let put = self.store.put(&keys::node(&beat.id), body).await;
            self.note("PUT do batimento", &put);
        }
        self.observe().await;
    }

    /// Lista `nodes/` e atualiza o que se sabe de cada nó.
    pub(crate) async fn observe(&self) {
        self.observe_bodies(false).await;
    }

    /// Como `observe`; com `all_bodies`, relê também o conteúdo dos arquivos que só mudaram (o
    /// estado `member`/`joining` e os ips), que a partida do nó precisa. No dia a dia só o arquivo
    /// visto pela primeira vez é lido: o ETag basta para saber que o nó bate.
    pub(crate) async fn observe_bodies(&self, all_bodies: bool) {
        let listed = self.store.list(keys::NODES_PREFIX).await;
        self.note("LIST de nodes/", &listed);
        let Ok(objects) = listed else {
            lock_state(&self.beats.state).listed_ok = false;
            return;
        };
        let me = self.node_id();
        let now = Instant::now();
        let mut present = Vec::new();
        let mut to_read = Vec::new();
        {
            let mut state = lock_state(&self.beats.state);
            state.listed_ok = true;
            for object in &objects {
                let Some(id) = keys::node_id_of(&object.key) else {
                    continue;
                };
                present.push(id.to_owned());
                match state.seen.get_mut(id) {
                    None => {
                        state.seen.insert(
                            id.to_owned(),
                            Seen {
                                etag: object.etag.clone(),
                                changed_at: now,
                                proven: false,
                                body: None,
                            },
                        );
                        to_read.push((id.to_owned(), object.key.clone()));
                    }
                    Some(seen) if seen.etag != object.etag => {
                        seen.etag.clone_from(&object.etag);
                        seen.changed_at = now;
                        seen.proven = true;
                        if all_bodies || !self.topology.is_active() {
                            to_read.push((id.to_owned(), object.key.clone()));
                        }
                    }
                    Some(_) => {}
                }
            }
            state.seen.retain(|id, _| present.contains(id));
        }
        // Lê o conteúdo só do que mudou (ips e estado); o próprio arquivo não precisa.
        for (id, key) in to_read {
            if id == me {
                continue;
            }
            if let Ok(Some(object)) = self.store.get(&key).await
                && let Ok(beat) = serde_json::from_slice::<Heartbeat>(&object.body)
                && let Some(seen) = lock_state(&self.beats.state).seen.get_mut(&id)
            {
                seen.body = Some(beat);
            }
        }
    }

    /// O que se conclui do batimento do nó `id` (nunca de si mesmo).
    pub fn liveness(&self, id: &str) -> Liveness {
        let state = lock_state(&self.beats.state);
        if !state.listed_ok {
            return Liveness::Unknown;
        }
        match state.seen.get(id) {
            // Sem arquivo: nunca bateu ou saiu limpo.
            None => Liveness::Dead,
            Some(seen) if seen.changed_at.elapsed() >= self.rules.dead_after => Liveness::Dead,
            Some(_) => Liveness::Fresh,
        }
    }

    /// Os outros nós que o bucket mostra, com o que se sabe deles.
    pub fn observed(&self) -> Vec<(String, Option<Heartbeat>, Liveness)> {
        let ids: Vec<String> = lock_state(&self.beats.state).seen.keys().cloned().collect();
        let me = self.node_id();
        ids.into_iter()
            .filter(|id| *id != me)
            .map(|id| {
                let beat = lock_state(&self.beats.state)
                    .seen
                    .get(&id)
                    .and_then(|s| s.body.clone());
                let liveness = self.liveness(&id);
                (id, beat, liveness)
            })
            .collect()
    }

    /// Observa o bucket por `backup.dead-after` e diz se há um cluster batendo (doc 23, 1.7,
    /// exemplo C). Só conta quem está no estado `member`: um nó que também está subindo não prova
    /// que o cluster existe.
    pub async fn wait_for_verdict(&self) -> Verdict {
        loop {
            self.observe_bodies(true).await;
            let me = self.node_id();
            let (listed_ok, members): (bool, Vec<(Heartbeat, bool, Duration)>) = {
                let state = lock_state(&self.beats.state);
                (
                    state.listed_ok,
                    state
                        .seen
                        .iter()
                        .filter(|(id, _)| **id != me)
                        .filter_map(|(_, s)| {
                            s.body
                                .clone()
                                .map(|b| (b, s.proven, s.changed_at.elapsed()))
                        })
                        .filter(|(b, ..)| b.state == "member" || b.state == "draining")
                        .collect(),
                )
            };
            if !listed_ok {
                return Verdict::Unavailable;
            }
            let alive: Vec<Heartbeat> = members
                .iter()
                .filter(|(_, proven, since)| *proven && *since < self.rules.dead_after)
                .map(|(b, ..)| b.clone())
                .collect();
            if !alive.is_empty() {
                return Verdict::Alive(alive);
            }
            if members.is_empty() {
                // Nenhum arquivo, ou só de nós que também estão subindo (não provam nada).
                return Verdict::Empty;
            }
            if members
                .iter()
                .all(|(_, _, since)| *since >= self.rules.dead_after)
            {
                return Verdict::AllDead;
            }
            if self.cancel.is_cancelled() {
                return Verdict::Unavailable;
            }
            // Dá ao observador tempo de ver o batimento mudar (ou não) antes de concluir.
            let wait = (self.rules.heartbeat_interval / 2).max(Duration::from_millis(50));
            tokio::time::sleep(wait).await;
        }
    }

    /// O nó saiu limpo (drain concluído, parada): remove o próprio arquivo e para de bater.
    pub async fn leave(&self) {
        self.beating.store(false, Ordering::SeqCst);
        let deleted = self.store.delete(&keys::node(&self.node_id())).await;
        self.note("DELETE do batimento", &deleted);
    }

    /// O nó voltou do drain como nó novo: o batimento recomeça.
    pub fn resume_beating(&self) {
        self.beating.store(true, Ordering::SeqCst);
        self.wake.notify_one();
    }
}

fn lock_state(mutex: &Mutex<BeatState>) -> std::sync::MutexGuard<'_, BeatState> {
    lock(mutex)
}
