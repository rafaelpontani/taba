//! A API do cluster pela porta do cluster, em HTTP de verdade (doc 06c, "Membros e canal"). O
//! ingresso entre nós usa a rede em memória; aqui o que se vê é o contrato das rotas: o formato
//! dos corpos, os status e quem pode chamar.

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

/// Um nó que ainda não tem a tabela: atende tudo com `503 cluster.node-syncing`, como um nó
/// que acabou de subir. Serve de "nó novo" que o membro consegue alcançar.
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
                "ainda sem a tabela",
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
    assert_eq!(member["port"], node.node.cluster_addr().unwrap().port());
    assert_eq!(member["state"], "active");

    // A porta do cluster responde 404 ao que não é /cluster/*, e a da API ao que é.
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
    // Rota do cluster que não existe.
    let response = node
        .client
        .get(node.cluster_url("/cluster/nada"))
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
    // O "nó novo" está na rede em memória, e o membro consegue conectar de volta a ele.
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
    assert_eq!(table["version"], 2, "a tabela já traz o commit");
    let ids: Vec<&str> = table["members"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["nodeId"].as_str().unwrap())
        .collect();
    assert_eq!(ids.len(), 2);
    assert!(ids.contains(&new_id.as_str()));
    // O membro vê a mesma tabela depois.
    assert_eq!(cluster.member_ids(1).len(), 2);
    cluster.shutdown().await;
}

/// UC: UC-ING-014
#[tokio::test]
async fn the_bodies_of_the_cluster_routes_are_validated() {
    let cluster = TestCluster::start(1).await;
    let node = cluster.node(1);
    let join = node.cluster_url("/cluster/nodes/join");

    // Campo que falta.
    let response = node
        .client
        .post(&join)
        .json(&json!({"nodeId": guid()}))
        .send()
        .await
        .unwrap();
    assert_eq!(error_code(response, 400).await, "validation.required");

    // Campo que a rota não define.
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

    // Valor inválido: o `nodeId` não é um GUID v7.
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

    // Outro `Content-Type`.
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
    // O teste chama de 127.0.0.1, que não consta na tabela (o nó tem o IP 10.0.0.1).
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
