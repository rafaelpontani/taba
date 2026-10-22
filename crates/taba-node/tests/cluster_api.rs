//! The cluster API through the cluster port, over real HTTP (doc 06c, "Members and channel"). The
//! join between nodes uses the in-memory network; what is seen here is the routes' contract: the format
//! of the bodies, the statuses and who may call.

mod common;

use std::sync::Arc;

use serde_json::{Value, json};

use common::cluster::TestCluster;
use common::error_code;
use taba_node::cluster::protocol::{Request, Response};
use taba_node::cluster::transport::{BoxFuture, Handler, PeerAddr};
use taba_node::core::error::{ErrorCode, TabaError};
use taba_node::core::id;

fn guid() -> String {
    id::new_at(1_791_288_000_000)
}

/// A node that does not have the table yet: serves everything with `503 cluster.node-syncing`, like a node
/// that just started. It serves as the "new node" that the member can reach.
struct JustStarted;

impl Handler for JustStarted {
    fn handle<'a>(
        &'a self,
        _caller: &'a str,
        _request: Request,
    ) -> BoxFuture<'a, Result<Response, TabaError>> {
        Box::pin(async {
            Err(TabaError::new(
                ErrorCode::ClusterNodeSyncing,
                "still without the table",
            ))
        })
    }
}

/// UC: UC-SEG-002, UC-ING-002
#[tokio::test]
async fn the_cluster_port_serves_only_cluster_routes_and_the_client_port_none() {
    let cluster = TestCluster::start(1).await;
    let node = cluster.node(1);

    let response = node
        .client
        .get(node.cluster_url("/cluster/nodes"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert!(response.headers().contains_key("x-request-id"));
    let table: Value = response.json().await.unwrap();
    assert_eq!(table["version"], 1);
    let member = &table["members"][0];
    assert_eq!(member["nodeId"], cluster.node_id(1));
    assert_eq!(member["addresses"], json!(["10.0.0.1"]));
    assert_eq!(member["port"], node.node.cluster_addr().port());
    assert_eq!(member["state"], "active");

    // The cluster port answers 404 to what is not /cluster/*, and the API's to what is.
    let response = node
        .client
        .get(node.cluster_url("/topics"))
        .send()
        .await
        .unwrap();
    assert_eq!(error_code(response, 404).await, "route.not-found");
    let response = node
        .client
        .get(node.client_url("/cluster/nodes"))
        .send()
        .await
        .unwrap();
    assert_eq!(error_code(response, 404).await, "route.not-found");
    // A cluster route that does not exist.
    let response = node
        .client
        .get(node.cluster_url("/cluster/nothing"))
        .send()
        .await
        .unwrap();
    assert_eq!(error_code(response, 404).await, "route.not-found");
    cluster.shutdown().await;
}

/// UC: UC-ING-003, UC-ING-010
#[tokio::test]
async fn the_join_route_registers_the_node_and_answers_with_the_table() {
    let cluster = TestCluster::start(1).await;
    let node = cluster.node(1);
    // The "new node" is on the in-memory network, and the member can connect back to it.
    cluster
        .network
        .register(PeerAddr::new("10.0.0.77", 9100), Arc::new(JustStarted));
    let new_id = guid();

    let response = node
        .client
        .post(node.cluster_url("/cluster/nodes/join"))
        .json(&json!({
            "nodeId": new_id,
            "addresses": ["10.0.0.77"],
            "port": 9100,
            "version": "0.9.0"
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
    let table: Value = response.json().await.unwrap();
    assert_eq!(table["version"], 2, "the table already carries the commit");
    let ids: Vec<&str> = table["members"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["nodeId"].as_str().unwrap())
        .collect();
    assert_eq!(ids.len(), 2);
    assert!(ids.contains(&new_id.as_str()));
    // The member sees the same table afterwards.
    assert_eq!(cluster.member_ids(1).len(), 2);
    cluster.shutdown().await;
}

/// UC: UC-ING-014
#[tokio::test]
async fn the_bodies_of_the_cluster_routes_are_validated() {
    let cluster = TestCluster::start(1).await;
    let node = cluster.node(1);
    let join = node.cluster_url("/cluster/nodes/join");

    // A missing field.
    let response = node
        .client
        .post(&join)
        .json(&json!({"nodeId": guid()}))
        .send()
        .await
        .unwrap();
    assert_eq!(error_code(response, 400).await, "validation.required");

    // A field the route does not define.
    let response = node
        .client
        .post(&join)
        .json(&json!({
            "nodeId": guid(), "addresses": ["10.0.0.77"], "port": 9100,
            "version": "0.9.0", "extra": true
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(error_code(response, 400).await, "validation.unknown-field");

    // Invalid value: the `nodeId` is not a GUID v7.
    let response = node
        .client
        .post(&join)
        .json(&json!({
            "nodeId": "n1", "addresses": ["10.0.0.77"], "port": 9100, "version": "0.9.0"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(error_code(response, 400).await, "validation.failed");

    // Another `Content-Type`.
    let response = node
        .client
        .post(&join)
        .header("content-type", "text/plain")
        .body("{}")
        .send()
        .await
        .unwrap();
    assert_eq!(error_code(response, 415).await, "media.unsupported");

    cluster.shutdown().await;
}

/// UC: UC-SEG-018
#[tokio::test]
async fn a_caller_that_is_not_a_member_is_refused_on_the_phase_routes() {
    let cluster = TestCluster::start(1).await;
    let node = cluster.node(1);
    // The test calls from 127.0.0.1, which is not in the table (the node has the IP 10.0.0.1).
    for route in ["commit", "rollback"] {
        let response = node
            .client
            .post(node.cluster_url(&format!("/cluster/nodes/{route}")))
            .json(&json!({"operationId": guid()}))
            .send()
            .await
            .unwrap();
        assert_eq!(
            error_code(response, 403).await,
            "cluster.not-member",
            "{route}"
        );
    }
    let response = node
        .client
        .post(node.cluster_url("/cluster/nodes/register"))
        .json(&json!({
            "operationId": guid(),
            "coordinatorId": guid(),
            "node": {"nodeId": guid(), "addresses": ["10.0.0.9"], "port": 8081, "state": "active"}
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(error_code(response, 403).await, "cluster.not-member");
    cluster.shutdown().await;
}

/// UC: UC-ING-006
#[tokio::test]
async fn a_node_without_the_table_answers_503_with_retry_after() {
    let mut cluster = TestCluster::empty();
    let builder = cluster.builder(1, false);
    cluster.spawn_node(builder).await.unwrap();
    let node = cluster.node(1);

    let response = node
        .client
        .get(node.cluster_url("/cluster/nodes"))
        .send()
        .await
        .unwrap();
    assert!(response.headers().contains_key("retry-after"));
    assert_eq!(error_code(response, 503).await, "cluster.node-syncing");
    cluster.shutdown().await;
}
