//! The cluster and the nodes in the client API (doc 34, D-TUI-2, 3, 4 and 9): who the token is, the node list,
//! the root-only routes and what a `403` says it lacked, the statistics within the token's prefixes, a node's
//! operations reached through any node, and emptying a queue in the whole cluster.

mod common;

use std::time::Duration;

use common::cluster::{TestCluster, eventually_async};
use common::{TestNode, error_code};
use reqwest::{Method, Response};
use serde_json::{Value, json};

const ROOT: &str = "raiz-raiz-raiz-raiz-raiz-raiz-raiz-0001";

async fn call(
    node: &TestNode,
    method: Method,
    path: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> Response {
    let mut request = node.client.request(method, node.client_url(path));
    if let Some(token) = token {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    if let Some(body) = body {
        request = request.json(&body);
    }
    request.send().await.unwrap()
}

async fn get(node: &TestNode, path: &str, token: Option<&str>) -> Response {
    call(node, Method::GET, path, token, None).await
}

async fn json_of(response: Response, status: u16) -> Value {
    assert_eq!(response.status(), status);
    response.json().await.unwrap()
}

/// Creates a token with the root one and returns its secret.
async fn token(node: &TestNode, permissions: &[&str], prefixes: &[&str]) -> String {
    node.clock.advance(Duration::from_secs(1));
    let body = json!({ "name": "test", "permissions": permissions, "topicPrefixes": prefixes });
    let created = call(node, Method::POST, "/tokens", Some(ROOT), Some(body)).await;
    json_of(created, 201).await["token"]
        .as_str()
        .unwrap()
        .to_owned()
}

async fn queue(node: &TestNode, name: &str) {
    node.clock.advance(Duration::from_secs(1));
    let body = json!({ "name": name, "type": "queue" });
    let created = call(node, Method::POST, "/topics", Some(ROOT), Some(body)).await;
    assert_eq!(created.status(), 201, "{name}");
}

async fn publish(node: &TestNode, topic: &str, n: u32) {
    let response = call(
        node,
        Method::POST,
        &format!("/topics/{topic}/messages"),
        Some(ROOT),
        Some(json!({ "n": n })),
    )
    .await;
    assert_eq!(response.status(), 201, "{:?}", response.text().await);
}

async fn start_single() -> TestNode {
    TestNode::builder()
        .production()
        .env("TABA_AUTH_ROOT_TOKEN", ROOT)
        .start()
        .await
}

/// UC: UC-AUT-037
#[tokio::test]
async fn a_forbidden_answer_says_what_the_token_lacked() {
    let node = start_single().await;
    let reader = token(&node, &["consume"], &["orders"]).await;

    // A root-only route: `required` is `root`.
    node.clock.advance(Duration::from_secs(1));
    let denied = get(&node, "/cluster/status", Some(&reader)).await;
    assert_eq!(denied.status(), 403);
    let body: Value = denied.json().await.unwrap();
    assert_eq!(body["code"], "auth.forbidden");
    assert_eq!(body["required"], "root");

    // A permission route: `required` is the permission.
    queue(&node, "orders").await;
    let pruned = call(
        &node,
        Method::DELETE,
        "/topics/orders/messages",
        Some(&reader),
        None,
    )
    .await;
    assert_eq!(pruned.status(), 403);
    assert_eq!(pruned.json::<Value>().await.unwrap()["required"], "admin");

    // Other errors do not carry the field.
    let missing = get(&node, "/topics/orders/nothing", Some(ROOT)).await;
    assert!(
        missing
            .json::<Value>()
            .await
            .unwrap()
            .get("required")
            .is_none()
    );
    node.shutdown().await;
}

/// UC: UC-AUT-038
#[tokio::test]
async fn auth_me_says_who_the_token_is_and_what_it_can_do() {
    let node = start_single().await;
    let writer = token(&node, &["publish", "consume"], &["orders", "billing"]).await;

    let root = json_of(get(&node, "/auth/me", Some(ROOT)).await, 200).await;
    assert_eq!(root["root"], true);
    assert_eq!(root["authEnabled"], true);
    assert_eq!(
        root["permissions"],
        json!(["publish", "consume", "create", "admin"])
    );
    assert_eq!(root["topicPrefixes"], json!([]));

    let me = json_of(get(&node, "/auth/me", Some(&writer)).await, 200).await;
    assert_eq!(me["root"], false);
    assert_eq!(me["name"], "test");
    assert_eq!(me["permissions"], json!(["publish", "consume"]));
    assert_eq!(me["topicPrefixes"], json!(["orders", "billing"]));
    assert!(me["id"].is_string());

    assert_eq!(get(&node, "/auth/me", None).await.status(), 401);
    node.shutdown().await;
}

/// UC: UC-AUT-039
#[tokio::test]
async fn any_valid_token_lists_the_nodes_and_sees_the_statistics_of_its_prefixes() {
    let node = start_single().await;
    queue(&node, "orders-eu").await;
    queue(&node, "billing").await;
    let reader = token(&node, &["consume"], &["orders"]).await;

    let nodes = json_of(get(&node, "/nodes", Some(&reader)).await, 200).await;
    let items = nodes["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["self"], true);
    assert_eq!(items[0]["drainState"], "active");
    assert!(items[0]["version"].is_string());
    assert!(
        items[0].get("addresses").is_none(),
        "no addresses for a plain token"
    );

    // The list only has what the prefixes reach; the other topic is out of scope.
    let stats = json_of(get(&node, "/stats/topics", Some(&reader)).await, 200).await;
    let names: Vec<&str> = stats["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["orders-eu", "orders-eu-dead-letter"]);
    assert_eq!(
        get(&node, "/stats/topics/orders-eu", Some(&reader))
            .await
            .status(),
        200
    );
    assert_eq!(
        error_code(
            get(&node, "/stats/topics/billing", Some(&reader)).await,
            403
        )
        .await,
        "auth.out-of-scope"
    );
    assert_eq!(get(&node, "/nodes", None).await.status(), 401);
    node.shutdown().await;
}

/// UC: UC-OPS-046
#[tokio::test]
async fn the_cluster_and_node_routes_need_the_root_token_and_an_unknown_node_is_404() {
    let node = start_single().await;
    let admin = token(&node, &["admin"], &[]).await;
    let nodes = json_of(get(&node, "/nodes", Some(ROOT)).await, 200).await;
    let id = nodes["items"][0]["nodeId"].as_str().unwrap().to_owned();

    for path in [
        "/cluster/status".to_owned(),
        format!("/nodes/{id}/metrics"),
        format!("/nodes/{id}/drain"),
        format!("/nodes/{id}/backup"),
        "/cluster/backup".to_owned(),
    ] {
        let denied = get(&node, &path, Some(&admin)).await;
        assert_eq!(denied.status(), 403, "{path}");
        assert_eq!(denied.json::<Value>().await.unwrap()["required"], "root");
    }

    let status = json_of(get(&node, "/cluster/status", Some(ROOT)).await, 200).await;
    assert_eq!(status["nodes"].as_array().unwrap().len(), 1);

    let metrics = get(&node, &format!("/nodes/{id}/metrics"), Some(ROOT)).await;
    assert_eq!(metrics.status(), 200);
    assert!(metrics.text().await.unwrap().contains("taba_"));

    let drain = json_of(
        get(&node, &format!("/nodes/{id}/drain"), Some(ROOT)).await,
        200,
    )
    .await;
    assert_eq!(drain["drainState"], "active");

    let unknown = get(&node, "/nodes/no-such-node/metrics", Some(ROOT)).await;
    assert_eq!(error_code(unknown, 404).await, "node.not-found");
    node.shutdown().await;
}

/// UC: UC-OPS-047
#[tokio::test]
async fn a_node_answers_for_itself_or_calls_the_node_that_has_the_data_once() {
    let cluster = TestCluster::start_with(3, &[("TABA_AUTH_ROOT_TOKEN", ROOT)]).await;
    let (first, third) = (cluster.node(1), cluster.node(3));
    let third_id = cluster.node_id(3);

    let nodes = json_of(get(first, "/nodes", Some(ROOT)).await, 200).await;
    let items = nodes["items"].as_array().unwrap();
    assert_eq!(items.len(), 3);
    assert!(items.iter().all(|n| n["reachable"] == true));
    assert_eq!(items.iter().filter(|n| n["self"] == true).count(), 1);

    // Its own data: nothing crosses the cluster port.
    cluster.network.clear_calls();
    let own = cluster.node_id(1);
    assert_eq!(
        get(first, &format!("/nodes/{own}/metrics"), Some(ROOT))
            .await
            .status(),
        200
    );
    assert_eq!(cluster.network.count("node-call"), 0);

    // Another node's: one call, and the answer is that node's (its drain, not the receiver's).
    let status = json_of(
        get(first, &format!("/nodes/{third_id}/drain"), Some(ROOT)).await,
        200,
    )
    .await;
    assert_eq!(status["drainState"], "active");
    assert_eq!(cluster.network.count("node-call"), 1);

    // Starting the drain of the third node through the first one drains the third.
    let started = call(
        first,
        Method::POST,
        &format!("/nodes/{third_id}/drain"),
        Some(ROOT),
        None,
    )
    .await;
    assert_eq!(started.status(), 202);
    eventually_async("the third node is drained", || async {
        let response = third.client_get_ops("/node/drain").await;
        response.json::<Value>().await.unwrap()["drainState"] == "drained"
    })
    .await;

    // It shows in the list as drained, still addressable to bring it back.
    let nodes = json_of(get(first, "/nodes", Some(ROOT)).await, 200).await;
    let drained = nodes["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["nodeId"] == third_id.as_str())
        .unwrap();
    assert_eq!(drained["state"], "drained");
    let again = call(
        first,
        Method::GET,
        &format!("/nodes/{third_id}/drain"),
        Some(ROOT),
        None,
    )
    .await;
    assert_eq!(again.status(), 200);
    assert_eq!(
        again.json::<Value>().await.unwrap()["drainState"],
        "drained"
    );

    // An unknown node does not leave the node.
    cluster.network.clear_calls();
    let unknown = get(
        first,
        "/nodes/00000000-0000-7000-8000-000000000000/metrics",
        Some(ROOT),
    )
    .await;
    assert_eq!(error_code(unknown, 404).await, "node.not-found");
    assert_eq!(cluster.network.count("node-call"), 0);
    cluster.shutdown().await;
}

/// UC: UC-TOP-047
#[tokio::test]
async fn pruning_empties_the_queue_on_every_node_and_the_queue_keeps_working() {
    let cluster = TestCluster::start_with(3, &[("TABA_AUTH_ROOT_TOKEN", ROOT)]).await;
    let (first, second) = (cluster.node(1), cluster.node(2));
    queue(first, "orders").await;
    for n in 0..12 {
        publish(cluster.node(1 + (n as usize % 3)), "orders", n).await;
    }
    // Some messages are invisible when the prune comes.
    let delivered = get(
        first,
        "/topics/orders/messages?maxMessages=3&visibilityTimeout=300",
        Some(ROOT),
    )
    .await;
    assert_eq!(delivered.status(), 200);

    cluster.network.clear_calls();
    let pruned = call(
        second,
        Method::DELETE,
        "/topics/orders/messages",
        Some(ROOT),
        None,
    )
    .await;
    let body = json_of(pruned, 200).await;
    assert_eq!(
        body["removed"], 12,
        "each message counts once, not once per copy"
    );
    assert_eq!(body["nodes"].as_array().unwrap().len(), 3);
    assert_eq!(
        cluster.network.count("prune"),
        3 - 1,
        "the receiver prunes itself without the network"
    );

    // Nothing is left anywhere, visible or invisible.
    for number in 1..=3 {
        let node = cluster.node(number);
        let again = get(
            node,
            "/topics/orders/messages?maxMessages=10&visibilityTimeout=300",
            Some(ROOT),
        )
        .await;
        let body = json_of(again, 200).await;
        assert!(
            body["messages"].as_array().unwrap().is_empty(),
            "node {number}"
        );
    }

    // The topic stays and still delivers.
    publish(second, "orders", 99).await;
    let after = get(
        first,
        "/topics/orders/messages?maxMessages=10&visibilityTimeout=300",
        Some(ROOT),
    )
    .await;
    assert_eq!(
        json_of(after, 200).await["messages"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    cluster.shutdown().await;
}

/// UC: UC-TOP-048
#[tokio::test]
async fn pruning_an_unknown_topic_is_404_and_needs_the_admin_permission() {
    let node = start_single().await;
    let missing = call(
        &node,
        Method::DELETE,
        "/topics/ghost/messages",
        Some(ROOT),
        None,
    )
    .await;
    assert_eq!(error_code(missing, 404).await, "topic.not-found");

    queue(&node, "orders").await;
    let creator = token(&node, &["create", "publish", "consume"], &[]).await;
    let refused = call(
        &node,
        Method::DELETE,
        "/topics/orders/messages",
        Some(&creator),
        None,
    )
    .await;
    assert_eq!(error_code(refused, 403).await, "auth.forbidden");

    let admin = token(&node, &["admin"], &["orders"]).await;
    publish(&node, "orders", 1).await;
    let pruned = call(
        &node,
        Method::DELETE,
        "/topics/orders/messages",
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(json_of(pruned, 200).await["removed"], 1);
    node.shutdown().await;
}

/// UC: UC-TOP-049
#[tokio::test]
async fn a_prune_that_fails_on_a_node_names_it_and_repeating_it_is_safe() {
    let cluster = TestCluster::start_with(3, &[("TABA_AUTH_ROOT_TOKEN", ROOT)]).await;
    let first = cluster.node(1);
    queue(first, "orders").await;
    for n in 0..9 {
        publish(cluster.node(1 + (n as usize % 3)), "orders", n).await;
    }
    let third_id = cluster.node_id(3);
    cluster.network.isolate(&common::cluster::host_of(3));

    let failed = call(
        first,
        Method::DELETE,
        "/topics/orders/messages",
        Some(ROOT),
        None,
    )
    .await;
    assert_eq!(failed.status(), 503);
    let body: Value = failed.json().await.unwrap();
    assert_eq!(body["code"], "cluster.prune-incomplete");
    assert!(body["message"].as_str().unwrap().contains(&third_id));
    assert_eq!(body["details"][0]["field"], third_id.as_str());

    // The nodes that answered were emptied; the node that came back is emptied by repeating the call.
    cluster.network.restore(&common::cluster::host_of(3));
    let repeated = call(
        first,
        Method::DELETE,
        "/topics/orders/messages",
        Some(ROOT),
        None,
    )
    .await;
    let body = json_of(repeated, 200).await;
    assert_eq!(body["nodes"].as_array().unwrap().len(), 3);
    for number in 1..=3 {
        let again = get(
            cluster.node(number),
            "/topics/orders/messages?maxMessages=10&visibilityTimeout=300",
            Some(ROOT),
        )
        .await;
        let body = json_of(again, 200).await;
        assert!(
            body["messages"].as_array().unwrap().is_empty(),
            "node {number}"
        );
    }
    cluster.shutdown().await;
}

/// UC: UC-OPS-048
#[tokio::test]
async fn a_node_that_does_not_answer_is_a_failure_that_names_it_with_no_retry() {
    let cluster = TestCluster::start_with(3, &[("TABA_AUTH_ROOT_TOKEN", ROOT)]).await;
    let first = cluster.node(1);
    let third_id = cluster.node_id(3);
    cluster.network.isolate(&common::cluster::host_of(3));
    cluster.network.clear_calls();

    let denied = get(first, &format!("/nodes/{third_id}/metrics"), Some(ROOT)).await;
    assert_eq!(denied.status(), 502);
    let body: Value = denied.json().await.unwrap();
    assert_eq!(body["code"], "forward.bad-gateway");
    assert!(body["message"].as_str().unwrap().contains(&third_id));
    assert_eq!(
        cluster.network.count("node-call"),
        1,
        "a single attempt: no retry"
    );
    cluster.shutdown().await;
}

async fn publish_in_group(node: &TestNode, topic: &str, group: &str, n: u32) {
    let response = call(
        node,
        Method::POST,
        &format!("/topics/{topic}/messages?groupId={group}"),
        Some(ROOT),
        Some(json!({ "n": n })),
    )
    .await;
    assert_eq!(response.status(), 201, "{:?}", response.text().await);
}

/// UC: UC-OPS-049
#[tokio::test]
async fn the_statistics_are_the_clusters_not_one_nodes() {
    let cluster = TestCluster::start_with(3, &[("TABA_AUTH_ROOT_TOKEN", ROOT)]).await;
    let first = cluster.node(1);
    queue(first, "orders").await;
    for n in 0..12u32 {
        let group = ["a", "b", "c"][n as usize % 3];
        publish_in_group(cluster.node(1 + (n as usize % 3)), "orders", group, n).await;
    }

    // Through any node: the cluster's 12 messages, complete.
    for number in 1..=3 {
        let node = cluster.node(number);
        let detail = json_of(get(node, "/stats/topics/orders", Some(ROOT)).await, 200).await;
        assert_eq!(detail["visibleMessages"], 12, "through node {number}");
        assert_eq!(detail["depth"], 12);
        assert_eq!(detail["partial"], false);
        assert_eq!(detail["missingNodes"], json!([]));
    }
    let list = json_of(get(first, "/stats/topics", Some(ROOT)).await, 200).await;
    let orders = list["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "orders")
        .unwrap();
    assert_eq!(orders["visibleMessages"], 12);
    assert_eq!(list["partial"], false);

    // `scope=node` is that node alone, and the nodes add up to the cluster.
    let mut sum = 0;
    for number in 1..=3 {
        let alone = json_of(
            get(
                cluster.node(number),
                "/stats/topics/orders?scope=node",
                Some(ROOT),
            )
            .await,
            200,
        )
        .await;
        sum += alone["visibleMessages"].as_u64().unwrap();
    }
    assert_eq!(sum, 12);

    // The groups: each group once, with its messages from every node.
    let groups = json_of(
        get(cluster.node(2), "/stats/topics/orders/groups", Some(ROOT)).await,
        200,
    )
    .await;
    let items = groups["items"].as_array().unwrap();
    assert_eq!(items.len(), 3);
    assert!(items.iter().all(|g| g["messages"] == 4), "{items:?}");
    cluster.shutdown().await;
}

/// UC: UC-OPS-050
#[tokio::test]
async fn a_node_that_does_not_answer_makes_the_consolidated_reads_partial_and_says_which() {
    let cluster = TestCluster::start_with(3, &[("TABA_AUTH_ROOT_TOKEN", ROOT)]).await;
    let first = cluster.node(1);
    queue(first, "orders").await;
    for n in 0..12u32 {
        publish(cluster.node(1 + (n as usize % 3)), "orders", n).await;
    }
    let third_id = cluster.node_id(3);
    cluster.network.isolate(&common::cluster::host_of(3));

    let missing = |body: &Value| -> Vec<(String, String)> {
        body["missingNodes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| {
                (
                    m["nodeId"].as_str().unwrap().to_owned(),
                    m["reason"].as_str().unwrap().to_owned(),
                )
            })
            .collect()
    };

    let detail = json_of(get(first, "/stats/topics/orders", Some(ROOT)).await, 200).await;
    assert_eq!(detail["partial"], true);
    let gone = missing(&detail);
    assert_eq!(gone.len(), 1);
    assert_eq!(gone[0].0, third_id);
    assert!(["timeout", "unreachable"].contains(&gone[0].1.as_str()));
    assert!(detail["visibleMessages"].as_u64().unwrap() <= 12);

    let list = json_of(get(first, "/stats/topics", Some(ROOT)).await, 200).await;
    assert_eq!(list["partial"], true);
    assert_eq!(missing(&list)[0].0, third_id);

    let nodes = json_of(get(first, "/nodes", Some(ROOT)).await, 200).await;
    assert_eq!(nodes["partial"], true);
    assert_eq!(missing(&nodes)[0].0, third_id);

    let status = json_of(get(first, "/cluster/status", Some(ROOT)).await, 200).await;
    assert_eq!(status["partial"], true);
    assert_eq!(missing(&status)[0].0, third_id);

    // The node that returns completes the reads again.
    cluster.network.restore(&common::cluster::host_of(3));
    let again = json_of(get(first, "/stats/topics/orders", Some(ROOT)).await, 200).await;
    assert_eq!(again["partial"], false);
    assert_eq!(again["visibleMessages"], 12);
    cluster.shutdown().await;
}

/// UC: UC-OPS-051
#[tokio::test]
async fn the_cluster_status_carries_the_load_of_each_node_and_the_backup_is_consolidated() {
    use taba_node::storage::backup::MemoryStore;
    let s3 = std::sync::Arc::new(MemoryStore::default());
    let cluster = TestCluster::start_with_backup(3, &[("TABA_AUTH_ROOT_TOKEN", ROOT)], s3).await;
    let first = cluster.node(1);

    let status = json_of(get(first, "/cluster/status", Some(ROOT)).await, 200).await;
    for node in status["nodes"].as_array().unwrap() {
        let runtime = &node["status"]["runtime"];
        assert!(runtime["activeRequests"].is_number(), "{runtime}");
        assert!(runtime["rateLimitRejected"].is_number());
        assert!(node["status"]["storage"]["memoryUsedBytes"].is_number());
    }

    let backup = json_of(get(first, "/cluster/backup", Some(ROOT)).await, 200).await;
    assert_eq!(backup["partial"], false);
    let nodes = backup["nodes"].as_array().unwrap();
    assert_eq!(nodes.len(), 3);
    assert!(
        nodes
            .iter()
            .all(|n| n["s3Reachable"] == true && n["nodeId"].is_string())
    );
    cluster.shutdown().await;
}

/// UC: UC-OPS-051
#[tokio::test]
async fn without_a_backup_the_cluster_backup_route_says_so() {
    let cluster = TestCluster::start_with(2, &[("TABA_AUTH_ROOT_TOKEN", ROOT)]).await;
    let response = get(cluster.node(1), "/cluster/backup", Some(ROOT)).await;
    assert_eq!(error_code(response, 422).await, "backup.not-configured");
    cluster.shutdown().await;
}

/// A node that left by drain shows in `GET /nodes` of **every** node, whichever one receives the call: a node that
/// returns from a drain comes back as a new node with an empty memory, and the answer still has to be the same.
#[tokio::test]
async fn a_drained_node_shows_in_the_list_of_every_node_even_one_that_came_back_from_a_drain() {
    let cluster = TestCluster::start_with(4, &[("TABA_AUTH_ROOT_TOKEN", ROOT)]).await;
    let (second, third) = (cluster.node(2), cluster.node(3));
    let (second_id, third_id) = (cluster.node_id(2), cluster.node_id(3));

    for (node, id) in [(third, &third_id), (second, &second_id)] {
        let started = call(
            cluster.node(1),
            Method::POST,
            &format!("/nodes/{id}/drain"),
            Some(ROOT),
            None,
        )
        .await;
        assert_eq!(started.status(), 202);
        eventually_async("the node is drained", || async {
            let response = node.client_get_ops("/node/drain").await;
            response.json::<Value>().await.unwrap()["drainState"] == "drained"
        })
        .await;
    }
    // The second node comes back (new id, nothing remembered): it must still list the third as drained.
    let back = second
        .client
        .delete(second.operations_url("/node/drain"))
        .send()
        .await
        .unwrap();
    assert_eq!(back.status(), 202);
    eventually_async("the second node is back", || async {
        second.node.node_id() != second_id
            && cluster
                .table(1)
                .is_some_and(|t| t.member(&second.node.node_id()).is_some())
    })
    .await;

    for number in [1, 2, 4] {
        let nodes = json_of(get(cluster.node(number), "/nodes", Some(ROOT)).await, 200).await;
        let items = nodes["items"].as_array().unwrap();
        let state_of = |id: &str| {
            items
                .iter()
                .find(|n| n["nodeId"] == id)
                .map(|n| n["state"].clone())
        };
        assert_eq!(
            state_of(&third_id),
            Some(json!("drained")),
            "node {number} lost the drained node: {items:?}"
        );
        // The one that came back has another id: the old id is not listed as drained next to the new one.
        assert_eq!(state_of(&second_id), None, "node {number}: {items:?}");
    }
    cluster.shutdown().await;
}

async fn drain_node(node: &TestNode) {
    let started = node
        .client
        .post(node.operations_url("/node/drain"))
        .send()
        .await
        .unwrap();
    assert!(started.status().is_success());
    eventually_async("the node is drained", || async {
        let response = node.client_get_ops("/node/drain").await;
        response.json::<Value>().await.unwrap()["drainState"] == "drained"
    })
    .await;
}

/// With every node in drain the data API refuses, but the reads that say what the cluster is doing keep answering,
/// and they say the cluster is stopped; while another node serves, a drained node's answer is the cluster's.
#[tokio::test]
async fn a_cluster_with_every_node_in_drain_still_says_it_is_stopped() {
    let cluster = TestCluster::start_with(2, &[("TABA_AUTH_ROOT_TOKEN", ROOT)]).await;
    let (first, second) = (cluster.node(1), cluster.node(2));
    // One node still serves: the drained one answers with the cluster's picture.
    drain_node(second).await;
    let status = json_of(get(second, "/cluster/status", Some(ROOT)).await, 200).await;
    assert_eq!(status["summary"]["stopped"], false, "{status}");
    assert_eq!(status["summary"]["servingNodes"], 1, "{status}");

    // Nobody serves: the data API refuses, the state reads answer and say why.
    drain_node(first).await;
    let refused = get(first, "/topics", Some(ROOT)).await;
    assert_eq!(refused.status(), 503);
    for node in [first, second] {
        let resp = get(node, "/cluster/status", Some(ROOT)).await;
        let code = resp.status();
        let body = resp.text().await.unwrap();
        assert_eq!(code, 200, "{body}");
        let status: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(status["summary"]["stopped"], true, "{status}");
        assert_eq!(status["summary"]["servingNodes"], 0, "{status}");
        json_of(get(node, "/nodes", Some(ROOT)).await, 200).await;
        json_of(get(node, "/auth/me", Some(ROOT)).await, 200).await;
    }
    cluster.shutdown().await;
}
