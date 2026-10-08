//! O canal entre os nós no cluster simulado (doc 04, seção 6.1; doc 13, seções 12.8 e 12.9; doc 06c;
//! doc 20b, nível **C**): conexões, sinal de vida, `seq` e `confirm`, reenvio, fila de saída,
//! reconexão e as métricas `taba.cluster.channel.*`.
//!
//! O canal é emulado sobre o `Transport` (`POST /cluster/channel`): cada lado chama o outro.

mod common;

use std::time::Duration;

use common::cluster::{
    MANY_RETRIES, QUIET, TestCluster, address_of, channels_of, create_queue, eventually,
    eventually_async, host_of, settled, signals_of, total,
};
use common::error_code;
use taba_node::cluster::channel::Channels;
use taba_node::cluster::protocol::{
    ChannelBatch, ChannelFrame, Keepalive, ReadyCount, Request, Response, Signal,
};
use taba_node::cluster::transport::{CallError, Transport};
use taba_node::core::error::ErrorCode;
use taba_node::core::id;

fn guid() -> String {
    id::new_at(1_791_288_000_000)
}

fn ready(topic: &str, has_ready: bool) -> Signal {
    Signal::Ready {
        topic: topic.to_owned(),
        has_ready,
    }
}

fn frame(seq: u64, signal: Signal) -> ChannelFrame {
    ChannelFrame { seq, signal }
}

/// Uma chamada do canal feita pelo nó `from_number` ao nó 1, como se fosse ele.
async fn call_channel(
    cluster: &TestCluster,
    from_number: usize,
    batch: ChannelBatch,
) -> Result<Response, CallError> {
    let transport = cluster.network.transport(host_of(from_number));
    transport
        .call(&address_of(cluster, 1), Request::Channel(batch))
        .await
}

fn confirmed(response: Response) -> Vec<u64> {
    match response {
        Response::Confirm(confirm) => confirm.seqs,
        other => panic!("esperava um confirm, veio {other:?}"),
    }
}

/// UC: UC-CAN-001, UC-CAN-024
#[tokio::test]
async fn every_node_has_one_channel_to_each_other_node_and_measures_it() {
    let cluster = TestCluster::start(3).await;
    settled(&cluster).await;

    for number in 1..=3 {
        let me = cluster.node_id(number);
        let peers = channels_of(&cluster, number).peers();
        let ids: Vec<String> = peers.iter().map(|p| p.node_id.clone()).collect();
        assert_eq!(peers.len(), 2, "N−1 canais no nó {number}");
        assert!(!ids.contains(&me), "nenhum canal consigo mesmo");
        for peer in &peers {
            assert_eq!(peer.version, Some(3), "o sinal de vida traz a versão");
            assert_eq!(peer.state.as_deref(), Some("active"));
        }

        let node = cluster.node(number);
        eventually_async("as métricas do canal", || async {
            let text = node.metrics().await;
            total(&text, "taba_cluster_channel_connections", "") == 2.0
                && total(&text, "taba_cluster_channel_rtt_seconds_count", "") > 0.0
        })
        .await;
        let text = node.metrics().await;
        for id in &ids {
            assert_eq!(
                total(
                    &text,
                    "taba_cluster_channel_state",
                    &format!("taba_peer=\"{id}\"")
                ),
                1.0,
                "o canal com {id} está conectado"
            );
        }
        assert_eq!(
            total(&text, "taba_cluster_channel_queue_out", ""),
            0.0,
            "nada pendente"
        );
    }
    cluster.shutdown().await;
}

/// UC: UC-CAN-003, UC-CAN-012
#[tokio::test]
async fn the_node_that_joins_calls_every_member_and_only_members_are_answered() {
    let cluster = TestCluster::start(3).await;
    settled(&cluster).await;

    // O nó 3 entrou por último e abriu o canal com cada nó que recebeu na resposta.
    let calls = cluster.network.calls();
    for target in [1, 2] {
        assert!(
            calls.iter().any(|c| c.from == host_of(3)
                && c.to.host == host_of(target)
                && c.request == "channel"),
            "o nó 3 chama o nó {target}"
        );
    }

    // Quem não consta na tabela é recusado pela rede em memória...
    let outsider = cluster.network.transport("10.0.0.50");
    let batch = ChannelBatch {
        to: String::new(),
        from: guid(),
        keepalive: None,
        frames: vec![frame(1, ready("x", true))],
    };
    let error = outsider
        .call(&address_of(&cluster, 1), Request::Channel(batch.clone()))
        .await
        .unwrap_err();
    match error {
        CallError::Remote(e) => assert_eq!(e.code, ErrorCode::ClusterNotMember),
        other => panic!("esperava o erro do par, veio {other}"),
    }
    assert!(
        signals_of(&cluster, 1).ready_peers("x").is_empty(),
        "o quadro de quem não é membro não é processado"
    );

    // ...e pela rota HTTP do cluster (o teste chama de 127.0.0.1, que não consta na tabela).
    let node = cluster.node(1);
    let response = node
        .client
        .post(node.cluster_url("/cluster/channel"))
        .json(&batch)
        .send()
        .await
        .unwrap();
    assert_eq!(error_code(response, 403).await, "cluster.not-member");
    cluster.shutdown().await;
}

/// UC: UC-CAN-006, UC-CAN-009, UC-CAN-011
#[tokio::test]
async fn frames_are_confirmed_after_processing_and_a_repeated_seq_is_ignored() {
    let cluster = TestCluster::start_with(2, &[QUIET]).await;
    settled(&cluster).await;
    let node2 = cluster.node_id(2);
    let signals = signals_of(&cluster, 1);

    // Um `seq` bem acima dos que o nó 2 usa de verdade.
    let base = 1_000_000;
    let batch = ChannelBatch {
        to: String::new(),
        from: node2.clone(),
        keepalive: None,
        frames: vec![
            frame(base + 1, ready("x", true)),
            frame(base + 2, ready("y", true)),
        ],
    };
    let confirm = confirmed(call_channel(&cluster, 2, batch).await.unwrap());
    assert_eq!(
        confirm,
        vec![base + 1, base + 2],
        "o confirm agrupa os seqs"
    );
    // O confirm só volta depois de processar.
    assert_eq!(signals.ready_peers("x"), vec![node2.clone()]);
    assert_eq!(signals.ready_peers("y"), vec![node2.clone()]);

    // A confirmação se perdeu e o emissor reenviou o mesmo `seq`, agora com outro conteúdo: o
    // receptor devolve o confirm e não processa de novo.
    let repeated = ChannelBatch {
        to: String::new(),
        from: node2.clone(),
        keepalive: None,
        frames: vec![frame(base + 1, ready("x", false))],
    };
    let confirm = confirmed(call_channel(&cluster, 2, repeated).await.unwrap());
    assert_eq!(confirm, vec![base + 1]);
    assert_eq!(
        signals.ready_peers("x"),
        vec![node2.clone()],
        "o seq já processado foi ignorado"
    );
    let next = ChannelBatch {
        to: String::new(),
        from: node2.clone(),
        keepalive: None,
        frames: vec![frame(base + 3, ready("x", false))],
    };
    assert_eq!(
        confirmed(call_channel(&cluster, 2, next).await.unwrap()),
        vec![base + 3]
    );
    assert!(signals.ready_peers("x").is_empty(), "o seq novo vale");

    // O sinal de vida não tem `seq`: não é confirmado nem fica na fila de ninguém.
    let alive = ChannelBatch {
        to: String::new(),
        from: node2.clone(),
        keepalive: Some(Keepalive {
            at: "2026-10-05T12:00:00.000Z".into(),
            load: 0,
            version: 99,
            state: "active".into(),
            ready: vec![ReadyCount {
                topic: "z".into(),
                count: 3,
            }],
        }),
        frames: vec![],
    };
    assert!(
        confirmed(call_channel(&cluster, 2, alive).await.unwrap()).is_empty(),
        "o sinal de vida não tem confirm"
    );
    let seen = channels_of(&cluster, 1)
        .peers()
        .into_iter()
        .find(|p| p.node_id == node2)
        .unwrap();
    assert_eq!(seen.version, Some(99), "o sinal de vida foi aplicado");
    assert_eq!(signals.ready_peers("z"), vec![node2]);
    for number in 1..=2 {
        assert!(
            channels_of(&cluster, number)
                .peers()
                .iter()
                .all(|p| p.queue == 0),
            "o sinal de vida não fica na fila de saída do nó {number}"
        );
    }
    cluster.shutdown().await;
}

/// Um `GET` de long polling no nó 1 (a fila está vazia): o nó avisa os outros que tem ouvinte.
fn listen(
    cluster: &TestCluster,
    number: usize,
    topic: &str,
    seconds: u32,
) -> tokio::task::JoinHandle<()> {
    let node = cluster.node(number);
    let client = node.client.clone();
    let url = node.client_url(&format!(
        "/topics/{topic}/messages?waitTime={seconds}&maxMessages=1&visibilityTimeout=60"
    ));
    tokio::spawn(async move {
        let _ = client.get(url).send().await;
    })
}

/// UC: UC-CAN-010, UC-FAL-006
#[tokio::test]
async fn a_signal_without_confirm_waits_in_the_queue_and_goes_out_when_the_channel_returns() {
    let cluster = TestCluster::start_with(3, &[MANY_RETRIES]).await;
    create_queue(cluster.node(1), "pedidos").await;
    settled(&cluster).await;
    let node2 = cluster.node_id(2);
    let channels = channels_of(&cluster, 1);
    let pending = |channels: &Channels| {
        channels
            .peers()
            .into_iter()
            .find(|p| p.node_id == node2)
            .unwrap()
    };

    cluster.network.partition(&host_of(1), &host_of(2));
    let waiting = listen(&cluster, 1, "pedidos", 3);
    let channels_for_wait = channels.clone();
    let node2_for_wait = node2.clone();
    eventually("o sinal do ouvinte na fila para o nó 2", move || {
        let peer = channels_for_wait
            .peers()
            .into_iter()
            .find(|p| p.node_id == node2_for_wait)
            .unwrap();
        !peer.up && peer.queue >= 1
    })
    .await;
    // O nó 3 continua alcançável: a fila para ele esvazia sozinha.
    let third = cluster.node_id(3);
    let channels_for_third = channels.clone();
    eventually("o nó 3 confirma o sinal", move || {
        channels_for_third
            .peers()
            .into_iter()
            .find(|p| p.node_id == third)
            .is_some_and(|p| p.up && p.queue == 0)
    })
    .await;

    cluster.network.heal(&host_of(1), &host_of(2));
    let after = channels.clone();
    let node2_after = node2.clone();
    eventually("o canal volta e a fila esvazia", move || {
        after
            .peers()
            .into_iter()
            .find(|p| p.node_id == node2_after)
            .is_some_and(|p| p.up && p.queue == 0)
    })
    .await;
    assert!(pending(&channels).up);
    // Ninguém foi excluído: o canal voltou antes de a votação ter motivo para abrir.
    assert_eq!(cluster.member_ids(1).len(), 3);
    assert_eq!(cluster.network.count("vote"), 0);
    waiting.await.unwrap();
    cluster.shutdown().await;
}

/// UC: UC-CAN-020
#[tokio::test]
async fn a_queue_that_waits_too_long_opens_the_vote_without_waiting_for_the_retries() {
    let cluster = TestCluster::start_with(
        3,
        &[
            MANY_RETRIES,
            ("TABA_CLUSTER_CHANNEL_QUEUE_MAX_AGE", "300ms"),
        ],
    )
    .await;
    create_queue(cluster.node(1), "pedidos").await;
    settled(&cluster).await;
    let version = cluster.table(1).unwrap().version;

    cluster.network.partition(&host_of(1), &host_of(2));
    let waiting = listen(&cluster, 1, "pedidos", 3);

    // O sinal envelhece na fila: a votação abre sem esperar as 1000 tentativas.
    let node = cluster.node(1);
    eventually_async("a fila estourada abre a votação", || async {
        let text = node.metrics().await;
        total(
            &text,
            "taba_cluster_channel_queue_dropped_total",
            "taba_reason=\"overflow\"",
        ) >= 1.0
            && total(&text, "taba_cluster_vote_total", "") >= 1.0
    })
    .await;
    assert!(
        cluster.network.count("vote") >= 1,
        "o nó 1 pediu o voto dos outros"
    );

    // O nó 3 ainda alcança o nó 2 e vota não: ninguém sai da tabela.
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(cluster.table(1).unwrap().version, version);
    assert_eq!(cluster.member_ids(1).len(), 3);

    cluster.network.heal(&host_of(1), &host_of(2));
    waiting.await.unwrap();
    cluster.shutdown().await;
}

/// UC: UC-CAN-021, UC-FAL-003
#[tokio::test]
async fn a_channel_that_returns_before_the_retries_end_changes_nothing() {
    let cluster = TestCluster::start(3).await;
    create_queue(cluster.node(1), "pedidos").await;
    settled(&cluster).await;
    let version = cluster.table(1).unwrap().version;

    // As duas próximas chamadas do canal ao nó 2 se perdem (menos que as 3 tentativas).
    cluster.network.lose_kind(&host_of(2), "channel", 2);
    let waiting = listen(&cluster, 1, "pedidos", 2);

    let nodes = [cluster.node(1), cluster.node(3)];
    eventually_async("a reconexão contada", || async {
        let mut ok = 0.0;
        for node in nodes {
            let text = node.metrics().await;
            ok += total(
                &text,
                "taba_cluster_channel_reconnects_total",
                "taba_outcome=\"ok\"",
            );
        }
        ok >= 1.0
    })
    .await;
    settled(&cluster).await;

    assert_eq!(
        cluster.network.count("vote"),
        0,
        "nenhuma votação foi aberta"
    );
    assert_eq!(cluster.table(1).unwrap().version, version);
    assert_eq!(cluster.member_ids(1).len(), 3);
    waiting.await.unwrap();
    cluster.shutdown().await;
}

/// O envio com confirmação (o que o nó que acabou de ficar pronto usa): volta quando todos os pares
/// confirmaram e desiste no prazo se algum não confirma, deixando o sinal na fila dele.
#[tokio::test]
async fn a_broadcast_with_confirm_returns_when_every_peer_confirmed_and_gives_up_at_the_deadline() {
    let cluster = TestCluster::start_with(3, &[MANY_RETRIES]).await;
    settled(&cluster).await;
    let channels = channels_of(&cluster, 1);
    let state = || Signal::NodeState {
        state: "active".to_owned(),
    };

    let started = std::time::Instant::now();
    channels
        .broadcast_confirmed(state(), Duration::from_secs(5))
        .await;
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "todos confirmaram: não espera o prazo"
    );
    assert!(channels.peers().iter().all(|p| p.queue == 0));

    // O nó 2 não confirma: o envio espera o prazo inteiro e o sinal continua na fila dele.
    cluster.network.partition(&host_of(1), &host_of(2));
    let started = std::time::Instant::now();
    channels
        .broadcast_confirmed(state(), Duration::from_millis(600))
        .await;
    assert!(started.elapsed() >= Duration::from_millis(600));
    let node2 = cluster.node_id(2);
    let waiting = channels
        .peers()
        .into_iter()
        .find(|p| p.node_id == node2)
        .unwrap();
    assert!(waiting.queue >= 1, "o sinal sem confirmação fica na fila");
    cluster.shutdown().await;
}

async fn publish_json(node: &common::TestNode, topic: &str, body: serde_json::Value) {
    let response = node
        .client
        .post(node.client_url(&format!("/topics/{topic}/messages")))
        .json(&body)
        .send()
        .await
        .expect("requisição");
    assert_eq!(response.status(), 201);
}

/// UC: UC-CAN-005
#[tokio::test]
async fn the_keepalive_carries_the_time_the_state_the_version_and_the_busiest_topics_up_to_the_limit()
 {
    let cluster = TestCluster::start(3).await;
    for topic in ["a", "b", "c", "d"] {
        create_queue(cluster.node(1), topic).await;
        for n in 0..12 {
            publish_json(cluster.node(1), topic, serde_json::json!({ "n": n })).await;
        }
    }
    settled(&cluster).await;
    let version = cluster.table(1).unwrap().version;

    // Cada nó é dono de parte das mensagens de cada tópico; um deles tem prontas em três ou mais.
    let (richest, all) = (1..=3)
        .map(|n| (n, signals_of(&cluster, n).keepalive(10)))
        .max_by_key(|(_, keepalive)| keepalive.ready.len())
        .unwrap();
    assert!(all.ready.len() >= 3, "tópicos com prontas: {:?}", all.ready);
    assert_eq!(all.state, "active");
    assert_eq!(all.version, version);
    assert!(all.at.ends_with('Z'), "a data e hora do nó: {}", all.at);

    // Só os tópicos com prontas, os de mais mensagens primeiro, até o limite
    // (`cluster.channel.keepalive.max-topics`).
    let limited = signals_of(&cluster, richest).keepalive(2);
    assert_eq!(limited.ready.len(), 2);
    assert!(limited.ready[0].count >= limited.ready[1].count);
    assert_eq!(limited.ready, all.ready[..2]);
    assert!(all.ready.iter().all(|r| r.count > 0));
    cluster.shutdown().await;
}

/// UC: UC-CAN-008
#[tokio::test]
async fn a_channel_opens_with_a_snapshot_of_the_state_the_listeners_and_the_ready_topics() {
    let cluster = TestCluster::start(3).await;
    create_queue(cluster.node(1), "a").await;
    create_queue(cluster.node(1), "b").await;
    for n in 0..12 {
        publish_json(cluster.node(1), "a", serde_json::json!({ "n": n })).await;
    }
    // Um `GET` com espera no nó 2 é um ouvinte do tópico `b`.
    let waiting = {
        let url = cluster.node(2).client_url("/topics/b/messages?waitTime=20");
        let client = cluster.node(2).client.clone();
        tokio::spawn(async move { client.get(url).send().await })
    };
    let signals = signals_of(&cluster, 2);
    let probe = signals.clone();
    eventually("o ouvinte do nó 2 aparecer", move || {
        probe.local_listener_count("b") == 1
    })
    .await;

    // O instantâneo: o estado primeiro e depois um `listeners` e um `ready` para cada tópico em que
    // o estado é verdadeiro.
    let snapshot = signals.snapshot();
    assert_eq!(
        snapshot[0],
        Signal::NodeState {
            state: "active".into()
        }
    );
    assert!(snapshot.contains(&Signal::Listeners {
        topic: "b".into(),
        has_listeners: true
    }));
    let with_ready = (1..=3)
        .filter(|n| {
            signals_of(&cluster, *n)
                .snapshot()
                .contains(&ready("a", true))
        })
        .count();
    assert!(with_ready >= 1, "algum nó é dono de mensagens de `a`");
    assert!(
        !snapshot.contains(&ready("b", true)),
        "só o que é verdadeiro vai no instantâneo"
    );
    waiting.abort();
    cluster.shutdown().await;
}

/// UC: UC-FAL-001
#[tokio::test]
async fn there_is_no_ping_apart_from_the_channel_between_the_nodes() {
    let cluster = TestCluster::start(3).await;
    settled(&cluster).await;
    cluster.network.clear_calls();
    tokio::time::sleep(Duration::from_secs(1)).await;
    let kinds: std::collections::BTreeSet<&str> = cluster
        .network
        .calls()
        .iter()
        .map(|call| call.request)
        .collect();
    assert!(!kinds.is_empty(), "o canal conversa sozinho");
    assert_eq!(
        kinds,
        std::collections::BTreeSet::from(["channel"]),
        "em repouso só o canal fala: sem ping nem sonda à parte"
    );
    cluster.shutdown().await;
}
