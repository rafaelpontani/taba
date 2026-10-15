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
