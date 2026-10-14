//! A lista dos nós vivos na parada programada do cluster (doc 23, "Backup em S3", seção 1.11).
//!
//! `POST /cluster/stop` faz o drain em **todos** os nós, pela mesma etapa de envio do encerramento por
//! sinal. O último a concluir (quem coordenou a chamada, que espera os outros) grava
//! `cluster/stop.json` com os nós que estavam vivos e a ordem em que pararam. É a base da
//! restauração: quem sobe depois sabe que o cluster foi parado de propósito (não morreu) e não
//! precisa esperar o prazo de morte.

use serde::{Deserialize, Serialize};

use super::Backup;
use crate::storage::backup::keys;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StopList {
    #[serde(rename = "atMs")]
    pub at_ms: u64,
    /// Quem coordenou a parada e gravou a lista.
    pub coordinator: String,
    /// Os nós que concluíram o drain, na ordem em que pararam.
    pub nodes: Vec<String>,
}

impl Backup {
    /// Grava a lista da parada. `false` se o S3 não respondeu (a parada segue, sem a lista).
    pub async fn write_stop_list(&self, nodes: Vec<String>) -> bool {
        let list = StopList {
            at_ms: self.clock.now_millis(),
            coordinator: self.node_id(),
            nodes,
        };
        let body = serde_json::to_vec(&list).expect("a lista serializa");
        let put = self.store.put(keys::STOP_LIST, body).await;
        self.note("PUT da lista de parada", &put);
        put.is_ok()
    }

    /// A lista da última parada programada, se o bucket a tem.
    pub async fn read_stop_list(&self) -> Option<StopList> {
        let object = self.store.get(keys::STOP_LIST).await.ok().flatten()?;
        serde_json::from_slice(&object.body).ok()
    }
}
