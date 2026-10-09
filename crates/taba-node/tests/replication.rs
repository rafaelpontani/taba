//! A replicação no cluster simulado (doc 05, seção 7.2; doc 13; doc 14; doc 20b, nível **C**):
//! definições de tópico em todos os nós, escrita, entrega e ack por quórum nos nós responsáveis.

mod common;

use std::time::Duration;

use serde_json::{Value, json};

use common::cluster::{NO_VOTE, TestCluster, eventually};
use common::{TestNode, error_code};
use taba_node::core::clock::Clock;
use taba_node::core::ring::message_position;

fn messages_of(node: &TestNode) -> std::sync::Arc<taba_node::engine::messages::Messages> {
    node.node
        .cluster()
        .and_then(|c| c.replication())
        .map(|r| r.messages.messages().clone())
        .expect("o nó está no cluster")
}

async fn create_queue(node: &TestNode, name: &str) -> reqwest::Response {
    node.client
        .post(node.client_url("/topics"))
        .json(&json!({"name": name, "type": "queue"}))
        .send()
        .await
        .expect("requisição")
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

/// UC: UC-DEF-025, UC-DEF-029
#[tokio::test]
async fn topic_definitions_reach_every_node_before_the_api_answers() {
    let cluster = TestCluster::start(3).await;

    let response = create_queue(cluster.node(1), "pedidos").await;
    assert_eq!(response.status(), 201);
    // Já no `201` todo nó conhece a definição: o commit foi confirmado em todos.
    for number in 1..=3 {
        let node = cluster.node(number);
        let got = node
            .client
            .get(node.client_url("/topics/pedidos"))
            .send()
            .await
            .unwrap();
        assert_eq!(got.status(), 200, "nó {number}");
    }
    let ids: Vec<Value> = {
        let mut ids = Vec::new();
        for number in 1..=3 {
            let node = cluster.node(number);
            let body: Value = node
                .client
                .get(node.client_url("/topics/pedidos"))
                .send()
                .await
                .unwrap()
                .json()
                .await
                .unwrap();
            ids.push(body["id"].clone());
        }
        ids
    };
    assert!(
        ids.windows(2).all(|w| w[0] == w[1]),
        "o mesmo id em todos: {ids:?}"
    );

    // O nome já existe: `409` sem deixar nada pendente.
    let again = create_queue(cluster.node(2), "pedidos").await;
    assert_eq!(error_code(again, 409).await, "topic.already-exists");

    // Alterar e remover por outros nós.
    let patch = cluster
        .node(2)
        .client
        .patch(cluster.node(2).client_url("/topics/pedidos"))
        .json(&json!({"tags": {"time": "pagamentos"}}))
        .send()
        .await
        .unwrap();
    assert_eq!(patch.status(), 200);
    for number in 1..=3 {
        let node = cluster.node(number);
        let body: Value = node
            .client
            .get(node.client_url("/topics/pedidos"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(body["tags"]["time"], "pagamentos", "nó {number}");
    }
    let del = cluster
        .node(3)
        .client
        .delete(cluster.node(3).client_url("/topics/pedidos"))
        .send()
        .await
        .unwrap();
    assert_eq!(del.status(), 204);
    for number in 1..=3 {
        let node = cluster.node(number);
        let got = node
            .client
            .get(node.client_url("/topics/pedidos"))
            .send()
            .await
            .unwrap();
        assert_eq!(got.status(), 404, "nó {number}");
    }
    cluster.shutdown().await;
}

/// UC: UC-DEF-015, UC-DEF-016
#[tokio::test]
async fn a_new_node_pulls_the_topic_table_after_it_joins() {
    let mut cluster = TestCluster::start(2).await;
    assert_eq!(create_queue(cluster.node(1), "pedidos").await.status(), 201);
    assert_eq!(
        create_queue(cluster.node(2), "pagamentos").await.status(),
        201
    );

    cluster.add_node(false).await;

    let node = cluster.node(3);
    for topic in ["pedidos", "pagamentos"] {
        let got = node
            .client
            .get(node.client_url(&format!("/topics/{topic}")))
            .send()
            .await
            .unwrap();
        assert_eq!(got.status(), 200, "{topic}");
    }
    // A listagem de tópicos veio por stream de um dos membros, não na resposta do ingresso.
    assert!(cluster.network.count("list-topics") >= 1);
    assert!(cluster.network.count("list-tokens") >= 1);
    cluster.shutdown().await;
}

/// UC: UC-ESC-002, UC-ESC-003, UC-ESC-004, UC-ESC-005, UC-ENT-001, UC-ANE-008
#[tokio::test]
async fn a_publication_reaches_every_responsible_node_and_only_the_primary_owns_it() {
    let cluster = TestCluster::start(3).await;
    create_queue(cluster.node(1), "pedidos").await;

    let mut ids = Vec::new();
    for n in 0..12 {
        ids.push(publish(cluster.node(1 + n % 3), "pedidos", json!({"n": n})).await);
    }
    for id in &ids {
        // O fator é 3 e há 3 nós: todos guardam a mensagem (a durabilidade plena vem em seguida).
        for number in 1..=3 {
            let messages = messages_of(cluster.node(number));
            let topic = "pedidos".to_owned();
            let id = id.clone();
            eventually("a réplica guardar a mensagem", move || {
                messages.message_info(&topic, &id).unwrap().is_some()
            })
            .await;
        }
        let ring = cluster.node(1).node.cluster().unwrap().membership().ring();
        let primary = ring.primary(message_position(id)).unwrap().to_owned();
        for number in 1..=3 {
            let info = messages_of(cluster.node(number))
                .message_info("pedidos", id)
                .unwrap()
                .unwrap();
            assert_eq!(
                info.owned,
                cluster.node_id(number) == primary,
                "nó {number}, mensagem {id}"
            );
        }
    }
    // O corpo é o mesmo em todas as cópias.
    let id = &ids[0];
    for number in 1..=3 {
        let (_, body) = messages_of(cluster.node(number))
            .read_message("pedidos", id)
            .unwrap()
            .unwrap();
        assert_eq!(body, br#"{"n":0}"#);
    }
    cluster.shutdown().await;
}

/// UC: UC-CNS-014, UC-ENT-017, UC-ENT-002, UC-INV-006, UC-INV-014
#[tokio::test]
async fn a_get_on_any_node_delivers_what_the_other_nodes_own() {
    let cluster = TestCluster::start(3).await;
    create_queue(cluster.node(1), "pedidos").await;
    let mut published = Vec::new();
    for n in 0..9 {
        published.push(publish(cluster.node(1), "pedidos", json!({"n": n})).await);
    }

    // Um nó só atende o `GET`: o que ele não tem como dono vem do `take` aos outros.
    let mut got: Vec<Value> = Vec::new();
    for _ in 0..40 {
        let more = consume(
            cluster.node(2),
            "pedidos",
            "maxMessages=10&visibilityTimeout=60",
        )
        .await;
        got.extend(more);
        if got.len() >= 9 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    let mut ids: Vec<String> = got
        .iter()
        .map(|m| m["id"].as_str().unwrap().to_owned())
        .collect();
    ids.sort();
    let mut expected = published.clone();
    expected.sort();
    assert_eq!(ids, expected, "cada mensagem entregue uma vez");
    for message in &got {
        assert_eq!(message["deliveryCount"], 1);
        assert!(message["receiptToken"].is_string());
    }

    // Quando o cliente recebeu a mensagem, o quórum (dono mais mais uma cópia) já tinha o token.
    for message in &got {
        let id = message["id"].as_str().unwrap();
        let token = message["receiptToken"].as_str().unwrap();
        let with_token = (1..=3)
            .filter(|n| {
                messages_of(cluster.node(*n))
                    .message_info("pedidos", id)
                    .unwrap()
                    .is_some_and(|i| i.meta.receipt_token.as_deref() == Some(token))
            })
            .count();
        assert!(with_token >= 2, "{id}: só {with_token} cópias com o token");
    }

    // Nada mais é entregue enquanto estão invisíveis.
    let none = consume(cluster.node(3), "pedidos", "maxMessages=10").await;
    assert!(none.is_empty());
    cluster.shutdown().await;
}

/// UC: UC-INV-008, UC-INV-010
#[tokio::test]
async fn an_ack_on_any_node_removes_the_message_everywhere_and_a_stale_token_is_refused() {
    let cluster = TestCluster::start(3).await;
    create_queue(cluster.node(1), "pedidos").await;
    let id = publish(cluster.node(1), "pedidos", json!({"x": 1})).await;

    let mut got = Vec::new();
    for _ in 0..40 {
        got = consume(
            cluster.node(1),
            "pedidos",
            "maxMessages=1&visibilityTimeout=60",
        )
        .await;
        if !got.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert_eq!(got.len(), 1);
    let token = got[0]["receiptToken"].as_str().unwrap().to_owned();

    // O token errado: `409`, e nada muda.
    let stale = cluster
        .node(3)
        .client
        .delete(
            cluster
                .node(3)
                .client_url(&format!("/topics/pedidos/messages/{id}?receiptToken=outro")),
        )
        .send()
        .await
        .unwrap();
    assert_eq!(error_code(stale, 409).await, "message.stale-receipt");
    assert!(
        messages_of(cluster.node(1))
            .message_info("pedidos", &id)
            .unwrap()
            .is_some()
    );

    // O ack por outro nó remove a mensagem de todos.
    let ack = cluster
        .node(3)
        .client
        .delete(cluster.node(3).client_url(&format!(
            "/topics/pedidos/messages/{id}?receiptToken={token}"
        )))
        .send()
        .await
        .unwrap();
    assert_eq!(ack.status(), 204);
    for number in 1..=3 {
        let messages = messages_of(cluster.node(number));
        let id = id.clone();
        eventually("a mensagem sumir da réplica", move || {
            messages.message_info("pedidos", &id).unwrap().is_none()
        })
        .await;
    }
    // Ack de novo: a mensagem não existe mais.
    let again = cluster
        .node(2)
        .client
        .delete(cluster.node(2).client_url(&format!(
            "/topics/pedidos/messages/{id}?receiptToken={token}"
        )))
        .send()
        .await
        .unwrap();
    assert_eq!(error_code(again, 404).await, "message.not-found");
    cluster.shutdown().await;
}

/// UC: UC-MSG-016, UC-ESC-010, UC-ESC-013, UC-ESC-014, UC-ESC-016, UC-ESC-019, UC-ESC-020, UC-ESC-022, UC-ESC-024
#[tokio::test]
async fn with_a_node_down_only_the_publications_it_owns_fail_and_the_replica_catches_up() {
    let cluster = TestCluster::start_with(3, NO_VOTE).await;
    create_queue(cluster.node(1), "pedidos").await;
    // O nó 3 cai da rede (isolado): rf 3 e quórum 2 aguentam um nó fora, mas só se não for o
    // primário da mensagem.
    cluster.network.isolate("10.0.0.3");

    let (mut ok_ids, mut failed) = (Vec::new(), 0);
    for n in 0..14 {
        let response = cluster
            .node(1)
            .client
            .post(cluster.node(1).client_url("/topics/pedidos/messages"))
            .json(&json!({"n": n}))
            .send()
            .await
            .unwrap();
        match response.status().as_u16() {
            201 => {
                let body: Value = response.json().await.unwrap();
                ok_ids.push(body["id"].as_str().unwrap().to_owned());
            }
            503 => {
                assert!(response.headers().contains_key("retry-after"));
                assert_eq!(
                    error_code(response, 503).await,
                    "cluster.quorum-unavailable"
                );
                failed += 1;
            }
            other => panic!("status inesperado {other}"),
        }
    }
    assert!(
        !ok_ids.is_empty() && failed > 0,
        "ok {} falhou {failed}",
        ok_ids.len()
    );

    // Os nós 1 e 2 têm exatamente o que foi aceito; as publicações recusadas não deixaram nada
    // (o registro foi desfeito).
    for number in [1, 2] {
        let messages = messages_of(cluster.node(number));
        let expected = ok_ids.len() as u64;
        eventually("só as aceitas guardadas", move || {
            messages.pending() == expected
        })
        .await;
    }
    assert_eq!(messages_of(cluster.node(3)).pending(), 0);

    // O coordenador contou o rollback e, depois do teto de commit, o aviso da réplica atrasada.
    tokio::time::sleep(Duration::from_millis(700)).await;
    let metrics = cluster.node(1).metrics().await;
    assert!(
        common::series(&metrics, "taba_write_rollbacks_total").is_some_and(|n| n >= 1.0),
        "{metrics}"
    );
    assert!(common::series(&metrics, "taba_write_commit_timeouts_total").is_some_and(|n| n >= 1.0));
    assert!(
        common::series(
            &metrics,
            "taba_write_coordinated_total{taba_outcome=\"failed\"}"
        )
        .is_some_and(|n| n >= 1.0)
    );

    // O nó volta: o registro e o commit em segundo plano o alcançam, e o bloco volta ao fator pleno.
    cluster.network.restore("10.0.0.3");
    let third = messages_of(cluster.node(3));
    let expected = ok_ids.clone();
    eventually("o nó atrasado receber tudo", move || {
        expected
            .iter()
            .all(|id| third.message_info("pedidos", id).unwrap().is_some())
    })
    .await;
    cluster.shutdown().await;
}

/// UC: UC-ESC-015
#[tokio::test]
async fn a_lost_call_is_retried_before_the_quorum_gives_up() {
    let cluster = TestCluster::start(3).await;
    create_queue(cluster.node(1), "pedidos").await;
    // Perde as duas primeiras chamadas de registro a cada nó: com três tentativas passa.
    for host in ["10.0.0.2", "10.0.0.3"] {
        cluster.network.lose_kind(host, "write-register", 2);
    }
    let id = publish(cluster.node(1), "pedidos", json!({"a": 1})).await;
    let messages = messages_of(cluster.node(1));
    assert!(
        messages.message_info("pedidos", &id).unwrap().is_some() || {
            eventually("a mensagem aparecer", || {
                messages.message_info("pedidos", &id).unwrap().is_some()
            })
            .await;
            true
        }
    );
    cluster.shutdown().await;
}

/// UC: UC-ACK-008, UC-ACK-009, UC-INV-012
#[tokio::test]
async fn an_ack_that_cannot_reach_the_primary_fails_and_the_message_stays() {
    let cluster = TestCluster::start_with(3, NO_VOTE).await;
    create_queue(cluster.node(1), "pedidos").await;
    let id = publish(cluster.node(1), "pedidos", json!({"x": 1})).await;
    let mut got = Vec::new();
    for _ in 0..40 {
        got = consume(
            cluster.node(1),
            "pedidos",
            "maxMessages=1&visibilityTimeout=60",
        )
        .await;
        if !got.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    let token = got[0]["receiptToken"].as_str().unwrap().to_owned();
    let primary = cluster
        .node(1)
        .node
        .cluster()
        .unwrap()
        .membership()
        .ring()
        .primary(message_position(&id))
        .unwrap()
        .to_owned();
    let primary_number = (1..=3).find(|n| cluster.node_id(*n) == primary).unwrap();
    // O ack vai por um nó que não é o primário, e o primário some da rede.
    let via = (1..=3).find(|n| *n != primary_number).unwrap();
    cluster.network.isolate(&format!("10.0.0.{primary_number}"));
    let ack = cluster
        .node(via)
        .client
        .delete(cluster.node(via).client_url(&format!(
            "/topics/pedidos/messages/{id}?receiptToken={token}"
        )))
        .send()
        .await
        .unwrap();
    assert_eq!(error_code(ack, 503).await, "cluster.quorum-unavailable");
    assert!(
        messages_of(cluster.node(primary_number))
            .message_info("pedidos", &id)
            .unwrap()
            .is_some(),
        "a mensagem segue lá"
    );

    // O primário volta e o mesmo ack passa.
    cluster.network.restore(&format!("10.0.0.{primary_number}"));
    let ack = cluster
        .node(via)
        .client
        .delete(cluster.node(via).client_url(&format!(
            "/topics/pedidos/messages/{id}?receiptToken={token}"
        )))
        .send()
        .await
        .unwrap();
    assert_eq!(ack.status(), 204);
    cluster.shutdown().await;
}

/// UC: UC-INV-007, UC-INV-016
#[tokio::test]
async fn the_visibility_can_be_changed_and_an_expired_one_is_redelivered_without_propagation() {
    let cluster = TestCluster::start(3).await;
    create_queue(cluster.node(1), "pedidos").await;
    let id = publish(cluster.node(2), "pedidos", json!({"x": 1})).await;
    let mut got = Vec::new();
    for _ in 0..40 {
        got = consume(
            cluster.node(3),
            "pedidos",
            "maxMessages=1&visibilityTimeout=60",
        )
        .await;
        if !got.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    let token = got[0]["receiptToken"].as_str().unwrap().to_owned();

    // O `PUT` por outro nó estende o prazo em todas as cópias, sem mudar o contador.
    let put = cluster
        .node(1)
        .client
        .put(cluster.node(1).client_url(&format!(
            "/topics/pedidos/messages/{id}/visibility?receiptToken={token}"
        )))
        .json(&json!({"visibilityTimeout": 600}))
        .send()
        .await
        .unwrap();
    assert_eq!(put.status(), 200);
    let wanted = cluster.clock.now_millis() + 600_000;
    for number in 1..=3 {
        let messages = messages_of(cluster.node(number));
        let id = id.clone();
        eventually("o novo prazo em todas as cópias", move || {
            messages
                .message_info("pedidos", &id)
                .unwrap()
                .is_some_and(|i| i.meta.invisible_until_ms == wanted && i.meta.delivery_count == 1)
        })
        .await;
    }

    // Passado o prazo a mensagem volta sozinha, pelo dono, e a próxima entrega sobe o contador.
    cluster.clock.advance(Duration::from_secs(601));
    let mut again = Vec::new();
    for _ in 0..40 {
        again = consume(
            cluster.node(2),
            "pedidos",
            "maxMessages=1&visibilityTimeout=60",
        )
        .await;
        if !again.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert_eq!(again.len(), 1);
    assert_eq!(again[0]["id"], id);
    assert_eq!(again[0]["deliveryCount"], 2);
    assert_ne!(again[0]["receiptToken"], got[0]["receiptToken"]);
    cluster.shutdown().await;
}

/// UC: UC-ESC-029, UC-INV-019
#[tokio::test]
async fn a_message_that_spent_its_deliveries_moves_to_the_dead_letter_on_every_node() {
    let cluster = TestCluster::start(3).await;
    let created = cluster
        .node(1)
        .client
        .post(cluster.node(1).client_url("/topics"))
        .json(&json!({"name": "pedidos", "type": "queue", "maxDeliveries": 2}))
        .send()
        .await
        .unwrap();
    assert_eq!(created.status(), 201);
    let id = publish(cluster.node(1), "pedidos", json!({"x": 1})).await;

    for round in 1..=2 {
        let mut got = Vec::new();
        for _ in 0..40 {
            got = consume(
                cluster.node(2),
                "pedidos",
                "maxMessages=1&visibilityTimeout=60",
            )
            .await;
            if !got.is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        assert_eq!(got.len(), 1, "entrega {round}");
        assert_eq!(got[0]["deliveryCount"], round);
        cluster.clock.advance(Duration::from_secs(61));
    }

    // A segunda invisibilidade venceu com as entregas gastas: o dono manda à dead-letter, e
    // todas as cópias fazem o movimento.
    for number in 1..=3 {
        let messages = messages_of(cluster.node(number));
        let id = id.clone();
        eventually("o movimento para a dead-letter", move || {
            messages
                .message_info("pedidos-dead-letter", &id)
                .unwrap()
                .is_some()
                && messages.message_info("pedidos", &id).unwrap().is_none()
        })
        .await;
    }
    let mut in_dead_letter = Vec::new();
    for _ in 0..40 {
        in_dead_letter = consume(cluster.node(3), "pedidos-dead-letter", "maxMessages=1").await;
        if !in_dead_letter.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert_eq!(in_dead_letter.len(), 1);
    assert_eq!(in_dead_letter[0]["id"], id);
    assert_eq!(in_dead_letter[0]["deadLetterReason"], "max-deliveries");
    cluster.shutdown().await;
}

/// UC: UC-CNS-030, UC-ENT-023, UC-ENT-024, UC-ENT-025
#[tokio::test]
async fn a_long_poll_on_one_node_is_woken_by_a_message_that_arrives_at_another() {
    let cluster = TestCluster::start(3).await;
    create_queue(cluster.node(1), "pedidos").await;

    // O ouvinte espera no nó 2; a mensagem entra pelo nó 1 e o dono pode ser qualquer um.
    let waiter = {
        let client = cluster.node(2).client.clone();
        let url = cluster
            .node(2)
            .client_url("/topics/pedidos/messages?waitTime=10&maxMessages=1&visibilityTimeout=60");
        tokio::spawn(async move { client.get(url).send().await.unwrap().json::<Value>().await })
    };
    // O interesse do nó 2 chega aos outros por sinal, uma vez só.
    let calls = cluster.network.clone();
    eventually("o sinal de ouvinte", move || calls.count("channel") >= 2).await;
    let id = publish(cluster.node(1), "pedidos", json!({"x": 1})).await;

    let body = tokio::time::timeout(Duration::from_secs(5), waiter)
        .await
        .expect("o long polling deveria acordar")
        .unwrap()
        .unwrap();
    assert_eq!(body["messages"][0]["id"], id);
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
        .unwrap();
    assert_eq!(response.status(), 201);
    let body: Value = response.json().await.unwrap();
    (
        body["id"].as_str().unwrap().to_owned(),
        body["token"].as_str().unwrap().to_owned(),
    )
}

async fn status_with(node: &TestNode, token: &str) -> u16 {
    node.client
        .get(node.client_url("/topics"))
        .header("authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap()
        .status()
        .as_u16()
}

/// UC: UC-DEF-034, UC-DEF-022, UC-ESC-030
#[tokio::test]
async fn tokens_reach_every_node_and_a_new_node_pulls_them() {
    let mut cluster = TestCluster::start_with(2, &[("TABA_AUTH_ROOT_TOKEN", ROOT)]).await;
    let (revoked_id, revoked) = create_token(cluster.node(1)).await;
    let (_, kept) = create_token(cluster.node(2)).await;

    // O token criado por um nó vale nos dois (a criação só responde com todos confirmando).
    for number in [1, 2] {
        assert_eq!(status_with(cluster.node(number), &revoked).await, 200);
        assert_eq!(status_with(cluster.node(number), &kept).await, 200);
    }

    // A revogação por outro nó vale em todos.
    let response = cluster
        .node(2)
        .client
        .delete(cluster.node(2).client_url(&format!("/tokens/{revoked_id}")))
        .header("authorization", format!("Bearer {ROOT}"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 204);
    for number in [1, 2] {
        assert_eq!(status_with(cluster.node(number), &revoked).await, 401);
        assert_eq!(status_with(cluster.node(number), &kept).await, 200);
    }

    // O nó novo puxa os tokens junto dos tópicos: o que vale, vale; o que foi revogado, não.
    cluster.add_node(false).await;
    assert_eq!(status_with(cluster.node(3), &kept).await, 200);
    assert_eq!(status_with(cluster.node(3), &revoked).await, 401);
    cluster.shutdown().await;
}

/// UC: UC-RET-006
#[tokio::test]
async fn every_node_expires_its_own_copies_by_ttl_without_propagating() {
    let cluster = TestCluster::start(3).await;
    let created = cluster
        .node(1)
        .client
        .post(cluster.node(1).client_url("/topics"))
        .json(&json!({"name": "pedidos", "type": "queue", "ttl": 60}))
        .send()
        .await
        .unwrap();
    assert_eq!(created.status(), 201);
    let id = publish(cluster.node(1), "pedidos", json!({"x": 1})).await;
    for number in 1..=3 {
        let messages = messages_of(cluster.node(number));
        let id = id.clone();
        eventually("a cópia guardada", move || {
            messages.message_info("pedidos", &id).unwrap().is_some()
        })
        .await;
    }
    cluster.network.clear_calls();

    // O tempo passa para todos; a varredura de cada nó apaga a sua cópia, sem chamada entre nós.
    cluster.clock.advance(Duration::from_secs(61));
    for number in 1..=3 {
        let messages = messages_of(cluster.node(number));
        let id = id.clone();
        eventually("a cópia vencida apagada", move || {
            messages.message_info("pedidos", &id).unwrap().is_none()
        })
        .await;
    }
    assert!(
        cluster
            .network
            .calls()
            .iter()
            .all(|c| !c.request.starts_with("write") && !c.request.starts_with("ack")),
        "a expiração é local e determinística"
    );
    cluster.shutdown().await;
}

/// Os nós responsáveis por uma mensagem, em ordem (primário, secundário, terciário), como número.
fn owners_of(cluster: &TestCluster, id: &str) -> Vec<usize> {
    let ring = cluster.node(1).node.cluster().unwrap().membership().ring();
    ring.owners(message_position(id), 3)
        .into_iter()
        .map(|owner| (1..=3).find(|n| cluster.node_id(*n) == owner).unwrap())
        .collect()
}

fn host(number: usize) -> String {
    format!("10.0.0.{number}")
}

/// Entrega a mensagem pelo `GET` no nó dado e devolve o `receiptToken`.
async fn deliver_at(node: &TestNode) -> String {
    let mut got = Vec::new();
    for _ in 0..80 {
        got = consume(node, "pedidos", "maxMessages=1&visibilityTimeout=300").await;
        if !got.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert_eq!(got.len(), 1);
    got[0]["receiptToken"].as_str().unwrap().to_owned()
}

/// UC: UC-MSG-014, UC-MSG-015, UC-ESC-004, UC-ESC-006, UC-ESC-007, UC-ESC-011
#[tokio::test]
async fn the_answer_comes_only_after_the_quorum_committed_whichever_node_took_the_call() {
    let cluster = TestCluster::start(3).await;
    create_queue(cluster.node(1), "pedidos").await;
    for n in 0..12usize {
        let via = 1 + n % 3;
        let id = publish(cluster.node(via), "pedidos", json!({"n": n})).await;
        // Na resposta o primário já tem a mensagem e é dono dela, e o quórum de 2 já a tem
        // commitada: sem esperar.
        let owners = owners_of(&cluster, &id);
        let primary = messages_of(cluster.node(owners[0]))
            .message_info("pedidos", &id)
            .unwrap()
            .expect("o primário já tem");
        assert!(primary.owned);
        let holders = (1..=3)
            .filter(|n| {
                messages_of(cluster.node(*n))
                    .message_info("pedidos", &id)
                    .unwrap()
                    .is_some()
            })
            .count();
        assert!(holders >= 2, "mensagem {n}: só {holders} nós a têm");
    }
    cluster.shutdown().await;
}

/// UC: UC-ACK-010, UC-ESC-020, UC-ESC-021, UC-ESC-024
#[tokio::test]
async fn an_ack_decided_by_the_quorum_is_not_undone_by_a_replica_that_is_down() {
    let cluster = TestCluster::start_with(3, NO_VOTE).await;
    create_queue(cluster.node(1), "pedidos").await;
    let id = publish(cluster.node(1), "pedidos", json!({"x": 1})).await;
    let owners = owners_of(&cluster, &id);
    let (primary, secondary, tertiary) = (owners[0], owners[1], owners[2]);
    let token = deliver_at(cluster.node(primary)).await;
    for number in 1..=3 {
        let messages = messages_of(cluster.node(number));
        let id = id.clone();
        let token = token.clone();
        eventually("a entrega em todas as cópias", move || {
            messages
                .message_info("pedidos", &id)
                .unwrap()
                .is_some_and(|i| i.meta.receipt_token.as_deref() == Some(token.as_str()))
        })
        .await;
    }

    // O terciário cai: o ack (primário + secundário) é decidido e não é desfeito.
    cluster.network.isolate(&host(tertiary));
    let ack = cluster
        .node(secondary)
        .client
        .delete(cluster.node(secondary).client_url(&format!(
            "/topics/pedidos/messages/{id}?receiptToken={token}"
        )))
        .send()
        .await
        .unwrap();
    assert_eq!(ack.status(), 204);
    for number in [primary, secondary] {
        let messages = messages_of(cluster.node(number));
        let id = id.clone();
        eventually("o ack no quórum", move || {
            messages.message_info("pedidos", &id).unwrap().is_none()
        })
        .await;
    }
    assert!(
        messages_of(cluster.node(tertiary))
            .message_info("pedidos", &id)
            .unwrap()
            .is_some(),
        "o nó que caiu ainda tem a cópia"
    );

    // Quando volta, o coordenador, que insistiu em segundo plano, o alcança.
    cluster.network.restore(&host(tertiary));
    let messages = messages_of(cluster.node(tertiary));
    let wanted = id.clone();
    eventually("o ack chegar à réplica atrasada", move || {
        messages.message_info("pedidos", &wanted).unwrap().is_none()
    })
    .await;
    cluster.shutdown().await;
}

/// UC: UC-CNS-019, UC-CNS-021, UC-INV-012
#[tokio::test]
async fn a_get_whose_registrations_fail_answers_503_and_leaves_the_messages_ready() {
    let cluster = TestCluster::start(3).await;
    create_queue(cluster.node(1), "pedidos").await;
    let mut ids = Vec::new();
    for n in 0..9 {
        ids.push(publish(cluster.node(1), "pedidos", json!({"n": n})).await);
    }
    let owner = owners_of(&cluster, &ids[0])[0];

    // Nenhum registro de invisibilidade chega a outro nó: o quórum não fecha para ninguém.
    for number in 1..=3 {
        cluster
            .network
            .lose_kind(&host(number), "visibility-register", 1_000_000);
    }
    let response = cluster
        .node(owner)
        .client
        .get(
            cluster
                .node(owner)
                .client_url("/topics/pedidos/messages?maxMessages=10&visibilityTimeout=60"),
        )
        .send()
        .await
        .unwrap();
    assert!(response.headers().contains_key("retry-after"));
    assert_eq!(error_code(response, 503).await, "delivery.unavailable");

    // As mensagens que falharam nunca entraram na resposta e seguem prontas, como estavam.
    for number in 1..=3 {
        cluster
            .network
            .lose_kind(&host(number), "visibility-register", 0);
    }
    let mut got = Vec::new();
    for _ in 0..80 {
        got.extend(
            consume(
                cluster.node(owner),
                "pedidos",
                "maxMessages=10&visibilityTimeout=60",
            )
            .await,
        );
        if !got.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert!(!got.is_empty());
    assert!(got.iter().all(|m| m["deliveryCount"] == 1));
    cluster.shutdown().await;
}

/// UC: UC-CNS-023, UC-ESC-017, UC-ACK-008
#[tokio::test]
async fn a_rollback_that_does_not_arrive_leaves_a_pending_that_expires_by_itself() {
    let cluster = TestCluster::start(3).await;
    create_queue(cluster.node(1), "pedidos").await;
    let id = publish(cluster.node(1), "pedidos", json!({"x": 1})).await;
    let owners = owners_of(&cluster, &id);
    let (primary, secondary, tertiary) = (owners[0], owners[1], owners[2]);
    let token = deliver_at(cluster.node(primary)).await;

    // O ack entra pelo terciário: o registro não chega ao primário, o quórum não fecha, e o
    // rollback não chega ao secundário (que já tinha registrado).
    cluster
        .network
        .lose_kind(&host(primary), "ack-register", 1_000_000);
    cluster
        .network
        .lose_kind(&host(secondary), "ack-rollback", 1_000_000);
    let ack_url = format!("/topics/pedidos/messages/{id}?receiptToken={token}");
    let first = cluster
        .node(tertiary)
        .client
        .delete(cluster.node(tertiary).client_url(&ack_url))
        .send()
        .await
        .unwrap();
    assert_eq!(error_code(first, 503).await, "cluster.quorum-unavailable");
    let replica = cluster
        .node(secondary)
        .node
        .cluster()
        .unwrap()
        .replication()
        .unwrap()
        .replica
        .clone();
    assert_eq!(
        replica.pending(),
        1,
        "o registro ficou pendente no secundário"
    );
    for number in 1..=3 {
        assert!(
            messages_of(cluster.node(number))
                .message_info("pedidos", &id)
                .unwrap()
                .is_some(),
            "a mensagem segue em todos"
        );
    }

    // `write.pending.ttl` (30 s) passa e o pendente some sozinho; o mesmo ack agora passa.
    cluster.network.lose_kind(&host(primary), "ack-register", 0);
    cluster
        .network
        .lose_kind(&host(secondary), "ack-rollback", 0);
    cluster.clock.advance(Duration::from_secs(31));
    assert_eq!(replica.pending(), 0);
    let again = cluster
        .node(tertiary)
        .client
        .delete(cluster.node(tertiary).client_url(&ack_url))
        .send()
        .await
        .unwrap();
    assert_eq!(again.status(), 204);
    cluster.shutdown().await;
}

#[tokio::test]
async fn a_lost_ready_signal_is_corrected_by_the_periodic_repeat() {
    let cluster = TestCluster::start(3).await;
    create_queue(cluster.node(1), "pedidos").await;
    // Nenhum sinal chega a ninguém enquanto as mensagens são publicadas.
    for number in 1..=3 {
        cluster
            .network
            .lose_kind(&host(number), "channel", 1_000_000);
    }
    let mut published = Vec::new();
    for n in 0..9 {
        published.push(publish(cluster.node(1), "pedidos", json!({"n": n})).await);
    }
    // O nó 2 só conhece as próprias; o resto aparece quando o estado é repetido.
    for number in 1..=3 {
        cluster.network.lose_kind(&host(number), "channel", 0);
    }
    let mut got: Vec<Value> = Vec::new();
    for _ in 0..200 {
        got.extend(
            consume(
                cluster.node(2),
                "pedidos",
                "maxMessages=10&visibilityTimeout=300",
            )
            .await,
        );
        if got.len() >= published.len() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(
        got.len(),
        published.len(),
        "o estado repetido corrigiu a tabela"
    );
    cluster.shutdown().await;
}
