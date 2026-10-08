//! A fila FIFO no cluster (doc 17, seções 16.3 a 16.7; doc 20a, UC-FIF; nível **C**): o grupo decide
//! o bloco, a publicação é coordenada pelo primário do grupo, o quórum inclui o primário e o
//! secundário, e a deduplicação e a ordem sobrevivem à troca de primário.

mod common;

use std::time::Duration;

use reqwest::Response;
use serde_json::{Value, json};

use common::TestNode;
use common::cluster::{TestCluster, host_of, settled, total};
use taba_node::core::ring::fifo_position;

const TOPIC: &str = "pedidos";

async fn fifo_queue(node: &TestNode, name: &str) -> String {
    let response = node
        .client
        .post(node.client_url("/topics"))
        .json(&json!({ "name": name, "type": "queue", "fifo": true }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 201, "{:?}", response.text().await);
    response.json::<Value>().await.unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

async fn publish(node: &TestNode, group: &str, dedup: Option<&str>, body: &str) -> Response {
    let mut query = format!("?groupId={group}");
    if let Some(key) = dedup {
        query.push_str(&format!("&deduplicationId={key}"));
    }
    node.client
        .post(node.client_url(&format!("/topics/{TOPIC}/messages{query}")))
        .header("content-type", "application/json")
        .body(body.to_owned())
        .send()
        .await
        .unwrap()
}

async fn get(node: &TestNode, max: u32) -> Vec<Value> {
    let response = node
        .client_get(&format!(
            "/topics/{TOPIC}/messages?maxMessages={max}&visibilityTimeout=600"
        ))
        .await;
    assert_eq!(response.status(), 200);
    response.json::<Value>().await.unwrap()["messages"]
        .as_array()
        .unwrap()
        .clone()
}

/// Espera o `GET` trazer algo (o aviso entre nós leva um instante).
async fn get_some(node: &TestNode, max: u32) -> Vec<Value> {
    for _ in 0..80 {
        let got = get(node, max).await;
        if !got.is_empty() {
            return got;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Vec::new()
}

async fn ack(node: &TestNode, message: &Value) -> Response {
    node.client
        .delete(node.client_url(&format!(
            "/topics/{TOPIC}/messages/{}?receiptToken={}",
            message["id"].as_str().unwrap(),
            message["receiptToken"].as_str().unwrap()
        )))
        .send()
        .await
        .unwrap()
}

/// O número do nó que é o primário do grupo (e o do secundário), pela tabela do nó 1.
fn owners_of_group(cluster: &TestCluster, topic_id: &str, group: &str) -> Vec<usize> {
    let service = cluster.node(1).node.cluster().unwrap();
    let owners = service.owners_of(fifo_position(topic_id, group)).unwrap();
    owners
        .iter()
        .map(|m| {
            (1..=3)
                .find(|n| cluster.node_id(*n) == m.node_id)
                .expect("é um dos nós do teste")
        })
        .collect()
}

/// UC: UC-FIF-026, UC-FIF-010, UC-FIF-002, UC-FIF-015, UC-FIF-044, UC-FIF-047, UC-FWD-011
#[tokio::test]
async fn a_publication_from_any_node_is_coordinated_by_the_primary_of_the_group() {
    let cluster = TestCluster::start(3).await;
    settled(&cluster).await;
    let topic_id = fifo_queue(cluster.node(1), TOPIC).await;
    let owners = owners_of_group(&cluster, &topic_id, "g1");
    let primary = owners[0];
    let others: Vec<usize> = (1..=3).filter(|n| *n != primary).collect();

    // Publicar no próprio primário não repassa; nos outros, um repasse cada.
    cluster.network.clear_calls();
    let first = publish(cluster.node(primary), "g1", None, r#"{"n":1}"#).await;
    assert_eq!(first.status(), 201);
    assert_eq!(cluster.network.count("fifo-publish"), 0);
    let mut ids = vec![
        first.json::<Value>().await.unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned(),
    ];
    for (round, node) in others.iter().enumerate() {
        let before = cluster.network.count("fifo-publish");
        let response = publish(
            cluster.node(*node),
            "g1",
            None,
            &format!(r#"{{"n":{}}}"#, round + 2),
        )
        .await;
        assert_eq!(response.status(), 201);
        assert_eq!(cluster.network.count("fifo-publish"), before + 1);
        let body: Value = response.json().await.unwrap();
        assert_eq!(body["duplicate"], false);
        ids.push(body["id"].as_str().unwrap().to_owned());
    }
    // Os `id` do grupo crescem na ordem em que o primário aceitou.
    assert!(ids.windows(2).all(|w| w[0] < w[1]), "{ids:?}");

    // Repetir o conteúdo no mesmo grupo, de qualquer nó, devolve a original.
    let again = publish(cluster.node(others[0]), "g1", None, r#"{"n":1}"#).await;
    assert_eq!(again.status(), 200);
    let body: Value = again.json().await.unwrap();
    assert_eq!(
        (body["id"].as_str().unwrap(), body["duplicate"].as_bool()),
        (ids[0].as_str(), Some(true))
    );

    // A entrega é uma por vez, na ordem, de qualquer nó.
    // O recibo leva a posição do grupo: um recibo malformado é `409`, de qualquer nó.
    let stale = cluster
        .node(others[0])
        .client
        .delete(cluster.node(others[0]).client_url(&format!(
            "/topics/{TOPIC}/messages/{}?receiptToken=malformado",
            ids[0]
        )))
        .send()
        .await
        .unwrap();
    assert_eq!(code_of(stale, 409).await, "message.stale-receipt");

    for expected in &ids {
        let got = get_some(cluster.node(others[1]), 10).await;
        assert_eq!(got.len(), 1, "um grupo, uma mensagem em voo: {got:?}");
        assert_eq!(got[0]["id"], *expected);
        assert_eq!(ack(cluster.node(others[0]), &got[0]).await.status(), 204);
    }
    assert!(get(cluster.node(1), 10).await.is_empty());

    // `taba.fifo.forward` conta os repasses ao primário do grupo (um por nó que não o é).
    let mut forwarded = 0.0;
    for number in 1..=3 {
        forwarded += total(
            &cluster.node(number).metrics().await,
            "taba_fifo_forward_total",
            "taba_outcome=\"ok\"",
        );
    }
    assert_eq!(forwarded, 3.0, "duas publicações repassadas e a duplicata");
    cluster.shutdown().await;
}

async fn code_of(response: Response, status: u16) -> String {
    assert_eq!(response.status(), status);
    response.json::<Value>().await.unwrap()["code"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn messages_of(node: &TestNode) -> std::sync::Arc<taba_node::engine::messages::Messages> {
    node.node
        .cluster()
        .and_then(|c| c.replication())
        .map(|r| r.messages.messages().clone())
        .expect("o nó está no cluster")
}

/// UC: UC-FIF-030, UC-FIF-032, UC-FIF-031
#[tokio::test]
async fn a_primary_or_a_secondary_that_is_down_but_still_in_the_table_stops_the_group() {
    let cluster = TestCluster::start_with(3, common::cluster::NO_VOTE).await;
    settled(&cluster).await;
    let topic_id = fifo_queue(cluster.node(1), TOPIC).await;
    let owners = owners_of_group(&cluster, &topic_id, "g1");
    let (primary, secondary, third) = (owners[0], owners[1], owners[2]);

    // Primário fora do ar e ainda na tabela: a publicação do grupo, de qualquer nó, é `503`.
    cluster.network.isolate(&host_of(primary));
    let blocked = publish(cluster.node(third), "g1", None, r#"{"n":1}"#).await;
    assert_eq!(code_of(blocked, 503).await, "fifo.primary-unavailable");
    cluster.network.restore(&host_of(primary));

    // Secundário fora do ar e ainda na tabela: a escrita, a entrega e o ack do grupo respondem
    // `503`, mesmo com o terciário saudável (o quórum inclui o secundário).
    let ok = publish(cluster.node(primary), "g1", None, r#"{"n":1}"#).await;
    assert_eq!(ok.status(), 201);
    let delivered = get_some(cluster.node(primary), 10).await;
    assert_eq!(delivered.len(), 1);
    cluster.network.isolate(&host_of(secondary));
    let write = publish(cluster.node(primary), "g1", None, r#"{"n":2}"#).await;
    assert_eq!(code_of(write, 503).await, "cluster.quorum-unavailable");
    let ack_result = ack(cluster.node(primary), &delivered[0]).await;
    assert_eq!(ack_result.status(), 503);
    cluster.network.restore(&host_of(secondary));

    // Voltou: tudo segue (nada ficou pendente).
    assert_eq!(ack(cluster.node(third), &delivered[0]).await.status(), 204);
    let after = publish(cluster.node(third), "g1", None, r#"{"n":3}"#).await;
    assert_eq!(after.status(), 201);
    cluster.shutdown().await;
}

/// UC: UC-FIF-033, UC-FIF-034, UC-FIF-035
#[tokio::test]
async fn when_the_primary_leaves_the_secondary_takes_the_group_without_reordering_or_forgetting() {
    let cluster = TestCluster::start(3).await;
    settled(&cluster).await;
    let topic_id = fifo_queue(cluster.node(1), TOPIC).await;
    let owners = owners_of_group(&cluster, &topic_id, "g1");
    let (primary, secondary, third) = (owners[0], owners[1], owners[2]);

    let mut ids = Vec::new();
    for n in 1..=3 {
        let response = publish(cluster.node(third), "g1", None, &format!(r#"{{"n":{n}}}"#)).await;
        assert_eq!(response.status(), 201);
        ids.push(
            response.json::<Value>().await.unwrap()["id"]
                .as_str()
                .unwrap()
                .to_owned(),
        );
    }
    // m1 é confirmada, e m2 sai em voo (a invisibilidade vale 600 s).
    let first = get_some(cluster.node(third), 10).await;
    assert_eq!(first[0]["id"], ids[0]);
    assert_eq!(ack(cluster.node(third), &first[0]).await.status(), 204);
    let second = get_some(cluster.node(third), 10).await;
    assert_eq!(second[0]["id"], ids[1]);

    // O primário cai e é excluído; o secundário herda o grupo.
    let old = cluster.node_id(primary);
    cluster.network.isolate(&host_of(primary));
    common::cluster::eventually("o primário ser excluído", || {
        [secondary, third]
            .iter()
            .all(|n| !cluster.member_ids(*n).contains(&old))
    })
    .await;

    // Sem reordenação: m1 não volta, e m2 (em voo) segue em voo; nada é entregue ainda.
    assert!(get(cluster.node(secondary), 10).await.is_empty());
    assert!(get(cluster.node(third), 10).await.is_empty());

    // A deduplicação foi herdada: repetir m3 devolve a original.
    let duplicate = publish(cluster.node(secondary), "g1", None, r#"{"n":3}"#).await;
    assert_eq!(duplicate.status(), 200);
    let body: Value = duplicate.json().await.unwrap();
    assert_eq!(body["id"], ids[2]);
    assert_eq!(body["duplicate"], true);
    // E m1, já confirmada, também: a linha curta existe em todos os nós.
    let again = publish(cluster.node(third), "g1", None, r#"{"n":1}"#).await;
    assert_eq!(again.status(), 200);

    // O `id` do grupo continua crescendo.
    let next = publish(cluster.node(third), "g1", None, r#"{"n":4}"#).await;
    assert_eq!(next.status(), 201);
    let next_id = next.json::<Value>().await.unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(next_id > ids[2]);

    // Vencida a invisibilidade, m2 volta como cabeça do grupo, depois m3 e m4, uma por vez.
    cluster.clock.advance(Duration::from_secs(601));
    let redelivered = get_some(cluster.node(third), 10).await;
    assert_eq!(redelivered.len(), 1);
    assert_eq!(redelivered[0]["id"], ids[1]);
    assert_eq!(
        ack(cluster.node(secondary), &redelivered[0]).await.status(),
        204
    );
    for expected in [&ids[2], &next_id] {
        let got = get_some(cluster.node(secondary), 10).await;
        assert_eq!(got[0]["id"], *expected);
        assert_eq!(ack(cluster.node(third), &got[0]).await.status(), 204);
    }
    cluster.shutdown().await;
}

/// UC: UC-FIF-009
#[tokio::test]
async fn simultaneous_publications_with_the_same_key_create_one_message() {
    let cluster = TestCluster::start(3).await;
    settled(&cluster).await;
    fifo_queue(cluster.node(1), TOPIC).await;
    let mut set = tokio::task::JoinSet::new();
    for round in 0..9 {
        let node = cluster.node(1 + round % 3);
        let client = node.client.clone();
        let url = node.client_url(&format!(
            "/topics/{TOPIC}/messages?groupId=g7&deduplicationId=mesma"
        ));
        set.spawn(async move {
            let response = client
                .post(url)
                .header("content-type", "application/json")
                .body(r#"{"a":1}"#)
                .send()
                .await
                .unwrap();
            let status = response.status().as_u16();
            let body: Value = response.json().await.unwrap();
            (status, body["id"].as_str().unwrap().to_owned())
        });
    }
    let mut results = Vec::new();
    while let Some(done) = set.join_next().await {
        results.push(done.unwrap());
    }
    let created = results.iter().filter(|(status, _)| *status == 201).count();
    assert_eq!(created, 1, "{results:?}");
    assert!(results.iter().all(|(_, id)| *id == results[0].1));
    cluster.shutdown().await;
}

/// UC: UC-FIF-028
#[tokio::test]
async fn a_node_that_is_not_the_primary_of_the_group_answers_421() {
    use taba_node::cluster::protocol::{FIFO_PLACEHOLDER_ID, MessageRecord, Request};
    use taba_node::cluster::transport::{CallError, Transport};

    let cluster = TestCluster::start(3).await;
    settled(&cluster).await;
    let topic_id = fifo_queue(cluster.node(1), TOPIC).await;
    let owners = owners_of_group(&cluster, &topic_id, "g1");
    let not_primary = owners[1];
    let record = MessageRecord {
        id: FIFO_PLACEHOLDER_ID.into(),
        topic: TOPIC.into(),
        body: br#"{"a":1}"#.to_vec(),
        content_type: "application/json".into(),
        group_id: Some("g1".into()),
        deduplication_id: None,
        delivery_count: 0,
        invisible_until_ms: None,
        receipt_token: None,
        dead_lettered_ms: None,
        dead_letter_reason: None,
        delete_at_ms: None,
    };
    let caller = cluster.network.transport(host_of(owners[0]));
    let reply = caller
        .call(
            &common::cluster::address_of(&cluster, not_primary),
            Request::FifoPublish {
                topic: TOPIC.into(),
                record,
            },
        )
        .await;
    match reply {
        Err(CallError::Remote(error)) => {
            assert_eq!(
                error.code,
                taba_node::core::error::ErrorCode::FifoMisdirected
            );
        }
        other => panic!("esperava `fifo.misdirected`: {other:?}"),
    }
    let _ = messages_of(cluster.node(1));
    cluster.shutdown().await;
}

/// Uma publicação feita direto ao motor do nó, como o primário do grupo a decidiria.
fn draft(
    node: &TestNode,
    group: &str,
    key: &str,
    body: &str,
) -> taba_node::engine::messages::Draft {
    let messages = messages_of(node);
    let view = messages.queue_view(TOPIC).expect("a fila existe");
    messages
        .draft_fifo_publish(
            &view,
            &taba_node::engine::messages::PublishRequest {
                body: body.as_bytes().to_vec(),
                content_type: Some("application/json".into()),
                group_id: Some(group.to_owned()),
                deduplication_id: Some(key.to_owned()),
            },
        )
        .expect("rascunho")
}

fn is_duplicate_of(draft: &taba_node::engine::messages::Draft, id: &str) -> bool {
    matches!(draft, taba_node::engine::messages::Draft::Duplicate { id: original } if original == id)
}

/// UC: UC-FIF-029, UC-FIF-038, UC-FIF-041, UC-SIN-022
#[tokio::test]
async fn the_ack_line_lives_in_every_node_and_a_new_primary_only_coordinates_once_ready() {
    let mut cluster = TestCluster::start(2).await;
    settled(&cluster).await;
    fifo_queue(cluster.node(1), TOPIC).await;
    let sent = publish(cluster.node(1), "g1", Some("k1"), r#"{"n":1}"#).await;
    let first_id = sent.json::<Value>().await.unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        publish(cluster.node(2), "g1", Some("k2"), r#"{"n":2}"#)
            .await
            .status(),
        201
    );
    // O ack da primeira deixa a linha curta (`deleteAt`) em todos os nós, decidida por quórum.
    let got = get_some(cluster.node(2), 10).await;
    assert_eq!(got[0]["id"], first_id);
    assert_eq!(ack(cluster.node(1), &got[0]).await.status(), 204);
    for number in [1, 2] {
        common::cluster::eventually("a linha curta em todos os nós", || {
            is_duplicate_of(
                &draft(cluster.node(number), "g1", "k1", r#"{"n":1}"#),
                &first_id,
            )
        })
        .await;
    }

    // Um terceiro nó entra e para antes de ficar pronto; ele já está na tabela.
    let third = common::cluster::start_isolated(&mut cluster, &[]).await;
    let gate = common::cluster::pause_sync(&cluster, third, "replay");
    common::cluster::wait_phase(cluster.node(third), "replay").await;
    let topic_id = {
        let view: Value = cluster
            .node(1)
            .client_get(&format!("/topics/{TOPIC}"))
            .await
            .json()
            .await
            .unwrap();
        view["id"].as_str().unwrap().to_owned()
    };
    let service = cluster.node(1).node.cluster().unwrap();
    let new_id = cluster.node_id(third);
    let group = (0..400)
        .map(|n| format!("novo-{n}"))
        .find(|group| {
            service
                .owners_of(fifo_position(&topic_id, group))
                .unwrap()
                .first()
                .is_some_and(|m| m.node_id == new_id)
        })
        .expect("algum grupo tem o nó novo como primário");

    // Enquanto o primário novo não está pronto, as publicações do grupo são `503`, e os primários
    // antigos não coordenam por ele.
    for number in [1, 2] {
        let blocked = publish(cluster.node(number), &group, None, r#"{"a":1}"#).await;
        assert_eq!(code_of(blocked, 503).await, "fifo.primary-unavailable");
    }

    // Pronto o nó, ele tem a linha curta da mensagem confirmada, levada pela sincronização.
    gate.add_permits(1);
    common::cluster::wait_joined(cluster.node(third)).await;
    common::cluster::eventually("a linha curta no nó novo", || {
        is_duplicate_of(
            &draft(cluster.node(third), "g1", "k1", r#"{"n":1}"#),
            &first_id,
        )
    })
    .await;
    let ok = publish(cluster.node(1), &group, None, r#"{"a":1}"#).await;
    assert_eq!(ok.status(), 201);
    cluster.shutdown().await;
}

/// UC: UC-FIF-025
#[tokio::test]
async fn the_dead_letter_of_a_fifo_stays_in_the_block_of_the_group() {
    let cluster = TestCluster::start(3).await;
    settled(&cluster).await;
    let created = cluster
        .node(1)
        .client
        .post(cluster.node(1).client_url("/topics"))
        .json(&json!({ "name": TOPIC, "type": "queue", "fifo": true, "maxDeliveries": 1 }))
        .send()
        .await
        .unwrap();
    assert_eq!(created.status(), 201);
    let topic_id = created.json::<Value>().await.unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let owners = owners_of_group(&cluster, &topic_id, "g1");
    let sent = publish(cluster.node(owners[2]), "g1", None, r#"{"x":1}"#).await;
    let id = sent.json::<Value>().await.unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let got = get_some(cluster.node(owners[2]), 10).await;
    assert_eq!(got[0]["id"], id);
    // A entrega gastou o único `maxDeliveries`: ao vencer a invisibilidade, vai à dead-letter.
    cluster.clock.advance(Duration::from_secs(601));
    let dead = format!("{TOPIC}-dead-letter");
    for number in 1..=3 {
        let messages = messages_of(cluster.node(number));
        let (dead, id) = (dead.clone(), id.clone());
        common::cluster::eventually("o movimento para a dead-letter", move || {
            messages.message_info(&dead, &id).unwrap().is_some()
        })
        .await;
    }
    // Uma operação só no bloco do grupo da fila de origem: a posição guardada é a do grupo.
    let wanted = fifo_position(&topic_id, "g1");
    for number in 1..=3 {
        let info = messages_of(cluster.node(number))
            .message_info(&dead, &id)
            .unwrap()
            .unwrap();
        assert_eq!(info.meta.position, wanted, "no nó {number}");
    }
    // A dead-letter é uma fila comum: não é FIFO e entrega sem grupo ocupado.
    let in_dead_letter: Value = cluster
        .node(1)
        .client_get(&format!("/topics/{dead}/messages?maxMessages=5"))
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(in_dead_letter["messages"][0]["id"], id);
    cluster.shutdown().await;
}

/// UC: UC-FIF-043
#[tokio::test]
async fn a_distributor_keeps_the_order_by_group_in_a_fifo_target() {
    let cluster = TestCluster::start(3).await;
    settled(&cluster).await;
    fifo_queue(cluster.node(1), TOPIC).await;
    let distributor = cluster
        .node(1)
        .client
        .post(cluster.node(1).client_url("/topics"))
        .json(&json!({ "name": "todos", "type": "distributor", "prefixes": [TOPIC] }))
        .send()
        .await
        .unwrap();
    assert_eq!(distributor.status(), 201, "{:?}", distributor.text().await);
    let mut ids = Vec::new();
    for (round, node) in [1usize, 2, 3, 1].iter().enumerate() {
        let response = cluster
            .node(*node)
            .client
            .post(
                cluster
                    .node(*node)
                    .client_url("/topics/todos/messages?groupId=g1"),
            )
            .header("content-type", "application/json")
            .body(format!(r#"{{"n":{round}}}"#))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let body: Value = response.json().await.unwrap();
        ids.push(body["deliveries"][0]["id"].as_str().unwrap().to_owned());
    }
    assert!(ids.windows(2).all(|w| w[0] < w[1]), "{ids:?}");
    for expected in &ids {
        let got = get_some(cluster.node(2), 10).await;
        assert_eq!(got.len(), 1);
        assert_eq!(got[0]["id"], *expected);
        assert_eq!(ack(cluster.node(3), &got[0]).await.status(), 204);
    }
    cluster.shutdown().await;
}

/// UC: UC-FIF-042, UC-CNS-037
#[tokio::test]
async fn auto_create_accepts_fifo_only_when_the_replication_factor_allows_it() {
    let cluster = TestCluster::start(3).await;
    settled(&cluster).await;
    let auto = cluster
        .node(2)
        .client_get("/topics/auto/messages?autoCreate=true&fifo=true")
        .await;
    assert_eq!(auto.status(), 200);
    for number in 1..=3 {
        let topic: Value = cluster
            .node(number)
            .client_get("/topics/auto")
            .await
            .json()
            .await
            .unwrap();
        assert_eq!(topic["fifo"], true, "no nó {number}");
    }
    cluster.shutdown().await;

    let single = TestCluster::start_with(2, &[("TABA_REPLICATION_FACTOR", "1")]).await;
    settled(&single).await;
    let refused = single
        .node(1)
        .client_get("/topics/auto/messages?autoCreate=true&fifo=true")
        .await;
    assert_eq!(code_of(refused, 422).await, "topic.fifo-needs-replication");
    let created = single
        .node(1)
        .client
        .post(single.node(1).client_url("/topics"))
        .json(&json!({ "name": "fila", "type": "queue", "fifo": true }))
        .send()
        .await
        .unwrap();
    assert_eq!(code_of(created, 422).await, "topic.fifo-needs-replication");
    single.shutdown().await;
}
