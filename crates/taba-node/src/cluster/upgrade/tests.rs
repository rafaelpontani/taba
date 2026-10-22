use std::cmp::Ordering;
use std::time::Duration;

use super::*;

fn peer(node_id: &str, state: MemberState, version: PeerVersion) -> PeerView {
    PeerView {
        node_id: node_id.into(),
        state,
        version,
    }
}

fn known(version: &str) -> PeerVersion {
    PeerVersion::Known(version.into())
}

/// Node `b` of version 0.2.0 with three members of 0.1.0 beside it (four active in all).
fn view(peers: Vec<PeerView>) -> View {
    View {
        me: "b".into(),
        my_version: "0.2.0".into(),
        ready: true,
        peers,
        pending: None,
        drain_deadline: Duration::from_secs(60),
    }
}

fn three_old() -> Vec<PeerView> {
    vec![
        peer("c", MemberState::Active, known("0.1.0")),
        peer("d", MemberState::Active, known("0.1.0")),
        peer("e", MemberState::Active, known("0.1.0")),
    ]
}

/// UC: UC-ING-035
#[test]
fn versions_compare_by_precedence_and_a_candidate_comes_before_its_final() {
    assert_eq!(compare("0.1.9", "0.1.10"), Some(Ordering::Less));
    assert_eq!(compare("0.2.0", "0.1.99"), Some(Ordering::Greater));
    assert_eq!(compare("1.0.0", "1.0.0"), Some(Ordering::Equal));
    assert_eq!(compare("0.2.0-rc.1", "0.2.0"), Some(Ordering::Less));
    assert_eq!(compare("0.2.0-rc.2", "0.2.0-rc.10"), Some(Ordering::Less));
    assert_eq!(compare("v0.2.0+build5", "0.2.0"), Some(Ordering::Equal));
    assert_eq!(compare("not-a-version", "0.2.0"), None);
    assert_eq!(compare("0.2", "0.2.0"), None);
}

/// UC: UC-ING-029
#[test]
fn the_newest_version_drains_the_oldest_member_first() {
    let mut peers = three_old();
    peers[1].version = PeerVersion::Previous;
    assert_eq!(
        decide(&view(peers)),
        Decision::Drain("d".into()),
        "a member that publishes no version is the oldest"
    );
    assert_eq!(decide(&view(three_old())), Decision::Drain("c".into()));
}

/// UC: UC-ING-029
#[test]
fn a_cluster_of_one_version_has_nothing_to_retire() {
    let peers = vec![
        peer("c", MemberState::Active, known("0.2.0")),
        peer("d", MemberState::Active, known("0.2.0")),
    ];
    assert_eq!(decide(&view(peers)), Decision::Idle);
}

/// UC: UC-ING-029
#[test]
fn the_next_drain_waits_for_the_previous_one_to_leave() {
    let mut peers = three_old();
    peers[0].state = MemberState::Draining;
    assert_eq!(decide(&view(peers)), Decision::Wait("draining"));

    let mut waiting = view(three_old());
    waiting.pending = Some(Pending {
        node_id: "c".into(),
        elapsed: Duration::from_secs(5),
    });
    assert_eq!(decide(&waiting), Decision::Wait("draining"));
}

/// UC: UC-ING-030
#[test]
fn the_drain_does_not_start_if_it_would_leave_fewer_than_three_active() {
    // Three active nodes: draining one leaves two.
    let peers = vec![
        peer("c", MemberState::Active, known("0.1.0")),
        peer("d", MemberState::Active, known("0.1.0")),
    ];
    assert_eq!(decide(&view(peers)), Decision::Wait("min-active"));

    // Four active nodes: draining one leaves three.
    assert_eq!(decide(&view(three_old())), Decision::Drain("c".into()));

    // A member that is not active does not count toward the three.
    let mut peers = three_old();
    peers[2].state = MemberState::Suspect;
    assert_eq!(decide(&view(peers)), Decision::Wait("min-active"));
}

/// UC: UC-ING-030
#[test]
fn the_drain_does_not_start_while_the_node_is_not_ready_or_a_member_is_syncing() {
    let mut not_ready = view(three_old());
    not_ready.ready = false;
    assert_eq!(decide(&not_ready), Decision::Wait("not-ready"));

    let mut peers = three_old();
    peers.push(peer("f", MemberState::Syncing, known("0.2.0")));
    assert_eq!(decide(&view(peers)), Decision::Wait("syncing"));
}

/// UC: UC-ING-030
#[test]
fn a_member_that_did_not_answer_makes_the_round_decide_nothing() {
    let mut peers = three_old();
    peers[0].version = PeerVersion::Unknown;
    assert_eq!(decide(&view(peers)), Decision::Wait("unknown-version"));
}

/// UC: UC-ING-031
#[test]
fn a_drain_that_does_not_finish_in_time_stops_the_upgrade_on_that_member() {
    let mut late = view(three_old());
    late.pending = Some(Pending {
        node_id: "c".into(),
        elapsed: Duration::from_secs(61),
    });
    assert_eq!(decide(&late), Decision::Stalled("c".into()));
}

/// UC: UC-ING-033
#[test]
fn an_earlier_version_does_not_drain_the_newer_ones() {
    // The node is the earlier one: whoever is newer retires it, not the other way around.
    let mut earlier = view(vec![
        peer("c", MemberState::Active, known("0.3.0")),
        peer("d", MemberState::Active, known("0.3.0")),
        peer("e", MemberState::Active, known("0.2.0")),
        peer("f", MemberState::Active, known("0.2.0")),
    ]);
    earlier.my_version = "0.2.0".into();
    assert_eq!(decide(&earlier), Decision::Idle);
}

/// UC: UC-ING-034
#[test]
fn only_the_lowest_id_among_the_newest_acts() {
    let mut peers = three_old();
    peers.push(peer("a", MemberState::Active, known("0.2.0")));
    assert_eq!(
        decide(&view(peers)),
        Decision::Wait("not-actor"),
        "`a` has the lowest ID of the nodes in 0.2.0"
    );
}

/// UC: UC-ING-032
#[test]
fn a_member_supports_what_a_version_brought_only_if_it_announced_that_version() {
    let upgrade = Upgrade::default();
    upgrade.remember(
        [
            ("new".to_owned(), known("0.2.0")),
            ("old".to_owned(), known("0.1.0")),
            ("silent".to_owned(), PeerVersion::Previous),
            ("lost".to_owned(), PeerVersion::Unknown),
        ]
        .into_iter()
        .collect(),
    );
    assert!(upgrade.supports("new", "0.2.0"));
    assert!(!upgrade.supports("old", "0.2.0"));
    assert!(!upgrade.supports("silent", "0.2.0"));
    assert!(!upgrade.supports("lost", "0.2.0"));
    assert!(!upgrade.supports("unseen", "0.2.0"));
}
