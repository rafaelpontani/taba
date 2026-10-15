//! Asking every node and saying what is missing: the common part of the client API's routes that
//! **consolidate** the cluster (doc 34: `/stats/topics`, `/nodes`, `/cluster/status`, `/cluster/backup`).
//!
//! Each node answers what **it** sees, on the cluster API; the node that received the client call asks all
//! the members at once (itself without the network), under the call timeout of the cluster (5 s, fixed),
//! and joins the answers. A node that does not answer in time, or at all, does not fail the read: the
//! response is `200` and says it is incomplete with the **envelope** below.
//!
//! ```json
//! { "partial": true, "missingNodes": [{ "nodeId": "01a1…", "reason": "timeout" }] }
//! ```
//!
//! `reason` is `timeout`, `unreachable` (no answer at all) or `error` (the node answered with an error).
//! Writes do not use this: a write that fails on a node is a `503` naming it.

use std::sync::Arc;

use serde_json::{Map, Value, json};

use super::AppState;
use crate::cluster::members::Member;
use crate::cluster::protocol::{Request, Response};
use crate::cluster::service::ClusterService;
use crate::cluster::transport::CallError;

/// A node that did not answer a read, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Missing {
    pub node_id: String,
    pub reason: &'static str,
}

impl Missing {
    pub fn json(&self) -> Value {
        json!({ "nodeId": self.node_id, "reason": self.reason })
    }
}

/// What a node answered, in the table's order: its `nodeId` and its response, or why there is none.
pub type Answers = Vec<(String, Result<Response, &'static str>)>;

fn reason_of(error: &CallError) -> &'static str {
    match error {
        CallError::Timeout => "timeout",
        CallError::Unreachable(_) => "unreachable",
        CallError::Remote(_) => "error",
    }
}

/// Asks each of `members` for `request`, all at once, and returns the answers in the members' order.
pub async fn ask_members(
    cluster: &Arc<ClusterService>,
    members: &[Member],
    request: &Request,
) -> Answers {
    let mut set = tokio::task::JoinSet::new();
    for (index, member) in members.iter().enumerate() {
        let (cluster, member, request) = (cluster.clone(), member.clone(), request.clone());
        set.spawn(async move {
            let answer = cluster
                .call_member(&member, request)
                .await
                .map_err(|e| reason_of(&e));
            (index, (member.node_id, answer))
        });
    }
    let mut answers = Vec::with_capacity(members.len());
    while let Some(Ok(answer)) = set.join_next().await {
        answers.push(answer);
    }
    answers.sort_by_key(|(index, _)| *index);
    answers.into_iter().map(|(_, answer)| answer).collect()
}

/// Asks every member of the table. `None` when there is no table (a node alone, or one that has not joined
/// yet): the cluster is this node, and the caller reads it locally.
pub async fn ask_all(state: &AppState, request: &Request) -> Option<Answers> {
    let cluster = state.cluster.as_ref()?;
    let table = cluster.membership().table()?;
    Some(ask_members(cluster, &table.members, request).await)
}

/// The nodes that did not answer.
pub fn missing_of(answers: &Answers) -> Vec<Missing> {
    answers
        .iter()
        .filter_map(|(node_id, answer)| {
            answer.as_ref().err().map(|reason| Missing {
                node_id: node_id.clone(),
                reason,
            })
        })
        .collect()
}

/// Puts the envelope in a response object: `partial` and, when there is something missing, `missingNodes`.
pub fn put_envelope(out: &mut Map<String, Value>, missing: &[Missing]) {
    out.insert("partial".into(), json!(!missing.is_empty()));
    out.insert(
        "missingNodes".into(),
        Value::Array(missing.iter().map(Missing::json).collect()),
    );
}
