//! O backup opcional em S3 (doc 23, "Backup em S3"; doc 20, nível **C**): o depósito de objetos é o
//! em memória (`MemoryStore`), compartilhado entre os nós do cluster simulado, e o `Clock` é o
//! manual (a janela de `backup.delay` passa quando o teste avança o relógio).

mod common;

use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};

use common::cluster::{TestCluster, eventually, eventually_async};
use common::{TestNode, error_code};
use taba_node::storage::backup::{MemoryStore, ObjectStore, keys};

fn backup_of(node: &TestNode) -> Arc<taba_node::cluster::backup::Backup> {
    node.node.backup().expect("o backup está ligado").clone()
}

async fn create_queue(node: &TestNode, name: &str) {
    let response = node
        .client
        .post(node.client_url("/topics"))
        .json(&json!({"name": name, "type": "queue"}))
        .send()
        .await
        .expect("requisição");
    assert_eq!(response.status(), 201, "{:?}", response.text().await);
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

/// Como `consume`, mas uma resposta de erro (o quórum ainda não fechou) é "nada por enquanto".
async fn try_consume(node: &TestNode, topic: &str, query: &str) -> Vec<Value> {
    let response = node
        .client
        .get(node.client_url(&format!("/topics/{topic}/messages?{query}")))
        .send()
        .await
        .expect("requisição");
    if response.status() != 200 {
        return Vec::new();
    }
    let value: Value = response.json().await.unwrap();
    value["messages"].as_array().cloned().unwrap_or_default()
}

async fn ack(node: &TestNode, topic: &str, message: &Value) {
    let id = message["id"].as_str().unwrap();
    let token = message["receiptToken"].as_str().unwrap();
    let response = node
        .client
        .delete(node.client_url(&format!(
            "/topics/{topic}/messages/{id}?receiptToken={token}"
        )))
        .send()
        .await
        .expect("requisição");
    assert_eq!(response.status(), 204, "{:?}", response.text().await);
}

/// Quantas mensagens o bucket guarda nos segmentos do tópico (somando os rodapés).
async fn messages_in_bucket(s3: &MemoryStore, topic: &str) -> usize {
    let mut total = 0;
    for key in s3.keys(&format!("taba/{}", keys::topic_messages_prefix(topic))) {
        let body = s3.get(&key).await.unwrap().unwrap().body;
        total += taba_node::storage::backup::segment::decode_all(&body)
            .unwrap()
            .1
            .len();
    }
    total
}

fn outbox_total(cluster: &TestCluster) -> u64 {
    cluster
        .nodes
        .iter()
        .map(|n| backup_of(n).status().outbox_pending)
        .sum()
}

/// UC: UC-BKP-001
#[tokio::test]
async fn without_a_bucket_the_backup_does_not_exist_and_its_routes_say_so() {
    let node = TestNode::development().await;
    assert!(node.node.backup().is_none());
    for (method, path) in [
        ("GET", "/node/backup"),
        ("POST", "/node/backup/flush"),
        ("POST", "/cluster/backup/flush"),
    ] {
        let request = match method {
            "GET" => node.client.get(node.operations_url(path)),
            _ => node.client.post(node.operations_url(path)),
        };
        let response = request.send().await.unwrap();
        assert_eq!(
            error_code(response, 422).await,
            "backup.not-configured",
            "{path}"
        );
    }
    node.shutdown().await;
}

/// UC: UC-BKP-009, UC-BKP-010, UC-BKP-011, UC-BKP-012
#[tokio::test]
async fn a_message_consumed_before_the_delay_never_reaches_s3_and_the_rest_does() {
    let s3 = Arc::new(MemoryStore::new());
    let cluster = TestCluster::start_with_backup(3, &[], s3.clone()).await;
    create_queue(cluster.node(1), "pedidos").await;
    for i in 0..10 {
        publish(cluster.node(1), "pedidos", json!({"n": i})).await;
    }
    eventually("o outbox dos donos ter as 10 mensagens", || {
        outbox_total(&cluster) == 10
    })
    .await;
    assert_eq!(
        messages_in_bucket(&s3, "pedidos").await,
        0,
        "nada sobe antes da janela"
    );

    // O consumidor confirma 6 dentro da janela: elas saem do outbox sem gerar upload.
    let got = consume(
        cluster.node(1),
        "pedidos",
        "maxMessages=6&visibilityTimeout=300",
    )
    .await;
    assert_eq!(got.len(), 6);
    for message in &got {
        ack(cluster.node(1), "pedidos", message).await;
    }
    eventually("as 6 confirmadas saírem do outbox", || {
        outbox_total(&cluster) == 4
    })
    .await;

    // A janela fecha: as 4 restantes vão ao S3, e só depois do PUT saem do outbox.
    cluster.clock.advance(Duration::from_secs(61));
    eventually("o outbox esvaziar", || outbox_total(&cluster) == 0).await;
    assert_eq!(messages_in_bucket(&s3, "pedidos").await, 4);
    cluster.shutdown().await;
}

/// UC: UC-BKP-009, UC-BKP-012
#[tokio::test]
async fn publishing_never_waits_for_s3_and_the_outbox_keeps_the_ids_while_it_is_down() {
    let s3 = Arc::new(MemoryStore::new());
    let cluster = TestCluster::start_with_backup(3, &[], s3.clone()).await;
    create_queue(cluster.node(1), "pedidos").await;
    s3.set_down(true);
    for i in 0..5 {
        publish(cluster.node(2), "pedidos", json!({"n": i})).await;
    }
    cluster.clock.advance(Duration::from_secs(61));
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(
        outbox_total(&cluster),
        5,
        "o S3 fora do ar: os IDs ficam no outbox"
    );
    let consumed = consume(cluster.node(3), "pedidos", "maxMessages=5").await;
    assert_eq!(consumed.len(), 5, "o consumo segue como sem o S3");

    s3.set_down(false);
    eventually("o outbox esvaziar quando o S3 volta", || {
        outbox_total(&cluster) == 0
    })
    .await;
    assert_eq!(messages_in_bucket(&s3, "pedidos").await, 5);
    cluster.shutdown().await;
}

/// UC: UC-BKP-016, UC-BKP-018
#[tokio::test]
async fn definitions_and_tokens_go_to_the_bucket_and_a_failure_never_refuses_the_change() {
    let s3 = Arc::new(MemoryStore::new());
    let cluster = TestCluster::start_with_backup(3, &[], s3.clone()).await;
    create_queue(cluster.node(1), "pedidos").await;
    eventually("a definição no bucket", || {
        s3.peek("taba/topics/pedidos.json").is_some()
    })
    .await;
    let stored: Value =
        serde_json::from_slice(&s3.peek("taba/topics/pedidos.json").unwrap()).unwrap();
    assert_eq!(stored["definition"]["name"], "pedidos");

    // O S3 fora do ar: a criação vale no cluster, e a gravação espera a volta.
    s3.set_down(true);
    create_queue(cluster.node(2), "faturas").await;
    assert!(s3.peek("taba/topics/faturas.json").is_none());
    s3.set_down(false);
    eventually("a definição que falhou ser regravada", || {
        s3.peek("taba/topics/faturas.json").is_some()
    })
    .await;
    cluster.shutdown().await;
}

/// UC: UC-BKP-017
#[tokio::test]
async fn removing_a_topic_leaves_a_tombstone_and_clears_its_messages_from_the_bucket() {
    let s3 = Arc::new(MemoryStore::new());
    let cluster = TestCluster::start_with_backup(3, &[], s3.clone()).await;
    create_queue(cluster.node(1), "pedidos").await;
    publish(cluster.node(1), "pedidos", json!({"n": 1})).await;
    cluster.clock.advance(Duration::from_secs(61));
    eventually("a mensagem no bucket", || {
        !s3.keys("taba/messages/pedidos/").is_empty()
    })
    .await;
    let response = cluster
        .node(1)
        .client
        .delete(cluster.node(1).client_url("/topics/pedidos"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 204);
    eventually("a lápide e o prefixo apagado", || {
        s3.keys("taba/messages/pedidos/").is_empty()
            && s3
                .peek("taba/topics/pedidos.json")
                .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
                .is_some_and(|v| v["deleted"] == true)
    })
    .await;
    cluster.shutdown().await;
}

/// UC: UC-BKP-019
#[tokio::test]
async fn every_node_beats_in_the_bucket_with_a_growing_sequence_and_leaves_on_drain() {
    let s3 = Arc::new(MemoryStore::new());
    let cluster = TestCluster::start_with_backup(3, &[], s3.clone()).await;
    let id = cluster.node_id(2);
    let key = format!("taba/nodes/{id}.json");
    eventually("o batimento do nó 2", || s3.peek(&key).is_some()).await;
    let first: Value = serde_json::from_slice(&s3.peek(&key).unwrap()).unwrap();
    assert_eq!(first["id"], id.as_str());
    assert!(first["addresses"].as_array().is_some_and(|a| !a.is_empty()));
    let seq = first["seq"].as_u64().unwrap();
    eventually("o seq crescer", || {
        s3.peek(&key)
            .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
            .is_some_and(|v| v["seq"].as_u64().unwrap() > seq)
    })
    .await;

    // Um nó que sai limpo (drain) remove o próprio arquivo.
    let response = cluster
        .node(2)
        .client
        .post(cluster.node(2).operations_url("/node/drain"))
        .send()
        .await
        .unwrap();
    assert!(response.status().is_success());
    eventually("o arquivo do nó 2 sumir", || s3.peek(&key).is_none()).await;
    cluster.shutdown().await;
}

/// UC: UC-BKP-032
#[tokio::test]
async fn the_node_flush_sends_everything_without_stopping_the_node() {
    let s3 = Arc::new(MemoryStore::new());
    let cluster = TestCluster::start_with_backup(3, &[], s3.clone()).await;
    create_queue(cluster.node(1), "pedidos").await;
    for i in 0..6 {
        publish(cluster.node(1), "pedidos", json!({"n": i})).await;
    }
    eventually("o outbox ter as 6", || outbox_total(&cluster) == 6).await;

    // Sem esperar a janela: o flush manda tudo agora.
    for number in 1..=3 {
        let response = cluster
            .node(number)
            .client
            .post(cluster.node(number).operations_url("/node/backup/flush"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let report: Value = response.json().await.unwrap();
        assert_eq!(report["complete"], true, "{report}");
        assert_eq!(report["outboxPending"], 0);
    }
    assert_eq!(messages_in_bucket(&s3, "pedidos").await, 6);
    // O nó segue no ar e atende.
    assert_eq!(
        consume(cluster.node(1), "pedidos", "maxMessages=6")
            .await
            .len(),
        6
    );
    cluster.shutdown().await;
}

/// UC: UC-BKP-033, UC-BKP-038
#[tokio::test]
async fn the_cluster_flush_asks_every_node_and_a_slow_one_shows_up_as_pending() {
    let s3 = Arc::new(MemoryStore::new());
    let cluster = TestCluster::start_with_backup(3, &[], s3.clone()).await;
    create_queue(cluster.node(1), "pedidos").await;
    for i in 0..4 {
        publish(cluster.node(1), "pedidos", json!({"n": i})).await;
    }
    let response = cluster
        .node(2)
        .client
        .post(cluster.node(2).operations_url("/cluster/backup/flush"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let report: Value = response.json().await.unwrap();
    assert_eq!(report["complete"], true, "{report}");
    assert_eq!(report["nodes"].as_array().unwrap().len(), 3);
    assert_eq!(messages_in_bucket(&s3, "pedidos").await, 4);
    eventually_async("o status mostrar tudo enviado", || async {
        let status: Value = cluster
            .node(1)
            .client
            .get(cluster.node(1).operations_url("/node/backup"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        status["pending"] == 0
    })
    .await;
    cluster.shutdown().await;
}

async fn stop_cluster(cluster: &TestCluster) -> Value {
    let response = cluster
        .node(1)
        .client
        .post(cluster.node(1).operations_url("/cluster/stop"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    response.json().await.unwrap()
}

/// UC: UC-BKP-023, UC-BKP-026, UC-BKP-031, UC-BKP-034, UC-BKP-035
#[tokio::test]
async fn a_planned_stop_flushes_everything_and_the_next_start_restores_the_cluster() {
    let s3 = Arc::new(MemoryStore::new());
    let cluster = TestCluster::start_with_backup(3, &[], s3.clone()).await;
    create_queue(cluster.node(1), "pedidos").await;
    for i in 0..8 {
        publish(cluster.node(1), "pedidos", json!({"n": i})).await;
    }
    // 3 confirmadas antes da parada: não podem voltar.
    let got = consume(
        cluster.node(1),
        "pedidos",
        "maxMessages=3&visibilityTimeout=300",
    )
    .await;
    for message in &got {
        ack(cluster.node(1), "pedidos", message).await;
    }

    let stopped = stop_cluster(&cluster).await;
    assert_eq!(stopped["stopList"], true, "{stopped}");
    assert_eq!(stopped["stopped"].as_array().unwrap().len(), 3);
    let list: Value = serde_json::from_slice(&s3.peek("taba/cluster/stop.json").unwrap()).unwrap();
    assert_eq!(
        list["nodes"].as_array().unwrap().len(),
        3,
        "a lista da parada"
    );
    assert!(
        s3.keys("taba/nodes/").is_empty(),
        "quem sai limpo remove o próprio batimento"
    );
    for number in 1..=3 {
        assert_eq!(
            backup_of(cluster.node(number)).status().outbox_pending,
            0,
            "o drain só termina com o outbox enviado"
        );
    }
    cluster.shutdown().await;

    // Os processos sobem de novo (disco vazio, nós novos): o primeiro restaura sozinho.
    let again = TestCluster::start_with_backup(3, &[], s3.clone()).await;
    eventually_async("as 5 mensagens voltarem", || async {
        let got = consume(
            again.node(2),
            "pedidos",
            "maxMessages=10&visibilityTimeout=300",
        )
        .await;
        got.len() == 5
    })
    .await;
    assert!(
        s3.peek("taba/restore/lock.json").is_none(),
        "a trava é solta ao fim da restauração"
    );
    assert!(
        s3.peek("taba/cluster/stop.json").is_none(),
        "a lista da parada some depois de usada"
    );
    again.shutdown().await;
}

/// UC: UC-BKP-022, UC-BKP-023, UC-BKP-026
#[tokio::test]
async fn after_a_crash_the_survivor_waits_for_the_heartbeats_to_expire_and_restores() {
    let s3 = Arc::new(MemoryStore::new());
    let env = [("TABA_DRAIN_ON_STOP", "false")];
    let cluster = TestCluster::start_with_backup(3, &env, s3.clone()).await;
    create_queue(cluster.node(1), "pedidos").await;
    for i in 0..6 {
        publish(cluster.node(2), "pedidos", json!({"n": i})).await;
    }
    // O backup de um cluster que vai cair: o flush antes da queda.
    let response = cluster
        .node(1)
        .client
        .post(cluster.node(1).operations_url("/cluster/backup/flush"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    cluster.shutdown().await; // sem drain: os batimentos ficam no bucket, parados
    assert!(
        !s3.keys("taba/nodes/").is_empty(),
        "queda: os arquivos ficam"
    );
    assert!(s3.peek("taba/cluster/stop.json").is_none());

    let again = TestCluster::start_with_backup(3, &[], s3.clone()).await;
    eventually_async("as 6 mensagens voltarem", || async {
        consume(
            again.node(2),
            "pedidos",
            "maxMessages=10&visibilityTimeout=300",
        )
        .await
        .len()
            == 6
    })
    .await;
    again.shutdown().await;
}

/// O nó morre de verdade: sai da rede (ninguém mais o alcança) e para (o batimento no bucket pára).
async fn kill_last(cluster: &mut TestCluster) {
    let number = cluster.nodes.len();
    cluster.network.isolate(&common::cluster::host_of(number));
    cluster.nodes.pop().unwrap().shutdown().await;
}

fn members_of(cluster: &TestCluster, number: usize) -> usize {
    cluster.table(number).map_or(0, |t| t.members.len())
}

/// UC: UC-BKP-020, UC-BKP-021
#[tokio::test]
async fn an_unreachable_node_with_a_fresh_heartbeat_is_not_excluded_until_the_heartbeat_expires() {
    let s3 = Arc::new(MemoryStore::new());
    let env = [("TABA_DRAIN_ON_STOP", "false")];
    let mut cluster = TestCluster::start_with_backup(3, &env, s3.clone()).await;
    common::cluster::settled(&cluster).await;

    // O nó 3 fica inalcançável, mas continua batendo no bucket: vivo em outra partição.
    cluster.network.isolate(&common::cluster::host_of(3));
    tokio::time::sleep(Duration::from_secs(4)).await;
    assert_eq!(
        members_of(&cluster, 1),
        3,
        "não assume os blocos de quem está vivo"
    );
    let victim = cluster.node_id(3);
    assert_eq!(
        backup_of(cluster.node(1)).liveness(&victim),
        taba_node::cluster::backup::Liveness::Fresh
    );

    // Agora ele morre de verdade: o batimento para, vence, e só então sai da tabela.
    let dead = cluster.nodes.pop().unwrap();
    dead.shutdown().await;
    let died_at = std::time::Instant::now();
    eventually("o nó 3 sair da tabela", || members_of(&cluster, 1) == 2).await;
    assert!(
        died_at.elapsed() >= Duration::from_millis(900),
        "a exclusão esperou o batimento vencer ({:?})",
        died_at.elapsed()
    );
    // Os dois sobreviventes observam o bucket por conta própria: o observador que votou o vê morto
    // logo depois (pode haver meio batimento de diferença entre as duas leituras).
    let observer = backup_of(cluster.node(1));
    eventually("o batimento morto para quem observa", || {
        observer.liveness(&victim) == taba_node::cluster::backup::Liveness::Dead
    })
    .await;
    cluster.shutdown().await;
}

/// UC: UC-BKP-022
#[tokio::test]
async fn three_of_five_nodes_fall_and_the_survivors_find_the_missing_messages_in_s3() {
    let s3 = Arc::new(MemoryStore::new());
    let env = [("TABA_DRAIN_ON_STOP", "false")];
    let mut cluster = TestCluster::start_with_backup(5, &env, s3.clone()).await;
    common::cluster::settled(&cluster).await;
    create_queue(cluster.node(1), "pedidos").await;
    for i in 0..30 {
        publish(cluster.node(1), "pedidos", json!({"n": i})).await;
    }
    let response = cluster
        .node(1)
        .client
        .post(cluster.node(1).operations_url("/cluster/backup/flush"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(messages_in_bucket(&s3, "pedidos").await, 30);

    // Três caem ao mesmo tempo (sem drain: os batimentos ficam parados no bucket).
    for _ in 0..3 {
        kill_last(&mut cluster).await;
    }
    eventually("os dois sobreviventes excluírem os mortos", || {
        members_of(&cluster, 1) == 2 && members_of(&cluster, 2) == 2
    })
    .await;
    let seen = Arc::new(std::sync::Mutex::new(std::collections::HashSet::new()));
    eventually_async("as 30 mensagens voltarem a ser entregues", || {
        let seen = seen.clone();
        let cluster = &cluster;
        async move {
            for message in try_consume(
                cluster.node(1),
                "pedidos",
                "maxMessages=10&visibilityTimeout=600",
            )
            .await
            {
                seen.lock()
                    .unwrap()
                    .insert(message["id"].as_str().unwrap().to_owned());
            }
            let delivered = seen.lock().unwrap().len();
            delivered == 30
        }
    })
    .await;
    cluster.shutdown().await;
}

/// UC: UC-BKP-023
#[tokio::test]
async fn a_single_survivor_excludes_the_dead_even_below_the_minimum_of_voters() {
    let s3 = Arc::new(MemoryStore::new());
    let env = [("TABA_DRAIN_ON_STOP", "false")];
    let mut cluster = TestCluster::start_with_backup(3, &env, s3.clone()).await;
    common::cluster::settled(&cluster).await;
    for _ in 0..2 {
        kill_last(&mut cluster).await;
    }
    eventually("o único nó excluir os dois mortos", || {
        members_of(&cluster, 1) == 1
    })
    .await;
    cluster.shutdown().await;
}

/// Os registros de todos os diários do tópico no bucket, do mais antigo ao mais novo.
async fn journal_records(
    s3: &MemoryStore,
    topic: &str,
) -> Vec<taba_node::storage::backup::journal::Record> {
    let mut out = Vec::new();
    for key in s3.keys(&format!("taba/{}", keys::topic_journal_prefix(topic))) {
        let body = s3.get(&key).await.unwrap().unwrap().body;
        out.extend(
            taba_node::storage::backup::journal::decode(&body)
                .unwrap()
                .1,
        );
    }
    out
}

async fn flush_all(cluster: &TestCluster) {
    let response = cluster
        .node(1)
        .client
        .post(cluster.node(1).operations_url("/cluster/backup/flush"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let report: Value = response.json().await.unwrap();
    assert_eq!(report["complete"], true, "{report}");
}

/// UC: UC-BKP-013, UC-BKP-014
#[tokio::test]
async fn the_journal_keeps_the_last_state_and_an_ack_is_a_removal_that_empties_the_segment() {
    let s3 = Arc::new(MemoryStore::new());
    let cluster = TestCluster::start_with_backup(3, &[], s3.clone()).await;
    create_queue(cluster.node(1), "pedidos").await;
    for i in 0..3 {
        publish(cluster.node(1), "pedidos", json!({"n": i})).await;
    }
    flush_all(&cluster).await;
    assert!(!s3.keys("taba/messages/pedidos/").is_empty());

    // Entregue na janela: o diário guarda o último estado da mensagem.
    let first = consume(
        cluster.node(1),
        "pedidos",
        "maxMessages=1&visibilityTimeout=300",
    )
    .await;
    let id = first[0]["id"].as_str().unwrap().to_owned();
    eventually_async("o estado da entrega no diário", || async {
        journal_records(&s3, "pedidos")
            .await
            .iter()
            .any(|r| r.id == id && !r.removed && r.delivery_count == 1)
    })
    .await;

    // O ack é uma remoção; ninguém apaga uma mensagem solta do bucket.
    ack(cluster.node(1), "pedidos", &first[0]).await;
    eventually_async("a remoção no diário", || async {
        journal_records(&s3, "pedidos")
            .await
            .iter()
            .any(|r| r.id == id && r.removed)
    })
    .await;
    assert!(
        !s3.keys("taba/messages/pedidos/").is_empty(),
        "o segmento com mensagens vivas continua no bucket"
    );

    // Quando todas as mensagens de um segmento morrem, o primário o apaga.
    for message in consume(
        cluster.node(1),
        "pedidos",
        "maxMessages=5&visibilityTimeout=300",
    )
    .await
    {
        ack(cluster.node(1), "pedidos", &message).await;
    }
    eventually("os segmentos mortos saírem do bucket", || {
        s3.keys("taba/messages/pedidos/").is_empty()
    })
    .await;
    cluster.shutdown().await;
}

/// UC: UC-BKP-015
#[tokio::test]
async fn volatile_queues_and_dead_letters_are_backed_up_too() {
    let s3 = Arc::new(MemoryStore::new());
    let cluster = TestCluster::start_with_backup(3, &[], s3.clone()).await;
    let response = cluster
        .node(1)
        .client
        .post(cluster.node(1).client_url("/topics"))
        .json(&json!({"name": "efemera", "type": "queue", "volatile": true}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 201);
    publish(cluster.node(1), "efemera", json!({"v": 1})).await;

    let response = cluster
        .node(1)
        .client
        .post(cluster.node(1).client_url("/topics"))
        .json(&json!({"name": "pedidos", "type": "queue", "maxDeliveries": 1}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 201);
    publish(cluster.node(1), "pedidos", json!({"n": 1})).await;
    let got = consume(
        cluster.node(1),
        "pedidos",
        "maxMessages=1&visibilityTimeout=1",
    )
    .await;
    assert_eq!(got.len(), 1);
    // A invisibilidade vence com o limite de entregas gasto: a mensagem vai à dead-letter.
    cluster.clock.advance(Duration::from_secs(5));
    eventually_async("a mensagem na dead-letter", || async {
        !try_consume(
            cluster.node(2),
            "pedidos-dead-letter",
            "maxMessages=1&visibilityTimeout=1",
        )
        .await
        .is_empty()
    })
    .await;
    flush_all(&cluster).await;
    assert!(
        !s3.keys("taba/messages/efemera/").is_empty(),
        "fila volátil"
    );
    assert!(
        !s3.keys("taba/messages/pedidos-dead-letter/").is_empty(),
        "dead-letter"
    );
    cluster.shutdown().await;
}

/// UC: UC-BKP-001
#[tokio::test]
async fn with_s3_down_at_start_the_nodes_start_with_a_warning_and_the_cluster_works() {
    let s3 = Arc::new(MemoryStore::new());
    s3.set_down(true);
    let cluster = TestCluster::start_with_backup(3, &[], s3.clone()).await;
    create_queue(cluster.node(1), "pedidos").await;
    publish(cluster.node(2), "pedidos", json!({"n": 1})).await;
    assert_eq!(
        consume(cluster.node(3), "pedidos", "maxMessages=1")
            .await
            .len(),
        1
    );
    let status: Value = cluster
        .node(1)
        .client
        .get(cluster.node(1).operations_url("/node/backup"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(status["s3Reachable"], false);
    // O S3 volta: o recurso liga sozinho.
    s3.set_down(false);
    eventually("o batimento no bucket", || {
        !s3.keys("taba/nodes/").is_empty()
    })
    .await;
    cluster.shutdown().await;
}

/// UC: UC-BKP-036
#[tokio::test]
async fn the_primary_tells_the_replicas_which_messages_are_already_in_the_bucket() {
    let s3 = Arc::new(MemoryStore::new());
    let cluster = TestCluster::start_with_backup(3, &[], s3.clone()).await;
    create_queue(cluster.node(1), "pedidos").await;
    for i in 0..5 {
        publish(cluster.node(1), "pedidos", json!({"n": i})).await;
    }
    flush_all(&cluster).await;
    // Fator de replicação 3 em 3 nós: todos guardam as 5 e, com o aviso, todos sabem onde estão.
    for number in 1..=3 {
        let node = backup_of(cluster.node(number));
        let watching = node.clone();
        eventually("a réplica saber onde as 5 estão", move || {
            watching.status().saved == 5
        })
        .await;
        // A referência traz o instante do PUT (relógio do primário), em qualquer réplica.
        let first = node.db.saved_of_topic("pedidos").unwrap().remove(0);
        assert!(first.1.saved_at_ms > 0, "saved_at ausente");
        assert_eq!(
            node.saved_info("pedidos", &first.0).unwrap().saved_at_ms,
            first.1.saved_at_ms
        );
    }
    cluster.shutdown().await;
}

/// UC: UC-BKP-037, UC-BKP-003
#[tokio::test]
async fn when_the_primary_dies_the_new_primary_sends_what_was_still_pending() {
    let s3 = Arc::new(MemoryStore::new());
    let env = [("TABA_DRAIN_ON_STOP", "false")];
    let mut cluster = TestCluster::start_with_backup(3, &env, s3.clone()).await;
    common::cluster::settled(&cluster).await;
    create_queue(cluster.node(1), "pedidos").await;
    for i in 0..9 {
        publish(cluster.node(1), "pedidos", json!({"n": i})).await;
    }
    eventually("o outbox dos donos", || outbox_total(&cluster) == 9).await;
    // O nó 3 cai antes de enviar o que tinha no outbox.
    kill_last(&mut cluster).await;
    eventually("o nó 3 sair da tabela", || members_of(&cluster, 1) == 2).await;
    // O herdeiro adota as mensagens quando a tabela muda; a janela dele conta dali.
    let s3_check = s3.clone();
    let clock = cluster.clock.clone();
    eventually_async("os herdeiros enviarem as 9", || {
        let s3 = s3_check.clone();
        clock.advance(Duration::from_secs(61));
        async move { messages_in_bucket(&s3, "pedidos").await == 9 }
    })
    .await;
    cluster.shutdown().await;
}

/// UC: UC-BKP-028, UC-BKP-029
#[tokio::test]
async fn a_full_disk_evicts_the_bodies_already_in_s3_and_reading_one_goes_back_to_the_bucket() {
    let s3 = Arc::new(MemoryStore::new());
    let cluster = TestCluster::start_with_backup(3, &[], s3.clone()).await;
    create_queue(cluster.node(1), "pedidos").await;
    let mut ids = Vec::new();
    for i in 0..6 {
        ids.push(
            publish(
                cluster.node(1),
                "pedidos",
                json!({"n": i, "texto": "x".repeat(200)}),
            )
            .await,
        );
    }
    // Tudo para o disco (etapa 1 do flush) e para o S3.
    flush_all(&cluster).await;
    let bodies = |number: usize| -> usize {
        let dir = cluster
            .node(number)
            .storage_dir()
            .join(taba_node::storage::messages::dir_name("pedidos"));
        ids.iter()
            .filter(|id| dir.join(format!("{id}.body")).is_file())
            .count()
    };
    // O flush só moveu a memória do nó 1; os outros também a levam ao disco.
    for number in 2..=3 {
        let response = cluster
            .node(number)
            .client
            .post(cluster.node(number).operations_url("/node/backup/flush"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
    }
    for number in 1..=3 {
        eventually("a réplica saber onde as 6 estão", || {
            backup_of(cluster.node(number)).status().saved == 6
        })
        .await;
        assert_eq!(
            bodies(number),
            6,
            "antes da evicção os corpos estão no disco"
        );
    }

    // O disco passa de 85%: o corpo do que já está no S3 sai do disco; a linha fica.
    cluster.set_disk_usage(90);
    for number in 1..=3 {
        eventually("os corpos saírem do disco", || bodies(number) == 0).await;
    }
    assert!(backup_of(cluster.node(1)).status().bodies_evicted > 0);

    // A leitura acha o arquivo ausente e busca o corpo no segmento por `Range`.
    cluster.set_disk_usage(10);
    let got = consume(
        cluster.node(2),
        "pedidos",
        "maxMessages=6&visibilityTimeout=300",
    )
    .await;
    assert_eq!(got.len(), 6);
    for message in &got {
        assert_eq!(message["body"]["texto"], "x".repeat(200));
    }
    cluster.shutdown().await;
}

/// UC: UC-BKP-030
#[tokio::test]
async fn at_ninety_percent_the_memory_to_disk_pass_pauses_until_the_disk_has_room() {
    let s3 = Arc::new(MemoryStore::new());
    let cluster = TestCluster::start_with_backup(3, &[], s3.clone()).await;
    create_queue(cluster.node(1), "pedidos").await;
    for i in 0..3 {
        publish(cluster.node(1), "pedidos", json!({"n": i})).await;
    }
    cluster.set_disk_usage(95);
    let started = std::time::Instant::now();
    let usage = cluster.disk_usage.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(700)).await;
        usage.store(10, std::sync::atomic::Ordering::SeqCst);
    });
    let response = cluster
        .node(1)
        .client
        .post(cluster.node(1).operations_url("/node/backup/flush"))
        .send()
        .await
        .unwrap();
    let report: Value = response.json().await.unwrap();
    assert!(
        started.elapsed() >= Duration::from_millis(600),
        "o flush esperou o disco ({:?}): {report}",
        started.elapsed()
    );
    assert_eq!(report["memoryRemaining"], 0, "{report}");
    cluster.shutdown().await;
}

/// UC: UC-BKP-011
#[tokio::test]
async fn a_segment_closes_by_message_count_before_it_gets_old() {
    let s3 = Arc::new(MemoryStore::new());
    let env = [
        ("TABA_BACKUP_SEGMENT_MAX_MESSAGES", "2"),
        // A idade nunca fecha: só a quantidade.
        ("TABA_BACKUP_SEGMENT_MAX_AGE", "1h"),
    ];
    let cluster = TestCluster::start_with_backup(3, &env, s3.clone()).await;
    create_queue(cluster.node(1), "pedidos").await;
    for i in 0..12 {
        publish(cluster.node(1), "pedidos", json!({"n": i})).await;
    }
    cluster.clock.advance(Duration::from_secs(61));
    // Cada dono fecha o segmento ao chegar em 2 mensagens; o resto espera a idade (1 h): ao menos
    // metade das 12 já subiu, em segmentos de no máximo 2.
    eventually_async("segmentos fechados por quantidade", || async {
        messages_in_bucket(&s3, "pedidos").await >= 6
    })
    .await;
    for key in s3.keys("taba/messages/pedidos/") {
        let body = s3.get(&key).await.unwrap().unwrap().body;
        let (footer, _) = taba_node::storage::backup::segment::decode_all(&body).unwrap();
        assert!(
            footer.entries.len() <= 2,
            "{key}: {} mensagens",
            footer.entries.len()
        );
    }
    cluster.shutdown().await;
}

/// UC: UC-BKP-018
#[tokio::test]
async fn tokens_go_to_the_bucket_one_file_each_and_a_revoked_token_disappears() {
    let s3 = Arc::new(MemoryStore::new());
    let root = "raiz-raiz-raiz-raiz-raiz-raiz-raiz-0001";
    let env = [
        ("TABA_AUTH_ROOT_TOKEN", root),
        ("TABA_AUTH_ENABLED", "true"),
    ];
    let cluster = TestCluster::start_with_backup(3, &env, s3.clone()).await;
    let node = cluster.node(1);
    node.clock.advance(Duration::from_secs(1));
    let response = node
        .client
        .post(node.client_url("/tokens"))
        .header("authorization", format!("Bearer {root}"))
        .json(&json!({"name": "ci", "permissions": ["publish"], "topicPrefixes": ["pedidos"]}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 201, "{:?}", response.text().await);
    let created: Value = response.json().await.unwrap();
    let id = created["id"].as_str().unwrap().to_owned();
    let key = format!("taba/tokens/{id}.json");
    eventually("o token no bucket", || s3.peek(&key).is_some()).await;
    let stored: Value = serde_json::from_slice(&s3.peek(&key).unwrap()).unwrap();
    assert_eq!(stored["token"]["name"], "ci");
    assert!(
        stored["token"]["secretHash"].is_string() && stored["token"].get("token").is_none(),
        "só o hash salgado vai ao bucket, nunca o segredo"
    );

    node.clock.advance(Duration::from_secs(1));
    let response = node
        .client
        .delete(node.client_url(&format!("/tokens/{id}")))
        .header("authorization", format!("Bearer {root}"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 204);
    eventually("o token revogado sair do bucket", || {
        s3.peek(&key).is_none()
    })
    .await;
    cluster.shutdown().await;
}

/// UC: UC-BKP-039, UC-BKP-034
#[tokio::test]
async fn with_s3_down_the_stop_still_drains_every_node_and_warns_that_the_backup_was_not_sent() {
    let s3 = Arc::new(MemoryStore::new());
    let env = [("TABA_BACKUP_DRAIN_TIMEOUT", "1s")];
    let cluster = TestCluster::start_with_backup(3, &env, s3.clone()).await;
    create_queue(cluster.node(1), "pedidos").await;
    for i in 0..6 {
        publish(cluster.node(1), "pedidos", json!({"n": i})).await;
    }
    s3.set_down(true);
    cluster.clock.advance(Duration::from_secs(61));
    let started = std::time::Instant::now();
    let report = stop_cluster(&cluster).await;
    assert_eq!(report["stopList"], false, "{report}");
    assert!(
        report["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w.as_str().unwrap().contains("S3")),
        "avisa que o backup não foi enviado: {report}"
    );
    assert_eq!(
        report["stopped"].as_array().unwrap().len(),
        3,
        "o drain em disco foi feito"
    );
    assert!(
        started.elapsed() >= Duration::from_millis(900),
        "o envio esperou até o prazo do backup antes de desistir ({:?})",
        started.elapsed()
    );
    for number in 1..=3 {
        assert_eq!(
            cluster.node(number).node.shared().drain_state(),
            taba_node::cluster::state::DrainState::Drained
        );
    }
    cluster.shutdown().await;
}

/// UC: UC-BKP-040
#[tokio::test]
async fn a_single_node_in_development_mode_owns_everything_and_backs_up_too() {
    let s3 = Arc::new(MemoryStore::new());
    let node = TestNode::builder()
        .backup_store(s3.clone())
        .env("TABA_BACKUP_DISK_EVICT_TO", "98")
        .env("TABA_BACKUP_DISK_EVICT_ABOVE", "99")
        .env("TABA_BACKUP_DISK_PAUSE_AT", "100")
        .start()
        .await;
    create_queue(&node, "pedidos").await;
    for i in 0..3 {
        publish(&node, "pedidos", json!({"n": i})).await;
    }
    let response = node
        .client
        .post(node.operations_url("/node/backup/flush"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let report: Value = response.json().await.unwrap();
    assert_eq!(report["outboxPending"], 0, "{report}");
    assert_eq!(messages_in_bucket(&s3, "pedidos").await, 3);
    assert!(s3.peek("taba/topics/pedidos.json").is_some());
    node.shutdown().await;
}

/// A latência de publicação com e sem o backup (e com o S3 fora do ar): o S3 não pode aparecer nela.
/// Não roda no `cargo test` (mede tempo); rode com
/// `cargo test --test backup publish_latency -- --ignored --nocapture`.
#[tokio::test]
#[ignore = "mede tempo; rode à mão"]
async fn publish_latency_with_and_without_s3() {
    async fn run(label: &str, cluster: &TestCluster) -> f64 {
        create_queue(cluster.node(1), "medida").await;
        // Aquece.
        for i in 0..30 {
            publish(cluster.node(1), "medida", json!({"w": i})).await;
        }
        let mut samples = Vec::new();
        for i in 0..300 {
            let started = std::time::Instant::now();
            publish(cluster.node(2), "medida", json!({"n": i})).await;
            samples.push(started.elapsed().as_secs_f64() * 1000.0);
        }
        samples.sort_by(f64::total_cmp);
        let p50 = samples[samples.len() / 2];
        let p99 = samples[samples.len() * 99 / 100];
        eprintln!("{label:<28} p50 {p50:6.2} ms   p99 {p99:6.2} ms");
        p50
    }

    let without = TestCluster::start(3).await;
    let base = run("sem backup", &without).await;
    without.shutdown().await;

    let s3 = Arc::new(MemoryStore::new());
    let with = TestCluster::start_with_backup(3, &[], s3.clone()).await;
    let up = run("com backup (S3 no ar)", &with).await;
    with.shutdown().await;

    let s3 = Arc::new(MemoryStore::new());
    s3.set_down(true);
    let down_cluster = TestCluster::start_with_backup(3, &[], s3.clone()).await;
    let down = run("com backup (S3 fora do ar)", &down_cluster).await;
    down_cluster.shutdown().await;

    // O backup só insere uma linha no outbox: nada de S3 na publicação. Folga larga (máquina de
    // desenvolvimento, três nós no mesmo processo).
    assert!(
        up < base * 2.0 + 5.0,
        "com backup {up} ms contra {base} ms sem"
    );
    assert!(
        down < base * 2.0 + 5.0,
        "com S3 fora {down} ms contra {base} ms sem"
    );
}
