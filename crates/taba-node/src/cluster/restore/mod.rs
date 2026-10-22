//! Rules of the restoration of the previous lives (doc 37, sections 4.4, 4.5 and 4.8). Pure functions, with no
//! clock, network or disk (but for the manifest file): the same rules the cold start and the restoration apply,
//! so the property of section 4.8 can be tested over them.
//!
//! - [`life_restorable`]: a life is restored only if it was last seen **after** the cluster cut-off date
//!   (the latest start of the continuous recording among the members, section 4.4), with a margin for the
//!   difference between clocks.
//! - [`expected_nodes`] and [`start`]: the restoration of the old messages only **starts** when every node the
//!   biggest old table expected has announced its manifest, or after the grace period `G`; in strict mode it
//!   stays held until the operator forces it (section 4.8).
//! - [`Manifest`]: what each life leaves in its folder (section 4.5).

use std::io;
use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Name of the manifest file inside the folder of a life.
pub const MANIFEST_FILE: &str = crate::storage::messages::MessageStore::LIFE_MANIFEST;

/// What the restoration needs from the configuration (`cluster.restore.*`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rules {
    /// `G`: how long the restoration waits for the expected nodes.
    pub grace: Duration,
    /// After `G`, hold until the operator forces (instead of going on with a warning).
    pub strict: bool,
    /// Tolerance when comparing the clock of the node that wrote a life with the cut-off date.
    pub clock_slack: Duration,
}

impl From<&crate::config::ClusterRestore> for Rules {
    fn from(config: &crate::config::ClusterRestore) -> Self {
        Self {
            grace: config.grace,
            strict: config.strict,
            clock_slack: config.clock_slack,
        }
    }
}

/// A member of the table of a past life.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestMember {
    pub node_id: String,
    /// The node's IPs (the addresses to probe on a cold start, section 4.14).
    pub addresses: Vec<String>,
    pub port: u16,
}

/// The manifest of a life (section 4.5): written in the folder of the life, periodically and on a clean stop.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    /// The node ID of the life (the folder has this name).
    pub node_id: String,
    /// The **active** members of the table the life last saw.
    pub members: Vec<ManifestMember>,
    /// The version of that table.
    pub table_version: u64,
    /// The last instant (ms since the epoch) the life was seen active.
    pub last_seen_ms: u64,
    /// The largest message `id` the life generated (section 4.10); the new generator starts above it.
    pub watermark: Option<String>,
    /// When the life's log of excluded messages started recording (ms), to merge it with the node's log when the
    /// life is restored (section 4.4). Absent in a manifest from before this field.
    #[serde(default)]
    pub excluded_since_ms: Option<u64>,
}

impl Manifest {
    /// Writes the manifest in `dir`, replacing the previous one atomically (a crash leaves the old or the new
    /// file, never half of one).
    pub fn write(&self, dir: &Path) -> io::Result<()> {
        std::fs::create_dir_all(dir)?;
        let tmp = dir.join(format!("{MANIFEST_FILE}.tmp"));
        let json = serde_json::to_vec_pretty(self).map_err(io::Error::other)?;
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, dir.join(MANIFEST_FILE))
    }

    /// Reads the manifest of the folder; `None` if there is none (a life of an earlier version) or if it is
    /// unreadable (then the folder is not a restorable life).
    pub fn read(dir: &Path) -> Option<Self> {
        let bytes = std::fs::read(dir.join(MANIFEST_FILE)).ok()?;
        serde_json::from_slice(&bytes).ok()
    }
}

/// A life is restorable if it was last seen after the cut-off date plus the clock margin (the margin leans to the
/// safe side: a life that is too close to the date is not restored, and follows the path of the loss list).
/// Without a cut-off date nothing is known about the cluster's log: no life is restorable.
pub fn life_restorable(last_seen_ms: u64, cutoff_ms: Option<u64>, slack: Duration) -> bool {
    let Some(cutoff) = cutoff_ms else {
        return false;
    };
    let slack = u64::try_from(slack.as_millis()).unwrap_or(u64::MAX);
    last_seen_ms > cutoff.saturating_add(slack)
}

/// The nodes the restoration waits for: the active members of the **biggest** old table (the highest
/// version) among the manifests announced. Sorted, without repetition.
pub fn expected_nodes(manifests: &[Manifest]) -> Vec<String> {
    let Some(newest) = manifests
        .iter()
        .max_by_key(|m| (m.table_version, &m.node_id))
    else {
        return Vec::new();
    };
    let mut ids: Vec<String> = newest.members.iter().map(|m| m.node_id.clone()).collect();
    ids.sort();
    ids.dedup();
    ids
}

/// Whether the restoration of the old messages starts now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Start {
    /// Every expected node announced its life.
    Go,
    /// It starts with the nodes that are present, and says who is missing (`G` passed, or the operator forced).
    GoWithMissing(Vec<String>),
    /// Waiting for the expected nodes, until `G`.
    Wait(Vec<String>),
    /// `G` passed in strict mode: held until the operator forces.
    Held(Vec<String>),
}

/// The rule of section 4.8. `announced` are the old node IDs whose life some present node announced.
pub fn start(
    expected: &[String],
    announced: &[String],
    elapsed: Duration,
    rules: &Rules,
    forced: bool,
) -> Start {
    let missing: Vec<String> = expected
        .iter()
        .filter(|id| !announced.contains(id))
        .cloned()
        .collect();
    if missing.is_empty() {
        return Start::Go;
    }
    if forced {
        return Start::GoWithMissing(missing);
    }
    if elapsed < rules.grace {
        return Start::Wait(missing);
    }
    if rules.strict {
        Start::Held(missing)
    } else {
        Start::GoWithMissing(missing)
    }
}

#[cfg(test)]
mod tests;
