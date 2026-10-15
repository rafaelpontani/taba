//! The topic and token definitions shared by all the nodes, with an owner on the ring: who
//! decides, the version, the insistence on the nodes that do not answer and the removal tombstone (level **C** of
//! doc 20): several real nodes in the same process, linked by the in-memory network.

mod common;

use common::TestNode;
use common::cluster::{
    TestCluster, address_of, eventually_async, host_of, name_owned_by, settled, total,
};
use common::error_code;
use serde_json::json;
use taba_node::cluster::protocol::{
    DefinitionApply, DefinitionKind, DefinitionOperation, DefinitionProposal, Request, Response,
    StreamQuery,
};
use taba_node::cluster::transport::{CallError, Transport};
use taba_node::core::error::ErrorCode;
use taba_node::core::ring::definition_position;

/// The error code the peer returned.
fn remote_code(error: CallError) -> ErrorCode {
    match error {
        CallError::Remote(e) => e.code,
        other => panic!("expected a peer error, got {other}"),
    }
}

async fn has_topic(cluster: &TestCluster, number: usize, name: &str) -> bool {
    cluster
        .node(number)
        .client_get(&format!("/topics/{name}"))
        .await
        .status()
        == 200
}

async fn create(cluster: &TestCluster, number: usize, name: &str) -> reqwest::Response {
    let node = cluster.node(number);
    node.client
        .post(node.client_url("/topics"))
        .json(&json!({"name": name, "type": "queue"}))
        .send()
        .await
        .expect("request")
}

/// The slice of the topic stream that node `number` serves, as a peer would ask for it.
async fn stream_of(cluster: &TestCluster, number: usize) -> Vec<serde_json::Value> {
    let peer = cluster
        .network
        .transport(host_of(if number == 1 { 2 } else { 1 }));
    match peer
        .call(
            &address_of(cluster, number),
            Request::ListTopics(StreamQuery::default()),
        )
        .await
        .expect("stream")
    {
        Response::Stream(page) => page.items,
        other => panic!("expected a stream, got {other:?}"),
    }
}

/// A node that does not answer does not prevent the creation: the owner insists on it until it comes back.
/// UC: UC-DEF-031
#[tokio::test]
async fn a_topic_created_while_a_node_is_unreachable_reaches_it_when_it_returns() {
    let cluster = TestCluster::start(3).await;
    let name = name_owned_by(&cluster, 1, "late-");
    cluster.network.isolate(&host_of(3));

    let response = create(&cluster, 1, &name).await;
    assert_eq!(
        response.status(),
        201,
        "without a register on all the nodes"
    );
    assert!(has_topic(&cluster, 1, &name).await);
    assert!(
        has_topic(&cluster, 2, &name).await,
        "the reachable node already has it"
    );
    assert!(
        !has_topic(&cluster, 3, &name).await,
        "the isolated one not yet"
    );

    cluster.network.restore(&host_of(3));
    eventually_async("the node that came back to receive the topic", || async {
        has_topic(&cluster, 3, &name).await
    })
    .await;
    cluster.shutdown().await;
}

/// Whoever receives the call is not the owner: proposes to the owner, who decides and spreads it.
/// UC: UC-DEF-025, UC-DEF-033
#[tokio::test]
async fn a_change_received_by_a_node_that_is_not_the_owner_is_decided_by_the_owner() {
    let cluster = TestCluster::start(3).await;
    let name = name_owned_by(&cluster, 2, "owned-");
    cluster.network.clear_calls();

    let response = create(&cluster, 1, &name).await;
    assert_eq!(response.status(), 201);
    assert_eq!(cluster.network.count("definition-propose"), 1);
    for number in 1..=3 {
        eventually_async("the topic on every node", || async {
            has_topic(&cluster, number, &name).await
        })
        .await;
    }
    // The owner gave version 1 and all keep it.
    for number in 1..=3 {
        let items = stream_of(&cluster, number).await;
        let item = items
            .iter()
            .find(|i| i["name"] == name.as_str())
            .expect("the topic in the stream");
        assert_eq!(item["version"], 1, "node {number}");
    }
    cluster.shutdown().await;
}

/// Two creations of the same name at the same time, on different nodes: the owner serializes and only one counts.
/// UC: UC-DEF-028, UC-DEF-029
#[tokio::test]
async fn two_nodes_creating_the_same_name_at_once_leave_one_topic_and_one_conflict() {
    let cluster = TestCluster::start(3).await;
    let name = name_owned_by(&cluster, 3, "race-");

    let (first, second) = tokio::join!(create(&cluster, 1, &name), create(&cluster, 2, &name),);
    let mut statuses = [first.status().as_u16(), second.status().as_u16()];
    statuses.sort_unstable();
    assert_eq!(statuses, [201, 409]);
    let loser = if first.status() == 409 { first } else { second };
    assert_eq!(error_code(loser, 409).await, "topic.already-exists");
    cluster.shutdown().await;
}

/// The owner is down and still in the table: the change fails with `503` and the client repeats (D107).
/// UC: UC-DEF-032
#[tokio::test]
async fn a_change_whose_owner_is_unreachable_fails_with_503() {
    let cluster = TestCluster::start(3).await;
    let name = name_owned_by(&cluster, 3, "orphan-");
    cluster.network.isolate(&host_of(3));

    let response = create(&cluster, 1, &name).await;
    assert_eq!(
        error_code(response, 503).await,
        "cluster.quorum-unavailable"
    );
    assert!(!has_topic(&cluster, 1, &name).await, "nothing was applied");
    assert!(!has_topic(&cluster, 2, &name).await);
    cluster.shutdown().await;
}

/// The removal leaves a tombstone with the version: a delayed change (lower version) does not resurrect the
/// topic.
/// UC: UC-DEF-027, UC-DEF-026
#[tokio::test]
async fn a_late_change_does_not_bring_a_removed_topic_back() {
    let cluster = TestCluster::start(3).await;
    let name = name_owned_by(&cluster, 1, "gone-");
    assert_eq!(create(&cluster, 1, &name).await.status(), 201);
    for number in 1..=3 {
        eventually_async("the topic on every node", || async {
            has_topic(&cluster, number, &name).await
        })
        .await;
    }
    let creation = stream_of(&cluster, 2)
        .await
        .into_iter()
        .find(|i| i["name"] == name.as_str())
        .expect("the topic in the stream");

    let node = cluster.node(1);
    let removed = node
        .client
        .delete(node.client_url(&format!("/topics/{name}")))
        .send()
        .await
        .expect("request");
    assert_eq!(removed.status(), 204);
    for number in 1..=3 {
        eventually_async("the topic removed on every node", || async {
            !has_topic(&cluster, number, &name).await
        })
        .await;
    }

    // The creation (version 1) arrives again after the removal (version 2): it is ignored.
    let late = DefinitionApply {
        kind: DefinitionKind::Topic,
        key: name.clone(),
        version: 1,
        deleted: false,
        body: creation,
        relay: false,
    };
    let peer = cluster.network.transport(host_of(1));
    for number in [2, 3] {
        peer.call(
            &address_of(&cluster, number),
            Request::DefinitionApply(late.clone()),
        )
        .await
        .expect("the node accepts the call");
        assert!(
            !has_topic(&cluster, number, &name).await,
            "node {number} does not resurrect the topic"
        );
    }
    cluster.shutdown().await;
}

/// Changing twice gives increasing versions, and all the nodes end on the last one.
/// UC: UC-DEF-026
#[tokio::test]
async fn each_change_gets_the_next_version_and_every_node_ends_on_the_last() {
    let cluster = TestCluster::start(3).await;
    let name = name_owned_by(&cluster, 2, "ver-");
    assert_eq!(create(&cluster, 1, &name).await.status(), 201);
    let node = cluster.node(3);
    let patched = node
        .client
        .patch(node.client_url(&format!("/topics/{name}")))
        .json(&json!({"tags": {"team": "pay"}}))
        .send()
        .await
        .expect("request");
    assert_eq!(patched.status(), 200);

    for number in 1..=3 {
        eventually_async("version 2 on every node", || async {
            stream_of(&cluster, number)
                .await
                .iter()
                .any(|i| i["name"] == name.as_str() && i["version"] == 2)
        })
        .await;
    }
    cluster.shutdown().await;
}

/// Two copies of the same version and different content (two decisions that did not know of each other)
/// end up equal on every node, whatever the arrival order: the one with the lowest content fingerprint
/// wins. The tie is counted; repeating the same copy is not a tie.
/// UC: UC-DEF-035
#[tokio::test]
async fn two_copies_with_the_same_version_end_the_same_in_any_arrival_order() {
    let cluster = TestCluster::start(3).await;
    let name = name_owned_by(&cluster, 1, "tie-");
    assert_eq!(create(&cluster, 1, &name).await.status(), 201);
    for number in 1..=3 {
        eventually_async("the topic on every node", || async {
            has_topic(&cluster, number, &name).await
        })
        .await;
    }
    let base = stream_of(&cluster, 1)
        .await
        .into_iter()
        .find(|i| i["name"] == name.as_str())
        .expect("the topic in the stream");
    let copy = |team: &str| DefinitionApply {
        kind: DefinitionKind::Topic,
        key: name.clone(),
        version: 2,
        deleted: false,
        body: {
            let mut body = base.clone();
            body["tags"] = json!({"team": team});
            body
        },
        relay: false,
    };
    let (first, second) = (copy("alpha"), copy("beta"));
    let sender = cluster.network.transport(host_of(1));
    let send = |number: usize, change: DefinitionApply| {
        let (sender, address) = (sender.clone(), address_of(&cluster, number));
        async move {
            sender
                .call(&address, Request::DefinitionApply(change))
                .await
                .expect("the node accepts the call");
        }
    };
    send(2, first.clone()).await;
    send(2, second.clone()).await;
    send(3, second.clone()).await;
    send(3, first.clone()).await;

    let tags_of = |number: usize| {
        let cluster = &cluster;
        let name = name.clone();
        async move {
            stream_of(cluster, number)
                .await
                .into_iter()
                .find(|i| i["name"] == name.as_str())
                .map(|i| i["tags"].clone())
        }
    };
    let (on_two, on_three) = (tags_of(2).await, tags_of(3).await);
    assert_eq!(on_two, on_three, "the two nodes end with the same copy");

    let conflicts = |number: usize| {
        let node = cluster.node(number);
        async move {
            total(
                &node.metrics().await,
                "taba_cluster_definitions_conflicts_total",
                "",
            )
        }
    };
    assert_eq!(conflicts(2).await, 1.0, "node 2 counted the tie");
    assert_eq!(conflicts(3).await, 1.0, "node 3 counted the tie");

    // Repeating the copy that won, or the one that lost, changes nothing and the one that lost is only a new tie.
    send(2, first.clone()).await;
    send(2, second.clone()).await;
    assert_eq!(tags_of(2).await, on_two, "the winning copy stays");
    assert_eq!(
        conflicts(2).await,
        2.0,
        "only the losing copy counts as a tie again"
    );
    cluster.shutdown().await;
}

/// The node that joins also reads the tombstones of what was removed: without them it would not know the version already used,
/// and a delayed creation of a lower version (or the recreation with version 1) would resurrect or be
/// ignored by the other nodes. The tombstones come out of the `GET /cluster/definitions/tombstones` stream.
/// UC: UC-DEF-036
#[tokio::test]
async fn a_new_node_learns_the_tombstones_and_a_late_creation_does_not_bring_the_topic_back() {
    let mut cluster = TestCluster::start(2).await;
    let name = name_owned_by(&cluster, 1, "gone-");
    assert_eq!(create(&cluster, 1, &name).await.status(), 201);
    for number in 1..=2 {
        eventually_async("the topic on every node", || async {
            has_topic(&cluster, number, &name).await
        })
        .await;
    }
    let creation = stream_of(&cluster, 1)
        .await
        .into_iter()
        .find(|i| i["name"] == name.as_str())
        .expect("the topic in the stream");
    let node = cluster.node(1);
    let removed = node
        .client
        .delete(node.client_url(&format!("/topics/{name}")))
        .send()
        .await
        .expect("request");
    assert_eq!(removed.status(), 204);
    for number in 1..=2 {
        eventually_async("the topic removed on every node", || async {
            !has_topic(&cluster, number, &name).await
        })
        .await;
    }

    cluster.add_node(false).await;

    // The new node read the tombstone (version 2) from the node that served the load.
    let tombstones = |number: usize| {
        let (peer, address) = (
            cluster.network.transport(host_of(1)),
            address_of(&cluster, number),
        );
        async move {
            match peer
                .call(&address, Request::ListTombstones(StreamQuery::default()))
                .await
                .expect("stream")
            {
                Response::Stream(page) => page.items,
                other => panic!("expected a stream, got {other:?}"),
            }
        }
    };
    let learned = tombstones(3).await;
    assert!(
        learned
            .iter()
            .any(|i| i["kind"] == "topic" && i["key"] == name.as_str() && i["version"] == 2),
        "the new node keeps the tombstone: {learned:?}"
    );
    assert_eq!(tombstones(1).await.len(), learned.len());

    // The delayed creation (version 1) arrives at the new node: with the tombstone, it is ignored.
    let late = DefinitionApply {
        kind: DefinitionKind::Topic,
        key: name.clone(),
        version: 1,
        deleted: false,
        body: creation,
        relay: false,
    };
    cluster
        .network
        .transport(host_of(1))
        .call(&address_of(&cluster, 3), Request::DefinitionApply(late))
        .await
        .expect("the node accepts the call");
    assert!(
        !has_topic(&cluster, 3, &name).await,
        "the new node does not resurrect the topic"
    );
    cluster.shutdown().await;
}

/// With no other reachable owner the change does not exist on another node: the owner refuses (`503`) instead
/// of answering with a single copy.
/// UC: UC-DEF-030
#[tokio::test]
async fn a_change_with_no_other_owner_reachable_fails_with_503() {
    let cluster = TestCluster::start(3).await;
    let name = name_owned_by(&cluster, 1, "alone-");
    cluster.network.isolate(&host_of(2));
    cluster.network.isolate(&host_of(3));

    let response = create(&cluster, 1, &name).await;
    assert_eq!(
        error_code(response, 503).await,
        "cluster.quorum-unavailable"
    );
    assert!(!has_topic(&cluster, 1, &name).await, "nothing was applied");
    cluster.shutdown().await;
}

/// The owner checks the change against its state: an unknown topic is `404` and the `expectedId` of a
/// removal that does not match is `409`, without applying anything.
/// UC: UC-DEF-029
#[tokio::test]
async fn the_owner_refuses_a_change_that_does_not_fit_the_current_state() {
    let cluster = TestCluster::start(3).await;
    let name = name_owned_by(&cluster, 1, "strict-");
    let peer = cluster.network.transport(host_of(2));
    let owner = address_of(&cluster, 1);
    let proposal = |operation, expected_id: Option<&str>| {
        Request::DefinitionPropose(DefinitionProposal {
            kind: DefinitionKind::Topic,
            operation,
            key: name.clone(),
            body: json!({"name": name, "id": "01927f3a-0000-7000-8000-000000000001"}),
            expected_id: expected_id.map(str::to_owned),
        })
    };

    let unknown = peer
        .call(&owner, proposal(DefinitionOperation::Delete, None))
        .await
        .unwrap_err();
    assert_eq!(remote_code(unknown), ErrorCode::TopicNotFound);

    assert_eq!(create(&cluster, 1, &name).await.status(), 201);
    let stale = peer
        .call(
            &owner,
            proposal(
                DefinitionOperation::Delete,
                Some("01927f3a-0000-7000-8000-0000000000ff"),
            ),
        )
        .await
        .unwrap_err();
    assert_eq!(remote_code(stale), ErrorCode::ClusterConflict);
    assert!(
        has_topic(&cluster, 2, &name).await,
        "the topic is still there"
    );
    cluster.shutdown().await;
}

const ROOT: &str = "raiz-raiz-raiz-raiz-raiz-raiz-raiz-0001";

async fn create_token(node: &TestNode) -> (String, String) {
    let response = node
        .client
        .post(node.client_url("/tokens"))
        .header("authorization", format!("Bearer {ROOT}"))
        .json(&json!({"name": "app", "permissions": ["consume"], "topicPrefixes": []}))
        .send()
        .await
        .expect("request");
    assert_eq!(response.status(), 201);
    let body: serde_json::Value = response.json().await.expect("corpo");
    (
        body["id"].as_str().expect("id").to_owned(),
        body["token"].as_str().expect("token").to_owned(),
    )
}

async fn status_with(node: &TestNode, token: &str) -> u16 {
    node.client
        .get(node.client_url("/topics"))
        .header("authorization", format!("Bearer {token}"))
        .send()
        .await
        .expect("request")
        .status()
        .as_u16()
}

/// The token also has an owner (its `id` on the ring): the revocation counts at once on the nodes that answer and
/// reaches the node that was down when it comes back.
/// UC: UC-DEF-034, UC-DEF-031
#[tokio::test]
async fn a_token_revoked_while_a_node_is_unreachable_stops_working_there_when_it_returns() {
    let cluster = TestCluster::start_with(3, &[("TABA_AUTH_ROOT_TOKEN", ROOT)]).await;
    let third = cluster.node_id(3);
    let membership = cluster.node(1).node.cluster().unwrap().membership();
    // A token whose owner is not the node that will be isolated.
    let (id, secret) = loop {
        let (id, secret) = create_token(cluster.node(1)).await;
        let owner = membership
            .ring()
            .primary(definition_position("token", &id))
            .map(str::to_owned);
        if owner.as_deref() != Some(third.as_str()) {
            break (id, secret);
        }
    };
    for number in 1..=3 {
        eventually_async("the token on every node", || async {
            status_with(cluster.node(number), &secret).await == 200
        })
        .await;
    }

    cluster.network.isolate(&host_of(3));
    let revoked = cluster
        .node(1)
        .client
        .delete(cluster.node(1).client_url(&format!("/tokens/{id}")))
        .header("authorization", format!("Bearer {ROOT}"))
        .send()
        .await
        .expect("request");
    assert_eq!(revoked.status(), 204);
    assert_eq!(status_with(cluster.node(1), &secret).await, 401);
    assert_eq!(status_with(cluster.node(2), &secret).await, 401);
    assert_eq!(
        status_with(cluster.node(3), &secret).await,
        200,
        "the isolated one did not learn yet"
    );

    cluster.network.restore(&host_of(3));
    eventually_async(
        "the node that came back to receive the revocation",
        || async { status_with(cluster.node(3), &secret).await == 401 },
    )
    .await;
    cluster.shutdown().await;
}

/// The owner does not reach a node, but the heir does: the definition reaches the node through the `definition` frame
/// the heir queues, without depending on the owner (D127). Before, only the owner insisted.
/// UC: UC-DEF-037
#[tokio::test]
async fn the_heir_carries_the_definition_to_a_node_the_owner_cannot_reach() {
    let cluster = TestCluster::start(3).await;
    settled(&cluster).await;
    let name = name_owned_by(&cluster, 1, "heir-");
    cluster.network.partition(&host_of(1), &host_of(3));

    let response = create(&cluster, 1, &name).await;
    assert_eq!(response.status(), 201, "the owner answers with the heir");
    assert!(has_topic(&cluster, 2, &name).await);
    eventually_async("node 3 to receive the topic through the heir", || async {
        has_topic(&cluster, 3, &name).await
    })
    .await;

    // The removal (a tombstone) follows the same path.
    let node = cluster.node(1);
    let removed = node
        .client
        .delete(node.client_url(&format!("/topics/{name}")))
        .send()
        .await
        .expect("request");
    assert_eq!(removed.status(), 204);
    eventually_async("node 3 to receive the removal through the heir", || async {
        !has_topic(&cluster, 3, &name).await
    })
    .await;
    cluster.shutdown().await;
}

/// The owner restarts after writing to the heir and before the delayed node confirms: the heir already
/// queued the frame, and the node receives it when it comes back, without the owner's list of pending ones (D127).
/// UC: UC-DEF-038
#[tokio::test]
async fn a_node_that_was_down_gets_the_latest_state_of_each_definition_from_the_queue() {
    let cluster = TestCluster::start(3).await;
    settled(&cluster).await;
    let name = name_owned_by(&cluster, 1, "queue-");
    cluster.network.isolate(&host_of(3));

    assert_eq!(create(&cluster, 1, &name).await.status(), 201);
    let node = cluster.node(1);
    for tags in [json!({"v": "1"}), json!({"v": "2"})] {
        let patched = node
            .client
            .patch(node.client_url(&format!("/topics/{name}")))
            .json(&json!({"tags": tags}))
            .send()
            .await
            .expect("request");
        assert_eq!(patched.status(), 200);
    }

    cluster.network.restore(&host_of(3));
    eventually_async("node 3 to end on the last version", || async {
        stream_of(&cluster, 3)
            .await
            .iter()
            .any(|i| i["name"] == name.as_str() && i["version"] == 3)
    })
    .await;
    let items = stream_of(&cluster, 3).await;
    let topic = items
        .iter()
        .find(|i| i["name"] == name.as_str())
        .expect("the topic");
    assert_eq!(topic["tags"], json!({"v": "2"}));
    cluster.shutdown().await;
}
