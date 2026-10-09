//! As definições de tópico e de token compartilhadas por todos os nós, com dono no anel: quem
//! decide, a versão, a insistência nos nós que não respondem e a lápide da remoção (nível **C** do
//! doc 20): vários nós de verdade no mesmo processo, ligados pela rede em memória.

mod common;

use common::TestNode;
use common::cluster::{TestCluster, address_of, eventually_async, host_of, name_owned_by};
use common::error_code;
use serde_json::json;
use taba_node::cluster::protocol::{
    DefinitionApply, DefinitionKind, DefinitionOperation, DefinitionProposal, Request, Response,
    StreamQuery,
};
use taba_node::cluster::transport::{CallError, Transport};
use taba_node::core::error::ErrorCode;
use taba_node::core::ring::definition_position;

/// O código do erro que o par devolveu.
fn remote_code(error: CallError) -> ErrorCode {
    match error {
        CallError::Remote(e) => e.code,
        other => panic!("esperava um erro do par, veio {other}"),
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
        .expect("requisição")
}

/// O trecho do stream de tópicos que o nó `number` serve, como um par pediria.
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
        other => panic!("esperava um stream, veio {other:?}"),
    }
}

/// Um nó que não responde não impede a criação: o dono insiste nele até ele voltar.
/// UC: UC-DEF-031
#[tokio::test]
async fn a_topic_created_while_a_node_is_unreachable_reaches_it_when_it_returns() {
    let cluster = TestCluster::start(3).await;
    let name = name_owned_by(&cluster, 1, "late-");
    cluster.network.isolate(&host_of(3));

    let response = create(&cluster, 1, &name).await;
    assert_eq!(response.status(), 201, "sem registro em todos os nós");
    assert!(has_topic(&cluster, 1, &name).await);
    assert!(
        has_topic(&cluster, 2, &name).await,
        "o nó alcançável já tem"
    );
    assert!(!has_topic(&cluster, 3, &name).await, "o isolado ainda não");

    cluster.network.restore(&host_of(3));
    eventually_async("o nó que voltou recebe o tópico", || async {
        has_topic(&cluster, 3, &name).await
    })
    .await;
    cluster.shutdown().await;
}

/// Quem recebe a chamada não é o dono: propõe ao dono, que decide e espalha.
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
        eventually_async("o tópico em todo nó", || async {
            has_topic(&cluster, number, &name).await
        })
        .await;
    }
    // O dono deu a versão 1 e todos a guardam.
    for number in 1..=3 {
        let items = stream_of(&cluster, number).await;
        let item = items
            .iter()
            .find(|i| i["name"] == name.as_str())
            .expect("o tópico no stream");
        assert_eq!(item["version"], 1, "nó {number}");
    }
    cluster.shutdown().await;
}

/// Duas criações do mesmo nome ao mesmo tempo, em nós diferentes: o dono serializa e só uma vale.
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

/// O dono fora do ar e ainda na tabela: a mudança falha com `503` e o cliente repete (D107).
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
    assert!(!has_topic(&cluster, 1, &name).await, "nada foi aplicado");
    assert!(!has_topic(&cluster, 2, &name).await);
    cluster.shutdown().await;
}

/// A remoção deixa uma lápide com a versão: uma mudança atrasada (versão menor) não ressuscita o
/// tópico.
/// UC: UC-DEF-027, UC-DEF-026
#[tokio::test]
async fn a_late_change_does_not_bring_a_removed_topic_back() {
    let cluster = TestCluster::start(3).await;
    let name = name_owned_by(&cluster, 1, "gone-");
    assert_eq!(create(&cluster, 1, &name).await.status(), 201);
    for number in 1..=3 {
        eventually_async("o tópico em todo nó", || async {
            has_topic(&cluster, number, &name).await
        })
        .await;
    }
    let creation = stream_of(&cluster, 2)
        .await
        .into_iter()
        .find(|i| i["name"] == name.as_str())
        .expect("o tópico no stream");

    let node = cluster.node(1);
    let removed = node
        .client
        .delete(node.client_url(&format!("/topics/{name}")))
        .send()
        .await
        .expect("requisição");
    assert_eq!(removed.status(), 204);
    for number in 1..=3 {
        eventually_async("o tópico removido em todo nó", || async {
            !has_topic(&cluster, number, &name).await
        })
        .await;
    }

    // A criação (versão 1) chega de novo depois da remoção (versão 2): é ignorada.
    let late = DefinitionApply {
        kind: DefinitionKind::Topic,
        key: name.clone(),
        version: 1,
        deleted: false,
        body: creation,
    };
    let peer = cluster.network.transport(host_of(1));
    for number in [2, 3] {
        peer.call(
            &address_of(&cluster, number),
            Request::DefinitionApply(late.clone()),
        )
        .await
        .expect("o nó aceita a chamada");
        assert!(
            !has_topic(&cluster, number, &name).await,
            "nó {number} não ressuscita o tópico"
        );
    }
    cluster.shutdown().await;
}

/// Alterar duas vezes dá versões crescentes, e todos os nós terminam na última.
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
        .expect("requisição");
    assert_eq!(patched.status(), 200);

    for number in 1..=3 {
        eventually_async("a versão 2 em todo nó", || async {
            stream_of(&cluster, number)
                .await
                .iter()
                .any(|i| i["name"] == name.as_str() && i["version"] == 2)
        })
        .await;
    }
    cluster.shutdown().await;
}

/// Sem nenhum outro dono alcançável a mudança não existe em outro nó: o dono recusa (`503`) em
/// vez de responder com uma cópia só.
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
    assert!(!has_topic(&cluster, 1, &name).await, "nada foi aplicado");
    cluster.shutdown().await;
}

/// O dono confere a mudança com o estado dele: tópico desconhecido é `404` e o `expectedId` de uma
/// remoção que não confere é `409`, sem aplicar nada.
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
    assert!(has_topic(&cluster, 2, &name).await, "o tópico segue lá");
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
        .expect("requisição");
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
        .expect("requisição")
        .status()
        .as_u16()
}

/// O token também tem dono (o `id` dele no anel): a revogação vale na hora nos nós que respondem e
/// chega ao nó que estava fora do ar quando ele volta.
/// UC: UC-DEF-034, UC-DEF-031
#[tokio::test]
async fn a_token_revoked_while_a_node_is_unreachable_stops_working_there_when_it_returns() {
    let cluster = TestCluster::start_with(3, &[("TABA_AUTH_ROOT_TOKEN", ROOT)]).await;
    let third = cluster.node_id(3);
    let membership = cluster.node(1).node.cluster().unwrap().membership();
    // Um token cujo dono não é o nó que vai ficar isolado.
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
        eventually_async("o token em todo nó", || async {
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
        .expect("requisição");
    assert_eq!(revoked.status(), 204);
    assert_eq!(status_with(cluster.node(1), &secret).await, 401);
    assert_eq!(status_with(cluster.node(2), &secret).await, 401);
    assert_eq!(
        status_with(cluster.node(3), &secret).await,
        200,
        "o isolado ainda não soube"
    );

    cluster.network.restore(&host_of(3));
    eventually_async("o nó que voltou recebe a revogação", || async {
        status_with(cluster.node(3), &secret).await == 401
    })
    .await;
    cluster.shutdown().await;
}
