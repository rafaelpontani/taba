//! A API de operação: o drain, `/health/cluster`, o filtro de IP e o estado de falha
//! (doc 06b; doc 04, seção 6.10).

mod common;

use std::time::Duration;

use common::{TestNode, error_code};
use serde_json::{Value, json};

async fn create_queue(node: &TestNode, name: &str) {
    let response = node
        .client
        .post(node.client_url("/topics"))
        .json(&json!({ "name": name, "type": "queue" }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 201);
}

async fn publish(node: &TestNode, topic: &str, body: &str) -> reqwest::Response {
    node.client
        .post(node.client_url(&format!("/topics/{topic}/messages")))
        .header("content-type", "application/json")
        .body(body.to_owned())
        .send()
        .await
        .unwrap()
}

async fn consume(node: &TestNode, topic: &str) -> Vec<Value> {
    let response = node
        .client_get(&format!("/topics/{topic}/messages?maxMessages=10"))
        .await;
    assert_eq!(response.status(), 200);
    response.json::<Value>().await.unwrap()["messages"]
        .as_array()
        .unwrap()
        .clone()
}

async fn drain_call(node: &TestNode, method: reqwest::Method) -> reqwest::Response {
    node.client
        .request(method, node.operations_url("/node/drain"))
        .send()
        .await
        .unwrap()
}

async fn wait_for_drain_state(node: &TestNode, state: &str) -> Value {
    for _ in 0..200 {
        let status: Value = drain_call(node, reqwest::Method::GET)
            .await
            .json()
            .await
            .unwrap();
        if status["drainState"] == state {
            return status;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("o nó não chegou a `{state}`");
}

/// UC: UC-DRN-001, UC-DRN-004, UC-DRN-005, UC-DRN-007, UC-OPS-004
#[tokio::test]
async fn the_api_drain_takes_the_node_out_of_rotation_and_saves_the_memory() {
    let node = TestNode::development().await;
    create_queue(&node, "pedidos").await;
    assert_eq!(publish(&node, "pedidos", "1").await.status(), 201);

    let idle: Value = drain_call(&node, reqwest::Method::GET)
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(
        idle,
        json!({ "drainState": "active", "trigger": null, "startedAt": null, "pendingMessages": 1 })
    );

    let started = drain_call(&node, reqwest::Method::POST).await;
    assert_eq!(started.status(), 202);
    let started: Value = started.json().await.unwrap();
    assert_eq!(started["trigger"], "api");
    assert_eq!(started["startedAt"], "2026-10-06T12:00:00.000Z");
    let done = wait_for_drain_state(&node, "drained").await;
    assert_eq!(done["pendingMessages"], 0);
    assert!(
        node.storage_dir()
            .join("pedidos")
            .join("_topic.db")
            .is_file()
    );

    // Fora da rotação: pronto cai a 503 e vivo segue 200.
    let ready = node.client_get("/health/ready").await;
    assert_eq!(ready.status(), 503);
    assert_eq!(
        ready.json::<Value>().await.unwrap(),
        json!({ "health": "not-ready", "reason": "drained" })
    );
    assert_eq!(node.client_get("/health/live").await.status(), 200);

    // As chamadas de cliente não são atendidas: 503 com Retry-After.
    let refused = node.client_get("/topics/pedidos/messages").await;
    assert_eq!(refused.headers()["retry-after"], "5");
    assert_eq!(error_code(refused, 503).await, "cluster.node-draining");
    assert_eq!(
        error_code(publish(&node, "pedidos", "2").await, 503).await,
        "cluster.node-draining"
    );

    // Iniciar de novo: 200, sem efeito.
    let again = drain_call(&node, reqwest::Method::POST).await;
    assert_eq!(again.status(), 200);
    assert_eq!(
        again.json::<Value>().await.unwrap()["drainState"],
        "drained"
    );
    node.shutdown().await;
}

/// UC: UC-DRN-016, UC-DRN-019, UC-DRN-021
#[tokio::test]
async fn deleting_the_drain_brings_the_node_back_with_its_messages() {
    let node = TestNode::development().await;
    create_queue(&node, "pedidos").await;
    publish(&node, "pedidos", r#"{ "n": 1 }"#).await;

    let nothing = drain_call(&node, reqwest::Method::DELETE).await;
    assert_eq!(nothing.status(), 200, "sem drain, nada a cancelar");
    assert_eq!(
        nothing.json::<Value>().await.unwrap()["drainState"],
        "active"
    );

    drain_call(&node, reqwest::Method::POST).await;
    wait_for_drain_state(&node, "drained").await;
    let back = drain_call(&node, reqwest::Method::DELETE).await;
    assert_eq!(back.status(), 200);
    assert_eq!(back.json::<Value>().await.unwrap()["drainState"], "active");

    assert_eq!(node.client_get("/health/ready").await.status(), 200);
    let messages = consume(&node, "pedidos").await;
    assert_eq!(messages.len(), 1, "o que foi ao disco continua entregável");
    assert_eq!(messages[0]["body"], json!({ "n": 1 }));
    // Voltou a gravar em memória: a nova mensagem não vai ao disco.
    publish(&node, "pedidos", "2").await;
    let status: Value = drain_call(&node, reqwest::Method::GET)
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(status["pendingMessages"], 1);
    node.shutdown().await;
}

/// UC: UC-DRN-006, UC-OPS-035
#[tokio::test]
async fn the_operations_routes_only_accept_the_allowed_ips() {
    let node = TestNode::builder()
        .env("TABA_OPERATIONS_ALLOWED_IPS", "10.0.0.0/8, 192.168.1.10")
        .start()
        .await;
    // Só a lista: as rotas de operação ficam na porta da API, e a API de cliente segue aberta.
    assert_eq!(node.node.operations_addr(), None);
    for path in [
        "/health/live",
        "/health/ready",
        "/health/cluster",
        "/node/drain",
    ] {
        assert_eq!(
            error_code(node.client_get(path).await, 403).await,
            "ops.ip-not-allowed",
            "{path}"
        );
    }
    assert_eq!(
        error_code(
            node.client
                .post(node.client_url("/node/drain"))
                .send()
                .await
                .unwrap(),
            403
        )
        .await,
        "ops.ip-not-allowed"
    );
    assert_eq!(node.client_get("/topics").await.status(), 200);
    node.shutdown().await;

    let node = TestNode::builder()
        .env("TABA_OPERATIONS_ALLOWED_IPS", "127.0.0.0/8")
        .start()
        .await;
    assert_eq!(node.client_get("/health/ready").await.status(), 200);
    assert_eq!(node.client_get("/node/drain").await.status(), 200);
    node.shutdown().await;
}

/// UC: UC-OPS-035
#[tokio::test]
async fn with_an_operations_port_the_client_port_does_not_serve_the_operations_routes() {
    let node = TestNode::builder()
        .env("TABA_OPERATIONS_PORT", "0")
        .env("TABA_OPERATIONS_ALLOWED_IPS", "10.0.0.0/8")
        .start()
        .await;
    assert_eq!(
        error_code(node.client_get("/health/live").await, 404).await,
        "route.not-found"
    );
    let ops = node
        .client
        .get(node.operations_url("/health/live"))
        .send()
        .await
        .unwrap();
    assert_eq!(
        error_code(ops, 403).await,
        "ops.ip-not-allowed",
        "porta própria e filtro"
    );
    node.shutdown().await;
}

/// UC: UC-OPS-017, UC-OPS-018
#[tokio::test]
async fn cluster_health_describes_the_single_node() {
    let node = TestNode::builder()
        .env("TABA_MEMORY_LIMIT", "1000")
        .start()
        .await;
    create_queue(&node, "pedidos").await;
    publish(&node, "pedidos", "1").await;

    for scope in ["", "?scope=node", "?scope=cluster"] {
        let response = node.client_get(&format!("/health/cluster{scope}")).await;
        assert_eq!(response.status(), 200, "{scope}");
        let body: Value = response.json().await.unwrap();
        assert_eq!(body["partial"], false);
        assert_eq!(body["worstCondition"], "ok");
        assert_eq!(body["conditions"], json!([]));
        assert_eq!(body["summary"]["members"], json!({ "active": 1 }));
        let nodes = body["nodes"].as_array().unwrap();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0]["reachable"], true);
        assert_eq!(nodes[0]["nodeId"], node.node.node_id());
        let status = &nodes[0]["status"];
        assert_eq!(status["mode"], "development");
        assert_eq!(status["state"], "active");
        assert_eq!(status["drainState"], "active");
        assert_eq!(status["storage"]["memoryMessages"], 1);
        assert_eq!(status["storage"]["memoryUsedBytes"], 1);
        assert_eq!(status["storage"]["memoryLimitBytes"], 1000);
        assert_eq!(status["storage"]["diskMessages"], 0);
        assert_eq!(status["signals"]["quorumAvailable"], true);
    }
    assert_eq!(
        node.client_get("/health/cluster?scope=node")
            .await
            .json::<Value>()
            .await
            .unwrap()["scope"],
        "node"
    );

    let invalid = node.client_get("/health/cluster?scope=outro").await;
    assert_eq!(error_code(invalid, 400).await, "validation.failed");
    node.shutdown().await;
}

/// UC: UC-OPS-002, UC-FAL-023
#[tokio::test]
async fn a_corrupt_topic_database_fails_the_node() {
    let node = TestNode::builder()
        .env("TABA_MEMORY_LIMIT", "1")
        .start()
        .await;
    create_queue(&node, "pedidos").await;
    let folder = node.storage_dir().join("pedidos");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("_topic.db"), vec![0xAB; 4096]).unwrap();

    // O corpo passa de `memory.limit`: vai ao disco e acha o banco corrompido.
    let response = publish(&node, "pedidos", r#"{ "grande": true }"#).await;
    assert_eq!(error_code(response, 500).await, "internal.error");

    let ready = node.client_get("/health/ready").await;
    assert_eq!(ready.status(), 503);
    assert_eq!(
        ready.json::<Value>().await.unwrap(),
        json!({ "health": "not-ready", "reason": "failed" })
    );
    assert_eq!(node.client_get("/health/live").await.status(), 200);
    let status: Value = node
        .client_get("/health/cluster")
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(status["nodes"][0]["status"]["state"], "failed");
    node.shutdown().await;
}

fn save_mark(storage: &std::path::Path) -> Option<u64> {
    use std::sync::Arc;
    use taba_node::config::DbSynchronous;
    use taba_node::storage::db::NodeDb;
    use taba_node::storage::state::NodeStateStore;

    let db = Arc::new(NodeDb::open(storage, DbSynchronous::Normal).unwrap());
    NodeStateStore::new(db).take_save_mark().unwrap()
}

/// UC: UC-DRN-022, UC-DRN-023, UC-DRN-026
#[tokio::test]
async fn the_drain_writes_the_save_mark_and_the_next_start_consumes_it() {
    let tmp = tempfile::tempdir().unwrap();
    let storage = tmp.path().join("taba");
    let start = |drain_on_stop: &str| {
        TestNode::builder()
            .env("TABA_STORAGE_DIR", &storage.display().to_string())
            .env("TABA_DRAIN_ON_STOP", drain_on_stop)
            .try_start()
    };

    let node = start("true").await.unwrap();
    create_queue(&node, "pedidos").await;
    publish(&node, "pedidos", "1").await;
    node.shutdown().await;
    assert!(
        save_mark(&storage).is_some(),
        "o drain deixou a marca do save"
    );

    // A partida consome a marca: uma queda depois dela não passa por save limpo.
    taba_node::storage::state::NodeStateStore::new(std::sync::Arc::new(
        taba_node::storage::db::NodeDb::open(&storage, taba_node::config::DbSynchronous::Normal)
            .unwrap(),
    ))
    .mark_save_completed(1)
    .unwrap();
    let node = start("false").await.unwrap();
    node.shutdown().await;
    assert_eq!(
        save_mark(&storage),
        None,
        "sem drain na parada não há marca"
    );
}
