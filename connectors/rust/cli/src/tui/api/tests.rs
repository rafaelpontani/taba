use super::*;

const STATUS: &str = r#"{"collectedAt":"2026-10-15T12:00:00.000Z","partial":true,
 "missingNodes":[{"nodeId":"n3","reason":"timeout"}],"worstCondition":"member-unreachable",
 "conditions":[{"condition":"member-unreachable","severity":"warning"},{"condition":"quorum-lost","severity":"critical"}],
 "summary":{"members":{"active":2,"syncing":1},"membersVersion":7,"quorumAvailable":true,"syncingBlocks":4},
 "nodes":[
  {"nodeId":"n1","reachable":true,"status":{"version":"1.2.3","mode":"production","state":"active","drainState":"active",
    "startedAt":"2026-10-15T10:00:00.000Z",
    "storage":{"memoryUsedBytes":512,"memoryLimitBytes":1024,"memoryMessages":10,"diskMessages":20,"diskFreeBytes":9999},
    "signals":{"membersVersion":7,"quorumAvailable":true,"channels":2,"syncingBlocks":1,"blocksBelowReplication":3,"blocksBelowQuorum":0,"frozenNodes":0},
    "runtime":{"activeRequests":2.0,"rateLimitRejected":5.0,"channelRttP95Seconds":0.012}}},
  {"nodeId":"n3","reachable":false,"status":null}]}"#;

/// TUI: TUI-CLU-01
#[test]
fn cluster_status_is_read_with_conditions_nodes_and_the_partial_envelope() {
    let s = parse_cluster_status(STATUS).unwrap();
    assert!(s.partial);
    assert_eq!(
        s.missing,
        vec![Missing {
            node_id: "n3".into(),
            reason: "timeout".into()
        }]
    );
    assert_eq!(s.conditions.len(), 2);
    assert_eq!(s.conditions[1].severity, Severity::Critical);
    assert_eq!(s.members, vec![("active".into(), 2), ("syncing".into(), 1)]);
    assert_eq!(
        (s.members_version, s.quorum, s.syncing_blocks),
        (7, true, 4)
    );
    assert_eq!(s.nodes.len(), 2);
}

/// TUI: TUI-CLU-03
#[test]
fn a_node_status_carries_storage_signals_and_runtime() {
    let n = &parse_cluster_status(STATUS).unwrap().nodes[0];
    assert_eq!((n.memory_used, n.memory_limit), (512, 1024));
    assert_eq!((n.memory_messages, n.disk_messages), (10, 20));
    assert_eq!(n.below_replication, 3);
    assert_eq!((n.active_requests, n.rate_limited), (2.0, 5.0));
    assert_eq!(n.rtt_p95, Some(0.012));
    assert!((n.memory_ratio() - 0.5).abs() < 1e-9);
}

/// TUI: TUI-CLU-03
#[test]
fn an_unreachable_node_has_no_status() {
    let n = &parse_cluster_status(STATUS).unwrap().nodes[1];
    assert!(!n.reachable);
    assert_eq!(n.memory_limit, 0);
    assert_eq!(n.memory_ratio(), 0.0);
}

/// TUI: TUI-CLU-07
#[test]
fn the_node_list_is_read_without_addresses() {
    let nodes = parse_nodes(
        r#"{"partial":false,"missingNodes":[],"items":[
        {"nodeId":"a","self":true,"reachable":true,"state":"active","drainState":"active","version":"1"},
        {"nodeId":"b","self":false,"reachable":true,"state":"drained","drainState":"drained","version":"1"}]}"#,
    )
    .unwrap();
    assert_eq!(nodes.items.len(), 2);
    assert!(nodes.items[0].is_self);
    assert_eq!(nodes.items[1].state, "drained");
    assert!(!nodes.partial);
}

/// TUI: TUI-FRM-10
#[test]
fn me_says_what_the_token_can_do() {
    let me = parse_me(
        r#"{"authEnabled":true,"root":false,"name":"app","permissions":["publish","consume"]}"#,
    )
    .unwrap();
    assert!(!me.is_root());
    assert!(me.can("publish") && !me.can("admin"));
    let open = parse_me(r#"{"authEnabled":false,"root":false,"permissions":[]}"#).unwrap();
    assert!(open.is_root() && open.can("admin"));
}

/// TUI: TUI-NOD-03
#[test]
fn drain_status_is_read() {
    let d = parse_drain(
        r#"{"drainState":"draining","trigger":"api","startedAt":null,"pendingMessages":13}"#,
    )
    .unwrap();
    assert_eq!((d.state.as_str(), d.pending), ("draining", 13));
    assert!(!d.is_active() && !d.is_done());
}

/// TUI: TUI-NOD-04
#[test]
fn a_cluster_flush_lists_each_node_and_a_node_flush_is_one_entry() {
    let r = parse_flush(
        r#"{"complete":false,"nodes":[{"nodeId":"a","complete":true,"outboxPending":0},
        {"nodeId":"b","complete":false,"pending":true,"error":"timeout"}]}"#,
    )
    .unwrap();
    assert!(!r.complete);
    assert_eq!(r.nodes.len(), 2);
    assert_eq!(r.nodes[1].error.as_deref(), Some("timeout"));
    let one = parse_flush(r#"{"complete":true,"memoryRemaining":0,"timedOut":false,"nodeId":"a"}"#)
        .unwrap();
    assert_eq!(one.nodes.len(), 1);
    assert!(one.complete && one.nodes[0].complete);
}

/// TUI: TUI-NOD-05
#[test]
fn a_stop_report_lists_nodes_and_warnings() {
    let r = parse_stop(
        r#"{"stopped":["a","b"],"stopList":false,"warnings":["S3 down"],"nodes":[{"nodeId":"a","drained":true,"outboxPending":0}]}"#,
    )
    .unwrap();
    assert_eq!(r.stopped, vec!["a", "b"]);
    assert!(!r.stop_list);
    assert_eq!(r.warnings, vec!["S3 down"]);
    assert_eq!(r.nodes, vec![("a".to_owned(), true)]);
}

/// TUI: TUI-CLU-05
#[test]
fn each_call_maps_to_its_route_and_node_ids_are_encoded() {
    let c = ApiCall::DrainStart("n/1".into()).to_call();
    assert_eq!((c.method, c.path.as_str()), ("POST", "/nodes/n%2F1/drain"));
    let c = ApiCall::DrainCancel("a".into()).to_call();
    assert_eq!((c.method, c.path.as_str()), ("DELETE", "/nodes/a/drain"));
    assert_eq!(
        ApiCall::FlushCluster.to_call().path,
        "/cluster/backup/flush"
    );
    assert_eq!(ApiCall::StopCluster.to_call().path, "/cluster/stop");
    assert!(ApiCall::StopCluster.is_action() && !ApiCall::ClusterStatus.is_action());
}

/// TUI: TUI-NOD-02
#[test]
fn a_backup_is_read_leniently() {
    let b = parse_backup(
        r#"{"s3Reachable":true,"outboxPending":4,"lastSuccessAt":"2026-10-15T10:00:00Z"}"#,
    )
    .unwrap();
    assert_eq!((b.reachable, b.pending), (Some(true), Some(4)));
    assert!(b.last_success.is_some());
}

#[test]
fn bad_json_is_an_error_not_a_panic() {
    assert!(parse_cluster_status("<html>").is_err());
    assert!(parse_nodes("").is_err());
}

/// TUI: TUI-NOD-01
#[test]
fn what_a_real_development_node_answers_is_read() {
    // Copied from a node in development mode: no memory limit, channels as a map, a null round trip.
    let text = r#"{"collectedAt":"2026-10-15T20:21:14.532Z","conditions":[],"missingNodes":[],"nodes":[{"nodeId":"01a14139","reachable":true,"status":{"collectedAt":"x","drainState":"active","mode":"development","nodeId":"01a14139","runtime":{"activeRequests":1.0,"channelRttP95Seconds":null,"rateLimitRejected":0.0},"signals":{"blocksBelowQuorum":0,"blocksBelowReplication":0,"channels":{"a":1,"b":2},"frozenNodes":0,"membersVersion":0,"quorumAvailable":true,"syncingBlocks":0},"startedAt":"2026-10-15T20:21:08.585Z","state":"active","storage":{"diskFreeBytes":41568894976,"diskMessages":0,"memoryLimitBytes":null,"memoryMessages":0,"memoryUsedBytes":0},"version":"0.1.0"}}],"partial":false,"scope":"cluster","summary":{"members":{"active":1},"membersVersion":0,"quorumAvailable":true,"syncingBlocks":0},"worstCondition":"ok"}"#;
    let status = parse_cluster_status(text).unwrap();
    let node = &status.nodes[0];
    assert_eq!(node.channels, 2);
    assert_eq!((node.memory_limit, node.memory_ratio()), (0, 0.0));
    assert_eq!(node.rtt_p95, None);
    assert_eq!(node.disk_free, 41_568_894_976);
    assert!(node.quorum);
    let stop = parse_stop(r#"{"nodes":[{"drained":true,"nodeId":"a"}],"stopList":false,"stopped":["a"],"warnings":["S3 backup not configured: there is no stop list"]}"#).unwrap();
    assert_eq!(stop.stopped, vec!["a"]);
    assert_eq!(stop.warnings.len(), 1);
}
