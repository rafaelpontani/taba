use super::*;

#[test]
fn a_peer_address_is_host_and_port() {
    let addr: PeerAddr = "10.0.0.5:8081".parse().unwrap();
    assert_eq!(addr, PeerAddr::new("10.0.0.5", 8081));
    assert_eq!(addr.to_string(), "10.0.0.5:8081");
    let named: PeerAddr = "taba-lb.internal:9000".parse().unwrap();
    assert_eq!(named.host, "taba-lb.internal");
    // IPv6 in brackets: the host is everything before the last `:`.
    let v6: PeerAddr = "[::1]:8081".parse().unwrap();
    assert_eq!((v6.host.as_str(), v6.port), ("[::1]", 8081));
}

#[test]
fn a_malformed_peer_address_is_refused() {
    for bad in ["", "host", "host:", ":8081", "host:70000", "host:abc"] {
        assert!(bad.parse::<PeerAddr>().is_err(), "{bad}");
    }
}

#[test]
fn only_network_failures_are_worth_retrying() {
    use crate::core::error::{ErrorCode, TabaError};
    assert!(CallError::Unreachable("x".into()).is_network());
    assert!(CallError::Timeout.is_network());
    assert!(!CallError::Remote(TabaError::new(ErrorCode::ClusterConflict, "x")).is_network());
}

/// A node that listed the cluster through a load balancer that only reaches itself gets its own `node-syncing`:
/// that is "no cluster found", like a network failure, and any other error from a peer is not.
#[test]
fn a_node_without_a_member_table_counts_as_no_cluster_found() {
    use crate::core::error::{ErrorCode, TabaError};
    let syncing = CallError::Remote(TabaError::new(ErrorCode::ClusterNodeSyncing, "no table"));
    assert!(syncing.found_no_cluster());
    assert!(CallError::Timeout.found_no_cluster());
    let other = CallError::Remote(TabaError::new(ErrorCode::NodeNotFound, "other"));
    assert!(!other.found_no_cluster());
}
