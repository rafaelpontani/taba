//! As condições do cluster e o `GET /health/cluster` com todos os nós (doc 06b, 06c e 11; doc 20c,
//! nível **C**): `GET /cluster/status` em cada nó, a agregação em `scope=cluster`, e cada condição
//! (`member-unreachable`, `under-replicated`, `write-quorum-at-risk`, `quorum-lost`,
//! `membership-divergent`, `syncing`), com a severidade do doc 11.

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

/// Os nomes das condições vigentes, na ordem da resposta.
fn names(status: &Value) -> Vec<String> {
    status["conditions"]
        .as_array()
        .expect("lista de condições")
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
        .unwrap_or_else(|| panic!("sem a condição {condition} em {status}"))["severity"]
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

    // `GET /cluster/status`: o `NodeStatus` do nó, para quem é membro.
    let transport = cluster.network.transport(host_of(2));
    for number in 1..=3 {
        let Ok(Response::Status(status)) = transport
            .call(&address_of(&cluster, number), Request::Status)
            .await
        else {
            panic!("o nó {number} deveria responder o estado");
        };
        assert_eq!(status["nodeId"], cluster.node_id(number));
        assert_eq!(status["state"], "active");
        assert_eq!(status["signals"]["quorumAvailable"], true);
        assert_eq!(status["signals"]["channels"].as_object().unwrap().len(), 2);
        assert!(status.get("metrics").is_none(), "sem as métricas");
    }
    // Quem não é membro não lê o estado, e não existe `GET /cluster/metrics`.
    let stranger = cluster.network.transport("10.0.0.99");
    match stranger
        .call(&address_of(&cluster, 1), Request::Status)
        .await
    {
        Err(CallError::Remote(error)) => assert_eq!(error.code, ErrorCode::ClusterNotMember),
        other => panic!("esperava `not-member`, veio {other:?}"),
    }
    let missing = Request::from_http("GET", "/cluster/metrics", &[], &[]).unwrap_err();
    assert_eq!(missing.code, ErrorCode::RouteNotFound);

    // `scope=cluster` (o padrão) junta os três nós; nenhuma condição, `ok`.
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

    // `scope=node` é só o nó que atende, sem chamar os outros.
    cluster.network.clear_calls();
    let local = node_view(cluster.node(2)).await;
    assert_eq!(local["scope"], "node");
    assert_eq!(local["nodes"].as_array().unwrap().len(), 1);
    assert_eq!(local["nodes"][0]["nodeId"], cluster.node_id(2));
    assert_eq!(local["conditions"], json!([]));
    assert_eq!(cluster.network.count("status"), 0);

    // O `scope` desconhecido é `400`; as métricas dizem que nenhuma condição vigora.
    let invalid = cluster
        .node(1)
        .client_get_ops("/health/cluster?scope=outro")
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
        assert!(metrics.contains(&label), "a série {condition} existe com 0");
    }
    cluster.shutdown().await;
}

/// UC: UC-OPS-018, UC-FAL-024, UC-FAL-025
#[tokio::test]
async fn a_node_that_does_not_answer_is_unreachable_and_the_blocks_it_leads_are_at_risk() {
    let cluster = TestCluster::start_with(3, NO_VOTE).await;
    settled(&cluster).await;
    create_queue(cluster.node(1), "pedidos").await;
    let third = cluster.node_id(3);

    cluster.network.isolate(&host_of(3));
    let channels = channels_of(&cluster, 1);
    let id = third.clone();
    eventually("o canal do nó 1 com o 3 cair", move || {
        !channels.is_up(&id)
    })
    .await;

    // O nó que não responde aparece com `reachable: false`; a chamada não falha.
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

    // Com um dos três fora ainda há quórum de escrita (2), mas ele inclui o primário: nos blocos
    // cujo primário é o nó fora, as escritas falham até a exclusão, e a condição diz isso (D107).
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

    // As escritas seguem respondendo (UC-FAL-025), menos as dos blocos cujo primário é o nó fora:
    // o quórum sempre inclui o primário, então elas falham até a exclusão, que é o que a condição
    // acima anuncia. O ID é sorteado, então o teste manda várias e confere que as dos outros
    // blocos fecham.
    let mut accepted = 0;
    for n in 0..16 {
        let response = cluster
            .node(1)
            .client
            .post(cluster.node(1).client_url("/topics/pedidos/messages"))
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
        "as escritas dos blocos com primário vivo fecham"
    );

    // A métrica tem as mesmas condições.
    eventually_async("a métrica da condição", || async {
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
    create_queue(cluster.node(1), "pedidos").await;

    // O nó 1 perde os dois: as réplicas íntegras ficam abaixo de `write.quorum` e os saudáveis
    // abaixo de `cluster.vote.min-voters`.
    cluster.network.partition(&host_of(1), &host_of(2));
    cluster.network.partition(&host_of(1), &host_of(3));
    let channels = channels_of(&cluster, 1);
    let (two, three) = (cluster.node_id(2), cluster.node_id(3));
    eventually("os dois canais do nó 1 caírem", move || {
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
        "todo bloco abaixo do quórum também está abaixo da replicação"
    );

    // Sem quórum a escrita não fecha.
    let response = cluster
        .node(1)
        .client
        .post(cluster.node(1).client_url("/topics/pedidos/messages"))
        .json(&json!({"n": 1}))
        .send()
        .await
        .unwrap();
    assert!(
        response.status().is_server_error(),
        "a escrita sem quórum respondeu {}",
        response.status()
    );

    // Os outros dois se enxergam: nenhuma condição crítica do lado deles além de o nó 1 sumir.
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
    eventually_async("o nó isolado perder o quórum", || async {
        names(&node_view(lonely).await).contains(&"quorum-lost".to_owned())
    })
    .await;
    let view = node_view(lonely).await;
    assert_eq!(severity_of(&view, "quorum-lost"), "critical");
    assert_eq!(view["summary"]["quorumAvailable"], false);
    let metric = |text: &str| total(text, "taba_cluster_quorum_available", "");
    assert_eq!(metric(&lonely.metrics().await), 0.0);

    // Os outros dois somam o mínimo de votantes: o quórum existe para eles.
    for number in [1, 2] {
        let view = node_view(cluster.node(number)).await;
        assert!(!names(&view).contains(&"quorum-lost".to_owned()));
        assert_eq!(view["summary"]["quorumAvailable"], true);
        assert_eq!(metric(&cluster.node(number).metrics().await), 1.0);
    }
    cluster.shutdown().await;
}

/// O commit do ingresso demora a chegar aos membros, mas a mudança viaja também pelo quadro
/// `membership` do canal (D126): as tabelas se igualam antes dele e a divergência não dura. O alerta
/// `membership-divergent` para versões diferentes está nos testes de `health`.
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
    let number = cluster.spawn_node(builder).await.expect("o nó sobe");
    wait_joined(cluster.node(number)).await;

    // Com o commit ainda a caminho, os membros antigos já têm a tabela nova.
    eventually("as tabelas iguais antes do commit chegar", || {
        (1..=4).all(|n| cluster.table(n).is_some_and(|t| t.version == 4))
    })
    .await;
    cluster.network.set_delay_kind("commit", Duration::ZERO);
    eventually_async("as condições sumirem", || async {
        cluster_view(cluster.node(1)).await["conditions"] == json!([])
    })
    .await;
    cluster.shutdown().await;
}

/// UC: UC-FAL-024, UC-FAL-025
#[tokio::test]
async fn a_node_that_syncs_shows_as_syncing_and_under_replicated_but_not_at_risk() {
    let mut cluster = TestCluster::start(3).await;
    create_queue(cluster.node(1), "pedidos").await;
    for n in 0..6 {
        let response = cluster
            .node(1)
            .client
            .post(cluster.node(1).client_url("/topics/pedidos/messages"))
            .json(&json!({"n": n}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 201);
    }
    let fourth = start_isolated(&mut cluster, &[]).await;
    let gate = pause_sync(&cluster, fourth, "replay");
    wait_phase(cluster.node(fourth), "replay").await;

    // As réplicas do nó 4 ainda não estão íntegras, mas os três antigos fecham o quórum.
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
    eventually_async("o cluster voltar a ok", || async {
        let view = cluster_view(cluster.node(1)).await;
        view["worstCondition"] == "ok" && view["summary"]["members"] == json!({ "active": 4 })
    })
    .await;
    cluster.shutdown().await;
}
