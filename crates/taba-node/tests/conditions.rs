//! The cluster conditions and `GET /health/cluster` with all the nodes (doc 06b, 06c and 11; doc 20c,
//! level **C**): `GET /cluster/status` on each node, the aggregation in `scope=cluster`, and each condition
//! (`member-unreachable`, `under-replicated`, `write-quorum-at-risk`, `quorum-lost`,
//! `membership-divergent`, `syncing`), with the doc 11 severity.

mod common;

use std::time::Duration;

use serde_json::{Value, json};

use common::TestNode;
use common::cluster::{
    NO_VOTE, TestCluster, address_of, channels_of, create_queue, eventually, eventually_async,
    host_of, pause_sync, settled, start_isolated, total, wait_joined, wait_phase,
};
use taba_node::cluster::protocol::{Request, Response};
use taba_node::cluster::transport::{CallError, Transport};
use taba_node::core::error::ErrorCode;

/// The names of the conditions in force, in the response order.
fn names(status: &Value) -> Vec<String> {
    status["conditions"]
        .as_array()
        .expect("list of conditions")
        .iter()
        .map(|c| c["condition"].as_str().unwrap().to_owned())
        .collect()
}

fn severity_of(status: &Value, condition: &str) -> String {
    status["conditions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["condition"] == condition)
        .unwrap_or_else(|| panic!("no condition {condition} in {status}"))["severity"]
        .as_str()
        .unwrap()
        .to_owned()
}

async fn cluster_view(node: &TestNode) -> Value {
    node.stats("/health/cluster").await
}

async fn node_view(node: &TestNode) -> Value {
    node.stats("/health/cluster?scope=node").await
}

/// UC: UC-OPS-018, UC-CLR-009, UC-FAL-024
#[tokio::test]
async fn every_node_answers_its_status_and_a_healthy_cluster_reports_ok() {
    let cluster = TestCluster::start(3).await;
    settled(&cluster).await;

    // `GET /cluster/status`: the node's `NodeStatus`, for whoever is a member.
    let transport = cluster.network.transport(host_of(2));
    for number in 1..=3 {
        let Ok(Response::Status(status)) = transport
            .call(&address_of(&cluster, number), Request::Status)
            .await
        else {
            panic!("node {number} should answer the status");
        };
        assert_eq!(status["nodeId"], cluster.node_id(number));
        assert_eq!(status["state"], "active");
        assert_eq!(status["signals"]["quorumAvailable"], true);
        assert_eq!(status["signals"]["channels"].as_object().unwrap().len(), 2);
        assert!(status.get("metrics").is_none(), "without the metrics");
    }
    // Whoever is not a member does not read the status, and there is no `GET /cluster/metrics`.
    let stranger = cluster.network.transport("10.0.0.99");
    match stranger
        .call(&address_of(&cluster, 1), Request::Status)
        .await
    {
        Err(CallError::Remote(error)) => assert_eq!(error.code, ErrorCode::ClusterNotMember),
        other => panic!("expected `not-member`, got {other:?}"),
    }
    let missing = Request::from_http("GET", "/cluster/metrics", &[], &[]).unwrap_err();
    assert_eq!(missing.code, ErrorCode::RouteNotFound);

    // `scope=cluster` (the default) joins the three nodes; no condition, `ok`.
    let version = cluster.table(1).unwrap().version;
    let view = cluster_view(cluster.node(1)).await;
    assert_eq!(view["scope"], "cluster");
    assert_eq!(view["partial"], false);
    assert_eq!(view["worstCondition"], "ok");
    assert_eq!(view["conditions"], json!([]));
    assert_eq!(view["summary"]["members"], json!({ "active": 3 }));
    assert_eq!(view["summary"]["membersVersion"], version);
    assert_eq!(view["summary"]["quorumAvailable"], true);
    assert_eq!(view["summary"]["syncingBlocks"], 0);
    let nodes = view["nodes"].as_array().unwrap();
    assert_eq!(nodes.len(), 3);
    for entry in nodes {
        assert_eq!(entry["reachable"], true);
        assert_eq!(entry["status"]["nodeId"], entry["nodeId"]);
        assert_eq!(entry["status"]["signals"]["membersVersion"], version);
        assert_eq!(entry["status"]["signals"]["frozenNodes"], 0);
    }

    // `scope=node` is only the node that serves, without calling the others.
    cluster.network.clear_calls();
    let local = node_view(cluster.node(2)).await;
    assert_eq!(local["scope"], "node");
    assert_eq!(local["nodes"].as_array().unwrap().len(), 1);
    assert_eq!(local["nodes"][0]["nodeId"], cluster.node_id(2));
    assert_eq!(local["conditions"], json!([]));
    assert_eq!(cluster.network.count("status"), 0);

    // The unknown `scope` is `400`; the metrics say that no condition is in force.
    let invalid = cluster
        .node(1)
        .client_get_ops("/health/cluster?scope=other")
        .await;
    assert_eq!(invalid.status(), 400);
    let metrics = cluster.node(1).metrics().await;
    for condition in [
        "member-unreachable",
        "quorum-lost",
        "under-replicated",
        "node-frozen",
    ] {
        let label = format!("taba_condition=\"{condition}\"");
        assert_eq!(
            total(&metrics, "taba_cluster_condition", &label),
            0.0,
            "{condition}"
        );
        assert!(
            metrics.contains(&label),
            "the series {condition} exists with 0"
        );
    }
    cluster.shutdown().await;
}

/// UC: UC-OPS-018, UC-FAL-024, UC-FAL-025
#[tokio::test]
async fn a_node_that_does_not_answer_is_unreachable_and_the_blocks_it_leads_are_at_risk() {
    let cluster = TestCluster::start_with(3, NO_VOTE).await;
    settled(&cluster).await;
    create_queue(cluster.node(1), "orders").await;
    let third = cluster.node_id(3);

    cluster.network.isolate(&host_of(3));
    let channels = channels_of(&cluster, 1);
    let id = third.clone();
    eventually("node 1's channel with 3 to fall", move || {
        !channels.is_up(&id)
    })
    .await;

    // The node that does not answer appears with `reachable: false`; the call does not fail.
    let view = cluster_view(cluster.node(1)).await;
    assert_eq!(view["partial"], true);
    let silent = view["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["nodeId"] == third.as_str())
        .unwrap();
    assert_eq!(silent["reachable"], false);
    assert_eq!(silent["status"], Value::Null);

    // With one of the three out there is still a write quorum (2), but it includes the primary: in the blocks
    // whose primary is the node that is out, the writes fail until the exclusion, and the condition says so (D107).
    assert_eq!(
        names(&view),
        [
            "write-quorum-at-risk",
            "member-unreachable",
            "under-replicated"
        ]
    );
    assert_eq!(severity_of(&view, "write-quorum-at-risk"), "critical");
    assert_eq!(severity_of(&view, "member-unreachable"), "warning");
    assert_eq!(severity_of(&view, "under-replicated"), "warning");
    assert_eq!(view["worstCondition"], "write-quorum-at-risk");
    assert_eq!(view["summary"]["quorumAvailable"], true);

    // The writes keep answering (UC-FAL-025), except those of the blocks whose primary is the node that is out:
    // the quorum always includes the primary, so they fail until the exclusion, which is what the condition
    // above announces. The ID is drawn, so the test sends several and checks that those of the other
    // blocks close.
    let mut accepted = 0;
    for n in 0..16 {
        let response = cluster
            .node(1)
            .client
            .post(cluster.node(1).client_url("/topics/orders/messages"))
            .json(&json!({ "n": n }))
            .send()
            .await
            .unwrap();
        match response.status().as_u16() {
            201 => accepted += 1,
            503 => {
                let body: Value = response.json().await.unwrap();
                assert_eq!(body["code"], "cluster.quorum-unavailable");
            }
            other => panic!("status inesperado: {other}"),
        }
    }
    assert!(
        accepted > 0,
        "the writes of the blocks with a live primary close"
    );

    // The metric has the same conditions.
    eventually_async("the condition metric", || async {
        let metrics = cluster.node(1).metrics().await;
        total(
            &metrics,
            "taba_cluster_condition",
            "taba_condition=\"write-quorum-at-risk\"",
        ) == 1.0
            && total(
                &metrics,
                "taba_cluster_condition",
                "taba_condition=\"member-unreachable\"",
            ) == 1.0
            && total(
                &metrics,
                "taba_cluster_condition",
                "taba_condition=\"under-replicated\"",
            ) == 1.0
    })
    .await;
    cluster.shutdown().await;
}

/// UC: UC-FAL-025, UC-FAL-010
#[tokio::test]
async fn with_two_of_three_nodes_out_the_quorum_is_at_risk_and_the_writes_can_fail() {
    let cluster = TestCluster::start_with(3, NO_VOTE).await;
    settled(&cluster).await;
    create_queue(cluster.node(1), "orders").await;

    // Node 1 loses both: the sound replicas fall below `write.quorum` and the healthy ones
    // below `cluster.vote.min-voters`.
    cluster.network.partition(&host_of(1), &host_of(2));
    cluster.network.partition(&host_of(1), &host_of(3));
    let channels = channels_of(&cluster, 1);
    let (two, three) = (cluster.node_id(2), cluster.node_id(3));
    eventually("node 1's two channels to fall", move || {
        !channels.is_up(&two) && !channels.is_up(&three)
    })
    .await;

    let view = node_view(cluster.node(1)).await;
    assert_eq!(
        names(&view),
        ["write-quorum-at-risk", "quorum-lost", "member-unreachable"]
    );
    assert_eq!(severity_of(&view, "write-quorum-at-risk"), "critical");
    assert_eq!(severity_of(&view, "quorum-lost"), "critical");
    assert_eq!(view["worstCondition"], "write-quorum-at-risk");
    assert_eq!(view["summary"]["quorumAvailable"], false);
    let signals = &view["nodes"][0]["status"]["signals"];
    assert_eq!(signals["quorumAvailable"], false);
    assert!(signals["blocksBelowQuorum"].as_u64().unwrap() > 0);
    assert_eq!(
        signals["blocksBelowQuorum"], signals["blocksBelowReplication"],
        "every block below the quorum is also below the replication"
    );

    // Without a quorum the write does not close.
    let response = cluster
        .node(1)
        .client
        .post(cluster.node(1).client_url("/topics/orders/messages"))
        .json(&json!({"n": 1}))
        .send()
        .await
        .unwrap();
    assert!(
        response.status().is_server_error(),
        "the write without a quorum answered {}",
        response.status()
    );

    // The other two see each other: no critical condition on their side besides node 1 vanishing.
    let others = node_view(cluster.node(2)).await;
    assert!(!names(&others).contains(&"quorum-lost".to_owned()));
    cluster.shutdown().await;
}

/// UC: UC-FAL-010
#[tokio::test]
async fn the_quorum_is_lost_only_for_who_sees_fewer_healthy_nodes_than_the_minimum() {
    let cluster = TestCluster::start(3).await;
    settled(&cluster).await;
    cluster.network.isolate(&host_of(3));

    let lonely = cluster.node(3);
    eventually_async("the isolated node to lose the quorum", || async {
        names(&node_view(lonely).await).contains(&"quorum-lost".to_owned())
    })
    .await;
    let view = node_view(lonely).await;
    assert_eq!(severity_of(&view, "quorum-lost"), "critical");
    assert_eq!(view["summary"]["quorumAvailable"], false);
    let metric = |text: &str| total(text, "taba_cluster_quorum_available", "");
    assert_eq!(metric(&lonely.metrics().await), 0.0);

    // The other two add up the minimum of voters: the quorum exists for them.
    for number in [1, 2] {
        let view = node_view(cluster.node(number)).await;
        assert!(!names(&view).contains(&"quorum-lost".to_owned()));
        assert_eq!(view["summary"]["quorumAvailable"], true);
        assert_eq!(metric(&cluster.node(number).metrics().await), 1.0);
    }
    cluster.shutdown().await;
}

/// The join commit takes a while to reach the members, but the change also travels through the channel's
/// `membership` frame (D126): the tables equalize before it and the divergence does not last. The
/// `membership-divergent` alert for different versions is in the `health` tests.
/// UC: UC-CNC-014
#[tokio::test]
async fn a_delayed_commit_does_not_leave_the_tables_divergent_because_the_channel_carries_the_change()
 {
    let mut cluster = TestCluster::start(3).await;
    settled(&cluster).await;

    cluster
        .network
        .set_delay_kind("commit", Duration::from_millis(3_000));
    let builder = cluster.builder(4, false);
    let number = cluster.spawn_node(builder).await.expect("the node starts");
    wait_joined(cluster.node(number)).await;

    // With the commit still on its way, the old members already have the new table.
    eventually("the tables equal before the commit arrives", || {
        (1..=4).all(|n| cluster.table(n).is_some_and(|t| t.version == 4))
    })
    .await;
    cluster.network.set_delay_kind("commit", Duration::ZERO);
    eventually_async("the conditions to disappear", || async {
        cluster_view(cluster.node(1)).await["conditions"] == json!([])
    })
    .await;
    cluster.shutdown().await;
}

/// UC: UC-FAL-024, UC-FAL-025
#[tokio::test]
async fn a_node_that_syncs_shows_as_syncing_and_under_replicated_but_not_at_risk() {
    let mut cluster = TestCluster::start(3).await;
    create_queue(cluster.node(1), "orders").await;
    for n in 0..6 {
        let response = cluster
            .node(1)
            .client
            .post(cluster.node(1).client_url("/topics/orders/messages"))
            .json(&json!({"n": n}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 201);
    }
    let fourth = start_isolated(&mut cluster, &[]).await;
    let gate = pause_sync(&cluster, fourth, "replay");
    wait_phase(cluster.node(fourth), "replay").await;

    // Node 4's replicas are not yet sound, but the three old ones close the quorum.
    let view = cluster_view(cluster.node(1)).await;
    assert_eq!(names(&view), ["syncing", "under-replicated"]);
    assert_eq!(severity_of(&view, "syncing"), "warning");
    assert_eq!(
        view["summary"]["members"],
        json!({ "active": 3, "syncing": 1 })
    );
    assert!(view["summary"]["syncingBlocks"].as_u64().unwrap() > 0);
    let status_of_fourth = view["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["nodeId"] == cluster.node_id(fourth).as_str())
        .unwrap()["status"]
        .clone();
    assert_eq!(status_of_fourth["state"], "syncing");

    gate.add_permits(1);
    wait_joined(cluster.node(fourth)).await;
    eventually_async("the cluster to go back to ok", || async {
        let view = cluster_view(cluster.node(1)).await;
        view["worstCondition"] == "ok" && view["summary"]["members"] == json!({ "active": 4 })
    })
    .await;
    cluster.shutdown().await;
}
