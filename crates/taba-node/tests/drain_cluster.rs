//! O drain num cluster (doc 04, seções 6.9 e 6.10; doc 20c, DRN e FWD; nível **C**): o nó avisa os
//! pares, repassa as chamadas de cliente ao nó mais livre, sai do cluster sem votação e pode voltar
//! como nó novo.

mod common;

use std::time::Duration;

use serde_json::{Value, json};

use common::cluster::{NO_VOTE, TestCluster, eventually, eventually_async};
use common::{TestNode, error_code};

async fn drain_state(node: &TestNode) -> String {
    let response = node.client_get_ops("/node/drain").await;
    let body: Value = response.json().await.unwrap();
    body["drainState"].as_str().unwrap().to_owned()
}

async fn start_drain(node: &TestNode) {
    let response = node
        .client
        .post(node.operations_url("/node/drain"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 202);
}

async fn wait_drain_state(node: &TestNode, wanted: &str) {
    eventually_async(&format!("drainState {wanted}"), || async {
        drain_state(node).await == wanted
    })
    .await;
}

async fn publish(node: &TestNode, topic: &str, body: Value) -> String {
    let response = node
        .client
        .post(node.client_url(&format!("/topics/{topic}/messages")))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 201, "{:?}", response.text().await);
    response.json::<Value>().await.unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

async fn consume_all(node: &TestNode, topic: &str, expected: usize) -> Vec<Value> {
    let mut got: Vec<Value> = Vec::new();
    for _ in 0..80 {
        let response = node
            .client
            .get(node.client_url(&format!(
                "/topics/{topic}/messages?maxMessages=10&visibilityTimeout=300"
            )))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200, "{:?}", response.text().await);
        let body: Value = response.json().await.unwrap();
        got.extend(body["messages"].as_array().cloned().unwrap_or_default());
        if got.len() >= expected {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    got
}

/// UC: UC-DRN-007, UC-DRN-008, UC-DRN-011, UC-DRN-012, UC-FRZ-006, UC-FWD-001, UC-FWD-004, UC-FWD-009
#[tokio::test]
async fn a_draining_node_forwards_client_calls_and_leaves_the_cluster_without_a_vote() {
    let cluster = TestCluster::start(3).await;
    let node3 = cluster.node(3);
    node3
        .client
        .post(node3.client_url("/topics"))
        .json(&json!({"name": "pedidos", "type": "queue"}))
        .send()
        .await
        .unwrap();
    let mut published = Vec::new();
    for n in 0..12 {
        published.push(publish(cluster.node(1), "pedidos", json!({"n": n})).await);
    }
    let version_before = cluster.table(1).unwrap().version;

    start_drain(node3).await;
    wait_drain_state(node3, "drained").await;

    // Saiu da tabela dos outros por desregistro de duas fases (`reason: drain`), sem votação.
    for number in [1, 2] {
        let table = cluster.table(number).unwrap();
        assert_eq!(table.members.len(), 2, "nó {number}");
        assert_eq!(table.version, version_before + 1);
        assert!(!cluster.member_ids(number).contains(&cluster.node_id(3)));
    }
    assert!(cluster.network.count("unregister") >= 1);
    assert_eq!(cluster.network.count("rollback"), 0);

    // Os pares foram avisados antes de qualquer outra coisa: nenhum `take` vai mais a ele.
    let ready = node3.client_get_ops("/health/ready").await;
    assert_eq!(ready.status(), 503);
    assert_eq!(ready.json::<Value>().await.unwrap()["reason"], "drained");

    // As chamadas de cliente que ainda chegam a ele são repassadas, inclusive a publicação.
    let listed = node3
        .client
        .get(node3.client_url("/topics/pedidos"))
        .send()
        .await
        .unwrap();
    assert_eq!(listed.status(), 200);
    let id = publish(node3, "pedidos", json!({"depois": true})).await;
    published.push(id);
    assert!(cluster.network.count("forward") >= 2);

    // As mensagens de que ele era o dono ficaram com o novo dono, que já tinha a cópia.
    cluster.network.clear_calls();
    let got = consume_all(cluster.node(1), "pedidos", published.len()).await;
    let mut ids: Vec<String> = got
        .iter()
        .map(|m| m["id"].as_str().unwrap().to_owned())
        .collect();
    ids.sort();
    let mut expected = published.clone();
    expected.sort();
    assert_eq!(ids, expected);
    assert!(
        cluster
            .network
            .calls()
            .iter()
            .all(|c| !(c.request == "take" && c.to.host == "10.0.0.3")),
        "ninguém pede `take` a um nó fora do cluster"
    );

    // O `GET` repassado também funciona pelo nó em drain (o destino entrega).
    let via_drained = node3
        .client
        .get(node3.client_url("/topics/pedidos/messages?maxMessages=1"))
        .send()
        .await
        .unwrap();
    assert_eq!(via_drained.status(), 200);
    cluster.shutdown().await;
}

/// UC: UC-DRN-016
#[tokio::test]
async fn canceling_before_the_node_left_keeps_it_in_the_cluster() {
    let cluster = TestCluster::start_with(3, NO_VOTE).await;
    // Os outros não alcançam o nó 3: o desregistro não fecha e o drain fica em `draining`.
    cluster.network.isolate("10.0.0.3");
    let node3 = cluster.node(3);
    start_drain(node3).await;
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert_eq!(drain_state(node3).await, "draining");
    assert_eq!(cluster.table(3).unwrap().members.len(), 3);

    let cancel = node3
        .client
        .delete(node3.operations_url("/node/drain"))
        .send()
        .await
        .unwrap();
    assert_eq!(cancel.status(), 200);
    assert_eq!(drain_state(node3).await, "active");
    let ready = node3.client_get_ops("/health/ready").await;
    assert_eq!(ready.status(), 200);
    cluster.network.restore("10.0.0.3");
    cluster.shutdown().await;
}

/// UC: UC-DRN-018, UC-FRZ-006
#[tokio::test]
async fn after_leaving_the_node_returns_as_a_new_node_with_a_new_id() {
    let cluster = TestCluster::start(3).await;
    let node3 = cluster.node(3);
    let old_id = cluster.node_id(3);
    start_drain(node3).await;
    wait_drain_state(node3, "drained").await;

    let back = node3
        .client
        .delete(node3.operations_url("/node/drain"))
        .send()
        .await
        .unwrap();
    assert_eq!(back.status(), 202);

    // ID novo, bootstrap completo e a volta à tabela, sem freezing.
    eventually("o nó voltar ao cluster", || {
        let id = node3.node.node_id();
        id != old_id
            && cluster
                .table(1)
                .is_some_and(|t| t.members.len() == 3 && t.member(&id).is_some())
    })
    .await;
    assert_eq!(drain_state(node3).await, "active");
    assert!(!cluster.member_ids(1).contains(&old_id));
    eventually("o nó pronto de novo", || {
        node3.node.shared().state() == taba_node::cluster::state::NodeState::Active
    })
    .await;
    let ready = node3.client_get_ops("/health/ready").await;
    assert_eq!(ready.status(), 200);
    cluster.shutdown().await;
}

/// UC: UC-DRN-017
#[tokio::test]
async fn canceling_a_drain_that_is_leaving_is_a_conflict() {
    let cluster = TestCluster::start(3).await;
    // O commit do desregistro fica preso: a saída está decidida mas não termina.
    cluster.network.set_delay(Duration::from_millis(150));
    let node3 = cluster.node(3);
    start_drain(node3).await;
    let mut conflict = false;
    for _ in 0..40 {
        let response = node3
            .client
            .delete(node3.operations_url("/node/drain"))
            .send()
            .await
            .unwrap();
        if response.status() == 409 {
            assert_eq!(error_code(response, 409).await, "drain.conflict");
            conflict = true;
            break;
        }
        // O cancelamento passou (o drain ainda esvaziava a memória): começa de novo.
        if drain_state(node3).await == "active" {
            start_drain(node3).await;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(conflict, "devia haver uma janela com a saída em andamento");
    cluster.network.set_delay(Duration::ZERO);
    cluster.shutdown().await;
}

/// UC: UC-DRN-008, UC-ENT-022, UC-CAN-023
#[tokio::test]
async fn a_draining_node_answers_take_empty_and_the_peers_stop_asking() {
    use taba_node::cluster::protocol::{Request, Response, TakeRequest};
    use taba_node::cluster::transport::{PeerAddr, Transport};

    let cluster = TestCluster::start(3).await;
    let node3 = cluster.node(3);
    node3
        .client
        .post(node3.client_url("/topics"))
        .json(&json!({"name": "pedidos", "type": "queue"}))
        .send()
        .await
        .unwrap();
    for n in 0..9 {
        publish(cluster.node(1), "pedidos", json!({"n": n})).await;
    }
    // A saída não fecha (o desregistro não chega aos pares): o nó fica em `draining`, ainda na
    // tabela e alcançável.
    for host in ["10.0.0.1", "10.0.0.2"] {
        cluster.network.lose_kind(host, "unregister", 1_000_000);
    }
    start_drain(node3).await;
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(drain_state(node3).await, "draining");

    // Um `take` que ainda chegue é respondido vazio, mesmo havendo mensagens prontas.
    let asker = cluster.network.transport("10.0.0.1");
    let port = node3.node.cluster_addr().unwrap().port();
    let reply = asker
        .call(
            &PeerAddr::new("10.0.0.3", port),
            Request::Take {
                topic: "pedidos".into(),
                request: TakeRequest {
                    max_messages: 10,
                    visibility_timeout: 60,
                },
            },
        )
        .await
        .unwrap();
    assert!(matches!(reply, Response::Taken(t) if t.messages.is_empty()));

    // Os pares foram avisados por sinal e não pedem mais `take` a ele.
    assert!(cluster.network.count("channel") >= 2);
    cluster.network.clear_calls();
    let got = consume_all(cluster.node(1), "pedidos", 1).await;
    assert!(!got.is_empty());
    assert!(
        cluster
            .network
            .calls()
            .iter()
            .all(|c| !(c.request == "take" && c.to.host == "10.0.0.3"))
    );
    for host in ["10.0.0.1", "10.0.0.2"] {
        cluster.network.lose_kind(host, "unregister", 0);
    }
    // Cancela o drain: o nó volta a ativo e a parada do teste não espera `drain.timeout`.
    let cancel = node3
        .client
        .delete(node3.operations_url("/node/drain"))
        .send()
        .await
        .unwrap();
    assert_eq!(cancel.status(), 200);
    cluster.shutdown().await;
}

/// UC: UC-DRN-009, UC-DRN-010
#[tokio::test]
async fn while_it_drains_the_node_writes_every_new_replica_straight_to_disk() {
    let cluster = TestCluster::start(3).await;
    let node3 = cluster.node(3);
    node3
        .client
        .post(node3.client_url("/topics"))
        .json(&json!({"name": "pedidos", "type": "queue"}))
        .send()
        .await
        .unwrap();
    for n in 0..6 {
        publish(cluster.node(1), "pedidos", json!({"n": n})).await;
    }
    let messages = node3
        .node
        .cluster()
        .and_then(|c| c.replication())
        .map(|r| r.messages.messages().clone())
        .unwrap();
    eventually("a memória do nó 3 com as réplicas", || {
        messages.pending() > 0
    })
    .await;

    // A saída não fecha: o nó fica em `draining`, recebendo registros e commits dos pares.
    for host in ["10.0.0.1", "10.0.0.2"] {
        cluster.network.lose_kind(host, "unregister", 1_000_000);
    }
    start_drain(node3).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(messages.pending(), 0, "a memória foi toda para o disco");
    let disk_before = messages.storage_stats().disk_messages;
    assert!(disk_before > 0);
    for n in 0..6 {
        publish(cluster.node(1), "pedidos", json!({"depois": n})).await;
    }
    eventually("as réplicas novas em disco", || {
        messages.storage_stats().disk_messages > disk_before
    })
    .await;
    assert_eq!(messages.pending(), 0, "nada novo ficou em memória");

    for host in ["10.0.0.1", "10.0.0.2"] {
        cluster.network.lose_kind(host, "unregister", 0);
    }
    let cancel = node3
        .client
        .delete(node3.operations_url("/node/drain"))
        .send()
        .await
        .unwrap();
    assert_eq!(cancel.status(), 200);
    cluster.shutdown().await;
}

/// UC: UC-DRN-013, UC-CNC-003
#[tokio::test]
async fn two_nodes_draining_at_once_leave_one_after_the_other() {
    let cluster = TestCluster::start(3).await;
    let (node2, node3) = (cluster.node(2), cluster.node(3));
    start_drain(node2).await;
    start_drain(node3).await;
    wait_drain_state(node2, "drained").await;
    wait_drain_state(node3, "drained").await;

    // As duas saídas se efetivaram, uma depois da outra: sobrou só o nó 1, na época +2.
    let table = cluster.table(1).unwrap();
    assert_eq!(table.members.len(), 1);
    assert_eq!(table.version, 3 + 2);
    assert_eq!(cluster.member_ids(1), [cluster.node_id(1)]);
    cluster.shutdown().await;
}
