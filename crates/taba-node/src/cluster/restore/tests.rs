use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use super::*;

fn ids(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_owned()).collect()
}

fn rules(strict: bool) -> Rules {
    Rules {
        grace: Duration::from_secs(300),
        strict,
        clock_slack: Duration::from_secs(5),
    }
}

fn manifest(node: &str, version: u64, members: &[&str], last_seen_ms: u64) -> Manifest {
    Manifest {
        node_id: node.to_owned(),
        members: members
            .iter()
            .map(|id| ManifestMember {
                node_id: (*id).to_owned(),
                addresses: vec!["10.0.0.1".into()],
                port: 7000,
            })
            .collect(),
        table_version: version,
        last_seen_ms,
        watermark: Some("01H".into()),
    }
}

/// UC: UC-RTN-018
#[test]
fn a_life_is_restorable_only_if_seen_after_the_cutoff_plus_the_clock_slack() {
    let slack = Duration::from_secs(5);
    // Seen well after the cut-off date: restorable.
    assert!(life_restorable(100_000, Some(50_000), slack));
    // Inside the margin (or before the date): not restorable, it follows the path of the loss list.
    assert!(!life_restorable(54_000, Some(50_000), slack));
    assert!(!life_restorable(55_000, Some(50_000), slack));
    assert!(life_restorable(55_001, Some(50_000), slack));
    assert!(!life_restorable(10_000, Some(50_000), slack));
    // The cluster has no cut-off date to compare with: nothing is known, nothing is restored.
    assert!(!life_restorable(u64::MAX, None, slack));
}

/// UC: UC-RTN-019
#[test]
fn the_manifest_is_written_atomically_and_read_back() {
    let dir = tempfile::tempdir().unwrap();
    let life = dir.path().join("01OLDNODE");
    let first = manifest("01OLDNODE", 7, &["01OLDNODE", "01OTHER"], 1_000);
    first.write(&life).unwrap();
    assert_eq!(Manifest::read(&life), Some(first));
    // A second write (the periodic `last_seen`) replaces the first and leaves no temporary file behind.
    let second = manifest("01OLDNODE", 7, &["01OLDNODE", "01OTHER"], 2_000);
    second.write(&life).unwrap();
    assert_eq!(Manifest::read(&life), Some(second));
    let files: Vec<_> = std::fs::read_dir(&life)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(files, vec![MANIFEST_FILE.to_owned()]);
}

/// UC: UC-RTN-019
#[test]
fn a_folder_without_a_readable_manifest_is_not_a_life() {
    let dir = tempfile::tempdir().unwrap();
    // A life of an earlier version has no manifest.
    assert_eq!(Manifest::read(dir.path()), None);
    std::fs::write(dir.path().join(MANIFEST_FILE), b"{ not json").unwrap();
    assert_eq!(Manifest::read(dir.path()), None);
}

/// UC: UC-RTN-020
#[test]
fn the_expected_nodes_are_the_members_of_the_biggest_old_table() {
    let manifests = vec![
        manifest("a", 3, &["a", "b", "c"], 10),
        manifest("b", 5, &["b", "c", "d", "a"], 20),
        manifest("c", 5, &["b", "c", "d", "a"], 30),
    ];
    assert_eq!(expected_nodes(&manifests), ids(&["a", "b", "c", "d"]));
    assert!(expected_nodes(&[]).is_empty());
}

/// UC: UC-RTN-020
#[test]
fn the_restoration_waits_for_the_expected_nodes_until_the_grace_period() {
    let expected = ids(&["a", "b", "c"]);
    let rules = rules(false);
    let short = Duration::from_secs(10);
    let long = Duration::from_secs(300);
    // Everyone announced: it starts right away, however early.
    assert_eq!(
        start(
            &expected,
            &ids(&["a", "b", "c"]),
            Duration::ZERO,
            &rules,
            false
        ),
        Start::Go
    );
    // One is missing: waits until `G`...
    assert_eq!(
        start(&expected, &ids(&["a", "b"]), short, &rules, false),
        Start::Wait(ids(&["c"]))
    );
    // ...and then goes on with a warning, with the nodes that are present.
    assert_eq!(
        start(&expected, &ids(&["a", "b"]), long, &rules, false),
        Start::GoWithMissing(ids(&["c"]))
    );
}

/// UC: UC-RTN-020
#[test]
fn in_strict_mode_the_restoration_stays_held_after_the_grace_period_until_forced() {
    let expected = ids(&["a", "b", "c"]);
    let rules = rules(true);
    let long = Duration::from_secs(3600);
    assert_eq!(
        start(&expected, &ids(&["a"]), long, &rules, false),
        Start::Held(ids(&["b", "c"]))
    );
    // The operator forces: it goes on, with the risk declared, even before `G`.
    assert_eq!(
        start(&expected, &ids(&["a"]), Duration::ZERO, &rules, true),
        Start::GoWithMissing(ids(&["b", "c"]))
    );
    // Forcing changes nothing when nobody is missing.
    assert_eq!(start(&expected, &expected, long, &rules, true), Start::Go);
}

// ---- The property of section 4.8 ----
//
// A model of a cluster that stops as a whole: every node holds some messages, records the acks in its log of
// excluded messages (which trims the oldest rows and moves its start forward), and leaves the cluster at some
// instant (its `last_seen`). Then the nodes that are present restore. The model uses the rules of this module
// and the rule of the log (`since` moves to the instant of the newest row trimmed).

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

const STOP: u64 = 10_000_000;
const SLACK: Duration = Duration::from_secs(5);

struct Node {
    /// The instant it left the cluster (`STOP` if it stayed to the end): its `last_seen`.
    left_at: u64,
    /// The log of excluded messages: `(id, instant)`, oldest first.
    log: Vec<(usize, u64)>,
    /// The start of the continuous recording.
    since: u64,
    /// The messages the life's disk still holds.
    disk: BTreeSet<usize>,
}

struct World {
    nodes: Vec<Node>,
    /// Message -> the instant of its ack.
    acked: BTreeMap<usize, u64>,
}

fn world(rng: &mut Rng) -> World {
    let count = 3 + rng.below(3) as usize;
    let mut nodes: Vec<Node> = (0..count)
        .map(|_| Node {
            left_at: if rng.below(2) == 0 {
                STOP
            } else {
                1_000_000 + rng.below(STOP - 2_000_000)
            },
            log: Vec::new(),
            since: 0,
            disk: BTreeSet::new(),
        })
        .collect();
    let mut acked = BTreeMap::new();
    for id in 0..(20 + rng.below(40) as usize) {
        let published = rng.below(STOP - 1_000_000);
        // Three holders, as in the production default.
        let mut holders = BTreeSet::new();
        while holders.len() < 3.min(count) {
            holders.insert(rng.below(count as u64) as usize);
        }
        let holders: Vec<usize> = holders
            .into_iter()
            .filter(|h| nodes[*h].left_at > published)
            .collect();
        if holders.is_empty() {
            continue;
        }
        for h in &holders {
            nodes[*h].disk.insert(id);
        }
        if rng.below(3) == 0 {
            continue; // never acknowledged
        }
        let at = published + 1 + rng.below(STOP - published - 1);
        // The ack needs a holder that is still in the cluster.
        let live: Vec<usize> = holders
            .iter()
            .copied()
            .filter(|h| nodes[*h].left_at > at)
            .collect();
        if live.is_empty() {
            continue;
        }
        for h in live {
            nodes[h].disk.remove(&id);
            nodes[h].log.push((id, at));
        }
        acked.insert(id, at);
    }
    // The log trims its oldest rows (by the size or age limit) and moves its start to the newest trimmed.
    for node in &mut nodes {
        node.log.sort_by_key(|(_, at)| *at);
        let drop = rng.below(node.log.len() as u64 + 1) as usize;
        if drop > 0 {
            node.since = node.log[drop - 1].1;
            node.log.drain(..drop);
        }
    }
    World { nodes, acked }
}

/// The messages the restoration activates, with the nodes `present` and, or not, the rule of the window.
fn restored(world: &World, present: &[usize], check_window: bool) -> BTreeSet<usize> {
    let cutoff = present.iter().map(|n| world.nodes[*n].since).max();
    let dead: BTreeSet<usize> = present
        .iter()
        .flat_map(|n| world.nodes[*n].log.iter().map(|(id, _)| *id))
        .collect();
    let mut out = BTreeSet::new();
    for n in present {
        let node = &world.nodes[*n];
        if check_window && !life_restorable(node.left_at, cutoff, SLACK) {
            continue;
        }
        out.extend(node.disk.iter().filter(|id| !dead.contains(id)));
    }
    out
}

/// UC: UC-RTN-021
#[test]
fn with_every_expected_node_present_no_consumed_message_comes_back() {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    for _ in 0..2_000 {
        let world = world(&mut rng);
        let all: Vec<usize> = (0..world.nodes.len()).collect();
        let back = restored(&world, &all, true);
        for id in back {
            assert!(
                !world.acked.contains_key(&id),
                "message {id}, acknowledged at {:?}, was delivered again",
                world.acked.get(&id)
            );
        }
    }
}

/// UC: UC-RTN-021
#[test]
fn the_property_has_teeth_without_the_rule_of_the_window_a_consumed_message_comes_back() {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let mut violations = 0;
    for _ in 0..2_000 {
        let world = world(&mut rng);
        let all: Vec<usize> = (0..world.nodes.len()).collect();
        let back = restored(&world, &all, false);
        violations += back
            .iter()
            .filter(|id| world.acked.contains_key(id))
            .count();
    }
    assert!(
        violations > 0,
        "the model never produced a stale life: the property above proves nothing"
    );
}

/// UC: UC-RTN-022
#[test]
fn with_a_node_missing_only_what_every_logger_of_it_missed_can_come_back() {
    let mut rng = Rng(0xDEAD_BEEF_CAFE_F00D);
    let mut redelivered = 0;
    for _ in 0..2_000 {
        let world = world(&mut rng);
        let count = world.nodes.len();
        // Some nodes are absent (at least one, at least one present).
        let present: Vec<usize> = (0..count).filter(|_| rng.below(3) != 0).collect::<Vec<_>>();
        if present.is_empty() || present.len() == count {
            continue;
        }
        let back = restored(&world, &present, true);
        for id in back {
            let Some(at) = world.acked.get(&id) else {
                continue;
            };
            redelivered += 1;
            // A consumed message only comes back if no node that is present logged its ack.
            let logged_by_present = present.iter().any(|n| {
                world.nodes[*n]
                    .log
                    .iter()
                    .any(|(logged, when)| *logged == id && when == at)
            });
            assert!(
                !logged_by_present,
                "message {id} came back although a present node logged the ack"
            );
        }
    }
    assert!(
        redelivered > 0,
        "the model never lost a logger: the case of the missing node is not exercised"
    );
}

/// UC: UC-RTN-022
#[test]
fn nothing_starts_before_the_grace_period_while_an_expected_node_is_missing() {
    let mut rng = Rng(0x1234_5678_9ABC_DEF1);
    for strict in [false, true] {
        let rules = rules(strict);
        for _ in 0..1_000 {
            let expected: Vec<String> = (0..2 + rng.below(4)).map(|i| format!("n{i}")).collect();
            let announced: Vec<String> = expected
                .iter()
                .filter(|_| rng.below(3) != 0)
                .cloned()
                .collect();
            let elapsed = Duration::from_secs(rng.below(600));
            let decision = start(&expected, &announced, elapsed, &rules, false);
            let complete = expected.iter().all(|id| announced.contains(id));
            match decision {
                Start::Go => assert!(complete),
                Start::Wait(missing) => {
                    assert!(!complete && elapsed < rules.grace && !missing.is_empty());
                }
                Start::GoWithMissing(missing) => {
                    assert!(!complete && elapsed >= rules.grace && !rules.strict);
                    assert!(missing.iter().all(|id| !announced.contains(id)));
                }
                Start::Held(_) => assert!(!complete && elapsed >= rules.grace && rules.strict),
            }
        }
    }
}
