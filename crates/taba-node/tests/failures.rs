//! A falha de um nó no cluster simulado (doc 04, seções 6.3 e 6.8; doc 05, seção 7.3; doc 06c;
//! doc 20c, nível **C**): detecção, votação de exclusão, desregistro por falha, `node-failed`,
//! partição e a concorrência entre mudanças de membros.
//!
//! "Cair" é isolar o nó da rede em memória (`isolate`): para os outros ele deixa de responder, e ele
//! próprio deixa de alcançar os outros.

mod common;

use std::time::Duration;

use serde_json::{Value, json};

use common::TestNode;
use common::cluster::{
    MANY_RETRIES, NO_VOTE, TestCluster, address_of, channels_of, create_queue, eventually,
    eventually_async, host_of, name_owned_by, pause_sync, settled, start_isolated, total,
    wait_joined, wait_phase,
};
use taba_node::cluster::members::MemberTable;
use taba_node::cluster::protocol::{Ballot, Request, Response, VoteReason, VoteRequest};
use taba_node::cluster::state::NodeState;
use taba_node::cluster::transport::{PeerAddr, Transport};
use taba_node::core::clock::Clock;
use taba_node::core::id;
use taba_node::core::ring::message_position;

const OPENED_EXCLUDED: &str = "taba_outcome=\"excluded\",taba_role=\"opened\"";

fn guid() -> String {
    id::new_at(1_791_288_000_000)
}

/// As tabelas dos nós `numbers` são iguais, sem o nó `gone`, e têm `size` membros.
fn agree_without(cluster: &TestCluster, numbers: &[usize], gone: usize, size: usize) -> bool {
    let tables: Vec<Option<MemberTable>> = numbers.iter().map(|n| cluster.table(*n)).collect();
    let gone = cluster.node_id(gone);
    tables.iter().all(|t| {
        t.as_ref()
            .is_some_and(|t| t.members.len() == size && t.member(&gone).is_none())
    }) && tables.windows(2).all(|w| w[0] == w[1])
}

/// A soma da série `name` com o rótulo `label` em todos os nós `numbers`.
async fn sum_of(cluster: &TestCluster, numbers: &[usize], name: &str, label: &str) -> f64 {
    let mut sum = 0.0;
    for number in numbers {
        sum += total(&cluster.node(*number).metrics().await, name, label);
    }
    sum
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

fn messages_of(node: &TestNode) -> std::sync::Arc<taba_node::engine::messages::Messages> {
    node.node
        .cluster()
        .and_then(|c| c.replication())
        .map(|r| r.messages.messages().clone())
        .expect("o nó está no cluster")
}

/// Uma fila com algumas mensagens, publicadas por nós diferentes.
async fn queue_with_messages(cluster: &TestCluster, count: usize) -> Vec<String> {
    create_queue(cluster.node(1), "pedidos").await;
    let mut ids = Vec::new();
    for n in 0..count {
        let via = 1 + n % cluster.nodes.len();
        ids.push(publish(cluster.node(via), "pedidos", json!({"n": n})).await);
    }
    ids
}

/// UC: UC-FAL-004, UC-FAL-010, UC-FAL-014, UC-FAL-015, UC-FAL-016, UC-FAL-019, UC-FAL-020
#[tokio::test]
async fn a_node_that_stays_down_is_excluded_by_two_of_three_and_every_table_changes() {
    let cluster = TestCluster::start(3).await;
    settled(&cluster).await;
    let before = cluster.table(1).unwrap().version;

    cluster.network.isolate(&host_of(3));

    eventually("os dois saudáveis excluírem o nó 3", || {
        agree_without(&cluster, &[1, 2], 3, 2)
    })
    .await;
    assert_eq!(
        cluster.table(1).unwrap().version,
        before + 1,
        "uma única mudança de membros"
    );

    // O desregistro seguiu as duas fases, com os dois nós que sobram.
    assert!(cluster.network.count("vote") >= 1, "pediu o voto");
    assert!(cluster.network.count("unregister") >= 1, "fase 1");
    assert!(cluster.network.count("commit") >= 1, "fase 2");

    // A votação e o voto foram contados, e a mudança de membros também (por fase e motivo).
    eventually_async("a votação e o resultado nas métricas", || async {
        sum_of(
            &cluster,
            &[1, 2],
            "taba_cluster_vote_total",
            OPENED_EXCLUDED,
        )
        .await
            >= 1.0
            && sum_of(
                &cluster,
                &[1, 2],
                "taba_cluster_vote_total",
                "taba_role=\"cast\"",
            )
            .await
                >= 1.0
    })
    .await;
    for phase in ["register", "commit"] {
        let label = format!(
            "taba_operation=\"unregister\",taba_outcome=\"ok\",taba_phase=\"{phase}\",taba_reason=\"vote\""
        );
        eventually_async("a fase do desregistro por votação contada", || async {
            sum_of(
                &cluster,
                &[1, 2],
                "taba_cluster_membership_changes_total",
                &label,
            )
            .await
                >= 2.0
        })
        .await;
    }

    // O canal do excluído some dos dois que ficaram, e a fila de sinais para ele com ele.
    for number in [1, 2] {
        let channels = channels_of(&cluster, number);
        eventually("o canal do excluído desaparece", move || {
            channels.peers().len() == 1
        })
        .await;
    }

    // O nó isolado enxerga só a si mesmo como saudável: menos que `min-voters`, não exclui ninguém.
    let lonely = cluster.node(3);
    eventually_async("o nó isolado sem votantes suficientes", || async {
        total(
            &lonely.metrics().await,
            "taba_cluster_vote_total",
            "taba_outcome=\"quorum-lost\"",
        ) >= 1.0
    })
    .await;
    assert_eq!(cluster.member_ids(3).len(), 3, "o nó isolado não exclui");
    cluster.shutdown().await;
}

/// UC: UC-FAL-010
#[tokio::test]
async fn with_only_two_nodes_a_cut_excludes_nobody() {
    let cluster = TestCluster::start(2).await;
    settled(&cluster).await;
    let version = cluster.table(1).unwrap().version;

    cluster.network.isolate(&host_of(2));
    eventually_async("a votação sem votantes suficientes", || async {
        sum_of(
            &cluster,
            &[1, 2],
            "taba_cluster_vote_total",
            "taba_outcome=\"quorum-lost\"",
        )
        .await
            >= 2.0
    })
    .await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    for number in [1, 2] {
        assert_eq!(cluster.table(number).unwrap().version, version);
        assert_eq!(cluster.member_ids(number).len(), 2);
    }
    assert_eq!(cluster.network.count("unregister"), 0);
    cluster.shutdown().await;
}

/// UC: UC-FAL-011
#[tokio::test]
async fn vote_remove_answers_no_to_who_still_reaches_the_candidate_and_yes_to_who_does_not() {
    let cluster = TestCluster::start_with(3, &[MANY_RETRIES]).await;
    settled(&cluster).await;
    let (second, third) = (cluster.node_id(2), cluster.node_id(3));
    let node1 = address_of(&cluster, 1);
    let ask = |candidate: String| {
        let transport = cluster.network.transport(host_of(2));
        let node1 = node1.clone();
        let opened_by = second.clone();
        async move {
            let request = Request::Vote(VoteRequest {
                vote_id: guid(),
                candidate_node_id: candidate,
                opened_by,
                reason: VoteReason::ChannelDown,
            });
            match transport.call(&node1, request).await {
                Ok(Response::Vote(answer)) => answer.vote,
                other => panic!("esperava um voto, veio {other:?}"),
            }
        }
    };

    // O nó 1 alcança o nó 3: vota não. Quem não é membro e o próprio votante também são não.
    assert_eq!(ask(third.clone()).await, Ballot::No);
    assert_eq!(ask(guid()).await, Ballot::No);
    assert_eq!(ask(cluster.node_id(1)).await, Ballot::No, "estou vivo");

    // Cortado o nó 1 do nó 3, ele não o alcança mais e vota sim.
    cluster.network.partition(&host_of(1), &host_of(3));
    let channels = channels_of(&cluster, 1);
    eventually("o canal do nó 1 com o nó 3 cair", move || {
        channels.peers().iter().any(|p| !p.up)
    })
    .await;
    assert_eq!(ask(third.clone()).await, Ballot::Yes);

    // Os votos foram contados, e dar o voto não exclui ninguém.
    let text = cluster.node(1).metrics().await;
    assert!(total(&text, "taba_cluster_vote_total", "taba_outcome=\"yes\"") >= 1.0);
    assert!(total(&text, "taba_cluster_vote_total", "taba_outcome=\"no\"") >= 3.0);
    assert_eq!(cluster.member_ids(1).len(), 3);
    cluster.shutdown().await;
}

/// UC: UC-FAL-005, UC-FAL-023
#[tokio::test]
async fn a_node_that_announces_its_failure_is_excluded_while_its_channel_is_still_up() {
    let cluster = TestCluster::start(3).await;
    settled(&cluster).await;

    cluster.node(3).node.shared().set_failed();

    eventually("os dois saudáveis excluírem o nó que falhou", || {
        agree_without(&cluster, &[1, 2], 3, 2)
    })
    .await;
    // O canal nunca caiu: foi o aviso do nó, não a conexão, que abriu a votação.
    assert_eq!(
        sum_of(
            &cluster,
            &[1, 2],
            "taba_cluster_channel_reconnects_total",
            ""
        )
        .await,
        0.0
    );
    assert!(
        sum_of(
            &cluster,
            &[1, 2],
            "taba_cluster_vote_total",
            OPENED_EXCLUDED
        )
        .await
            >= 1.0
    );
    // O nó segue em `failed`, fora do rodízio, e não tenta voltar sozinho.
    let ready = cluster.node(3).client_get_ops("/health/ready").await;
    assert_eq!(ready.status(), 503);
    assert_eq!(ready.json::<Value>().await.unwrap()["reason"], "failed");
    cluster.shutdown().await;
}

/// UC: UC-FAL-005
#[tokio::test]
async fn a_queue_that_overflows_opens_the_vote_that_excludes_the_unreachable_node() {
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

    cluster.network.isolate(&host_of(3));
    // Um ouvinte no nó 1 gera um sinal para o nó 3 que não sai: a fila envelhece e estoura.
    let node = cluster.node(1);
    let client = node.client.clone();
    let url =
        node.client_url("/topics/pedidos/messages?waitTime=3&maxMessages=1&visibilityTimeout=60");
    let waiting = tokio::spawn(async move {
        let _ = client.get(url).send().await;
    });

    eventually("o nó 3 excluído depois do estouro da fila", || {
        agree_without(&cluster, &[1, 2], 3, 2)
    })
    .await;
    assert!(
        sum_of(
            &cluster,
            &[1],
            "taba_cluster_channel_queue_dropped_total",
            "taba_reason=\"overflow\""
        )
        .await
            >= 1.0
    );
    waiting.await.unwrap();
    cluster.shutdown().await;
}

/// UC: UC-FAL-002
#[tokio::test]
async fn a_node_that_stops_answering_with_the_connection_open_is_detected_and_excluded() {
    let cluster = TestCluster::start(3).await;
    settled(&cluster).await;

    // O nó 3 recebe as chamadas e não responde: a conexão está aberta, mas nada volta.
    cluster.network.set_silent(&host_of(3), true);
    eventually("o nó mudo ser excluído pela falta de resposta", || {
        agree_without(&cluster, &[1, 2], 3, 2)
    })
    .await;
    cluster.network.set_silent(&host_of(3), false);
    cluster.shutdown().await;
}

/// UC: UC-FAL-017
#[tokio::test]
async fn an_exclusion_that_fails_before_the_commit_rolls_back_and_the_vote_is_held_again() {
    let cluster = TestCluster::start(3).await;
    settled(&cluster).await;

    // A fase 1 do desregistro se perde nas três tentativas de cada coordenador possível.
    for host in [host_of(1), host_of(2)] {
        cluster.network.lose_kind(&host, "unregister", 3);
    }
    cluster.network.isolate(&host_of(3));

    eventually("a exclusão depois do rollback", || {
        agree_without(&cluster, &[1, 2], 3, 2)
    })
    .await;
    assert!(
        sum_of(
            &cluster,
            &[1, 2],
            "taba_cluster_vote_total",
            "taba_outcome=\"failed\",taba_role=\"opened\""
        )
        .await
            >= 1.0,
        "a primeira votação não efetivou a exclusão"
    );
    assert!(
        sum_of(
            &cluster,
            &[1, 2],
            "taba_cluster_membership_changes_total",
            "taba_operation=\"unregister\",taba_outcome=\"ok\",taba_phase=\"rollback\""
        )
        .await
            >= 1.0,
        "o rollback foi contado"
    );
    cluster.shutdown().await;
}

/// UC: UC-FAL-017
#[tokio::test]
async fn once_the_exclusion_is_decided_the_commit_is_insisted_until_every_node_confirms() {
    let cluster = TestCluster::start(3).await;
    settled(&cluster).await;
    for host in [host_of(1), host_of(2)] {
        cluster.network.lose_kind(&host, "commit", 3);
    }
    cluster.network.isolate(&host_of(3));

    eventually("a exclusão apesar dos commits perdidos", || {
        agree_without(&cluster, &[1, 2], 3, 2)
    })
    .await;
    // O coordenador insiste no commit; o quadro `membership` do canal também leva a mudança, e quem
    // chega primeiro acerta o nó (D126), então não se sabe quantos commits ainda foram precisos.
    assert!(
        cluster.network.count("commit") >= 1,
        "o coordenador tentou o commit"
    );
    assert_eq!(
        sum_of(
            &cluster,
            &[1, 2],
            "taba_cluster_vote_total",
            "taba_outcome=\"failed\""
        )
        .await,
        0.0,
        "decidido, não há volta"
    );
    cluster.shutdown().await;
}

/// UC: UC-FAL-008, UC-FAL-009, UC-FAL-012
#[tokio::test]
async fn when_several_nodes_fall_together_the_vote_goes_on_among_the_healthy_ones_one_at_a_time() {
    let cluster = TestCluster::start(5).await;
    settled(&cluster).await;
    let before = cluster.table(1).unwrap().version;

    // Uma zona inteira cai: três dos cinco. Só dois saudáveis restam, e os três não entram na base
    // (senão 2 de 5 não fechariam o quórum).
    for number in [3, 4, 5] {
        cluster.network.isolate(&host_of(number));
    }
    eventually("os três nós fora da tabela dos dois que restam", || {
        [1, 2].iter().all(|n| {
            cluster
                .table(*n)
                .is_some_and(|t| t.members.len() == 2 && t.version == before + 3)
        }) && cluster.table(1) == cluster.table(2)
    })
    .await;
    // Cada nó isolado enxerga só a si mesmo e não exclui ninguém.
    for number in [3, 4, 5] {
        assert_eq!(cluster.member_ids(number).len(), 5);
    }
    cluster.shutdown().await;
}

/// Nenhum UC: cada lado da partição exclui o outro (UC-FAL-013, primeira metade). A volta do
/// excluído é provada na 15b3 e a dos dois lados é o PRB-040.
#[tokio::test]
async fn a_partition_with_enough_healthy_nodes_on_both_sides_makes_each_side_exclude_the_other() {
    let cluster = TestCluster::start(4).await;
    settled(&cluster).await;

    for left in [1, 2] {
        for right in [3, 4] {
            cluster.network.partition(&host_of(left), &host_of(right));
        }
    }
    eventually("cada lado ficar só com os seus dois nós", || {
        let (a, b) = (cluster.member_ids(1), cluster.member_ids(3));
        a.len() == 2
            && b.len() == 2
            && cluster.table(1) == cluster.table(2)
            && cluster.table(3) == cluster.table(4)
            && a.iter().all(|id| !b.contains(id))
    })
    .await;
    cluster.shutdown().await;
}

/// UC: UC-FAL-021, UC-FAL-022
#[tokio::test]
async fn the_messages_of_the_excluded_node_wait_and_are_delivered_by_the_new_primary() {
    // Sem votação automática: o teste abre a votação quando quer.
    let cluster = TestCluster::start_with(4, NO_VOTE).await;
    let ids = queue_with_messages(&cluster, 40).await;
    let victim = cluster.node_id(4);
    let ring = cluster.node(1).node.cluster().unwrap().membership().ring();
    let owned_by_victim: Vec<String> = ids
        .iter()
        .filter(|id| ring.primary(message_position(id)) == Some(victim.as_str()))
        .cloned()
        .collect();
    assert!(!owned_by_victim.is_empty(), "o nó 4 é dono de alguma");
    let others: Vec<String> = ids
        .iter()
        .filter(|id| !owned_by_victim.contains(id))
        .cloned()
        .collect();

    // O nó 4 cai: as mensagens dele ficam sem entrega, e as dos outros seguem saindo.
    cluster.network.isolate(&host_of(4));
    let channels = channels_of(&cluster, 1);
    let victim_for_wait = victim.clone();
    eventually("o canal com o nó 4 cair", move || {
        channels
            .peers()
            .iter()
            .any(|p| p.node_id == victim_for_wait && !p.up)
    })
    .await;
    // Prazo de 10 s, como o `eventually`: runners lentos (arm64 no CI) levam mais que 5 s.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    let mut delivered: Vec<String> = Vec::new();
    while tokio::time::Instant::now() < deadline {
        for message in consume(
            cluster.node(1),
            "pedidos",
            "maxMessages=10&visibilityTimeout=300",
        )
        .await
        {
            delivered.push(message["id"].as_str().unwrap().to_owned());
        }
        if others.iter().all(|id| delivered.contains(id)) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert!(
        others.iter().all(|id| delivered.contains(id)),
        "as mensagens dos outros nós saem"
    );
    assert!(
        owned_by_victim.iter().all(|id| !delivered.contains(id)),
        "as do nó caído esperam o desregistro"
    );

    // O desregistro: o anel é recalculado e as mensagens passam a um novo primário.
    let voting = cluster
        .node(1)
        .node
        .cluster()
        .and_then(|c| c.replication())
        .unwrap()
        .voting
        .clone();
    voting.open(victim.clone(), VoteReason::ChannelDown).await;
    eventually("o nó 4 fora da tabela", || {
        agree_without(&cluster, &[1, 2, 3], 4, 3)
    })
    .await;
    let ring = cluster.node(1).node.cluster().unwrap().membership().ring();
    assert!(ring.nodes().all(|n| n != victim));
    assert_eq!(
        (1..=3)
            .map(|n| ring.primary_blocks_of(&cluster.node_id(n)))
            .sum::<usize>(),
        ring.block_count(),
        "os blocos do nó removido foram para os que ficaram"
    );

    // Cada mensagem segue em pelo menos `replication.factor − 1` = 2 nós.
    for id in &ids {
        let copies = (1..=3)
            .filter(|n| {
                messages_of(cluster.node(*n))
                    .message_info("pedidos", id)
                    .unwrap()
                    .is_some()
            })
            .count();
        assert!(copies >= 2, "a mensagem {id} tem {copies} cópia(s)");
    }

    // As que esperavam agora saem, entregues pelo novo primário.
    for _ in 0..200 {
        for message in consume(
            cluster.node(1),
            "pedidos",
            "maxMessages=10&visibilityTimeout=300",
        )
        .await
        {
            delivered.push(message["id"].as_str().unwrap().to_owned());
        }
        if owned_by_victim.iter().all(|id| delivered.contains(id)) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert!(
        owned_by_victim.iter().all(|id| delivered.contains(id)),
        "o novo primário entrega as mensagens do nó removido"
    );
    cluster.shutdown().await;
}

/// UC: UC-CNC-002, UC-CNC-007, UC-FRZ-005, UC-CAN-014
#[tokio::test]
async fn a_node_that_restarts_with_the_same_address_waits_for_the_vote_and_joins_as_a_new_node() {
    let mut cluster = TestCluster::start_with(3, &[("TABA_DRAIN_ON_STOP", "false")]).await;
    settled(&cluster).await;
    let old_id = cluster.node_id(3);
    let port = cluster.node(3).node.cluster_addr().unwrap().port();
    cluster.network.clear_calls();

    // O desregistro demora: a votação fica em andamento tempo suficiente para o nó novo chegar.
    cluster
        .network
        .set_delay_kind("unregister", Duration::from_millis(1500));
    cluster.nodes.pop().unwrap().shutdown().await;
    cluster.network.unregister(&PeerAddr::new(host_of(3), port));
    eventually("a votação de exclusão abrir", || {
        [1, 2].iter().any(|n| {
            cluster
                .node(*n)
                .node
                .cluster()
                .is_some_and(|c| !c.open_votes().is_empty())
        })
    })
    .await;

    // Com a votação aberta, o nó reinicia no mesmo IP e porta e pede para entrar.
    let builder = cluster
        .builder(3, false)
        .env("TABA_CLUSTER_PORT", &port.to_string());
    let number = cluster.spawn_node(builder).await.expect("o nó reinicia");
    let new_id = cluster.node_id(number);
    assert_ne!(new_id, old_id, "o ID é novo");

    // Em nenhum instante a tabela tem o nó antigo e o novo juntos.
    let mut both = false;
    let mut joined = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    while !joined {
        assert!(
            tokio::time::Instant::now() < deadline,
            "o nó novo não entrou"
        );
        for observer in [1, 2] {
            if let Some(table) = cluster.table(observer) {
                both |= table.member(&old_id).is_some() && table.member(&new_id).is_some();
                joined |= table.member(&new_id).is_some() && table.member(&old_id).is_none();
            }
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert!(!both, "o pedido do nó novo esperou o fim da votação");
    // O reinício é outro ID: só o antigo, excluído, fica em freezing, e o novo entrou sem esperar.
    let membership = cluster.node(1).node.cluster().unwrap().membership();
    let freezing = Duration::from_secs(300);
    let now = cluster.clock.now();
    assert!(
        membership
            .frozen_remaining(&old_id, now, freezing)
            .is_some()
    );
    assert!(
        membership
            .frozen_remaining(&new_id, now, freezing)
            .is_none()
    );
    // O balanceador também repete o pedido ao próprio nó novo (mesmo endereço): só vale o ingresso
    // que chegou a um membro, e ele chegou depois da votação aberta e antes do commit da exclusão.
    let calls = cluster.network.calls();
    let first = |name: &str| calls.iter().position(|c| c.request == name);
    let join = calls
        .iter()
        .position(|c| c.request == "join" && c.to.host != host_of(3));
    assert!(
        first("vote") < join && join < first("commit"),
        "o pedido de ingresso chegou no meio da votação"
    );
    assert_eq!(cluster.member_ids(1).len(), 3);
    wait_joined(cluster.node(number)).await;
    cluster.shutdown().await;
}

/// UC: UC-CNC-005
#[tokio::test]
async fn a_node_that_falls_while_another_syncs_is_excluded_only_when_the_sync_ends() {
    let mut cluster = TestCluster::start(2).await;
    queue_with_messages(&cluster, 6).await;
    let third = start_isolated(&mut cluster, &[("TABA_CLUSTER_SYNC_TIMEOUT", "300ms")]).await;
    let gate = pause_sync(&cluster, third, "replay");
    wait_phase(cluster.node(third), "replay").await;

    // O nó 2 cai com o 3 sincronizando: a votação fecha, mas o desregistro espera.
    cluster.network.isolate(&host_of(2));
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(cluster.member_ids(1).len(), 3, "nada mudou durante a carga");
    assert_eq!(
        cluster.node(third).node.shared().state(),
        NodeState::Syncing
    );

    gate.add_permits(1);
    eventually("o nó 2 sair depois de o 3 ficar pronto", || {
        cluster
            .table(1)
            .is_some_and(|t| t.members.len() == 2 && t.member(&cluster.node_id(2)).is_none())
    })
    .await;
    wait_joined(cluster.node(third)).await;
    cluster.shutdown().await;
}

/// UC: UC-SIN-020, UC-CNC-004, UC-FAL-023
#[tokio::test]
async fn a_syncing_node_that_fails_is_excluded_by_the_others() {
    let mut cluster = TestCluster::start(2).await;
    queue_with_messages(&cluster, 6).await;
    let number = start_isolated(&mut cluster, &[("TABA_CLUSTER_SYNC_TIMEOUT", "300ms")]).await;
    // Ninguém serve a listagem de IDs: o nó esgota o prazo e entra em falha.
    for host in [host_of(1), host_of(2)] {
        cluster.network.lose_kind(&host, "block-ids", 1_000_000);
    }
    cluster.network.restore(&host_of(number));

    let shared = cluster.node(number).node.shared().clone();
    eventually("o nó entrar em falha", move || {
        shared.state() == NodeState::Failed
    })
    .await;
    // O sinal de falha libera a votação, e a saída de um nó em sincronização é a exceção da regra
    // de uma mudança por vez.
    eventually("os outros excluírem o nó que falhou", || {
        agree_without(&cluster, &[1, 2], number, 2)
    })
    .await;
    cluster.shutdown().await;
}

/// UC: UC-CAN-025, UC-CAN-014, UC-CNC-007
#[tokio::test]
async fn a_node_that_restarts_before_the_fall_is_detected_answers_wrong_node_and_is_replaced() {
    // 40 tentativas de 50 ms: o nó reinicia muito antes de a queda ser declarada.
    let mut cluster = TestCluster::start_with(
        3,
        &[
            ("TABA_DRAIN_ON_STOP", "false"),
            ("TABA_CLUSTER_CHANNEL_RECONNECT_RETRIES", "40"),
        ],
    )
    .await;
    settled(&cluster).await;
    let old_id = cluster.node_id(3);
    let port = cluster.node(3).node.cluster_addr().unwrap().port();

    cluster.nodes.pop().unwrap().shutdown().await;
    cluster.network.unregister(&PeerAddr::new(host_of(3), port));
    let builder = cluster
        .builder(3, false)
        .env("TABA_CLUSTER_PORT", &port.to_string());
    let number = cluster.spawn_node(builder).await.expect("o nó reinicia");
    let new_id = cluster.node_id(number);
    assert_ne!(new_id, old_id);

    // Antes de a votação abrir, o processo novo já não passa pelo antigo: o canal dos outros lados
    // recebe `cluster.wrong-node` e conta como caído.
    eventually("a votação do nó antigo", || {
        [1, 2].iter().any(|n| {
            cluster
                .node(*n)
                .node
                .cluster()
                .is_some_and(|c| !c.open_votes().is_empty())
                || cluster.member_ids(*n).contains(&new_id)
        })
    })
    .await;
    eventually("o nó novo substituir o antigo na tabela", || {
        [1, 2].iter().all(|n| {
            let ids = cluster.member_ids(*n);
            ids.contains(&new_id) && !ids.contains(&old_id)
        })
    })
    .await;
    wait_joined(cluster.node(number)).await;
    assert_eq!(cluster.member_ids(1).len(), 3);
    cluster.shutdown().await;
}

/// UC: UC-FAL-026, UC-FAL-031
#[tokio::test]
async fn after_a_partition_where_each_side_excluded_the_other_the_losing_side_joins_again() {
    let cluster = TestCluster::start(4).await;
    settled(&cluster).await;
    for left in [1, 2] {
        for right in [3, 4] {
            cluster.network.partition(&host_of(left), &host_of(right));
        }
    }
    eventually("cada lado ficar só com os seus dois nós", || {
        cluster.member_ids(1).len() == 2
            && cluster.member_ids(3).len() == 2
            && cluster.table(1) == cluster.table(2)
            && cluster.table(3) == cluster.table(4)
    })
    .await;

    // A rede volta, mas as duas tabelas têm a mesma versão e nenhum canal liga um lado ao outro.
    // O freezing de cada lado já passou.
    for left in [1, 2] {
        for right in [3, 4] {
            cluster.network.heal(&host_of(left), &host_of(right));
        }
    }
    cluster.network.clear_calls();
    cluster.clock.advance(Duration::from_secs(301));

    // Cada lado consulta a tabela de quem excluiu: o perdedor (o do menor `nodeId`, num empate)
    // refaz o ingresso e as quatro tabelas voltam a ser uma só.
    eventually("os quatro nós numa tabela só", || {
        let tables: Vec<_> = (1..=4).map(|n| cluster.table(n)).collect();
        tables.windows(2).all(|w| w[0] == w[1])
            && tables[0].as_ref().is_some_and(|t| t.members.len() == 4)
    })
    .await;
    for number in 1..=4 {
        wait_joined(cluster.node(number)).await;
    }

    // Só o lado perdedor refez o ingresso: o que tem o menor `nodeId` de todos (empate de 2 a 2).
    let smallest = (1..=4).min_by_key(|n| cluster.node_id(*n)).unwrap();
    let losers: Vec<String> = if smallest <= 2 {
        vec![host_of(1), host_of(2)]
    } else {
        vec![host_of(3), host_of(4)]
    };
    let joiners: std::collections::BTreeSet<String> = cluster
        .network
        .calls()
        .iter()
        .filter(|call| call.request == "join" && call.from != "balancer")
        .map(|call| call.from.clone())
        .collect();
    assert!(!joiners.is_empty(), "o perdedor refez o ingresso");
    assert!(
        joiners.iter().all(|host| losers.contains(host)),
        "o vencedor não refez: {joiners:?}"
    );
    cluster.shutdown().await;
}

/// UC: UC-FAL-027
#[tokio::test]
async fn a_failed_node_forwards_the_client_calls_to_a_healthy_one_until_it_leaves() {
    let cluster = TestCluster::start(3).await;
    settled(&cluster).await;
    // A exclusão demora: o nó em falha segue na tabela e recebe chamadas por um instante.
    cluster
        .network
        .set_delay_kind("unregister", Duration::from_millis(1500));
    // O dono da definição não pode ser o nó em falha (ele só deixa de ser dono quando é excluído).
    let name = name_owned_by(&cluster, 1, "pedidos-");
    cluster.node(3).node.shared().set_failed();

    let created = cluster
        .node(3)
        .client
        .post(cluster.node(3).client_url("/topics"))
        .json(&json!({ "name": name, "type": "queue" }))
        .send()
        .await
        .unwrap();
    assert_eq!(created.status(), 201, "a chamada foi repassada a um nó são");
    assert_eq!(
        total(
            &cluster.node(3).metrics().await,
            "taba_cluster_forwarded_total",
            "taba_outcome=\"ok\""
        ),
        1.0,
        "o nó em falha repassou, não atendeu"
    );
    // O tópico foi criado no cluster, mas o nó em falha não o atende por conta própria.
    let listed = cluster
        .node(1)
        .client
        .get(cluster.node(1).client_url(&format!("/topics/{name}")))
        .send()
        .await
        .unwrap();
    assert_eq!(listed.status(), 200);
    cluster.network.set_delay_kind("unregister", Duration::ZERO);
    cluster.shutdown().await;
}

/// UC: UC-FAL-027
#[tokio::test]
async fn a_failed_single_node_refuses_the_client_calls_with_503() {
    let node = TestNode::development().await;
    node.node.shared().set_failed();
    let response = node.client_get("/topics").await;
    assert_eq!(response.status(), 503);
    assert!(response.headers().contains_key("retry-after"));
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["code"], "node.failed");
    // As rotas de operação continuam: é por elas que se vê a falha.
    let ready = node.client_get_ops("/health/ready").await;
    assert_eq!(ready.status(), 503);
    assert_eq!(ready.json::<Value>().await.unwrap()["reason"], "failed");
    node.shutdown().await;
}

/// UC: UC-CAN-022
#[tokio::test]
async fn once_the_exclusion_is_decided_the_nodes_drop_the_channel_and_the_return_starts_with_empty_queues()
 {
    let cluster = TestCluster::start(3).await;
    settled(&cluster).await;
    let third = cluster.node_id(3);
    cluster.network.isolate(&host_of(3));
    eventually("os dois saudáveis excluírem o nó 3", || {
        agree_without(&cluster, &[1, 2], 3, 2)
    })
    .await;

    // O canal e as filas para o excluído foram descartados: não acumulam mais.
    for number in [1, 2] {
        let peers = channels_of(&cluster, number).peers();
        assert!(
            peers.iter().all(|p| p.node_id != third),
            "nó {number}: o canal com o excluído foi descartado"
        );
    }
    let label = format!("taba_peer=\"{third}\"");
    assert_eq!(
        sum_of(&cluster, &[1, 2], "taba_cluster_channel_queue_out", &label).await,
        0.0
    );

    // Se o nó volta, refaz o bootstrap e os canais nascem novos e vazios.
    cluster.network.restore(&host_of(3));
    cluster.clock.advance(Duration::from_secs(301));
    wait_joined(cluster.node(3)).await;
    eventually("os canais novos, de pé e vazios", || {
        [1, 2].iter().all(|n| {
            channels_of(&cluster, *n)
                .peers()
                .iter()
                .any(|p| p.node_id == third && p.up && p.queue == 0)
        })
    })
    .await;
    cluster.shutdown().await;
}

/// UC: UC-FAL-029
#[tokio::test]
async fn only_the_lowest_node_id_among_the_healthy_coordinates_the_exclusion() {
    let cluster = TestCluster::start(5).await;
    settled(&cluster).await;
    let lowest = (1..=4)
        .min_by_key(|n| cluster.node_id(*n))
        .expect("quatro saudáveis");
    cluster.network.clear_calls();

    cluster.network.isolate(&host_of(5));
    eventually("os quatro saudáveis excluírem o nó 5", || {
        agree_without(&cluster, &[1, 2, 3, 4], 5, 4)
    })
    .await;

    // Todos abriram a votação, mas só o de menor ID registrou o desregistro nos outros: os demais
    // esperaram a vez e acharam o nó já fora da tabela.
    let coordinators: std::collections::BTreeSet<String> = cluster
        .network
        .calls()
        .iter()
        .filter(|call| call.request == "unregister")
        .map(|call| call.from.clone())
        .collect();
    assert_eq!(
        coordinators,
        std::collections::BTreeSet::from([host_of(lowest)]),
        "o coordenador da exclusão"
    );
    assert!(
        cluster.network.count("vote") >= 4,
        "todos os saudáveis votaram"
    );
    cluster.shutdown().await;
}
