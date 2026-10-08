//! A sincronização de blocos de um nó que entra (doc 05, seção 7.3; doc 04, seções 6.2 e 6.8; doc 20c,
//! nível **C**): a carga dos blocos, o banco de eventos, a verificação final, a troca para o modo
//! normal, a concorrência de mudanças de membros e a limpeza dos blocos que um nó perdeu.

mod common;

use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};

use common::TestNode;
use common::cluster::{TestCluster, eventually, host_of, pause_sync, start_isolated, wait_phase};
use taba_node::cluster::members::MemberState;
use taba_node::cluster::protocol::{Request, Response, StreamQuery};
use taba_node::cluster::state::NodeState;
use taba_node::core::ring::message_position;
use taba_node::engine::messages::Messages;

fn messages_of(node: &TestNode) -> Arc<Messages> {
    node.node
        .cluster()
        .and_then(|c| c.replication())
        .map(|r| r.messages.messages().clone())
        .expect("o nó está no cluster")
}

async fn create_queue(node: &TestNode, name: &str) {
    let response = node
        .client
        .post(node.client_url("/topics"))
        .json(&json!({"name": name, "type": "queue"}))
        .send()
        .await
        .expect("requisição");
    assert_eq!(response.status(), 201);
}

async fn publish(node: &TestNode, topic: &str, body: Value) -> String {
    let response = node
        .client
        .post(node.client_url(&format!("/topics/{topic}/messages")))
        .json(&body)
        .send()
        .await
        .expect("requisição");
    assert_eq!(response.status(), 201, "{:?}", response.text().await);
    let value: Value = response.json().await.unwrap();
    value["id"].as_str().unwrap().to_owned()
}

async fn consume(node: &TestNode, topic: &str, query: &str) -> Vec<Value> {
    let response = node
        .client
        .get(node.client_url(&format!("/topics/{topic}/messages?{query}")))
        .send()
        .await
        .expect("requisição");
    assert_eq!(response.status(), 200, "{:?}", response.text().await);
    let value: Value = response.json().await.unwrap();
    value["messages"].as_array().cloned().unwrap_or_default()
}

async fn ack(node: &TestNode, topic: &str, id: &str, token: &str) -> u16 {
    node.client
        .delete(node.client_url(&format!(
            "/topics/{topic}/messages/{id}?receiptToken={token}"
        )))
        .send()
        .await
        .expect("requisição")
        .status()
        .as_u16()
}

fn member_state(cluster: &TestCluster, observer: usize, of: usize) -> Option<MemberState> {
    cluster
        .table(observer)?
        .member(&cluster.node_id(of))
        .map(|m| m.state)
}

fn held(node: &TestNode, topic: &str, ids: &[String]) -> usize {
    let messages = messages_of(node);
    ids.iter()
        .filter(|id| messages.message_info(topic, id).unwrap().is_some())
        .count()
}

fn owned(node: &TestNode, topic: &str, ids: &[String]) -> usize {
    let messages = messages_of(node);
    ids.iter()
        .filter(|id| {
            messages
                .message_info(topic, id)
                .unwrap()
                .is_some_and(|info| info.owned)
        })
        .count()
}

async fn two_nodes_with_messages(count: usize, env: &[(&str, &str)]) -> (TestCluster, Vec<String>) {
    let cluster = TestCluster::start_with(2, env).await;
    create_queue(cluster.node(1), "pedidos").await;
    let mut ids = Vec::new();
    for n in 0..count {
        ids.push(publish(cluster.node(1 + n % 2), "pedidos", json!({"n": n})).await);
    }
    (cluster, ids)
}

/// UC: UC-SIN-001, UC-SIN-002, UC-SIN-005, UC-SIN-006, UC-SIN-012, UC-ING-021, UC-ING-022,
/// UC-ING-024, UC-SIN-019, UC-ARM-015
#[tokio::test]
async fn a_new_node_loads_its_blocks_but_does_not_deliver_until_it_is_ready() {
    let (mut cluster, ids) = two_nodes_with_messages(40, &[]).await;
    let number = start_isolated(&mut cluster, &[]).await;
    let gate = pause_sync(&cluster, number, "replay");
    wait_phase(cluster.node(number), "replay").await;
    let node3 = cluster.node(number);

    // O nó consta na tabela, em sincronização, e ainda não atende: o `/health/ready` diz por quê.
    assert_eq!(node3.node.shared().state(), NodeState::Syncing);
    assert_eq!(
        member_state(&cluster, 1, number),
        Some(MemberState::Syncing),
        "os outros o enxergam em sincronização"
    );
    let ready = node3.client_get_ops("/health/ready").await;
    assert_eq!(ready.status(), 503);
    assert_eq!(ready.json::<Value>().await.unwrap()["reason"], "syncing");

    // A carga já gravou as mensagens dos blocos dele (com fator 3 e 3 nós, são todas), mas ele
    // não é dono de nenhuma: não entrega.
    assert_eq!(held(node3, "pedidos", &ids), ids.len());
    assert_eq!(owned(node3, "pedidos", &ids), 0);
    let definition = node3.client_get("/topics/pedidos").await;
    assert_eq!(
        definition.status(),
        200,
        "a chamada de cliente foi repassada a outro nó"
    );
    let got = consume(node3, "pedidos", "maxMessages=5&visibilityTimeout=300").await;
    assert_eq!(got.len(), 5, "o GET repassado entrega pelos outros donos");

    gate.add_permits(1);
    common::cluster::wait_joined(node3).await;
    assert_eq!(node3.node.shared().state(), NodeState::Active);
    assert_eq!(node3.client_get_ops("/health/ready").await.status(), 200);
    // Pronto, ele é o dono das mensagens dos blocos de que é primário.
    eventually("o nó ser dono das mensagens dos blocos dele", || {
        owned(node3, "pedidos", &ids) > 0
    })
    .await;
    let owners: usize = (1..=3)
        .map(|n| owned(cluster.node(n), "pedidos", &ids))
        .sum();
    assert!(
        owners >= ids.len() - 5,
        "cada mensagem tem um dono só: {owners}"
    );
    for n in 1..=3 {
        assert_eq!(
            member_state(&cluster, n, number),
            Some(MemberState::Active),
            "o nó {n} sabe que ele ficou pronto"
        );
    }
    assert!(
        !node3.storage_dir().join("_sync").exists(),
        "o banco de eventos é descartado"
    );
    cluster.shutdown().await;
}

/// UC: UC-SIN-010, UC-SIN-011, UC-SIN-013, UC-SIN-014, UC-SIN-015, UC-SIN-017, UC-SIN-018,
/// UC-SIN-007, UC-ING-022
#[tokio::test]
async fn events_that_arrive_during_the_sync_are_logged_and_replayed_over_the_load() {
    let (mut cluster, _ids) = two_nodes_with_messages(12, &[]).await;
    // Uma mensagem entregue (invisível) antes da entrada: o ack virá durante a sincronização.
    let delivered = consume(
        cluster.node(1),
        "pedidos",
        "maxMessages=1&visibilityTimeout=300",
    )
    .await;
    let (acked_id, token) = (
        delivered[0]["id"].as_str().unwrap().to_owned(),
        delivered[0]["receiptToken"].as_str().unwrap().to_owned(),
    );

    let number = start_isolated(&mut cluster, &[]).await;
    let gate = pause_sync(&cluster, number, "replay");
    wait_phase(cluster.node(number), "replay").await;
    let node3 = cluster.node(number);
    let syncer = node3
        .node
        .cluster()
        .and_then(|c| c.replication())
        .unwrap()
        .syncer
        .clone();
    assert!(syncer.is_recording());
    assert_eq!(
        held(node3, "pedidos", std::slice::from_ref(&acked_id)),
        1,
        "a carga trouxe a mensagem que será confirmada"
    );

    // Durante a sincronização: um ack, uma publicação nova e um tópico novo. O nó em sincronização
    // confirma o registro gravando o evento, então tudo responde como se ele fosse um nó normal.
    assert_eq!(
        ack(cluster.node(2), "pedidos", &acked_id, &token).await,
        204
    );
    let fresh: Vec<String> = {
        let mut out = Vec::new();
        for n in 0..6 {
            out.push(publish(cluster.node(1 + n % 2), "pedidos", json!({"novo": n})).await);
        }
        out
    };
    create_queue(cluster.node(1), "extras").await;
    let metrics = node3.metrics().await;
    assert!(
        common::series(&metrics, "taba_sync_log_events").unwrap() >= 3.0,
        "os eventos esperam no banco: {metrics}"
    );
    assert!(node3.storage_dir().join("_sync").join("sync.db").is_file());
    assert_eq!(
        held(node3, "pedidos", &fresh),
        0,
        "o nó em sincronização não processa os eventos: só os guarda"
    );

    gate.add_permits(1);
    common::cluster::wait_joined(node3).await;
    let messages = messages_of(node3);
    // O ack venceu a carga: a mensagem que ela trouxe velha sumiu.
    assert!(
        messages
            .message_info("pedidos", &acked_id)
            .unwrap()
            .is_none()
    );
    // A publicação feita durante a sincronização chegou por evento.
    assert_eq!(held(node3, "pedidos", &fresh), fresh.len());
    // O tópico criado durante a sincronização também.
    assert_eq!(node3.client_get("/topics/extras").await.status(), 200);
    // O banco esvaziou, o nó passou ao modo normal e já processa em tempo real.
    assert!(!syncer.is_recording());
    let after = publish(cluster.node(1), "pedidos", json!({"depois": true})).await;
    eventually("o evento em tempo real", || {
        messages.message_info("pedidos", &after).unwrap().is_some()
    })
    .await;
    assert_eq!(
        common::series(&node3.metrics().await, "taba_sync_log_events"),
        Some(0.0)
    );
    cluster.shutdown().await;
}

/// UC: UC-CNC-001, UC-CNC-002, UC-CNC-003, UC-CNC-006
#[tokio::test]
async fn no_other_membership_change_takes_effect_until_the_syncing_node_is_ready() {
    let (mut cluster, _ids) = two_nodes_with_messages(6, &[]).await;
    let third = start_isolated(&mut cluster, &[]).await;
    let gate = pause_sync(&cluster, third, "replay");
    wait_phase(cluster.node(third), "replay").await;

    // Um quarto nó pede para entrar, e o dreno do nó 2 pede para sair: os dois esperam.
    let fourth = {
        let builder = cluster.builder(4, false);
        cluster.spawn_node(builder).await.expect("sobe")
    };
    let drain = cluster
        .node(2)
        .client
        .post(cluster.node(2).operations_url("/node/drain"))
        .send()
        .await
        .unwrap();
    assert_eq!(drain.status(), 202);
    tokio::time::sleep(Duration::from_millis(600)).await;
    let table = cluster.table(1).unwrap();
    assert_eq!(table.members.len(), 3, "nada mudou: {table:?}");
    assert!(table.member(&cluster.node_id(fourth)).is_none());
    assert!(table.member(&cluster.node_id(2)).is_some());
    assert_ne!(
        cluster.node(fourth).node.shared().state(),
        NodeState::Active
    );

    // O nó fica pronto e as duas mudanças seguem, uma de cada vez.
    gate.add_permits(1);
    eventually("as duas mudanças de membros", || {
        cluster.table(1).is_some_and(|t| {
            t.member(&cluster.node_id(fourth)).is_some() && t.member(&cluster.node_id(2)).is_none()
        })
    })
    .await;
    common::cluster::wait_joined(cluster.node(fourth)).await;
    cluster.shutdown().await;
}

/// UC: UC-SIN-025, UC-SIN-026, UC-SIN-027, UC-SIN-028, UC-OPS-038
#[tokio::test]
async fn a_message_that_is_listed_but_has_no_content_anywhere_is_a_loss_and_the_sync_goes_on() {
    // Com a memória no mínimo, as mensagens vão para o disco, e dá para apagar um corpo.
    let (mut cluster, ids) = two_nodes_with_messages(8, &[("TABA_MEMORY_LIMIT", "1B")]).await;
    let target = ids[3].clone();
    for number in 1..=2 {
        let folder = cluster.node(number).storage_dir().join("pedidos");
        let body = folder.join(format!("{target}.body"));
        assert!(body.is_file(), "a mensagem está em disco no nó {number}");
        std::fs::remove_file(body).unwrap();
    }
    let number = start_isolated(&mut cluster, &[("TABA_MEMORY_LIMIT", "1B")]).await;
    cluster.network.restore(&host_of(number));
    common::cluster::wait_joined(cluster.node(number)).await;

    // As outras mensagens foram carregadas; a que ficou sem conteúdo é uma perda, registrada no
    // banco do nó, e a sincronização não parou nem o nó entrou em falha.
    let node3 = cluster.node(number);
    let others: Vec<String> = ids.iter().filter(|id| **id != target).cloned().collect();
    assert_eq!(held(node3, "pedidos", &others), others.len());
    assert_eq!(held(node3, "pedidos", std::slice::from_ref(&target)), 0);
    assert_eq!(node3.node.shared().state(), NodeState::Active);
    let db = taba_node::storage::db::NodeDb::open(
        &node3.storage_dir(),
        taba_node::config::DbSynchronous::Normal,
    )
    .unwrap();
    let losses = taba_node::storage::losses::LossStore::new(Arc::new(db))
        .list(None, 10)
        .unwrap();
    assert_eq!(losses.len(), 1, "{losses:?}");
    assert_eq!(losses[0].id, target);
    assert_eq!(losses[0].topic, "pedidos");

    // Quem detecta a perda a manda também aos outros responsáveis pelo bloco, para a informação
    // sobreviver à queda dele (doc 04, seção 6.6, "Tabela de perdas").
    for other in 1..=2 {
        let recovery = cluster
            .node(other)
            .node
            .cluster()
            .and_then(|c| c.replication())
            .map(|r| r.recovery.clone())
            .expect("o nó está no cluster");
        eventually("a perda chegar ao outro responsável", || {
            recovery
                .losses()
                .list(None, 10)
                .unwrap()
                .iter()
                .any(|l| l.id == target && l.topic == "pedidos")
        })
        .await;
    }
    cluster.shutdown().await;
}

/// UC: UC-SIN-020, UC-SIN-023
#[tokio::test]
async fn a_node_with_no_source_for_a_load_fails_after_the_timeout() {
    let (mut cluster, _ids) = two_nodes_with_messages(6, &[]).await;
    let number = start_isolated(&mut cluster, &[("TABA_CLUSTER_SYNC_TIMEOUT", "300ms")]).await;
    // Ninguém serve a listagem de IDs.
    for host in [host_of(1), host_of(2)] {
        cluster.network.lose_kind(&host, "block-ids", 1_000_000);
    }
    cluster.network.restore(&host_of(number));
    let node3 = cluster.node(number);
    let shared = node3.node.shared().clone();
    eventually("o nó entrar em falha", move || {
        shared.state() == NodeState::Failed
    })
    .await;
    let ready = node3.client_get_ops("/health/ready").await;
    assert_eq!(ready.status(), 503);
    assert_eq!(ready.json::<Value>().await.unwrap()["reason"], "failed");
    let metrics = node3.metrics().await;
    assert!(
        common::series(
            &metrics,
            "taba_sync_failures_total{taba_reason=\"no-source\"}"
        )
        .is_some_and(|n| n >= 1.0),
        "{metrics}"
    );
    // O nó em falha ainda consta como em sincronização e travaria a saída dos outros (até a
    // votação de exclusão existir): ele sai primeiro.
    cluster.nodes.pop().unwrap().shutdown().await;
    cluster.shutdown().await;
}

/// UC: UC-SIN-002, UC-SIN-009, UC-SIN-003, UC-SIN-004
#[tokio::test]
async fn the_block_routes_list_ids_in_order_and_serve_ranges_and_single_messages() {
    // Mensagens de sobra: com poucas, o sorteio dos IDs podia não pôr duas no mesmo bloco.
    let (cluster, ids) = two_nodes_with_messages(150, &[]).await;
    let service = cluster.node(1).node.cluster().unwrap().clone();
    let ring = service.membership().ring();
    let caller = host_of(2);

    // Todos os blocos juntos listam todas as mensagens, cada uma no bloco da sua posição.
    let mut listed = Vec::new();
    for block in ring.blocks() {
        let reply = service
            .handle(
                &caller,
                Request::BlockIds {
                    block: block.to_string(),
                    query: StreamQuery::default(),
                },
            )
            .await
            .unwrap();
        let Response::Stream(page) = reply else {
            panic!("esperava um stream")
        };
        let block_ids: Vec<String> = page
            .items
            .iter()
            .map(|i| i["id"].as_str().unwrap().to_owned())
            .collect();
        assert!(block_ids.windows(2).all(|w| w[0] < w[1]), "em ordem de ID");
        for id in &block_ids {
            assert_eq!(ring.block_of(message_position(id)), Some(block));
        }
        assert!(page.items.iter().all(|i| i["topic"] == "pedidos"));
        assert!(!page.end.has_more);
        listed.extend(block_ids);
    }
    listed.sort();
    let mut expected = ids.clone();
    expected.sort();
    assert_eq!(listed, expected);

    // Um bloco com várias mensagens: paginação por `limit`, retomada por `from` e faixa por `to`.
    let busiest = ring
        .blocks()
        .max_by_key(|b| {
            ids.iter()
                .filter(|id| ring.block_of(message_position(id)) == Some(*b))
                .count()
        })
        .unwrap();
    let in_block: Vec<String> = {
        let mut v: Vec<String> = ids
            .iter()
            .filter(|id| ring.block_of(message_position(id)) == Some(busiest))
            .cloned()
            .collect();
        v.sort();
        v
    };
    assert!(in_block.len() >= 2);
    let Response::Stream(first) = service
        .handle(
            &caller,
            Request::BlockMessages {
                block: busiest.to_string(),
                query: StreamQuery {
                    from: None,
                    to: None,
                    limit: Some(2),
                },
            },
        )
        .await
        .unwrap()
    else {
        panic!("esperava um stream")
    };
    assert_eq!(first.items.len(), 2);
    assert_eq!(first.end.has_more, in_block.len() > 2);
    assert_eq!(first.items[0]["id"], in_block[0].as_str());
    assert!(
        first.items[0]["body"].is_object(),
        "o corpo JSON vai nativo"
    );
    let Response::Stream(range) = service
        .handle(
            &caller,
            Request::BlockMessages {
                block: busiest.to_string(),
                query: StreamQuery {
                    from: Some(in_block[1].clone()),
                    to: Some(in_block[1].clone()),
                    limit: None,
                },
            },
        )
        .await
        .unwrap()
    else {
        panic!("esperava um stream")
    };
    assert_eq!(range.items.len(), 1);
    assert_eq!(range.items[0]["id"], in_block[1].as_str());

    // A mensagem avulsa: `200` no bloco certo, `404` no outro e num ID que não existe.
    let Response::Record(record) = service
        .handle(
            &caller,
            Request::BlockMessage {
                block: busiest.to_string(),
                id: in_block[0].clone(),
            },
        )
        .await
        .unwrap()
    else {
        panic!("esperava a mensagem")
    };
    assert_eq!(record.id, in_block[0]);
    assert_eq!(record.topic, "pedidos");
    let other_block = ring.blocks().find(|b| *b != busiest).unwrap();
    for (block, id) in [
        (other_block, in_block[0].clone()),
        (busiest, taba_node::core::id::new_at(1)),
    ] {
        let error = service
            .handle(
                &caller,
                Request::BlockMessage {
                    block: block.to_string(),
                    id,
                },
            )
            .await
            .unwrap_err();
        assert_eq!(error.code.as_str(), "message.not-found");
    }
    // Bloco malformado ou que o anel não tem; e quem não é membro.
    let bad = service
        .handle(
            &caller,
            Request::BlockIds {
                block: "xyz".into(),
                query: StreamQuery::default(),
            },
        )
        .await
        .unwrap_err();
    assert_eq!(bad.status(), 400);
    let unknown = service
        .handle(
            &caller,
            Request::BlockIds {
                block: "0000000000000001".into(),
                query: StreamQuery::default(),
            },
        )
        .await
        .unwrap_err();
    assert_eq!(unknown.status(), 409);
    let stranger = service
        .handle(
            "192.0.2.9",
            Request::BlockIds {
                block: busiest.to_string(),
                query: StreamQuery::default(),
            },
        )
        .await
        .unwrap_err();
    assert_eq!(stranger.code.as_str(), "cluster.not-member");
    cluster.shutdown().await;
}

/// UC: UC-ARM-022, UC-ARM-023, UC-SIN-021
#[tokio::test]
async fn a_node_that_lost_a_block_keeps_its_messages_until_the_whole_cluster_is_ready() {
    // Fator 2: com a entrada do terceiro nó, parte dos blocos deixa de ser dos dois primeiros.
    let (mut cluster, ids) = two_nodes_with_messages(40, &[("TABA_REPLICATION_FACTOR", "2")]).await;
    assert_eq!(held(cluster.node(1), "pedidos", &ids), 40);
    assert_eq!(held(cluster.node(2), "pedidos", &ids), 40);

    let number = start_isolated(&mut cluster, &[("TABA_REPLICATION_FACTOR", "2")]).await;
    let gate = pause_sync(&cluster, number, "replay");
    wait_phase(cluster.node(number), "replay").await;
    // Com o nó novo em sincronização, ninguém apaga nada: se ele falhar, o anel volta ao anterior.
    tokio::time::sleep(Duration::from_millis(700)).await;
    assert_eq!(held(cluster.node(1), "pedidos", &ids), 40);
    assert_eq!(held(cluster.node(2), "pedidos", &ids), 40);

    gate.add_permits(1);
    common::cluster::wait_joined(cluster.node(number)).await;
    // Íntegro o cluster, cada nó fica só com as mensagens dos blocos de que é responsável.
    let ring = cluster.node(1).node.cluster().unwrap().membership().ring();
    for n in 1..=3 {
        let me = cluster.node_id(n);
        let wanted: Vec<String> = ids
            .iter()
            .filter(|id| ring.owners(message_position(id), 2).contains(&me.as_str()))
            .cloned()
            .collect();
        let node = cluster.node(n);
        let ids_all = ids.clone();
        eventually("o nó guardar só o que é dele", || {
            held(node, "pedidos", &ids_all) == wanted.len()
        })
        .await;
        assert_eq!(held(node, "pedidos", &wanted), wanted.len());
    }
    let total: usize = (1..=3)
        .map(|n| held(cluster.node(n), "pedidos", &ids))
        .sum();
    assert_eq!(total, 80, "cada mensagem em 2 nós");
    cluster.shutdown().await;
}

/// UC: UC-SIN-004, UC-SIN-005
#[tokio::test]
async fn the_load_is_the_union_of_the_lists_of_the_sources_not_their_intersection() {
    let (mut cluster, mut ids) = two_nodes_with_messages(10, &[]).await;
    // Uma cópia que só o nó 1 recebeu (por exemplo, o commit nunca chegou ao nó 2).
    let only_on_one = {
        let node = cluster.node(1);
        let messages = messages_of(node);
        let view = messages.queue_view("pedidos").unwrap();
        let request = taba_node::engine::messages::PublishRequest {
            body: br#"{"so":"no-1"}"#.to_vec(),
            ..Default::default()
        };
        let (meta, body) = messages.draft_publish(&view, &request).unwrap();
        let id = meta.id.clone();
        messages.apply_publish("pedidos", meta, body).unwrap();
        id
    };
    assert_eq!(
        held(
            cluster.node(2),
            "pedidos",
            std::slice::from_ref(&only_on_one)
        ),
        0
    );
    ids.push(only_on_one.clone());

    let number = start_isolated(&mut cluster, &[]).await;
    cluster.network.restore(&host_of(number));
    common::cluster::wait_joined(cluster.node(number)).await;
    assert_eq!(
        held(cluster.node(number), "pedidos", &ids),
        ids.len(),
        "o que só uma origem tem também é carregado"
    );
    cluster.shutdown().await;
}

/// UC: UC-SIN-007, UC-SIN-012
#[tokio::test]
async fn a_message_of_a_topic_created_after_the_topic_load_waits_for_the_definition() {
    let (mut cluster, _ids) = two_nodes_with_messages(4, &[]).await;
    let number = start_isolated(&mut cluster, &[]).await;
    let gate = pause_sync(&cluster, number, "blocks");
    wait_phase(cluster.node(number), "blocks").await;

    // O nó já puxou as definições; este tópico (e a mensagem dele) nasce depois.
    create_queue(cluster.node(1), "novo").await;
    let late = publish(cluster.node(2), "novo", json!({"tarde": true})).await;

    gate.add_permits(1);
    common::cluster::wait_joined(cluster.node(number)).await;
    let node3 = cluster.node(number);
    assert_eq!(node3.client_get("/topics/novo").await.status(), 200);
    assert_eq!(held(node3, "novo", std::slice::from_ref(&late)), 1);
    cluster.shutdown().await;
}

/// UC: UC-SIN-016, UC-SIN-029
#[tokio::test]
async fn the_final_check_drops_what_the_load_brought_and_no_source_lists_anymore() {
    let (mut cluster, ids) = two_nodes_with_messages(12, &[]).await;
    let number = start_isolated(&mut cluster, &[]).await;
    let gate = pause_sync(&cluster, number, "verification");
    wait_phase(cluster.node(number), "verification").await;
    let node3 = cluster.node(number);
    assert_eq!(
        held(node3, "pedidos", &ids),
        ids.len(),
        "a carga trouxe tudo"
    );

    // Uma confirmação cujo pedido nunca chegou ao nó novo: some das origens sem passar pelo banco
    // de eventos.
    let gone = ids[5].clone();
    for n in 1..=2 {
        messages_of(cluster.node(n))
            .apply_ack("pedidos", &gone)
            .unwrap();
    }
    gate.add_permits(1);
    common::cluster::wait_joined(node3).await;
    assert_eq!(held(node3, "pedidos", std::slice::from_ref(&gone)), 0);
    let others: Vec<String> = ids.iter().filter(|id| **id != gone).cloned().collect();
    assert_eq!(held(node3, "pedidos", &others), others.len());
    cluster.shutdown().await;
}
