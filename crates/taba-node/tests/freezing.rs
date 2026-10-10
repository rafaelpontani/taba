//! O nó excluído que volta e o período de freezing (doc 04, seções 6.6 e 6.7; doc 20c, nível **C**):
//! o mesmo processo, com o mesmo ID, é recusado com `403 cluster.node-frozen` até o freezing acabar
//! e então refaz o ingresso do zero. O `Clock` é manual: o teste anda o relógio em vez de esperar.

mod common;

use std::time::Duration;

use common::cluster::{
    TestCluster, address_of, eventually, eventually_async, host_of, settled, signals_of, total,
    wait_joined,
};
use taba_node::cluster::protocol::{JoinRequest, Request, Signal};
use taba_node::cluster::state::NodeState;
use taba_node::cluster::transport::{CallError, Transport};
use taba_node::core::clock::Clock;
use taba_node::core::error::ErrorCode;

/// Os 5 minutos de `cluster.freezing.duration`.
const FREEZING: Duration = Duration::from_secs(300);

/// Um pedido de ingresso do nó `number`, como ele o faria (mesmo ID, mesmo endereço).
fn join_as(cluster: &TestCluster, number: usize) -> Request {
    let local = cluster
        .node(number)
        .node
        .cluster()
        .expect("o nó está no cluster")
        .local();
    Request::Join(JoinRequest {
        node_id: local.node_id,
        addresses: local.addresses,
        port: local.port,
        version: env!("CARGO_PKG_VERSION").to_owned(),
    })
}

/// O que o cluster responde a um pedido de ingresso do nó `number` feito ao nó `to`.
async fn join_error(cluster: &TestCluster, number: usize, to: usize) -> Option<(ErrorCode, u64)> {
    let transport = cluster.network.transport(host_of(number));
    match transport
        .call(&address_of(cluster, to), join_as(cluster, number))
        .await
    {
        Err(CallError::Remote(error)) => Some((error.code, error.retry_after.unwrap_or(0))),
        _ => None,
    }
}

/// Os nós `numbers` guardam o nó `gone` em freezing e têm a tabela sem ele.
fn frozen_and_gone(cluster: &TestCluster, numbers: &[usize], gone: usize) -> bool {
    let id = cluster.node_id(gone);
    numbers.iter().all(|n| {
        let service = cluster.node(*n).node.cluster().unwrap();
        !service.membership().is_member(&id)
            && service
                .membership()
                .frozen_remaining(&id, cluster.clock.now(), FREEZING)
                .is_some()
    })
}

/// UC: UC-FAL-013, UC-FRZ-001, UC-FRZ-002, UC-FRZ-003, UC-FRZ-004
#[tokio::test]
async fn an_excluded_node_that_stays_the_same_process_is_refused_until_the_freezing_ends() {
    let cluster = TestCluster::start(3).await;
    settled(&cluster).await;
    let id = cluster.node_id(3);

    cluster.network.isolate(&host_of(3));
    eventually("os dois saudáveis excluírem o nó 3", || {
        frozen_and_gone(&cluster, &[1, 2], 3)
    })
    .await;
    // O cluster guarda o ID do excluído e quando ele saiu; um ID que nunca saiu não está em freezing.
    let service = cluster.node(1).node.cluster().unwrap();
    assert!(
        service
            .membership()
            .frozen_remaining(&cluster.node_id(1), cluster.clock.now(), FREEZING)
            .is_none()
    );

    // A rede volta: os pares respondem `not-member`, o nó 3 vê que foi excluído, apaga o que tinha e
    // refaz o ingresso do zero (o mesmo processo, o mesmo ID).
    cluster.network.restore(&host_of(3));
    let third = cluster.node(3);
    eventually_async("o nó 3 ser recusado por freezing", || async {
        total(
            &third.metrics().await,
            "taba_cluster_bootstrap_failures_total",
            "taba_reason=\"frozen\"",
        ) >= 2.0
    })
    .await;
    assert_ne!(
        third.node.shared().state(),
        NodeState::Active,
        "fora do cluster enquanto o freezing dura"
    );
    assert_eq!(third.node.shared().node_id(), id, "continua o mesmo ID");

    // A recusa é `403 cluster.node-frozen` com o tempo que falta em `Retry-After`; o relógio
    // anda 2 minutos e faltam 3.
    assert_eq!(
        join_error(&cluster, 3, 1).await,
        Some((ErrorCode::ClusterNodeFrozen, 300))
    );
    cluster.clock.advance(Duration::from_secs(120));
    assert_eq!(
        join_error(&cluster, 3, 2).await,
        Some((ErrorCode::ClusterNodeFrozen, 180))
    );

    // Não há lista de espera: o cluster não guardou nenhum pedido. As tabelas seguem sem o nó 3,
    // sem mudança pendente, e o nó 3 é quem insiste (o laço do bootstrap).
    for number in [1, 2] {
        let membership = cluster.node(number).node.cluster().unwrap().membership();
        assert!(!membership.has_pending(cluster.clock.now()));
        assert_eq!(cluster.member_ids(number).len(), 2);
    }
    let attempts = cluster.network.count("join");
    eventually("o nó 3 repetir o pedido", || {
        cluster.network.count("join") > attempts + 1
    })
    .await;

    // Passado o freezing o nó volta, com o mesmo ID, e as três tabelas voltam a ser iguais.
    cluster.clock.advance(FREEZING);
    wait_joined(cluster.node(3)).await;
    eventually("as três tabelas iguais com o nó 3", || {
        let tables: Vec<_> = (1..=3).map(|n| cluster.table(n)).collect();
        tables.windows(2).all(|w| w[0] == w[1])
            && tables[0]
                .as_ref()
                .is_some_and(|t| t.members.len() == 3 && t.member(&id).is_some())
    })
    .await;
    assert_eq!(cluster.node(3).node.shared().node_id(), id);
    cluster.shutdown().await;
}

/// UC: UC-FRZ-007
#[tokio::test]
async fn the_nodes_in_freezing_show_in_the_metric_and_in_the_node_frozen_condition() {
    let cluster = TestCluster::start(3).await;
    settled(&cluster).await;
    let conditions = |status: &serde_json::Value| -> Vec<String> {
        status["conditions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["condition"].as_str().unwrap().to_owned())
            .collect()
    };
    let frozen_metric = |text: &str| total(text, "taba_cluster_freezing", "");

    assert_eq!(frozen_metric(&cluster.node(1).metrics().await), 0.0);
    assert!(
        !conditions(&cluster.node(1).stats("/health/cluster?scope=node").await)
            .contains(&"node-frozen".to_owned())
    );

    cluster.network.isolate(&host_of(3));
    eventually("o nó 3 ser excluído", || {
        frozen_and_gone(&cluster, &[1, 2], 3)
    })
    .await;

    // O nó 3 segue isolado: não volta a pedir ingresso e o freezing é só do que os outros guardam.
    for number in [1, 2] {
        let node = cluster.node(number);
        assert_eq!(frozen_metric(&node.metrics().await), 1.0);
        let status = node.stats("/health/cluster?scope=node").await;
        assert!(conditions(&status).contains(&"node-frozen".to_owned()));
        assert_eq!(status["nodes"][0]["status"]["signals"]["frozenNodes"], 1);
        let entry = status["conditions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["condition"] == "node-frozen")
            .unwrap()
            .clone();
        assert_eq!(entry["severity"], "warning");
    }

    // O período passa e o nó deixa de contar.
    cluster.clock.advance(FREEZING + Duration::from_secs(1));
    for number in [1, 2] {
        let node = cluster.node(number);
        assert_eq!(frozen_metric(&node.metrics().await), 0.0);
        let status = node.stats("/health/cluster?scope=node").await;
        assert!(!conditions(&status).contains(&"node-frozen".to_owned()));
    }
    cluster.shutdown().await;
}

/// UC: UC-FRZ-009
#[tokio::test]
async fn a_node_that_joins_after_the_exclusion_inherits_the_freezing_of_the_excluded() {
    let mut cluster = TestCluster::start(3).await;
    settled(&cluster).await;
    let excluded = cluster.node_id(3);
    cluster.network.isolate(&host_of(3));
    eventually("os dois saudáveis excluírem o nó 3", || {
        frozen_and_gone(&cluster, &[1, 2], 3)
    })
    .await;

    // O nó 4 entra depois do commit da exclusão: ele não o viu, e recebe o freezing na tabela.
    cluster.add_node(false).await;
    let membership = cluster.node(4).node.cluster().unwrap().membership();
    assert!(
        membership
            .frozen_remaining(&excluded, cluster.clock.now(), FREEZING)
            .is_some(),
        "o nó novo herdou o freezing"
    );
    // A tabela guardada não o leva, e as tabelas dos nós seguem iguais.
    assert!(membership.table().unwrap().frozen.is_empty());
    assert_eq!(cluster.table(4), cluster.table(1));

    // Por isso o pedido do excluído que cair no nó 4 também é recusado.
    let transport = cluster.network.transport(host_of(1));
    let reply = transport
        .call(&address_of(&cluster, 4), join_as(&cluster, 3))
        .await;
    match reply {
        Err(CallError::Remote(error)) => assert_eq!(error.code, ErrorCode::ClusterNodeFrozen),
        other => panic!("esperava cluster.node-frozen, veio {other:?}"),
    }
    cluster.shutdown().await;
}

/// UC: UC-FRZ-010, UC-FRZ-011
#[tokio::test]
async fn a_member_that_missed_the_exclusion_commit_learns_the_freezing_from_the_channel() {
    let cluster = TestCluster::start(4).await;
    settled(&cluster).await;
    let excluded = cluster.node_id(4);

    // O nó de maior ID entre os três saudáveis nunca é o que coordena a exclusão (coordena o de
    // menor ID), e fica sem receber o commit: é o membro "fora do ar" no commit.
    let lagging = (1..=3)
        .max_by_key(|n| cluster.node_id(*n))
        .expect("três nós saudáveis");
    let others: Vec<usize> = (1..=3).filter(|n| *n != lagging).collect();
    cluster
        .network
        .lose_kind(&host_of(lagging), "commit", 10_000);
    cluster.network.isolate(&host_of(4));
    eventually("os outros dois saudáveis excluírem o nó 4", || {
        frozen_and_gone(&cluster, &others, 4)
    })
    .await;

    // O commit não chegou ao retardatário: a tabela dele ainda tem o nó 4. Mesmo assim ele fica
    // sabendo do freezing, pelo quadro `frozen` do canal.
    let membership = cluster.node(lagging).node.cluster().unwrap().membership();
    eventually("o retardatário saber do freezing", || {
        membership
            .frozen_remaining(&excluded, cluster.clock.now(), FREEZING)
            .is_some()
    })
    .await;
    assert!(
        membership.is_member(&excluded),
        "a tabela do retardatário segue com o excluído (o acerto dela é da etapa 18b)"
    );
    assert!(membership.table().unwrap().frozen.is_empty());

    // O pedido do excluído que cair nele é recusado, e não aceito como "o mesmo nó repetindo".
    let transport = cluster.network.transport(host_of(others[0]));
    let reply = transport
        .call(&address_of(&cluster, lagging), join_as(&cluster, 4))
        .await;
    match reply {
        Err(CallError::Remote(error)) => assert_eq!(error.code, ErrorCode::ClusterNodeFrozen),
        other => panic!("esperava cluster.node-frozen, veio {other:?}"),
    }
    // Sem perder mais commits, o commit que seguia em segundo plano termina e o desligamento não espera.
    cluster.network.lose_kind(&host_of(lagging), "commit", 0);
    cluster.shutdown().await;
}

/// UC: UC-FRZ-014
#[tokio::test]
async fn the_channel_snapshot_carries_the_nodes_still_in_freezing() {
    let cluster = TestCluster::start(3).await;
    settled(&cluster).await;
    let excluded = cluster.node_id(3);
    assert!(
        !signals_of(&cluster, 1)
            .snapshot()
            .iter()
            .any(|s| matches!(s, Signal::Frozen { .. })),
        "sem exclusão, o instantâneo não leva freezing"
    );
    cluster.network.isolate(&host_of(3));
    eventually("os dois saudáveis excluírem o nó 3", || {
        frozen_and_gone(&cluster, &[1, 2], 3)
    })
    .await;
    let snapshot = signals_of(&cluster, 1).snapshot();
    assert!(
        snapshot
            .iter()
            .any(|s| matches!(s, Signal::Frozen { node_id, .. } if *node_id == excluded)),
        "o instantâneo leva o nó em freezing: {snapshot:?}"
    );

    // Passado o freezing, ele sai do instantâneo.
    cluster.clock.advance(FREEZING + Duration::from_secs(1));
    assert!(
        !signals_of(&cluster, 1)
            .snapshot()
            .iter()
            .any(|s| matches!(s, Signal::Frozen { .. }))
    );
    cluster.shutdown().await;
}
