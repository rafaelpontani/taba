//! The message operations between nodes in batches (doc 18, `POST /cluster/batch`; D186):
//! - level **C** (simulated cluster): concurrent operations to the same node travel in fewer calls than operations,
//!   and `cluster.batch.max-items=1` turns the batch off;
//! - level **I** (real nodes over TCP and mutual TLS): the route's contract, with a batch written by hand.

mod common;

use prost::Message;
use serde_json::{Value, json};

use common::cluster::TestCluster;
use common::real::{RealCluster, cert_dir, client_of, ip_of};
use taba_node::cluster::protocol::wire::{self, BatchReply, BatchRequest, Item, Kind, Phase};

const OPERATION: &str = "01900000-0000-7000-8000-000000000001";
const MESSAGE: &str = "01900000-0000-7000-8000-000000000002";

/// A commit of an operation nobody registered: the node answers `404 cluster.operation-unknown` for it.
fn unknown_commit(kind: Kind) -> Item {
    Item {
        kind: kind as i32,
        phase: Phase::Commit as i32,
        topic: "orders".to_owned(),
        id: MESSAGE.to_owned(),
        operation_id: OPERATION.to_owned(),
        body: None,
        traceparent: None,
    }
}

async fn publish_concurrently(cluster: &TestCluster, count: usize) {
    let node = cluster.node(1);
    let url = node.client_url("/topics/orders/messages");
    let mut tasks = Vec::new();
    for n in 0..count {
        let client = node.client.clone();
        let url = url.clone();
        tasks.push(tokio::spawn(async move {
            client
                .post(url)
                .json(&json!({ "n": n }))
                .send()
                .await
                .expect("request")
                .status()
        }));
    }
    for task in tasks {
        assert_eq!(task.await.unwrap(), 201);
    }
}

async fn create_queue(cluster: &TestCluster) {
    let node = cluster.node(1);
    let response = node
        .client
        .post(node.client_url("/topics"))
        .json(&json!({"name": "orders", "type": "queue"}))
        .send()
        .await
        .expect("request");
    assert_eq!(response.status(), 201);
}

/// UC: UC-ESC-039
#[tokio::test(flavor = "multi_thread")]
async fn operations_waiting_for_the_same_node_travel_in_fewer_calls_than_operations() {
    let cluster = TestCluster::start(3).await;
    create_queue(&cluster).await;
    cluster.network.clear_calls();

    publish_concurrently(&cluster, 300).await;

    let operations =
        cluster.network.count("write-register") + cluster.network.count("write-commit");
    let batches = cluster.network.count("batch");
    assert!(
        operations >= 600,
        "each publication registers and commits on other nodes: {operations}"
    );
    assert!(
        batches < operations,
        "{operations} operations went in {batches} calls: nothing was batched"
    );
    cluster.shutdown().await;
}

/// UC: UC-ESC-039
#[tokio::test(flavor = "multi_thread")]
async fn with_max_items_1_every_operation_goes_in_a_call_of_its_own() {
    let cluster = TestCluster::start_with(3, &[("TABA_CLUSTER_BATCH_MAX_ITEMS", "1")]).await;
    create_queue(&cluster).await;
    cluster.network.clear_calls();

    publish_concurrently(&cluster, 60).await;

    let operations =
        cluster.network.count("write-register") + cluster.network.count("write-commit");
    assert!(operations >= 120, "{operations}");
    // Each operation is a batch of one item (the single-operation form is never sent on its own).
    assert_eq!(cluster.network.count("batch"), operations);
    cluster.shutdown().await;
}

fn protobuf(
    client: &reqwest::Client,
    addr: std::net::SocketAddr,
    body: Vec<u8>,
) -> reqwest::RequestBuilder {
    client
        .post(format!("https://{addr}/cluster/batch"))
        .header("content-type", wire::CONTENT_TYPE)
        .body(body)
}

fn member_client(certs: &std::path::Path, as_node: usize) -> reqwest::Client {
    let (config, _) = client_of(certs);
    reqwest::Client::builder()
        .use_preconfigured_tls(config)
        .local_address(ip_of(as_node).parse::<std::net::IpAddr>().unwrap())
        .no_proxy()
        .build()
        .unwrap()
}

/// UC: UC-CLR-012
#[tokio::test(flavor = "multi_thread")]
async fn the_batch_route_answers_one_result_per_item_and_an_error_in_one_does_not_touch_the_others()
{
    let certs = cert_dir();
    let mut cluster = RealCluster::start(0, Some(certs.path())).await;
    cluster.add_node(&[]).await;
    cluster.add_node(&[]).await;
    cluster.settled(2).await;
    let addr = cluster.cluster_addr(1);
    let client = member_client(certs.path(), 2);

    let batch = BatchRequest {
        items: vec![
            unknown_commit(Kind::Write),
            // No kind at all: an error of this item only.
            Item {
                kind: Kind::Unspecified as i32,
                ..unknown_commit(Kind::Write)
            },
            unknown_commit(Kind::Ack),
            unknown_commit(Kind::Visibility),
        ],
    };
    let response = protobuf(&client, addr, batch.encode_to_vec())
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(
        response.headers()["content-type"],
        wire::CONTENT_TYPE,
        "the reply is protobuf too"
    );
    let reply = BatchReply::decode(response.bytes().await.unwrap()).unwrap();
    let codes: Vec<(u32, String)> = reply
        .results
        .iter()
        .map(|r| {
            (
                r.status,
                r.error.as_ref().map(|e| e.code.clone()).unwrap_or_default(),
            )
        })
        .collect();
    assert_eq!(codes.len(), 4, "one result per item: {codes:?}");
    assert_eq!(codes[0].1, "cluster.operation-unknown");
    assert_eq!(codes[0].0, 404);
    assert_eq!(
        codes[1].1, "validation.required",
        "the unreadable item is its own error"
    );
    assert_eq!(codes[2].1, "cluster.operation-unknown");
    assert_eq!(codes[3].1, "cluster.operation-unknown");

    // An empty batch is a valid batch.
    let empty = protobuf(&client, addr, BatchRequest::default().encode_to_vec())
        .send()
        .await
        .unwrap();
    assert_eq!(empty.status(), 200);
    assert!(
        BatchReply::decode(empty.bytes().await.unwrap())
            .unwrap()
            .results
            .is_empty()
    );
    cluster.shutdown().await;
}

/// UC: UC-CLR-012
#[tokio::test(flavor = "multi_thread")]
async fn the_batch_route_refuses_what_is_not_protobuf_and_what_is_not_a_batch() {
    let certs = cert_dir();
    let mut cluster = RealCluster::start(0, Some(certs.path())).await;
    cluster.add_node(&[]).await;
    cluster.add_node(&[]).await;
    cluster.settled(2).await;
    let addr = cluster.cluster_addr(1);
    let client = member_client(certs.path(), 2);

    let json_body = client
        .post(format!("https://{addr}/cluster/batch"))
        .header("content-type", "application/json")
        .body(r#"{"items":[]}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(json_body.status(), 415);
    let body: Value = json_body.json().await.unwrap();
    assert_eq!(body["code"], "media.unsupported");

    let garbage = protobuf(
        &client,
        addr,
        vec![0x0a, 0xff, 0xff, 0xff, 0xff, 0x0f, 0x01],
    )
    .send()
    .await
    .unwrap();
    assert_eq!(garbage.status(), 400);
    let body: Value = garbage.json().await.unwrap();
    assert_eq!(body["code"], "validation.failed");

    // The routes of one operation are gone from the cluster port.
    for path in [
        format!("/cluster/topics/orders/messages/{MESSAGE}/commit"),
        format!("/cluster/topics/orders/messages/{MESSAGE}/register"),
        format!("/cluster/topics/orders/messages/{MESSAGE}/ack/commit"),
        format!("/cluster/topics/orders/messages/{MESSAGE}/visibility/rollback"),
    ] {
        let old = client
            .post(format!("https://{addr}{path}"))
            .json(&json!({"operationId": OPERATION}))
            .send()
            .await
            .unwrap();
        assert_eq!(old.status(), 404, "{path}");
        let body: Value = old.json().await.unwrap();
        assert_eq!(body["code"], "route.not-found", "{path}");
    }
    cluster.shutdown().await;
}

/// UC: UC-CLR-012
#[tokio::test(flavor = "multi_thread")]
async fn a_batch_with_more_items_than_the_limit_is_refused_whole() {
    let certs = cert_dir();
    let mut cluster = RealCluster::start(0, Some(certs.path())).await;
    let env = [("TABA_CLUSTER_BATCH_MAX_ITEMS", "3")];
    cluster.add_node(&env).await;
    cluster.add_node(&env).await;
    cluster.settled(2).await;
    let addr = cluster.cluster_addr(1);
    let client = member_client(certs.path(), 2);

    let at_the_limit = BatchRequest {
        items: vec![unknown_commit(Kind::Write); 3],
    };
    let ok = protobuf(&client, addr, at_the_limit.encode_to_vec())
        .send()
        .await
        .unwrap();
    assert_eq!(ok.status(), 200);

    let over = BatchRequest {
        items: vec![unknown_commit(Kind::Write); 4],
    };
    let refused = protobuf(&client, addr, over.encode_to_vec())
        .send()
        .await
        .unwrap();
    assert_eq!(refused.status(), 413);
    let body: Value = refused.json().await.unwrap();
    assert_eq!(body["code"], "message.too-large");
    cluster.shutdown().await;
}
