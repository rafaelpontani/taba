//! O ingresso de nós num cluster simulado (doc 04, seções 6.2 e 6.5; doc 06c; nível **C** do
//! doc 20): vários nós de verdade no mesmo processo, ligados pela rede em memória.

mod common;

use std::time::Duration;

use common::cluster::{TestCluster, balancer, eventually, eventually_async, host_of, wait_joined};
use common::series;
use taba_node::cluster::members::Member;
use taba_node::cluster::members::MemberState;
use taba_node::cluster::protocol::{
    JoinRequest, NodeRegisterRequest, NodeUnregisterRequest, OperationRef, Request, Response,
    UnregisterReason,
};
use taba_node::cluster::transport::{CallError, PeerAddr, Transport};
use taba_node::core::error::ErrorCode;
use taba_node::core::id;
use taba_node::core::ring::message_position;

fn guid() -> String {
    id::new_at(1_791_288_000_000)
}

fn address_of(cluster: &TestCluster, number: usize) -> PeerAddr {
    PeerAddr::new(
        host_of(number),
        cluster.node(number).node.cluster_addr().unwrap().port(),
    )
}

/// O código do erro que o par devolveu.
fn remote_code(error: CallError) -> ErrorCode {
    match error {
        CallError::Remote(e) => e.code,
        other => panic!("esperava um erro do par, veio {other}"),
    }
}

fn newcomer(address: &str, port: u16) -> Member {
    Member {
        node_id: guid(),
        addresses: vec![address.to_owned()],
        port,
        state: MemberState::Active,
    }
}

/// UC: UC-ING-005, UC-ING-002
#[tokio::test]
async fn the_first_node_starts_the_cluster_alone() {
    let cluster = TestCluster::start(1).await;

    let table = cluster.table(1).expect("o nó tem a tabela");
    assert_eq!(table.version, 1);
    assert_eq!(table.members.len(), 1);
    let me = &table.members[0];
    assert_eq!(me.node_id, cluster.node_id(1));
    assert_eq!(me.addresses, ["10.0.0.1"]);
    assert_eq!(me.port, cluster.node(1).node.cluster_addr().unwrap().port());
    assert_eq!(me.state, MemberState::Active);

    let ready = cluster.node(1).client_get_ops("/health/ready").await;
    assert_eq!(ready.status(), 200, "registrado, o nó está pronto");
    cluster.shutdown().await;
}

/// UC: UC-ING-004, UC-ING-005
#[tokio::test]
async fn without_the_first_node_flag_the_node_keeps_trying_and_stays_out_of_rotation() {
    let mut cluster = TestCluster::empty();
    let builder = cluster.builder(1, false);
    cluster.spawn_node(builder).await.unwrap();

    // A listagem não acha ninguém: sem `cluster.first-node` isso é erro, e o nó tenta de novo.
    let network = cluster.network.clone();
    eventually("três tentativas de listagem", || {
        network.count("list-nodes") >= 3
    })
    .await;
    assert!(
        cluster.table(1).is_none(),
        "o nó não começa um cluster sozinho"
    );
    let ready = cluster.node(1).client_get_ops("/health/ready").await;
    assert_eq!(ready.status(), 503);
    assert_eq!(
        ready.json::<serde_json::Value>().await.unwrap()["reason"],
        "bootstrapping"
    );
    let metrics = cluster.node(1).metrics().await;
    let failures = series(
        &metrics,
        "taba_cluster_bootstrap_failures_total{taba_reason=\"list\"}",
    );
    assert!(failures.is_some_and(|n| n >= 2.0), "{failures:?}");

    // Aparece um cluster: o nó entra na tentativa seguinte.
    cluster.add_node(true).await;
    wait_joined(cluster.node(1)).await;
    assert_eq!(cluster.member_ids(1), cluster.member_ids(2));
    assert_eq!(cluster.member_ids(1).len(), 2);
    cluster.shutdown().await;
}

/// UC: UC-ING-017
#[tokio::test]
async fn the_node_gives_up_after_the_maximum_attempts() {
    let mut cluster = TestCluster::empty();
    let builder = cluster
        .builder(1, false)
        .env("TABA_CLUSTER_BOOTSTRAP_RETRY_MAX_ATTEMPTS", "3");
    cluster.spawn_node(builder).await.unwrap();

    let reason = tokio::time::timeout(Duration::from_secs(10), cluster.nodes[0].node.fatal())
        .await
        .expect("o bootstrap deveria esgotar as tentativas");
    assert!(reason.contains("3 tentativas"), "{reason}");
    assert_eq!(cluster.network.count("list-nodes"), 3);
    cluster.shutdown().await;
}

/// UC: UC-ING-002, UC-ING-003, UC-ING-008, UC-ING-010, UC-ING-011, UC-ING-027, UC-ANE-010
#[tokio::test]
async fn nodes_join_through_the_balancer_and_end_with_the_same_table() {
    let mut cluster = TestCluster::start(3).await;
    cluster.network.clear_calls();
    cluster.add_node(false).await;

    // Todos enxergam os quatro membros, na mesma época (um registro efetivado por nó que entrou).
    let ids: Vec<String> = {
        let mut ids: Vec<String> = (1..=4).map(|n| cluster.node_id(n)).collect();
        ids.sort();
        ids
    };
    for number in 1..=4 {
        let table = cluster.table(number).unwrap();
        assert_eq!(table.version, 4, "nó {number}");
        assert_eq!(cluster.member_ids(number), ids, "nó {number}");
    }

    // O anel de cada nó é o mesmo, e o primário de uma mensagem não depende de quem pergunta.
    for message in (0..100u64).map(|i| id::new_at(1_791_288_000_000 + i)) {
        let position = message_position(&message);
        let primaries: Vec<Option<String>> = (1..=4)
            .map(|n| {
                cluster
                    .node(n)
                    .node
                    .cluster()
                    .unwrap()
                    .membership()
                    .ring()
                    .primary(position)
                    .map(str::to_owned)
            })
            .collect();
        assert!(primaries.windows(2).all(|w| w[0] == w[1]), "{primaries:?}");
    }

    // Só a listagem passa pelo balanceador; o resto vai direto aos nós, pelos IPs da tabela.
    let calls = cluster.network.calls();
    assert!(
        calls
            .iter()
            .filter(|c| c.to == balancer())
            .all(|c| c.request == "list-nodes"),
        "{calls:?}"
    );
    assert_eq!(calls.iter().filter(|c| c.to == balancer()).count(), 1);
    // O membro sorteado conectou de volta ao nó novo (teste de conexão), e a fase 1 foi a
    // todos os outros membros (os dois que não coordenam) antes de qualquer commit.
    let joins = calls.iter().filter(|c| c.request == "join").count();
    assert_eq!(joins, 1);
    let new_host = host_of(4);
    assert!(
        calls
            .iter()
            .any(|c| c.request == "list-nodes" && c.to.host == new_host),
        "{calls:?}"
    );
    let registers: Vec<usize> = calls
        .iter()
        .enumerate()
        .filter(|(_, c)| c.request == "register")
        .map(|(i, _)| i)
        .collect();
    let commits: Vec<usize> = calls
        .iter()
        .enumerate()
        .filter(|(_, c)| c.request == "commit")
        .map(|(i, _)| i)
        .collect();
    assert_eq!(registers.len(), 2);
    assert_eq!(commits.len(), 2);
    assert!(registers.iter().max() < commits.iter().min(), "{calls:?}");
    assert!(
        calls
            .iter()
            .filter(|c| c.request == "register" || c.request == "commit")
            .all(|c| c.to.host != new_host),
        "o nó novo não participa das fases; recebe a tabela na resposta"
    );

    // A época e os membros também saem no `/metrics` de cada nó.
    for number in 1..=4 {
        let metrics = cluster.node(number).metrics().await;
        assert_eq!(series(&metrics, "taba_cluster_members_version"), Some(4.0));
        assert_eq!(
            series(&metrics, "taba_cluster_members{taba_state=\"active\"}"),
            Some(4.0)
        );
        assert_eq!(series(&metrics, "taba_ring_virtual_nodes"), Some(128.0));
    }
    cluster.shutdown().await;
}

/// UC: UC-ING-009
#[tokio::test]
async fn a_new_node_the_member_cannot_connect_back_to_is_refused() {
    let cluster = TestCluster::start(1).await;
    let outsider = cluster.network.transport("10.0.0.50");

    let error = outsider
        .call(
            &address_of(&cluster, 1),
            Request::Join(JoinRequest {
                node_id: guid(),
                addresses: vec!["10.0.0.50".into()],
                port: 9000,
                version: "0.9.0".into(),
            }),
        )
        .await
        .unwrap_err();

    assert_eq!(remote_code(error), ErrorCode::ClusterJoinUnreachable);
    assert_eq!(cluster.table(1).unwrap().version, 1, "nada foi registrado");
    assert_eq!(cluster.network.count("register"), 0);
    cluster.shutdown().await;
}

/// UC: UC-ING-013, UC-ING-015, UC-ING-016, UC-ING-017, UC-ING-020, UC-ANE-010
#[tokio::test]
async fn a_member_that_does_not_confirm_aborts_the_join_and_the_node_tries_again() {
    let mut cluster = TestCluster::start(2).await;
    // Os dois membros não se falam: quem coordenar o ingresso não alcança o outro.
    cluster.network.clear_calls();
    cluster.network.partition("10.0.0.1", "10.0.0.2");
    let builder = cluster.builder(3, false);
    cluster.spawn_node(builder).await.unwrap();

    // O registro não é confirmado por todos: rollback, `503 cluster.registration-failed`, e o nó
    // novo conta a falha da fase 1.
    let node = cluster.node(3);
    eventually_async("a falha da fase 1 contada", || async move {
        let metrics = node.metrics().await;
        series(
            &metrics,
            "taba_cluster_bootstrap_failures_total{taba_reason=\"phase1\"}",
        )
        .is_some_and(|n| n >= 1.0)
    })
    .await;
    assert_eq!(cluster.network.count("commit"), 0, "sem commit sem todos");
    for number in [1, 2] {
        let table = cluster.table(number).unwrap();
        assert_eq!((table.version, table.members.len()), (2, 2), "nó {number}");
        let membership = cluster.node(number).node.cluster().unwrap().membership();
        assert_eq!(
            membership.ring().node_count(),
            2,
            "o anel segue sem o nó que não entrou"
        );
    }
    assert!(cluster.table(3).is_none());
    let ready = cluster.node(3).client_get_ops("/health/ready").await;
    assert_eq!(ready.status(), 503);

    // Curada a partição, a nova tentativa fecha o ingresso: o anel passa a ter os três.
    cluster.network.heal("10.0.0.1", "10.0.0.2");
    wait_joined(cluster.node(3)).await;
    for number in 1..=3 {
        eventually("a tabela com três membros", || {
            cluster.member_ids(number).len() == 3
        })
        .await;
        let membership = cluster.node(number).node.cluster().unwrap().membership();
        assert_eq!(membership.ring().node_count(), 3);
        assert_eq!(cluster.table(number).unwrap().version, 3);
    }
    assert_eq!(
        cluster.network.count("rollback"),
        0,
        "só quem tinha confirmado recebe rollback (aqui, ninguém)"
    );
    cluster.shutdown().await;
}

/// UC: UC-ING-018
#[tokio::test]
async fn a_commit_that_does_not_arrive_does_not_undo_the_join() {
    let mut cluster = TestCluster::start(2).await;
    // O commit não chega a nenhum dos dois membros: o que não coordena fica sem confirmar.
    cluster.network.clear_calls();
    for host in ["10.0.0.1", "10.0.0.2"] {
        cluster.network.lose_kind(host, "commit", 1_000_000);
    }
    let builder = cluster.builder(3, false);
    cluster.spawn_node(builder).await.unwrap();

    // O `join` responde `503 cluster.commit-timeout` (o nó foi aceito), e o nó novo confere
    // na tabela do coordenador, em vez de repetir o pedido.
    wait_joined(cluster.node(3)).await;
    assert_eq!(
        cluster.network.count("join"),
        1,
        "o pedido não foi repetido"
    );
    assert_eq!(cluster.table(3).unwrap().version, 3);
    let versions: Vec<u64> = [1, 2]
        .iter()
        .map(|n| cluster.table(*n).unwrap().version)
        .collect();
    assert!(
        versions.contains(&3) && versions.contains(&2),
        "o coordenador efetivou, o outro ainda não: {versions:?}"
    );

    // O coordenador segue insistindo em segundo plano: quando a rede volta, todos convergem.
    for host in ["10.0.0.1", "10.0.0.2"] {
        cluster.network.lose_kind(host, "commit", 0);
    }
    for number in [1, 2] {
        eventually("o commit chegar ao outro membro", || {
            cluster.table(number).unwrap().version == 3
        })
        .await;
    }
    for number in 1..=3 {
        assert_eq!(cluster.member_ids(number).len(), 3, "nó {number}");
    }
    let mut timeouts = 0.0;
    for number in [1, 2] {
        let metrics = cluster.node(number).metrics().await;
        timeouts += series(
            &metrics,
            "taba_cluster_membership_changes_total{taba_operation=\"register\",taba_outcome=\"timeout\",taba_phase=\"commit\"}",
        )
        .unwrap_or(0.0);
    }
    assert_eq!(timeouts, 1.0, "o teto de commit esgotou uma vez");
    cluster.shutdown().await;
}

/// UC: UC-ING-014
#[tokio::test]
async fn a_second_pending_registration_conflicts_and_unknown_operations_are_refused() {
    let cluster = TestCluster::start(2).await;
    let member = cluster.network.transport("10.0.0.1");
    let target = address_of(&cluster, 2);
    let register = |operation_id: String| {
        Request::Register(NodeRegisterRequest {
            operation_id,
            coordinator_id: cluster.node_id(1),
            node: newcomer("10.0.0.60", 9001),
        })
    };

    let first = guid();
    let response = member.call(&target, register(first.clone())).await.unwrap();
    assert!(matches!(response, Response::Ack(a) if a.accepted && a.operation_id == first));
    // Repetir o mesmo `operationId` é idempotente.
    member.call(&target, register(first.clone())).await.unwrap();

    let error = member
        .call(&target, register(guid()))
        .await
        .expect_err("há outra mudança pendente");
    assert_eq!(remote_code(error), ErrorCode::ClusterConflict);

    let unknown = member
        .call(
            &target,
            Request::Commit(OperationRef {
                operation_id: guid(),
            }),
        )
        .await
        .unwrap_err();
    assert_eq!(remote_code(unknown), ErrorCode::ClusterOperationUnknown);
    let unknown = member
        .call(
            &target,
            Request::Rollback(OperationRef {
                operation_id: guid(),
            }),
        )
        .await
        .unwrap_err();
    assert_eq!(remote_code(unknown), ErrorCode::ClusterOperationUnknown);

    // O rollback libera o lugar: nada mudou na tabela.
    member
        .call(
            &target,
            Request::Rollback(OperationRef {
                operation_id: first,
            }),
        )
        .await
        .unwrap();
    let last = guid();
    member.call(&target, register(last.clone())).await.unwrap();
    // Desfaz a pendência: o nó 2 não conseguiria sair do cluster na parada do teste com ela.
    member
        .call(
            &target,
            Request::Rollback(OperationRef { operation_id: last }),
        )
        .await
        .unwrap();
    assert_eq!(cluster.table(2).unwrap().version, 2);
    cluster.shutdown().await;
}

/// UC: UC-SEG-018, UC-ING-002
#[tokio::test]
async fn only_members_call_the_phase_routes_but_anyone_lists_and_asks_to_join() {
    let cluster = TestCluster::start(2).await;
    let stranger = cluster.network.transport("10.0.0.99");
    let target = address_of(&cluster, 1);
    let operation = || OperationRef {
        operation_id: guid(),
    };

    for request in [
        Request::Register(NodeRegisterRequest {
            operation_id: guid(),
            coordinator_id: guid(),
            node: newcomer("10.0.0.60", 9001),
        }),
        Request::Unregister(NodeUnregisterRequest {
            operation_id: guid(),
            coordinator_id: guid(),
            node_id: cluster.node_id(2),
            reason: UnregisterReason::Vote,
        }),
        Request::Commit(operation()),
        Request::Rollback(operation()),
    ] {
        let name = request.name();
        let error = stranger.call(&target, request).await.unwrap_err();
        assert_eq!(remote_code(error), ErrorCode::ClusterNotMember, "{name}");
    }

    // Quem ainda não é membro lista os nós e pede o ingresso (aqui o teste de conexão falha, o
    // que prova que o pedido chegou a ser tratado, sem `403`).
    let listed = stranger.call(&target, Request::ListNodes).await.unwrap();
    assert!(matches!(listed, Response::Members(t) if t.members.len() == 2));
    let join = stranger
        .call(
            &target,
            Request::Join(JoinRequest {
                node_id: guid(),
                addresses: vec!["10.0.0.99".into()],
                port: 9000,
                version: "0.9.0".into(),
            }),
        )
        .await
        .unwrap_err();
    assert_eq!(remote_code(join), ErrorCode::ClusterJoinUnreachable);
    cluster.shutdown().await;
}

/// UC: UC-ING-006
#[tokio::test]
async fn a_node_that_has_no_table_yet_answers_503() {
    let mut cluster = TestCluster::empty();
    let builder = cluster.builder(1, false);
    cluster.spawn_node(builder).await.unwrap();
    let caller = cluster.network.transport("10.0.0.99");
    let target = address_of(&cluster, 1);

    let error = caller.call(&target, Request::ListNodes).await.unwrap_err();
    assert_eq!(remote_code(error), ErrorCode::ClusterNodeSyncing);
    // Sem a tabela o nó nem sabe quem é membro: as fases também respondem `503`, não `403`.
    let error = caller
        .call(
            &target,
            Request::Commit(OperationRef {
                operation_id: guid(),
            }),
        )
        .await
        .unwrap_err();
    assert_eq!(remote_code(error), ErrorCode::ClusterNodeSyncing);
    cluster.shutdown().await;
}

#[tokio::test]
async fn an_unregistration_removes_the_member_and_shrinks_the_ring() {
    let cluster = TestCluster::start(3).await;
    let member = cluster.network.transport("10.0.0.1");
    let target = address_of(&cluster, 2);
    let leaving = cluster.node_id(3);
    let operation_id = guid();

    member
        .call(
            &target,
            Request::Unregister(NodeUnregisterRequest {
                operation_id: operation_id.clone(),
                coordinator_id: cluster.node_id(1),
                node_id: leaving.clone(),
                reason: UnregisterReason::Drain,
            }),
        )
        .await
        .unwrap();
    assert_eq!(cluster.table(2).unwrap().members.len(), 3, "pendente");
    member
        .call(&target, Request::Commit(OperationRef { operation_id }))
        .await
        .unwrap();

    let membership = cluster.node(2).node.cluster().unwrap().membership();
    let table = membership.table().unwrap();
    assert_eq!((table.version, table.members.len()), (4, 2));
    assert!(!membership.is_member(&leaving));
    assert_eq!(membership.ring().node_count(), 2);
    cluster.shutdown().await;
}
