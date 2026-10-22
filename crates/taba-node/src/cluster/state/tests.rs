use super::*;

fn shared() -> NodeShared {
    NodeShared::new("id".into(), SystemTime::UNIX_EPOCH)
}

/// A node that is active (it finished the bootstrap and the synchronization).
fn active() -> NodeShared {
    let node = shared();
    node.set_state(NodeState::Active);
    node
}

#[test]
fn a_node_is_not_ready_at_birth() {
    assert_eq!(shared().not_ready_reason(), Some("bootstrapping"));
}

#[test]
fn not_ready_reasons_follow_the_documented_names() {
    let node = shared();
    node.set_state(NodeState::Syncing);
    assert_eq!(node.not_ready_reason(), Some("syncing"));
    node.set_state(NodeState::Returning);
    assert_eq!(node.not_ready_reason(), Some("returning"));
    node.set_state(NodeState::Active);
    assert_eq!(node.not_ready_reason(), None);
}

#[test]
fn drain_takes_the_node_out_of_rotation_whatever_its_cluster_state() {
    let node = active();
    node.set_drain_state(DrainState::Draining);
    assert_eq!(node.not_ready_reason(), Some("draining"));
    node.set_drain_state(DrainState::Drained);
    assert_eq!(node.not_ready_reason(), Some("drained"));
    node.set_drain_state(DrainState::Active);
    assert_eq!(node.not_ready_reason(), None);
}

#[test]
fn state_names_are_the_ones_of_the_metrics() {
    assert_eq!(NodeState::Joining.as_str(), "joining");
    assert_eq!(DrainState::Drained.as_str(), "drained");
}

/// UC: UC-OPS-002
#[test]
fn a_failed_node_is_not_ready() {
    let node = active();
    node.set_failed();
    assert_eq!(node.state(), NodeState::Failed);
    assert_eq!(node.not_ready_reason(), Some("failed"));
    assert_eq!(NodeState::Failed.as_str(), "failed");
}

#[test]
fn a_drain_transition_only_happens_from_the_expected_state() {
    let node = active();
    assert!(node.transition_drain(DrainState::Active, DrainState::Draining));
    assert!(!node.transition_drain(DrainState::Active, DrainState::Draining));
    assert_eq!(node.drain_state(), DrainState::Draining);
}
