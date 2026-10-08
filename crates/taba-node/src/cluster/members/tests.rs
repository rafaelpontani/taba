use std::time::{Duration, UNIX_EPOCH};

use super::*;

const TTL: Duration = Duration::from_secs(30);

fn at(secs: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_791_288_000 + secs)
}

fn member(id: &str) -> Member {
    Member {
        node_id: id.to_owned(),
        addresses: vec![format!("10.0.0.{}", id.len())],
        port: 8081,
        state: MemberState::Active,
    }
}

fn alone(id: &str) -> Membership {
    let membership = Membership::new(TTL, 16);
    membership.install(MemberTable {
        version: 1,
        members: vec![member(id)],
        ..MemberTable::default()
    });
    membership
}

#[test]
fn a_node_without_the_table_cannot_register_anything() {
    let membership = Membership::new(TTL, 16);
    assert_eq!(membership.table(), None);
    assert!(membership.ring().is_empty());
    assert_eq!(
        membership.register("op1", Change::Add(member("b")), at(0)),
        Err(MembershipError::NoTable)
    );
}

/// UC: UC-ING-010
#[test]
fn register_leaves_the_change_pending_and_commit_makes_it_effective() {
    let membership = alone("a");
    membership
        .register("op1", Change::Add(member("bb")), at(0))
        .unwrap();
    let table = membership.table().unwrap();
    assert_eq!(
        (table.version, table.members.len()),
        (1, 1),
        "pendente não muda a tabela"
    );
    assert!(!membership.is_member("bb"));

    let table = membership.commit("op1", at(1)).unwrap();
    assert_eq!(table.version, 2, "a época sobe a cada mudança efetivada");
    assert_eq!(
        table
            .members
            .iter()
            .map(|m| m.node_id.as_str())
            .collect::<Vec<_>>(),
        ["a", "bb"]
    );
    assert!(membership.is_member("bb"));
    assert!(!membership.has_pending(at(1)));
}

/// UC: UC-ING-014
#[test]
fn all_three_phases_are_idempotent_by_operation_id() {
    let membership = alone("a");
    membership
        .register("op1", Change::Add(member("bb")), at(0))
        .unwrap();
    membership
        .register("op1", Change::Add(member("bb")), at(1))
        .unwrap();
    membership.commit("op1", at(2)).unwrap();
    let again = membership.commit("op1", at(3)).unwrap();
    assert_eq!(
        again.version, 2,
        "repetir o commit não sobe a época de novo"
    );
    // Um register repetido depois do commit também não faz nada.
    membership
        .register("op1", Change::Add(member("bb")), at(4))
        .unwrap();
    assert_eq!(membership.version(), Some(2));

    membership
        .register("op2", Change::Add(member("ccc")), at(5))
        .unwrap();
    membership.rollback("op2", at(6)).unwrap();
    membership.rollback("op2", at(7)).unwrap();
    assert!(!membership.is_member("ccc"));
}

/// UC: UC-ING-014
#[test]
fn a_second_change_while_one_is_pending_is_a_conflict() {
    let membership = alone("a");
    membership
        .register("op1", Change::Add(member("bb")), at(0))
        .unwrap();
    assert!(matches!(
        membership.register("op2", Change::Add(member("ccc")), at(1)),
        Err(MembershipError::Conflict(_))
    ));
    membership.commit("op1", at(2)).unwrap();
    membership
        .register("op2", Change::Add(member("ccc")), at(3))
        .unwrap();
}

/// UC: UC-ING-014
#[test]
fn commit_and_rollback_of_an_unknown_operation_are_unknown() {
    let membership = alone("a");
    assert_eq!(
        membership.commit("nunca-chegou", at(0)),
        Err(MembershipError::UnknownOperation)
    );
    assert_eq!(
        membership.rollback("nunca-chegou", at(0)),
        Err(MembershipError::UnknownOperation)
    );
}

#[test]
fn a_pending_change_expires_after_the_pending_ttl() {
    let membership = alone("a");
    membership
        .register("op1", Change::Add(member("bb")), at(0))
        .unwrap();
    assert!(membership.has_pending(at(29)));
    assert!(!membership.has_pending(at(30)));
    assert_eq!(
        membership.commit("op1", at(31)),
        Err(MembershipError::UnknownOperation),
        "vencida, a operação é desconhecida"
    );
    // O lugar fica livre para outra mudança.
    membership
        .register("op2", Change::Add(member("ccc")), at(31))
        .unwrap();
}

#[test]
fn there_is_no_rollback_after_commit_and_no_commit_after_rollback() {
    let membership = alone("a");
    membership
        .register("op1", Change::Add(member("bb")), at(0))
        .unwrap();
    membership.commit("op1", at(1)).unwrap();
    assert!(matches!(
        membership.rollback("op1", at(2)),
        Err(MembershipError::Conflict(_))
    ));

    membership
        .register("op2", Change::Add(member("ccc")), at(3))
        .unwrap();
    membership.rollback("op2", at(4)).unwrap();
    assert!(matches!(
        membership.commit("op2", at(5)),
        Err(MembershipError::Conflict(_))
    ));
}

#[test]
fn adding_a_member_twice_or_removing_a_stranger_is_a_conflict() {
    let membership = alone("a");
    assert!(matches!(
        membership.register("op1", Change::Add(member("a")), at(0)),
        Err(MembershipError::Conflict(_))
    ));
    assert!(matches!(
        membership.register(
            "op2",
            Change::Remove {
                node_id: "x".into()
            },
            at(0)
        ),
        Err(MembershipError::Conflict(_))
    ));
    assert!(
        !membership.has_pending(at(0)),
        "o recusado não deixa pendência"
    );
}

/// UC: UC-ANE-010
#[test]
fn the_ring_follows_the_table() {
    let membership = alone("a");
    assert_eq!(membership.ring().node_count(), 1);
    membership
        .register("op1", Change::Add(member("bb")), at(0))
        .unwrap();
    assert_eq!(
        membership.ring().node_count(),
        1,
        "pendente não muda o anel"
    );
    membership.commit("op1", at(1)).unwrap();
    assert_eq!(membership.ring().node_count(), 2);

    membership
        .register(
            "op2",
            Change::Remove {
                node_id: "bb".into(),
            },
            at(2),
        )
        .unwrap();
    membership.commit("op2", at(3)).unwrap();
    let ring = membership.ring();
    assert_eq!(ring.nodes().collect::<Vec<_>>(), ["a"]);
    assert_eq!(membership.version(), Some(3));
}

/// UC: UC-ING-008
#[test]
fn callers_are_recognized_by_the_addresses_in_the_table() {
    let membership = alone("a");
    assert!(membership.is_member_address("10.0.0.1"));
    assert!(!membership.is_member_address("10.0.0.9"));
}

#[test]
fn the_memory_of_finished_operations_is_bounded() {
    let membership = alone("a");
    for n in 0..(FINISHED_MEMORY + 10) {
        let id = format!("op{n}");
        membership
            .register(&id, Change::Add(member(&format!("n{n}"))), at(0))
            .unwrap();
        membership.rollback(&id, at(0)).unwrap();
    }
    assert_eq!(
        membership.rollback("op0", at(0)),
        Err(MembershipError::UnknownOperation),
        "as mais antigas foram esquecidas"
    );
    membership
        .rollback(&format!("op{}", FINISHED_MEMORY + 9), at(0))
        .unwrap();
}

/// UC: UC-CNC-009
#[test]
fn a_coordinator_yields_to_an_older_registration_and_never_to_a_newer_one() {
    let membership = alone("a");
    // Este nó coordena a operação "op5".
    membership
        .register_coordinating("op5", Change::Add(member("x")), at(0))
        .unwrap();
    // Uma mais nova que chega de outro coordenador espera (conflito).
    assert!(matches!(
        membership.register("op7", Change::Add(member("y")), at(0)),
        Err(MembershipError::Conflict(_))
    ));
    assert!(membership.is_pending("op5"));
    // Uma mais antiga desloca a que este nó coordena, que passa a constar como desfeita.
    membership
        .register("op3", Change::Add(member("z")), at(0))
        .unwrap();
    assert!(!membership.is_pending("op5"));
    assert!(membership.is_pending("op3"));
    assert_eq!(membership.rollback("op5", at(0)), Ok(()));
}

/// UC: UC-CNC-009
#[test]
fn a_registration_held_for_another_coordinator_is_never_displaced() {
    let membership = alone("a");
    membership
        .register("op5", Change::Add(member("x")), at(0))
        .unwrap();
    // Mesmo uma mais antiga espera: quem a recebeu como participante já confirmou à outra.
    assert!(matches!(
        membership.register("op3", Change::Add(member("z")), at(0)),
        Err(MembershipError::Conflict(_))
    ));
    assert!(membership.is_pending("op5"));
}
