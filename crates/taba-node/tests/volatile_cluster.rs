//! O nó gestor das filas voláteis e `delivery.get.max-attempts` no cluster (doc 15, seções 14.5 e
//! 14.6; doc 13, seção 12.2; doc 20a, UC-VOL e UC-CNS; nível **C**).

mod common;

use std::time::Duration;

use serde_json::{Value, json};

use common::TestNode;
use common::cluster::{NO_VOTE, TestCluster, address_of, eventually, host_of, settled};
use taba_node::cluster::protocol::{Request, Response};
use taba_node::cluster::transport::{CallError, Transport};
use taba_node::core::error::ErrorCode;
use taba_node::core::ring::topic_position;

/// O relógio é manual: o espalhamento da primeira consulta (em tempo do relógio) só atrasaria o teste.
const NO_JITTER: (&str, &str) = ("TABA_TOPICS_VOLATILE_TAKEOVER_JITTER", "0s");
const IDLE_RETRY: (&str, &str) = ("TABA_TOPICS_VOLATILE_IDLE_CHECK_RETRY_INTERVAL", "5s");

async fn volatile_queue(node: &TestNode, name: &str, idle_timeout: u64) -> String {
    let response = node
        .client
        .post(node.client_url("/topics"))
        .json(&json!({
            "name": name, "type": "queue", "volatile": true, "idleTimeout": idle_timeout
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 201, "{:?}", response.text().await);
    response.json::<Value>().await.unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

async fn exists(node: &TestNode, name: &str) -> bool {
    node.client_get(&format!("/topics/{name}")).await.status() == 200
}

async fn get(node: &TestNode, name: &str, query: &str) -> reqwest::Response {
    node.client_get(&format!("/topics/{name}/messages?{query}"))
        .await
}

async fn wait_gone(cluster: &TestCluster, numbers: &[usize], name: &str) {
    for _ in 0..400 {
        let mut gone = true;
        for number in numbers {
            gone &= !exists(cluster.node(*number), name).await;
        }
        if gone {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("a fila `{name}` não foi excluída");
}

/// Quem gerencia a fila: o primário da posição do hash do `id` do tópico.
fn manager_of(cluster: &TestCluster, topic_id: &str) -> usize {
    let owners = cluster
        .node(1)
        .node
        .cluster()
        .unwrap()
        .owners_of(topic_position(topic_id))
        .unwrap();
    (1..=3)
        .find(|n| cluster.node_id(*n) == owners[0].node_id)
        .unwrap()
}

/// Um nó que não é o gestor.
fn other_than(manager: usize) -> usize {
    if manager == 1 { 2 } else { 1 }
}

async fn idle_of(
    cluster: &TestCluster,
    from: usize,
    to: usize,
    id: &str,
) -> Result<Value, CallError> {
    let caller = cluster.network.transport(host_of(from));
    match caller
        .call(
            &address_of(cluster, to),
            Request::Idle {
                topic_id: id.to_owned(),
            },
        )
        .await?
    {
        Response::Idle(idle) => Ok(serde_json::to_value(idle).unwrap()),
        other => panic!("resposta inesperada: {other:?}"),
    }
}

/// UC: UC-VOL-001, UC-VOL-006, UC-VOL-008, UC-VOL-009
#[tokio::test]
async fn the_manager_asks_every_node_and_deletes_only_when_none_had_a_get() {
    let cluster = TestCluster::start_with(3, &[NO_JITTER]).await;
    settled(&cluster).await;
    let id = volatile_queue(cluster.node(1), "efemera", 60).await;
    let manager = manager_of(&cluster, &id);
    let consumer = other_than(manager);

    // Um `GET` num nó que não é o gestor, 30 s depois da criação, é o que adia a exclusão.
    cluster.clock.advance(Duration::from_secs(30));
    assert_eq!(
        get(cluster.node(consumer), "efemera", "waitTime=0")
            .await
            .status(),
        200
    );

    // Passados os 60 s da criação, a consulta do gestor acha o `GET` de 40 s atrás: a fila fica.
    cluster.clock.advance(Duration::from_secs(40));
    tokio::time::sleep(Duration::from_millis(1500)).await;
    for number in 1..=3 {
        assert!(
            exists(cluster.node(number), "efemera").await,
            "no nó {number}"
        );
    }

    // Sem nenhum `GET` por 60 s, o gestor exclui a fila em todos os nós.
    cluster.clock.advance(Duration::from_secs(30));
    wait_gone(&cluster, &[1, 2, 3], "efemera").await;
    cluster.shutdown().await;
}

/// UC: UC-VOL-006, UC-VOL-023
#[tokio::test]
async fn many_queues_are_managed_by_different_nodes_and_all_expire() {
    let cluster = TestCluster::start_with(3, &[NO_JITTER]).await;
    settled(&cluster).await;
    let mut managers = std::collections::BTreeSet::new();
    let names: Vec<String> = (0..12).map(|n| format!("fila-{n}")).collect();
    for name in &names {
        let id = volatile_queue(cluster.node(1), name, 60).await;
        managers.insert(manager_of(&cluster, &id));
    }
    assert!(managers.len() >= 2, "{managers:?}");
    cluster.clock.advance(Duration::from_secs(61));
    for name in &names {
        wait_gone(&cluster, &[1, 2, 3], name).await;
    }
    cluster.shutdown().await;
}

/// UC: UC-VOL-010, UC-VOL-015
#[tokio::test]
async fn a_node_that_does_not_answer_or_a_manager_that_is_down_keeps_the_queue_alive() {
    let mut env = NO_VOTE.to_vec();
    env.extend([IDLE_RETRY, NO_JITTER]);
    let cluster = TestCluster::start_with(3, &env).await;
    settled(&cluster).await;
    let id = volatile_queue(cluster.node(1), "efemera", 60).await;
    let manager = manager_of(&cluster, &id);
    let silent = other_than(manager);

    // Um nó não responde (e ainda está na tabela): na dúvida, a fila continua viva.
    cluster.network.isolate(&host_of(silent));
    cluster.clock.advance(Duration::from_secs(61));
    tokio::time::sleep(Duration::from_millis(1500)).await;
    for number in (1..=3).filter(|n| *n != silent) {
        assert!(exists(cluster.node(number), "efemera").await);
    }
    // A fila segue aceitando publicações: a que cai num bloco cujo primário é o nó mudo espera
    // por ele (`503`, PRB-044), as outras fecham.
    let published = cluster
        .node(manager)
        .client
        .post(cluster.node(manager).client_url("/topics/efemera/messages"))
        .json(&json!({"a": 1}))
        .send()
        .await
        .unwrap();
    assert!([201, 503].contains(&published.status().as_u16()));

    // Voltou o nó: na nova tentativa (depois do intervalo) a fila é excluída.
    cluster.network.restore(&host_of(silent));
    cluster.clock.advance(Duration::from_secs(6));
    wait_gone(&cluster, &[1, 2, 3], "efemera").await;

    // O próprio gestor fora do ar e ainda na tabela: nenhuma fila dele é excluída.
    let id = volatile_queue(cluster.node(1), "outra", 60).await;
    let manager = manager_of(&cluster, &id);
    let alive = other_than(manager);
    cluster.network.isolate(&host_of(manager));
    cluster.clock.advance(Duration::from_secs(120));
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert!(exists(cluster.node(alive), "outra").await);
    cluster.shutdown().await;
}

/// UC: UC-VOL-017, UC-FAL-018
#[tokio::test]
async fn when_the_manager_leaves_the_new_primary_of_its_positions_takes_over() {
    let cluster = TestCluster::start_with(3, &[NO_JITTER]).await;
    settled(&cluster).await;
    let id = volatile_queue(cluster.node(1), "efemera", 60).await;
    let manager = manager_of(&cluster, &id);
    let survivors: Vec<usize> = (1..=3).filter(|n| *n != manager).collect();

    // O gestor cai e é excluído do cluster; o novo primário da posição assume a fila, sem estado
    // herdado, e a consulta que faz já mostra o prazo real.
    let old = cluster.node_id(manager);
    cluster.network.isolate(&host_of(manager));
    eventually("o gestor ser excluído", || {
        survivors
            .iter()
            .all(|n| !cluster.member_ids(*n).contains(&old))
    })
    .await;
    cluster.clock.advance(Duration::from_secs(61));
    wait_gone(&cluster, &survivors, "efemera").await;
    cluster.shutdown().await;
}

/// UC: UC-CLR-007, UC-VOL-011
#[tokio::test]
async fn the_idle_route_answers_a_duration_and_says_when_it_counts_from_the_start() {
    let cluster = TestCluster::start(3).await;
    settled(&cluster).await;
    let id = volatile_queue(cluster.node(1), "efemera", 3000).await;

    // Sem `GET` neste nó: a duração é o tempo desde que subiu.
    cluster.clock.advance(Duration::from_secs(7));
    let idle = idle_of(&cluster, 1, 2, &id).await.unwrap();
    assert_eq!(idle["topicId"], id);
    assert_eq!(idle["sinceStart"], true);
    assert!(idle["idleSeconds"].as_u64().unwrap() >= 7);

    // Com um `GET`, a duração é a de antes e `sinceStart` é falso.
    assert_eq!(
        get(cluster.node(2), "efemera", "waitTime=0").await.status(),
        200
    );
    cluster.clock.advance(Duration::from_secs(5));
    let idle = idle_of(&cluster, 1, 2, &id).await.unwrap();
    assert_eq!(idle["sinceStart"], false);
    assert_eq!(idle["idleSeconds"], 5);

    // Tópico desconhecido: `404`.
    match idle_of(&cluster, 1, 2, "00000000-0000-7000-8000-000000000000").await {
        Err(CallError::Remote(error)) => assert_eq!(error.code, ErrorCode::TopicNotFound),
        other => panic!("esperava `topic.not-found`: {other:?}"),
    }
    cluster.shutdown().await;
}

async fn publish_many(node: &TestNode, topic: &str, count: usize) {
    for n in 0..count {
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

fn delivered(body: &Value) -> usize {
    body["messages"].as_array().unwrap().len()
}

/// UC: UC-CNS-020, UC-CNS-022
#[tokio::test]
async fn a_get_tries_the_next_ids_after_a_failed_registration_up_to_the_attempt_limit() {
    let cluster = TestCluster::start(3).await;
    common::cluster::create_queue(cluster.node(1), "pedidos").await;
    publish_many(cluster.node(1), "pedidos", 30).await;

    // Um registro de invisibilidade que não fecha não derruba o `GET`: ele segue para o próximo
    // ID e entrega uma mensagem de outra.
    for number in 2..=3 {
        cluster
            .network
            .lose_kind(&host_of(number), "visibility-register", 1);
    }
    let response = get(
        cluster.node(1),
        "pedidos",
        "maxMessages=1&visibilityTimeout=60",
    )
    .await;
    assert_eq!(response.status(), 200);
    assert_eq!(delivered(&response.json().await.unwrap()), 1);
    for number in 2..=3 {
        cluster
            .network
            .lose_kind(&host_of(number), "visibility-register", 0);
    }

    // Sem nenhum registro fechando, o `GET` para depois de `delivery.get.max-attempts` falhas
    // (3 por padrão, em cada nó que entrega) e responde `503`, em vez de percorrer a fila inteira.
    for number in 1..=3 {
        cluster
            .network
            .lose_kind(&host_of(number), "visibility-register", 1_000_000);
    }
    cluster.network.clear_calls();
    let response = get(
        cluster.node(1),
        "pedidos",
        "maxMessages=1&visibilityTimeout=60",
    )
    .await;
    assert_eq!(response.status(), 503);
    let attempts = cluster.network.count("visibility-register");
    // 3 falhas × 2 pares × 3 tentativas (`write.retry.attempts`) × 3 nós que entregam.
    assert!(attempts <= 3 * 2 * 3 * 3, "{attempts} chamadas de registro");
    cluster.shutdown().await;
}

type Managed = Vec<std::collections::BTreeSet<String>>;

/// Quem gerencia cada fila agora, nó a nó (os `id` que cada um deriva da sua própria tabela).
fn managed_sets(cluster: &TestCluster, count: usize) -> Managed {
    (1..=count)
        .map(|n| {
            cluster
                .node(n)
                .node
                .volatile_managed()
                .into_iter()
                .collect()
        })
        .collect()
}

/// O nó (de 1 a `count`) que é o primário da posição do `id` na tabela do nó 1.
fn primary_of(cluster: &TestCluster, count: usize, id: &str) -> usize {
    let owners = cluster
        .node(1)
        .node
        .cluster()
        .unwrap()
        .owners_of(topic_position(id))
        .unwrap();
    (1..=count)
        .find(|n| cluster.node_id(*n) == owners[0].node_id)
        .unwrap()
}

async fn many_volatile_queues(cluster: &TestCluster, count: usize) -> Vec<String> {
    let mut ids = Vec::new();
    for n in 0..count {
        ids.push(volatile_queue(cluster.node(1), &format!("fila-{n:02}"), 3600).await);
    }
    ids
}

/// UC: UC-VOL-007, UC-VOL-016, UC-ING-023
#[tokio::test]
async fn the_managed_set_is_derived_from_the_table_and_a_new_node_manages_only_once_ready() {
    let mut cluster = TestCluster::start(3).await;
    settled(&cluster).await;
    let ids = many_volatile_queues(&cluster, 24).await;

    // Cada fila tem um gestor só, o primário da posição dela, e cada nó a deriva da própria tabela.
    eventually("cada fila ter o gestor que o anel manda", || {
        let sets = managed_sets(&cluster, 3);
        ids.iter().all(|id| {
            let holders: Vec<usize> = (1..=3).filter(|n| sets[*n - 1].contains(id)).collect();
            holders == [primary_of(&cluster, 3, id)]
        })
    })
    .await;

    // Criar e excluir um tópico muda o conjunto sozinho.
    let extra = volatile_queue(cluster.node(2), "fila-extra", 3600).await;
    let holder = primary_of(&cluster, 3, &extra);
    eventually("a fila nova entrar no conjunto do gestor", || {
        managed_sets(&cluster, 3)[holder - 1].contains(&extra)
    })
    .await;
    let deleted = cluster
        .node(3)
        .client
        .delete(cluster.node(3).client_url("/topics/fila-extra"))
        .send()
        .await
        .unwrap();
    assert_eq!(deleted.status(), 204);
    eventually("a fila excluída sair do conjunto", || {
        managed_sets(&cluster, 3)
            .iter()
            .all(|set| !set.contains(&extra))
    })
    .await;

    // O nó 4 entra e sincroniza: está na tabela e no anel, mas ainda não gerencia nada, e as filas
    // do trecho dele não são de ninguém até ele ficar pronto (nenhuma é excluída nesse intervalo).
    let fourth = common::cluster::start_isolated(&mut cluster, &[]).await;
    let gate = common::cluster::pause_sync(&cluster, fourth, "replay");
    common::cluster::wait_phase(cluster.node(fourth), "replay").await;
    eventually("os três nós terem o nó 4 na tabela", || {
        (1..=3).all(|n| cluster.member_ids(n).len() == 4)
    })
    .await;
    let moved: Vec<&String> = ids
        .iter()
        .filter(|id| primary_of(&cluster, 4, id) == 4)
        .collect();
    assert!(!moved.is_empty(), "o anel deu filas ao nó novo");
    eventually("as filas do trecho do nó 4 ficarem sem gestor", || {
        let sets = managed_sets(&cluster, 4);
        sets[3].is_empty()
            && moved
                .iter()
                .all(|id| (0..4).all(|n| !sets[n].contains(*id)))
            && ids
                .iter()
                .filter(|id| !moved.contains(id))
                .all(|id| (0..3).any(|n| sets[n].contains(id)))
    })
    .await;

    // Pronto o nó 4, ele é também gestor das filas do trecho dele, e os outros as largaram.
    gate.add_permits(1);
    common::cluster::wait_joined(cluster.node(fourth)).await;
    eventually("o nó 4 gerenciar as filas do trecho dele", || {
        let sets = managed_sets(&cluster, 4);
        ids.iter().all(|id| {
            let holders: Vec<usize> = (1..=4).filter(|n| sets[*n - 1].contains(id)).collect();
            holders == [primary_of(&cluster, 4, id)]
        }) && moved.iter().all(|id| sets[3].contains(*id))
    })
    .await;
    cluster.shutdown().await;
}

/// UC: UC-VOL-018
#[tokio::test]
async fn the_managed_set_of_every_node_goes_back_when_the_excluded_node_returns() {
    let cluster = TestCluster::start(3).await;
    settled(&cluster).await;
    let ids = many_volatile_queues(&cluster, 24).await;
    eventually("o conjunto de cada nó estabilizar", || {
        managed_sets(&cluster, 3)
            .iter()
            .map(|s| s.len())
            .sum::<usize>()
            == ids.len()
    })
    .await;
    let before = managed_sets(&cluster, 3);

    // O nó 3 é excluído: os dois outros dividem as filas dele.
    cluster.network.isolate(&host_of(3));
    eventually("os dois nós ficarem com todas as filas", || {
        let sets = managed_sets(&cluster, 2);
        cluster.member_ids(1).len() == 2
            && cluster.member_ids(2).len() == 2
            && ids
                .iter()
                .all(|id| sets.iter().filter(|s| s.contains(id)).count() == 1)
    })
    .await;

    // Passado o freezing, o nó volta com o mesmo ID, o anel é o de antes e o conjunto também.
    cluster.network.restore(&host_of(3));
    cluster.clock.advance(Duration::from_secs(301));
    common::cluster::wait_joined(cluster.node(3)).await;
    eventually("o conjunto de cada nó voltar ao de antes", || {
        managed_sets(&cluster, 3) == before
    })
    .await;
    cluster.shutdown().await;
}
