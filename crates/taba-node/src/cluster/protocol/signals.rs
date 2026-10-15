//! The channel frames between nodes (doc 13, sections 12.8 and 12.9; doc 06c, `GET /cluster/channel`) and the
//! forward of a client call (`* /cluster/forward/{path}`).
//!
//! The channel is a WebSocket connection per pair of nodes (`GET /cluster/channel`), and each text
//! message is a [`ChannelMessage`]: a batch (the heartbeat and the frames still unconfirmed) or the
//! `confirm` of what the other side processed. The outgoing queue, the `seq` and the resend are the channel's.

use serde::{Deserialize, Serialize};

use crate::cluster::members::Change;
use crate::cluster::protocol::DefinitionKind;

/// A channel frame (`type` and the fields of doc 06c).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum Signal {
    /// The topic started (or stopped) having long-polling listeners on the sending node.
    Listeners { topic: String, has_listeners: bool },
    /// The sending node started (or stopped) having ready messages of the topic.
    Ready { topic: String, has_ready: bool },
    /// New-message notice: there are `count` ready; come fetch them with `take`.
    Notify { topic: String, count: u64 },
    /// The node's state: `draining` (the peers stop asking for `take`), `active` (it became ready or
    /// cancelled the drain) or `failed` (the others are free to vote on the exclusion).
    NodeState { state: String },
    /// Node `node_id` was excluded by vote and is in freezing since `frozen_at` (ms since 1970;
    /// doc 04, section 6.7). Whoever did not know it stores it and passes it on; it does not touch the table.
    Frozen { node_id: String, frozen_at: u64 },
    /// A topic or token definition, in the state the sending node applied it (`version`, `deleted`
    /// and the body): whoever is behind applies it by the same rule as `definition-apply`. Queued by the
    /// owner and the heir; the new frame of the same definition replaces the old one in the queue (D127).
    Definition {
        kind: DefinitionKind,
        key: String,
        version: u64,
        deleted: bool,
        body: serde_json::Value,
    },
    /// A member change already effected (join, leave or exclusion), with the version the table
    /// took on and its `operationId` (doc 04, section 6.2). Whoever lost the register or the commit
    /// applies it on receipt, without depending on the pending change; the frame stays in the outgoing queue until
    /// `confirm` and is never replaced or expires (D126).
    Membership {
        operation_id: String,
        version: u64,
        change: Change,
    },
}

/// A signal with who sent it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignalFrame {
    /// The `nodeId` of the sender.
    pub from: String,
    #[serde(flatten)]
    pub signal: Signal,
}

/// How many ready messages a topic has on the node sending the heartbeat.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReadyCount {
    pub topic: String,
    pub count: u64,
}

/// The heartbeat (`keepalive`, doc 06c): without `seq`, it is neither confirmed nor resent, the next one
/// replaces it. It carries the node's state and the member table's version (the epoch), which the cluster
/// conditions compare.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Keepalive {
    /// The date and time of the sending node.
    pub at: String,
    /// The load indicator: client calls being forwarded in progress.
    pub load: u64,
    /// The version (epoch) of the node's member table, or `0` without a table.
    pub version: u64,
    /// `active`, `syncing`, `draining`, `failed`...
    pub state: String,
    /// Only the topics with ready messages, up to `cluster.channel.keepalive.max-topics`.
    pub ready: Vec<ReadyCount>,
}

/// A frame with `seq`: the sender only takes it out of the outgoing queue on receiving the `confirm`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelFrame {
    pub seq: u64,
    #[serde(flatten)]
    pub signal: Signal,
}

/// What a channel call carries: the heartbeat (when it is time) and the pending frames.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChannelBatch {
    /// The `nodeId` of the sender.
    pub from: String,
    /// The `nodeId` of whoever must receive. A new process on the same IP and port (another ID) answers
    /// `421` `cluster.wrong-node` instead of passing through the old node (D100). Empty: it does not check.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub to: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keepalive: Option<Keepalive>,
    #[serde(default)]
    pub frames: Vec<ChannelFrame>,
}

/// What travels through the channel, in JSON text messages: the sender's batch or the receiver's `confirm`.
/// The `confirm` is recognized by `"type":"confirm"`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChannelMessage {
    Batch(ChannelBatch),
    Confirm(Confirm),
}

impl ChannelMessage {
    pub fn to_text(&self) -> String {
        match self {
            ChannelMessage::Batch(batch) => serde_json::to_string(batch),
            ChannelMessage::Confirm(confirm) => serde_json::to_string(confirm),
        }
        .expect("the channel batch always becomes JSON")
    }

    pub fn from_text(text: &str) -> Result<Self, String> {
        let value: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
        if value.get("type").and_then(serde_json::Value::as_str) == Some("confirm") {
            serde_json::from_value(value)
                .map(ChannelMessage::Confirm)
                .map_err(|e| e.to_string())
        } else {
            serde_json::from_value(value)
                .map(ChannelMessage::Batch)
                .map_err(|e| e.to_string())
        }
    }
}

/// The `confirm`: the `seq`s already **processed** (including those that already had been, on a resend).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Confirm {
    #[serde(rename = "type")]
    pub kind: String,
    pub seqs: Vec<u64>,
}

impl Confirm {
    pub fn new(seqs: Vec<u64>) -> Self {
        Self {
            kind: "confirm".to_owned(),
            seqs,
        }
    }
}

/// A client call forwarded to another node: method, path with the query, headers and body
/// as they came.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForwardRequest {
    pub method: String,
    /// The original client API path, with the query (`/topics/orders/messages?waitTime=5`).
    pub path_and_query: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    /// The client IP already resolved by whoever forwards (the destination does not count the rate limit again).
    pub client_ip: Option<String>,
}

/// The response of a forwarded client call, as the destination gave it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForwardResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}
