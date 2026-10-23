//! Version upgrade (doc 23, rolling update): a node that joins and sees nodes of an **earlier** version
//! in the cluster drains them, one at a time, through the drain that already exists. The versions are not made
//! compatible with each other: the new one retires the old ones.
//!
//! The version of each member is the `version` of its `NodeStatus` (`Request::Status`), which every release
//! already publishes; a node that does not publish it counts as an earlier version. Nothing here changes the
//! member table or the join, so a cluster of already published versions understands the new node.
//!
//! [`decide`] is the whole rule, over a snapshot, with no clock or network; [`ClusterService::run_upgrade`]
//! only takes the snapshot, applies the decision and keeps the state that outlives a round.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::Value;

use super::members::{Member, MemberState};
use super::protocol::{ForwardRequest, Request, Response};
use super::service::ClusterService;
use super::state::{DrainState, NodeState};
use super::transport::CallError;
use crate::core::error::ErrorCode;

/// The cluster keeps at least this many active nodes after each drain of the upgrade.
pub const MIN_ACTIVE_AFTER_DRAIN: usize = 3;

/// What a node knows about the version of a member.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PeerVersion {
    /// The member published its version.
    Known(String),
    /// The member answered without a version (or does not have the route): an earlier release.
    Previous,
    /// The member did not answer: the round decides nothing.
    Unknown,
}

/// A member in the snapshot of a round.
#[derive(Debug, Clone)]
pub struct PeerView {
    pub node_id: String,
    pub state: MemberState,
    pub version: PeerVersion,
}

/// A drain this node asked for and is waiting to see finish.
#[derive(Debug, Clone)]
pub struct Pending {
    pub node_id: String,
    pub elapsed: Duration,
}

/// What a round sees.
#[derive(Debug, Clone)]
pub struct View {
    pub me: String,
    pub my_version: String,
    /// This node is `active` and not in drain.
    pub ready: bool,
    /// The other members.
    pub peers: Vec<PeerView>,
    pub pending: Option<Pending>,
    /// How long a drain may take before the upgrade stops on it.
    pub drain_deadline: Duration,
}

/// What the round does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// Nothing to retire.
    Idle,
    /// There is something to retire, but not now.
    Wait(&'static str),
    /// Drain this member.
    Drain(String),
    /// The drain of this member did not finish in time: the upgrade stops there and the condition shows up.
    Stalled(String),
}

#[derive(Debug, PartialEq, Eq)]
struct Parsed {
    core: [u64; 3],
    pre: Option<Vec<Ident>>,
}

#[derive(Debug, PartialEq, Eq)]
enum Ident {
    Number(u64),
    Text(String),
}

impl Ord for Ident {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Ident::Number(a), Ident::Number(b)) => a.cmp(b),
            (Ident::Number(_), Ident::Text(_)) => Ordering::Less,
            (Ident::Text(_), Ident::Number(_)) => Ordering::Greater,
            (Ident::Text(a), Ident::Text(b)) => a.cmp(b),
        }
    }
}

impl PartialOrd for Ident {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

fn parse(version: &str) -> Option<Parsed> {
    let version = version.trim().trim_start_matches('v');
    let version = version.split('+').next()?;
    let (core, pre) = match version.split_once('-') {
        Some((core, pre)) => (core, Some(pre)),
        None => (version, None),
    };
    let mut numbers = core.split('.').map(|part| part.parse::<u64>().ok());
    let core = [numbers.next()??, numbers.next()??, numbers.next()??];
    if numbers.next().is_some() {
        return None;
    }
    let pre = pre.map(|pre| {
        pre.split('.')
            .map(|part| {
                part.parse::<u64>()
                    .map_or_else(|_| Ident::Text(part.to_owned()), Ident::Number)
            })
            .collect()
    });
    Some(Parsed { core, pre })
}

/// Compares two versions (semver precedence: a candidate comes before its final). `None` if one of them is not a version.
pub fn compare(a: &str, b: &str) -> Option<Ordering> {
    let (a, b) = (parse(a)?, parse(b)?);
    Some(a.core.cmp(&b.core).then_with(|| match (&a.pre, &b.pre) {
        (None, None) => Ordering::Equal,
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (Some(a), Some(b)) => a.cmp(b),
    }))
}

impl PeerVersion {
    /// Is this version earlier than `mine`? A version that cannot be read does not count as earlier: it is
    /// never drained by mistake.
    fn older_than(&self, mine: &str) -> bool {
        match self {
            PeerVersion::Known(v) => compare(v, mine) == Some(Ordering::Less),
            PeerVersion::Previous => true,
            PeerVersion::Unknown => false,
        }
    }

    fn newer_than(&self, mine: &str) -> bool {
        matches!(self, PeerVersion::Known(v) if compare(v, mine) == Some(Ordering::Greater))
    }

    fn same_as(&self, mine: &str) -> bool {
        matches!(self, PeerVersion::Known(v) if compare(v, mine) == Some(Ordering::Equal))
    }
}

/// The rule of the upgrade. Only the newest version retires the earlier ones, and only one at a time.
pub fn decide(view: &View) -> Decision {
    if !view.ready {
        return Decision::Wait("not-ready");
    }
    if view.peers.iter().any(|p| p.state == MemberState::Syncing) {
        return Decision::Wait("syncing");
    }
    if view.peers.iter().any(|p| p.version == PeerVersion::Unknown) {
        return Decision::Wait("unknown-version");
    }
    let mine = view.my_version.as_str();
    // An earlier version that enters does not drain the newer ones, and a node that is not the newest has
    // nothing to retire.
    if view.peers.iter().any(|p| p.version.newer_than(mine)) {
        return Decision::Idle;
    }
    let mut older: Vec<&PeerView> = view
        .peers
        .iter()
        .filter(|p| p.version.older_than(mine))
        .collect();
    if older.is_empty() {
        return Decision::Idle;
    }
    // One actor per cluster: the lowest ID among the nodes of the newest version.
    let lowest = view
        .peers
        .iter()
        .filter(|p| p.version.same_as(mine))
        .map(|p| p.node_id.as_str())
        .chain(std::iter::once(view.me.as_str()))
        .min();
    if lowest != Some(view.me.as_str()) {
        return Decision::Wait("not-actor");
    }
    if let Some(pending) = &view.pending {
        return if pending.elapsed > view.drain_deadline {
            Decision::Stalled(pending.node_id.clone())
        } else {
            Decision::Wait("draining")
        };
    }
    if view.peers.iter().any(|p| p.state == MemberState::Draining) {
        return Decision::Wait("draining");
    }
    let active = 1 + view
        .peers
        .iter()
        .filter(|p| p.state == MemberState::Active)
        .count();
    if active.saturating_sub(1) < MIN_ACTIVE_AFTER_DRAIN {
        return Decision::Wait("min-active");
    }
    older.sort_by(|a, b| {
        let rank = |p: &PeerView| match &p.version {
            PeerVersion::Known(v) => Some(v.clone()),
            _ => None,
        };
        match (rank(a), rank(b)) {
            (None, Some(_)) => Ordering::Less,
            (Some(_), None) => Ordering::Greater,
            (Some(x), Some(y)) => compare(&x, &y).unwrap_or(Ordering::Equal),
            (None, None) => Ordering::Equal,
        }
        .then_with(|| a.node_id.cmp(&b.node_id))
    });
    Decision::Drain(older[0].node_id.clone())
}

/// What outlives a round: the versions seen, the drain in progress and whether the upgrade is stopped.
#[derive(Default)]
pub struct Upgrade {
    versions: Mutex<HashMap<String, PeerVersion>>,
    pending: Mutex<Option<(String, Instant)>>,
    stalled: Mutex<Option<String>>,
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

impl Upgrade {
    /// The member the upgrade stopped on, if the drain did not finish in time.
    pub fn stalled(&self) -> Option<String> {
        lock(&self.stalled).clone()
    }

    /// Does the member understand what `min_version` brought? Whoever did not announce a version, or did not
    /// answer, does not: what is new is not sent to it.
    pub fn supports(&self, node_id: &str, min_version: &str) -> bool {
        match lock(&self.versions).get(node_id) {
            Some(PeerVersion::Known(v)) => {
                matches!(
                    compare(v, min_version),
                    Some(Ordering::Equal | Ordering::Greater)
                )
            }
            _ => false,
        }
    }

    fn remember(&self, versions: HashMap<String, PeerVersion>) {
        *lock(&self.versions) = versions;
    }

    fn pending(&self) -> Option<(String, Instant)> {
        lock(&self.pending).clone()
    }

    fn set_pending(&self, value: Option<(String, Instant)>) {
        *lock(&self.pending) = value;
    }

    fn set_stalled(&self, value: Option<String>) {
        *lock(&self.stalled) = value;
    }
}

/// The version a member's `NodeStatus` carries.
fn version_of(status: &Value) -> PeerVersion {
    match status.get("version").and_then(Value::as_str) {
        Some(version) => PeerVersion::Known(version.to_owned()),
        None => PeerVersion::Previous,
    }
}

impl ClusterService {
    /// The version of this node's program (the build's, unless the test injects another).
    pub fn program_version(&self) -> &str {
        &self.rules().program_version
    }

    /// Does the member understand what `min_version` brought? See [`Upgrade::supports`]. This node always does
    /// for itself.
    pub fn peer_supports(&self, node_id: &str, min_version: &str) -> bool {
        if node_id == self.node_id() {
            return matches!(
                compare(self.program_version(), min_version),
                Some(Ordering::Equal | Ordering::Greater)
            );
        }
        self.upgrade().supports(node_id, min_version)
    }

    /// The member the upgrade stopped on (its drain did not finish in time), for the `upgrade-stalled` condition.
    pub fn upgrade_stalled(&self) -> Option<String> {
        self.upgrade().stalled()
    }

    async fn ask_version(&self, member: &Member) -> PeerVersion {
        match self
            .call_peer(member, Request::Status, self.rules().call_timeout)
            .await
        {
            Ok(Response::Status(status)) => version_of(&status),
            Ok(_) => PeerVersion::Previous,
            Err(CallError::Remote(error)) if error.code == ErrorCode::RouteNotFound => {
                PeerVersion::Previous
            }
            Err(_) => PeerVersion::Unknown,
        }
    }

    /// One round: reads the version of the members, decides and, if it is the case, asks a member to drain.
    pub async fn upgrade_round(&self) {
        let Some(table) = self.membership().table() else {
            return;
        };
        let me = self.node_id();
        if table.member(&me).is_none() {
            return;
        }
        let others: Vec<Member> = table
            .members
            .iter()
            .filter(|m| m.node_id != me)
            .cloned()
            .collect();
        let versions: HashMap<String, PeerVersion> =
            futures_util::future::join_all(others.iter().map(|member| async move {
                (member.node_id.clone(), self.ask_version(member).await)
            }))
            .await
            .into_iter()
            .collect();
        let upgrade = self.upgrade();
        upgrade.remember(versions.clone());

        // The drain this node asked for ended when the member left the table.
        if let Some((node_id, _)) = upgrade.pending()
            && table.member(&node_id).is_none()
        {
            upgrade.set_pending(None);
            upgrade.set_stalled(None);
        }
        let shared = self.node_shared();
        let view = View {
            me: me.clone(),
            my_version: self.program_version().to_owned(),
            ready: shared.state() == NodeState::Active
                && shared.drain_state() == DrainState::Active,
            peers: others
                .iter()
                .map(|m| PeerView {
                    node_id: m.node_id.clone(),
                    state: table.member(&m.node_id).map_or(m.state, |t| t.state),
                    version: versions
                        .get(&m.node_id)
                        .cloned()
                        .unwrap_or(PeerVersion::Unknown),
                })
                .collect(),
            pending: upgrade.pending().map(|(node_id, since)| Pending {
                node_id,
                elapsed: since.elapsed(),
            }),
            drain_deadline: self.rules().upgrade_drain_deadline,
        };
        match decide(&view) {
            Decision::Idle | Decision::Wait(_) => {
                if view.pending.is_none() {
                    upgrade.set_stalled(None);
                }
            }
            Decision::Stalled(node_id) => {
                if upgrade.stalled().as_deref() != Some(node_id.as_str()) {
                    tracing::error!(
                        node_id = %node_id,
                        "upgrade stopped: the node did not finish draining in time; decide whether to cancel its drain or stop it"
                    );
                }
                upgrade.set_stalled(Some(node_id));
            }
            Decision::Drain(node_id) => {
                let Some(member) = table.member(&node_id).cloned() else {
                    return;
                };
                tracing::info!(
                    node_id = %node_id,
                    mine = %view.my_version,
                    "upgrade: draining a member of an earlier version"
                );
                let request = Request::NodeCall(ForwardRequest {
                    method: "POST".into(),
                    path_and_query: "/node/drain".into(),
                    headers: Vec::new(),
                    body: Vec::new(),
                    client_ip: None,
                });
                match self
                    .call_peer(&member, request, self.rules().call_timeout)
                    .await
                {
                    Ok(Response::Forwarded(response)) if response.status < 300 => {
                        upgrade.set_pending(Some((node_id, Instant::now())));
                    }
                    Ok(other) => {
                        tracing::warn!(node_id = %node_id, response = ?other, "upgrade: the member did not accept the drain");
                    }
                    Err(e) => {
                        tracing::warn!(node_id = %node_id, error = %e, "upgrade: the drain request failed");
                    }
                }
            }
        }
    }

    /// Runs the upgrade rounds until the node stops.
    pub async fn run_upgrade(self: std::sync::Arc<Self>) {
        let interval = self.rules().upgrade_interval;
        let cancel = self.cancel_token().clone();
        loop {
            tokio::select! {
                () = cancel.cancelled() => return,
                () = tokio::time::sleep(interval) => {}
            }
            self.upgrade_round().await;
        }
    }
}

#[cfg(test)]
mod tests;
