//! Os avisos de mensagem nova entre os nós (doc 13, seção 12.8; doc 20b, nível **C**): o
//! responsável por tópico, a janela que agrupa as prontas num aviso só, a espera pelo `take`, a
//! passagem ao próximo nó com ouvinte e o resultado do aviso no nó que o recebe.

mod common;

use std::time::Duration;

use common::cluster::{
    TestCluster, create_queue, eventually, eventually_async, settled, signals_of, total,
};
use serde_json::json;
use taba_node::cluster::protocol::{Signal, SignalFrame};

async fn publish(cluster: &TestCluster, topic: &str, count: usize) {
    for n in 0..count {
        let node = cluster.node(1);
        let response = node
            .client
            .post(node.client_url(&format!("/topics/{topic}/messages")))
            .json(&json!({ "n": n }))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 201);
    }
}

/// Os avisos do tipo `outcome` na direção `direction` que o nó contou até agora.
async fn notices(cluster: &TestCluster, number: usize, direction: &str, outcome: &str) -> f64 {
    let text = cluster.node(number).metrics().await;
    total(
        &text,
        "taba_cluster_channel_notifications_total",
        &format!("taba_direction=\"{direction}\",taba_outcome=\"{outcome}\""),
    )
}

/// Os nós dizem a todos que têm ouvinte do tópico (sem `GET` de verdade: ninguém vem buscar).
async fn silent_listeners(
    cluster: &TestCluster,
    topic: &str,
    numbers: &[usize],
) -> Vec<impl Drop + use<>> {
    let guards: Vec<_> = numbers
        .iter()
        .map(|n| signals_of(cluster, *n).listener(topic))
        .collect();
    for number in 1..=cluster.nodes.len() {
        let signals = signals_of(cluster, number);
        let topic = topic.to_owned();
        // O próprio nó não guarda o interesse dele na tabela dos outros.
        let expected: Vec<String> = numbers
            .iter()
            .filter(|n| **n != number)
            .map(|n| cluster.node_id(*n))
            .collect();
        eventually("o interesse chegar a todos os nós", move || {
            let known = signals.listener_nodes(&topic);
            expected.iter().all(|id| known.contains(id))
        })
        .await;
    }
    guards
}

/// UC: UC-CAN-015
#[tokio::test]
async fn the_ready_messages_of_a_window_become_one_notice_with_the_quantity() {
    let cluster = TestCluster::start_with(
        3,
        &[
            ("TABA_CLUSTER_NOTIFY_WINDOW", "1500ms"),
            ("TABA_CLUSTER_NOTIFY_WAIT", "50ms"),
        ],
    )
    .await;
    create_queue(cluster.node(1), "avisos").await;
    settled(&cluster).await;
    let _listener = silent_listeners(&cluster, "avisos", &[2]).await;

    publish(&cluster, "avisos", 24).await;

    let signals = signals_of(&cluster, 2);
    eventually("o aviso chegar ao nó 2", {
        let signals = signals.clone();
        move || signals.last_notice_count("avisos").is_some()
    })
    .await;
    // Cada dono (os nós 1 e 3) mandou um aviso só, com as prontas da janela e não uma.
    tokio::time::sleep(Duration::from_millis(400)).await;
    let received = notices(&cluster, 2, "in", "received").await;
    assert!(received <= 2.0, "um aviso por dono, vieram {received}");
    let count = signals.last_notice_count("avisos").unwrap();
    assert!(count > 1, "o aviso traz a quantidade, veio {count}");
    cluster.shutdown().await;
}

/// UC: UC-CAN-016
#[tokio::test]
async fn the_notice_goes_to_one_listener_at_a_time_and_passes_on_when_nobody_comes() {
    let cluster = TestCluster::start_with(
        3,
        &[
            ("TABA_CLUSTER_NOTIFY_WINDOW", "20ms"),
            ("TABA_CLUSTER_NOTIFY_WAIT", "700ms"),
        ],
    )
    .await;
    create_queue(cluster.node(1), "avisos").await;
    settled(&cluster).await;
    let _listeners = silent_listeners(&cluster, "avisos", &[2, 3]).await;

    // Com mensagens bastantes, o nó 1 é dono de alguma e abre o ciclo dele.
    publish(&cluster, "avisos", 30).await;

    eventually_async("o primeiro aviso do nó 1", || async {
        notices(&cluster, 1, "out", "sent").await >= 1.0
    })
    .await;
    // Um nó por vez: enquanto o primeiro tem tempo de vir buscar, o segundo não é avisado.
    tokio::time::sleep(Duration::from_millis(250)).await;
    assert_eq!(notices(&cluster, 1, "out", "sent").await, 1.0);
    // Ninguém veio: passa ao próximo, e acabados os nós o ciclo termina.
    eventually_async("o aviso ao segundo nó", || async {
        notices(&cluster, 1, "out", "sent").await >= 2.0
    })
    .await;
    tokio::time::sleep(Duration::from_millis(900)).await;
    assert_eq!(notices(&cluster, 1, "out", "sent").await, 2.0);
    cluster.shutdown().await;
}

/// UC: UC-CAN-017
#[tokio::test]
async fn a_notice_is_a_hint_that_ends_fetched_or_empty() {
    let cluster = TestCluster::start_with(
        3,
        &[
            ("TABA_CLUSTER_NOTIFY_WINDOW", "20ms"),
            ("TABA_CLUSTER_NOTIFY_WAIT", "300ms"),
        ],
    )
    .await;
    create_queue(cluster.node(1), "avisos").await;
    settled(&cluster).await;
    let query = "waitTime=5&maxMessages=1&visibilityTimeout=60";

    // Vazio: o aviso (e a lista de prontas) do nó 1 está velho, e o `take` volta sem nada.
    let waiting = {
        let node = cluster.node(2);
        let client = node.client.clone();
        let url = node.client_url(&format!("/topics/avisos/messages?{query}"));
        tokio::spawn(async move { client.get(url).send().await })
    };
    let signals = signals_of(&cluster, 2);
    eventually("o ouvinte do nó 2", {
        let signals = signals.clone();
        move || signals.local_listener_count("avisos") == 1
    })
    .await;
    let from = cluster.node_id(1);
    signals.receive(SignalFrame {
        from: from.clone(),
        signal: Signal::Ready {
            topic: "avisos".into(),
            has_ready: true,
        },
    });
    signals.receive(SignalFrame {
        from,
        signal: Signal::Notify {
            topic: "avisos".into(),
            count: 1,
        },
    });
    eventually_async("o aviso velho contado como vazio", || async {
        notices(&cluster, 2, "in", "empty").await >= 1.0
    })
    .await;
    waiting.abort();

    // Buscado: um `GET` espera no nó 2 e a mensagem pronta é de outro nó. Se o dono for o próprio
    // nó 2 a mensagem não viaja, então repete até um dono de fora.
    for _ in 0..20 {
        if notices(&cluster, 2, "in", "fetched").await >= 1.0 {
            break;
        }
        let node = cluster.node(2);
        let client = node.client.clone();
        let url = node.client_url(&format!("/topics/avisos/messages?{query}"));
        let get = tokio::spawn(async move { client.get(url).send().await.unwrap().status() });
        eventually("o ouvinte do nó 2", {
            let signals = signals.clone();
            move || signals.local_listener_count("avisos") == 1
        })
        .await;
        publish(&cluster, "avisos", 1).await;
        assert_eq!(get.await.unwrap(), 200);
    }
    assert!(notices(&cluster, 2, "in", "fetched").await >= 1.0);
    cluster.shutdown().await;
}
