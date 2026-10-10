//! A recuperação de perdas e o retorno do nó (doc 04, seção 6.6; doc 20c, UC-RTN e UC-DRN-018;
//! nível **C**): o nó que descobre ter sido excluído, ou que volta do drain, consulta a lista de
//! perdas dos outros nós, procura cada mensagem no que ainda guarda e a envia ao responsável pelo
//! bloco, antes de apagar o estado local. O `Clock` é manual; os prazos de espera são reais.

mod common;

use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};

use common::TestNode;
use common::cluster::{TestCluster, eventually, eventually_async, host_of, total, wait_joined};
use taba_node::cluster::protocol::{LossRecord, MessageRecord, Request, Response, StreamQuery};
use taba_node::cluster::recovery::Recovery;
use taba_node::cluster::state::NodeState;
use taba_node::cluster::transport::{CallError, PeerAddr, Transport};
use taba_node::config::DbSynchronous;
use taba_node::core::clock::Clock;
use taba_node::core::error::ErrorCode;
use taba_node::core::id;
use taba_node::engine::messages::Messages;
use taba_node::storage::losses::Loss;
use taba_node::storage::messages::MessageStore;

/// Os 5 minutos de `cluster.freezing.duration`.
const FREEZING: Duration = Duration::from_secs(300);
const TOPIC: &str = "pedidos";

fn messages_of(node: &TestNode) -> Arc<Messages> {
    node.node
        .cluster()
        .and_then(|c| c.replication())
        .map(|r| r.messages.messages().clone())
        .expect("o nó está no cluster")
}

fn recovery_of(node: &TestNode) -> Arc<Recovery> {
    node.node
        .cluster()
        .and_then(|c| c.replication())
        .map(|r| r.recovery.clone())
        .expect("o nó está no cluster")
}

/// Uma mensagem como o nó 3 a guardaria: já entregue duas vezes e com uma invisibilidade vigente.
fn record(cluster: &TestCluster, n: u64, text: &str) -> MessageRecord {
    let now = cluster.clock.now_millis();
    MessageRecord {
        id: id::new_at(now + n),
        topic: TOPIC.into(),
        body: json!({ "texto": text }).to_string().into_bytes(),
        content_type: "application/json".into(),
        group_id: None,
        deduplication_id: None,
        delivery_count: 3,
        invisible_until_ms: Some(now + 3_600_000),
        receipt_token: Some(id::new_at(now + 100 + n)),
        dead_lettered_ms: None,
        dead_letter_reason: None,
        delete_at_ms: None,
    }
}

/// O nó passa a guardar a mensagem (como se a tivesse recebido de uma publicação).
fn hold(node: &TestNode, record: &MessageRecord) {
    messages_of(node)
        .apply_load(&record.topic, record.to_meta(), record.body.clone())
        .expect("grava");
    assert!(messages_of(node).holds(&record.topic, &record.id));
}

/// O nó declara a perda da mensagem (como a sincronização de um nó novo a declararia).
fn declare_loss(cluster: &TestCluster, number: usize, record: &MessageRecord) {
    recovery_of(cluster.node(number))
        .losses()
        .record(&Loss {
            topic: record.topic.clone(),
            id: record.id.clone(),
            block: "0000000000000001".into(),
            detected_ms: cluster.clock.now_millis(),
        })
        .unwrap();
}

fn has(node: &TestNode, id: &str) -> bool {
    messages_of(node).message_info(TOPIC, id).unwrap().is_some()
}

fn loss_count(cluster: &TestCluster, number: usize) -> u64 {
    recovery_of(cluster.node(number)).losses().count().unwrap()
}

async fn setup(env: &[(&str, &str)]) -> TestCluster {
    let cluster = TestCluster::start_with(3, env).await;
    common::cluster::settled(&cluster).await;
    common::cluster::create_queue(cluster.node(1), TOPIC).await;
    cluster
}

/// Isola o nó 3 e espera os outros dois o excluírem.
async fn exclude_third(cluster: &TestCluster) {
    let id = cluster.node_id(3);
    cluster.network.isolate(&host_of(3));
    eventually("os dois saudáveis excluírem o nó 3", || {
        [1, 2].iter().all(|n| !cluster.member_ids(*n).contains(&id))
    })
    .await;
}

async fn consume_ids(node: &TestNode) -> Vec<String> {
    let response = node
        .client
        .get(node.client_url(&format!(
            "/topics/{TOPIC}/messages?maxMessages=10&visibilityTimeout=300"
        )))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let body: Value = response.json().await.unwrap();
    body["messages"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|m| m["id"].as_str().unwrap().to_owned())
        .collect()
}

/// UC: UC-RTN-001, UC-RTN-002, UC-RTN-003, UC-RTN-004, UC-RTN-005, UC-RTN-007, UC-RTN-009, UC-RTN-010
#[tokio::test]
async fn an_excluded_node_sends_back_the_lost_messages_it_still_has_and_then_rejoins() {
    let cluster = setup(&[]).await;
    let lost = record(&cluster, 1, "perdida");
    let unlisted = record(&cluster, 2, "ninguem declarou");
    let absent = record(&cluster, 3, "o nó 3 não tem");
    for number in [1, 2] {
        declare_loss(&cluster, number, &lost);
        declare_loss(&cluster, number, &absent);
    }
    exclude_third(&cluster).await;
    hold(cluster.node(3), &lost);
    hold(cluster.node(3), &unlisted);
    let third_id = cluster.node_id(3);

    // A rede volta: o nó 3 vê que foi excluído, recupera o que a lista de perdas pede e só então
    // apaga o estado local.
    cluster.network.restore(&host_of(3));
    eventually("a mensagem perdida chegar aos dois responsáveis", || {
        has(cluster.node(1), &lost.id) && has(cluster.node(2), &lost.id)
    })
    .await;

    // Grava direto, com o contador de entregas; a invisibilidade e o recibo antigos somem.
    let info = messages_of(cluster.node(1))
        .message_info(TOPIC, &lost.id)
        .unwrap()
        .unwrap();
    assert_eq!(info.meta.delivery_count, 3);
    assert_eq!(info.meta.invisible_until_ms, 0);
    assert_eq!(info.meta.receipt_token, None);
    let owners: Vec<bool> = [1, 2]
        .iter()
        .map(|n| {
            messages_of(cluster.node(*n))
                .message_info(TOPIC, &lost.id)
                .unwrap()
                .unwrap()
                .owned
        })
        .collect();
    assert_eq!(owners.iter().filter(|o| **o).count(), 1, "um só dono");

    // Só o que a lista pede e o nó tem foi enviado: a mensagem que ninguém declarou perdida não
    // vai, e a perdida que o nó 3 não tem fica no registro.
    assert!(!has(cluster.node(1), &unlisted.id));
    assert!(!has(cluster.node(2), &unlisted.id));
    assert!(!has(cluster.node(1), &absent.id));

    // Baixa: o par recuperado sai das duas tabelas; o que não foi recuperado fica.
    eventually("a baixa da perda recuperada", || {
        loss_count(&cluster, 1) == 1 && loss_count(&cluster, 2) == 1
    })
    .await;

    // A mensagem volta a ficar pronta para entrega.
    let mut got = Vec::new();
    for _ in 0..40 {
        got = consume_ids(cluster.node(1)).await;
        if !got.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(got, vec![lost.id.clone()]);

    // Contagem: uma enviada pelo nó 3, aplicada nos dois responsáveis.
    let third = cluster.node(3).metrics().await;
    assert_eq!(
        total(
            &third,
            "taba_recover_messages_total",
            "taba_outcome=\"sent\""
        ),
        1.0
    );
    assert_eq!(
        total(
            &third,
            "taba_cluster_recover_total",
            "taba_direction=\"sent\""
        ),
        1.0
    );
    let applied: f64 = [
        cluster.node(1).metrics().await,
        cluster.node(2).metrics().await,
    ]
    .iter()
    .map(|text| {
        total(
            text,
            "taba_recover_messages_total",
            "taba_outcome=\"applied\"",
        )
    })
    .sum();
    assert_eq!(applied, 2.0);

    // Recuperado, o nó apagou o que tinha e refaz o ingresso do zero (o freezing só o atrasa).
    eventually("o nó 3 apagar o estado local", || {
        !has(cluster.node(3), &unlisted.id)
    })
    .await;
    cluster.clock.advance(FREEZING);
    wait_joined(cluster.node(3)).await;
    assert_eq!(cluster.node(3).node.node_id(), third_id);
    cluster.shutdown().await;
}

/// UC: UC-RTN-006, UC-RTN-010
#[tokio::test]
async fn the_node_keeps_its_old_data_while_a_node_does_not_answer_the_loss_query() {
    let cluster = setup(&[("TABA_CLUSTER_RECOVERY_TIMEOUT", "30s")]).await;
    let lost = record(&cluster, 1, "perdida");
    for number in [1, 2] {
        declare_loss(&cluster, number, &lost);
    }
    exclude_third(&cluster).await;
    hold(cluster.node(3), &lost);

    // O nó 2 não responde à consulta: o nó 3 espera, em `returning`, sem apagar nada.
    cluster
        .network
        .lose_kind("10.0.0.2", "message-loss", 100_000);
    cluster.network.restore(&host_of(3));
    eventually("o nó 3 em returning", || {
        cluster.node(3).node.shared().state() == NodeState::Returning
    })
    .await;
    let ready = cluster.node(3).client_get_ops("/health/ready").await;
    assert_eq!(ready.status(), 503);
    assert_eq!(ready.json::<Value>().await.unwrap()["reason"], "returning");
    let asked = cluster.network.count("message-loss");
    eventually("a consulta repetida ao nó que não respondeu", || {
        cluster.network.count("message-loss") >= asked + 3
    })
    .await;
    assert!(
        messages_of(cluster.node(3)).holds(TOPIC, &lost.id),
        "o estado antigo fica em quarentena"
    );
    assert!(!has(cluster.node(1), &lost.id));

    // O nó 2 volta a responder: a recuperação termina e só então o estado é apagado.
    cluster.network.lose_kind("10.0.0.2", "message-loss", 0);
    eventually("a mensagem chegar aos responsáveis", || {
        has(cluster.node(1), &lost.id) && has(cluster.node(2), &lost.id)
    })
    .await;
    eventually("o nó 3 sair de returning", || {
        cluster.node(3).node.shared().state() != NodeState::Returning
    })
    .await;
    assert!(!messages_of(cluster.node(3)).holds(TOPIC, &lost.id));
    cluster.shutdown().await;
}

/// UC: UC-RTN-006, UC-RTN-007
#[tokio::test]
async fn when_the_recovery_timeout_runs_out_the_node_goes_on_with_the_lists_it_has() {
    let cluster = setup(&[("TABA_CLUSTER_RECOVERY_TIMEOUT", "400ms")]).await;
    let lost = record(&cluster, 1, "perdida");
    declare_loss(&cluster, 1, &lost);
    exclude_third(&cluster).await;
    hold(cluster.node(3), &lost);

    // O nó 2 nunca responde: passado o prazo, vale a lista do nó 1 e o nó segue.
    cluster
        .network
        .lose_kind("10.0.0.2", "message-loss", 100_000);
    cluster.network.restore(&host_of(3));
    eventually("a mensagem chegar aos responsáveis", || {
        has(cluster.node(1), &lost.id) && has(cluster.node(2), &lost.id)
    })
    .await;
    eventually("o nó 3 apagar o estado local e sair de returning", || {
        cluster.node(3).node.shared().state() != NodeState::Returning
            && !messages_of(cluster.node(3)).holds(TOPIC, &lost.id)
    })
    .await;
    cluster.clock.advance(FREEZING);
    wait_joined(cluster.node(3)).await;
    cluster.shutdown().await;
}

/// UC: UC-DRN-018, UC-RTN-001, UC-RTN-003, UC-RTN-013
#[tokio::test]
async fn a_node_that_left_by_drain_recovers_the_lost_messages_before_it_returns_as_a_new_node() {
    let cluster = setup(&[]).await;
    let lost = record(&cluster, 1, "perdida");
    for number in [1, 2] {
        declare_loss(&cluster, number, &lost);
    }
    let node3 = cluster.node(3);
    hold(node3, &lost);
    let old_id = cluster.node_id(3);
    let drain = node3
        .client
        .post(node3.operations_url("/node/drain"))
        .send()
        .await
        .unwrap();
    assert_eq!(drain.status(), 202);
    eventually_async("drained", || async {
        let body: Value = node3
            .client_get_ops("/node/drain")
            .await
            .json()
            .await
            .unwrap();
        body["drainState"] == "drained"
    })
    .await;
    assert!(!has(cluster.node(1), &lost.id));

    let back = node3
        .client
        .delete(node3.operations_url("/node/drain"))
        .send()
        .await
        .unwrap();
    assert_eq!(back.status(), 202);

    // Antes de limpar o estado, o nó entrega aos responsáveis o que a lista de perdas pedia.
    eventually("a mensagem chegar aos responsáveis", || {
        has(cluster.node(1), &lost.id) && has(cluster.node(2), &lost.id)
    })
    .await;
    eventually("o nó voltar com ID novo", || {
        let id = node3.node.node_id();
        id != old_id
            && cluster
                .table(1)
                .is_some_and(|t| t.members.len() == 3 && t.member(&id).is_some())
    })
    .await;
    assert!(!messages_of(node3).holds(TOPIC, &lost.id));
    assert_eq!(
        total(
            &node3.metrics().await,
            "taba_recover_messages_total",
            "taba_outcome=\"sent\""
        ),
        1.0
    );
    cluster.shutdown().await;
}

/// UC: UC-RTN-001, UC-RTN-008, UC-RTN-012
#[tokio::test]
async fn the_loss_routes_take_a_caller_that_is_not_a_member_for_the_list_and_the_recovery_only() {
    let cluster = setup(&[("TABA_CLUSTER_MESSAGE_LOSS_TTL", "1h")]).await;
    let lost = record(&cluster, 1, "perdida");
    let outsider = cluster.network.transport("10.0.0.9");
    let first = common::cluster::address_of(&cluster, 1);

    // A lista de perdas e o `recover` aceitam quem não consta na tabela; o registro, não.
    declare_loss(&cluster, 1, &lost);
    let listed = outsider
        .call(&first, Request::MessageLoss(StreamQuery::default()))
        .await
        .expect("a lista aceita quem não é membro");
    let Response::Stream(page) = listed else {
        panic!("esperava um stream");
    };
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0]["id"], lost.id);
    assert_eq!(page.items[0]["topic"], TOPIC);
    let recovered = outsider
        .call(
            &first,
            Request::Recover {
                records: vec![],
                replicate: true,
            },
        )
        .await
        .expect("o recover aceita quem não é membro");
    assert!(matches!(recovered, Response::Recovered(r) if r.count == 0));
    let register = Request::LossRegister(LossRecord {
        topic: TOPIC.into(),
        id: lost.id.clone(),
        block: "0000000000000001".into(),
        detected_at: "2026-10-07T12:00:00.000Z".into(),
    });
    match outsider.call(&first, register.clone()).await {
        Err(CallError::Remote(e)) => assert_eq!(e.code, ErrorCode::ClusterNotMember),
        other => panic!("esperava `cluster.not-member`: {other:?}"),
    }

    // Um membro registra a perda no nó 2, que passa a listá-la, e a baixa a tira.
    let member = cluster.network.transport(host_of(1));
    let second = common::cluster::address_of(&cluster, 2);
    let ack = member.call(&second, register).await.unwrap();
    assert!(matches!(ack, Response::Loss(a) if a.changed));
    assert_eq!(loss_count(&cluster, 2), 1);
    let remove = Request::LossRemove {
        topic: TOPIC.into(),
        id: lost.id.clone(),
    };
    let ack = member.call(&second, remove.clone()).await.unwrap();
    assert!(matches!(ack, Response::Loss(a) if a.changed));
    let again = member.call(&second, remove).await.unwrap();
    assert!(matches!(again, Response::Loss(a) if !a.changed));
    assert_eq!(loss_count(&cluster, 2), 0);

    // O registro expira depois de `cluster.message-loss.ttl`.
    cluster.clock.advance(Duration::from_secs(2 * 3600));
    let Response::Stream(page) = outsider
        .call(&first, Request::MessageLoss(StreamQuery::default()))
        .await
        .unwrap()
    else {
        panic!("esperava um stream");
    };
    assert!(page.items.is_empty());
    assert_eq!(loss_count(&cluster, 1), 0);
    cluster.shutdown().await;
}

/// Reenviar o lote inteiro é seguro: a mensagem já aplicada é ignorada e a linha conta como feita.
///
/// UC: UC-RTN-009, UC-RTN-003
#[tokio::test]
async fn resending_a_recover_batch_ignores_what_was_already_applied() {
    let cluster = setup(&[]).await;
    let lost = record(&cluster, 1, "perdida");
    for number in [1, 2] {
        declare_loss(&cluster, number, &lost);
    }
    let outsider = cluster.network.transport("10.0.0.9");
    let target = common::cluster::address_of(&cluster, 1);
    let send = || Request::Recover {
        records: vec![lost.clone()],
        replicate: true,
    };
    for round in 0..2 {
        let reply = outsider.call(&target, send()).await.unwrap();
        assert!(
            matches!(reply, Response::Recovered(r) if r.count == 1),
            "envio {round}"
        );
    }
    assert!(has(cluster.node(1), &lost.id) && has(cluster.node(2), &lost.id));
    let skipped: f64 = [
        cluster.node(1).metrics().await,
        cluster.node(2).metrics().await,
    ]
    .iter()
    .map(|text| {
        total(
            text,
            "taba_recover_messages_total",
            "taba_outcome=\"skipped\"",
        )
    })
    .sum();
    assert_eq!(
        skipped, 2.0,
        "o segundo envio foi ignorado nos dois responsáveis"
    );
    cluster.shutdown().await;
}

/// UC: UC-RTN-011
#[tokio::test]
async fn in_development_mode_there_is_no_cluster_to_recover_from() {
    let node = TestNode::development().await;
    assert!(node.node.cluster().is_none());
    assert!(node.node.cluster_addr().is_none());
    node.shutdown().await;
}

/// UC: UC-RTN-002, UC-RTN-016
#[tokio::test]
async fn a_process_that_restarts_hands_back_the_lost_messages_its_old_disk_has() {
    let mut cluster = setup(&[("TABA_DRAIN_ON_STOP", "false"), ("TABA_MEMORY_LIMIT", "1B")]).await;
    let lost = record(&cluster, 1, "perdida");
    let unlisted = record(&cluster, 2, "ninguem declarou");
    for number in [1, 2] {
        declare_loss(&cluster, number, &lost);
    }

    // O processo cai sem drain e volta no mesmo endereço, com o mesmo disco: nasce como nó novo.
    let old_id = cluster.node_id(3);
    let (number, _) = restart_third(&mut cluster, &[], &[&lost, &unlisted]).await;
    assert_ne!(cluster.node_id(number), old_id, "o ID é novo");

    // Só a mensagem que a lista de perdas pede volta aos responsáveis.
    eventually("a mensagem perdida chegar aos dois responsáveis", || {
        has(cluster.node(1), &lost.id) && has(cluster.node(2), &lost.id)
    })
    .await;
    assert!(!has(cluster.node(1), &unlisted.id));
    assert!(!has(cluster.node(2), &unlisted.id));
    eventually("a baixa da perda recuperada", || {
        loss_count(&cluster, 1) == 0 && loss_count(&cluster, 2) == 0
    })
    .await;
    wait_joined(cluster.node(number)).await;
    assert!(!messages_of(cluster.node(number)).holds(TOPIC, &unlisted.id));
    cluster.shutdown().await;
}

/// O processo do nó 3 cai sem drain e volta no mesmo endereço, com o mesmo disco: nasce como nó
/// novo. Devolve o número do nó e a pasta do disco.
async fn restart_third(
    cluster: &mut TestCluster,
    env: &[(&str, &str)],
    on_disk: &[&MessageRecord],
) -> (usize, std::path::PathBuf) {
    let port = cluster.node(3).node.cluster_addr().unwrap().port();
    let TestNode { node, dir, .. } = cluster.nodes.pop().unwrap();
    node.shutdown().await;
    cluster.network.unregister(&PeerAddr::new(host_of(3), port));
    let storage = dir.path().join("taba");
    // O disco da vida anterior é gravado com o processo já morto: nenhum arquivo fica aberto.
    let old_disk = MessageStore::new(&storage, DbSynchronous::Off);
    for record in on_disk {
        old_disk
            .put(&record.topic, &record.to_meta(), &record.body)
            .expect("grava no disco");
    }
    drop(old_disk);
    let mut builder = cluster
        .builder(3, false)
        .env("TABA_CLUSTER_PORT", &port.to_string())
        .env("TABA_MEMORY_LIMIT", "1B")
        .env("TABA_STORAGE_DIR", &storage.display().to_string());
    for (key, value) in env {
        builder = builder.env(key, value);
    }
    let number = cluster
        .spawn_node(builder)
        .await
        .expect("o processo reinicia");
    // A pasta do disco fica de pé: o `TempDir` morre com o teste, depois da verificação.
    std::mem::forget(dir);
    (number, storage)
}

/// UC: UC-RTN-014, UC-RTN-015, UC-RTN-016
#[tokio::test]
async fn a_restarted_process_joins_without_waiting_and_recovers_its_old_disk_afterwards() {
    let mut cluster = setup(&[
        ("TABA_DRAIN_ON_STOP", "false"),
        ("TABA_MEMORY_LIMIT", "1B"),
        ("TABA_CLUSTER_RECOVERY_TIMEOUT", "30s"),
    ])
    .await;
    let from_one = record(&cluster, 1, "so o no 1 declarou");
    let from_two = record(&cluster, 2, "so o no 2 declarou");
    let unlisted = record(&cluster, 3, "ninguem declarou");
    declare_loss(&cluster, 1, &from_one);
    declare_loss(&cluster, 2, &from_two);
    let old_id = cluster.node_id(3);

    // O nó 2 não responde à consulta de perdas: antes, o reinício esperava até 5 minutos.
    cluster
        .network
        .lose_kind("10.0.0.2", "message-loss", 100_000);
    let (number, storage) = restart_third(
        &mut cluster,
        &[("TABA_CLUSTER_RECOVERY_TIMEOUT", "30s")],
        &[&from_one, &from_two, &unlisted],
    )
    .await;
    assert_ne!(cluster.node_id(number), old_id, "o ID é novo");
    wait_joined(cluster.node(number)).await;
    assert_eq!(
        cluster.node(number).node.shared().state(),
        NodeState::Active,
        "entra ativo, sem passar por returning"
    );

    // A lista do nó 1 já foi tratada; o nó 2 continua mudo e a pasta da vida anterior espera.
    eventually("a mensagem do nó 1 chegar aos responsáveis", || {
        has(cluster.node(1), &from_one.id) && has(cluster.node(2), &from_one.id)
    })
    .await;
    assert!(!has(cluster.node(1), &from_two.id));
    assert!(storage.join("_previous-life").is_dir());

    // O nó 2 volta a responder: a segunda perda também volta, e a pasta é apagada.
    cluster.network.lose_kind("10.0.0.2", "message-loss", 0);
    eventually("a mensagem do nó 2 chegar aos responsáveis", || {
        has(cluster.node(1), &from_two.id) && has(cluster.node(2), &from_two.id)
    })
    .await;
    eventually("a pasta da vida anterior ser apagada", || {
        !storage.join("_previous-life").exists()
    })
    .await;
    for node in [1, 2] {
        assert!(!has(cluster.node(node), &unlisted.id));
    }
    assert!(!messages_of(cluster.node(number)).holds(TOPIC, &unlisted.id));
    cluster.shutdown().await;
}

/// UC: UC-RTN-015
#[tokio::test]
async fn the_recovery_of_a_restarted_process_ends_at_the_timeout_even_if_a_node_stays_mute() {
    let mut cluster = setup(&[("TABA_DRAIN_ON_STOP", "false"), ("TABA_MEMORY_LIMIT", "1B")]).await;
    let lost = record(&cluster, 1, "perdida");
    declare_loss(&cluster, 1, &lost);
    cluster
        .network
        .lose_kind("10.0.0.2", "message-loss", 100_000);
    let (number, storage) = restart_third(
        &mut cluster,
        &[("TABA_CLUSTER_RECOVERY_TIMEOUT", "400ms")],
        &[&lost],
    )
    .await;
    wait_joined(cluster.node(number)).await;
    eventually("a mensagem chegar aos responsáveis", || {
        has(cluster.node(1), &lost.id) && has(cluster.node(2), &lost.id)
    })
    .await;
    eventually("a pasta da vida anterior ser apagada no prazo", || {
        !storage.join("_previous-life").exists()
    })
    .await;
    assert_eq!(
        cluster.node(number).node.shared().state(),
        NodeState::Active
    );
    cluster.shutdown().await;
}

/// UC: UC-RTN-017
#[tokio::test]
async fn the_first_node_of_a_new_cluster_wipes_the_old_disk_and_keeps_no_previous_life() {
    let dir = tempfile::tempdir().unwrap();
    let storage = dir.path().join("taba");
    let old_disk = MessageStore::new(&storage, DbSynchronous::Off);
    let cluster = TestCluster::empty();
    let old = record(&cluster, 1, "de um cluster que não existe mais");
    old_disk
        .put(&old.topic, &old.to_meta(), &old.body)
        .expect("grava no disco");
    drop(old_disk);
    assert!(storage.join(TOPIC).is_dir());

    let mut cluster = cluster;
    let builder = cluster
        .builder(1, true)
        .env("TABA_STORAGE_DIR", &storage.display().to_string());
    let number = cluster
        .spawn_node(builder)
        .await
        .expect("o primeiro nó sobe");
    wait_joined(cluster.node(number)).await;

    // Não há de quem recuperar: o disco antigo foi apagado, e nada ficou de lado.
    eventually("o disco antigo ser apagado", || {
        !storage.join(TOPIC).exists()
    })
    .await;
    assert!(!storage.join(MessageStore::PREVIOUS_LIFE).exists());
    assert_eq!(cluster.member_ids(number).len(), 1);
    cluster.shutdown().await;
}
