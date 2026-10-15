//! The list of live nodes at the cluster's planned stop (doc 23, "S3 backup", section 1.11).
//!
//! `POST /cluster/stop` does the drain on **all** nodes, through the same upload step as the shutdown by
//! signal. The last to finish (whoever coordinated the call, which waits for the others) writes
//! `cluster/stop.json` with the nodes that were alive and the order they stopped in. It is the basis of the
//! restore: whoever starts afterward knows the cluster was stopped on purpose (it did not die) and does not
//! need to wait for the death deadline.

use serde::{Deserialize, Serialize};

use super::Backup;
use crate::storage::backup::keys;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StopList {
    #[serde(rename = "atMs")]
    pub at_ms: u64,
    /// Whoever coordinated the stop and wrote the list.
    pub coordinator: String,
    /// The nodes that finished the drain, in the order they stopped.
    pub nodes: Vec<String>,
}

impl Backup {
    /// Writes the stop list. `false` if S3 did not answer (the stop goes on, without the list).
    pub async fn write_stop_list(&self, nodes: Vec<String>) -> bool {
        let list = StopList {
            at_ms: self.clock.now_millis(),
            coordinator: self.node_id(),
            nodes,
        };
        let body = serde_json::to_vec(&list).expect("the list serializes");
        let put = self.store.put(keys::STOP_LIST, body).await;
        self.note("PUT of the stop list", &put);
        put.is_ok()
    }

    /// The list of the last planned stop, if the bucket has it.
    pub async fn read_stop_list(&self) -> Option<StopList> {
        let object = self.store.get(keys::STOP_LIST).await.ok().flatten()?;
        serde_json::from_slice(&object.body).ok()
    }
}
