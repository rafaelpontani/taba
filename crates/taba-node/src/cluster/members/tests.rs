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
        "pending does not change the table"
    );
    assert!(!membership.is_member("bb"));

    let table = membership.commit("op1", at(1)).unwrap();
    assert_eq!(table.version, 2, "the epoch rises on each effected change");
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
        "repeating the commit does not raise the epoch again"
    );
    // A register repeated after the commit also does nothing.
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
        membership.commit("never-arrived", at(0)),
        Err(MembershipError::UnknownOperation)
    );
    assert_eq!(
        membership.rollback("never-arrived", at(0)),
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
        "expired, the operation is unknown"
    );
    // The place is free for another change.
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
        "the refused one leaves nothing pending"
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
        "pending does not change the ring"
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
        "the oldest were forgotten"
    );
    membership
        .rollback(&format!("op{}", FINISHED_MEMORY + 9), at(0))
        .unwrap();
}

/// UC: UC-CNC-009
#[test]
fn a_coordinator_yields_to_an_older_registration_and_never_to_a_newer_one() {
    let membership = alone("a");
    // This node coordinates operation "op5".
    membership
        .register_coordinating("op5", Change::Add(member("x")), at(0))
        .unwrap();
    // A newer one arriving from another coordinator waits (conflict).
    assert!(matches!(
        membership.register("op7", Change::Add(member("y")), at(0)),
        Err(MembershipError::Conflict(_))
    ));
    assert!(membership.is_pending("op5"));
    // An older one displaces the one this node coordinates, which is recorded as undone.
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
    // Even an older one waits: whoever received it as a participant already confirmed to the other.
    assert!(matches!(
        membership.register("op3", Change::Add(member("z")), at(0)),
        Err(MembershipError::Conflict(_))
    ));
    assert!(membership.is_pending("op5"));
}

const FREEZING: Duration = Duration::from_secs(300);

fn ms(secs: u64) -> u64 {
    at(secs).duration_since(UNIX_EPOCH).unwrap().as_millis() as u64
}

/// UC: UC-FRZ-012
#[test]
fn a_freezing_learned_twice_keeps_the_first_instant_and_only_the_first_is_news() {
    let membership = alone("a");
    assert!(membership.learn_frozen("x", ms(10), at(20), FREEZING));
    assert!(
        !membership.learn_frozen("x", ms(15), at(20), FREEZING),
        "the echo is not news"
    );
    assert_eq!(membership.frozen_at("x"), Some(ms(10)));
    assert_eq!(
        membership.frozen_remaining("x", at(20), FREEZING),
        Some(FREEZING - Duration::from_secs(10))
    );
}

/// UC: UC-FRZ-013
#[test]
fn a_freezing_that_already_ended_is_not_learned() {
    let membership = alone("a");
    assert!(!membership.learn_frozen("x", ms(10), at(10 + 300), FREEZING));
    assert_eq!(membership.frozen_at("x"), None);
    assert_eq!(membership.frozen_count(at(10 + 300), FREEZING), 0);
    // An instant in the future (clocks that do not match) also counts: the deadline counts from it.
    assert!(membership.learn_frozen("y", ms(40), at(30), FREEZING));
}

/// UC: UC-FRZ-014
#[test]
fn the_frozen_list_has_only_the_nodes_still_in_freezing() {
    let membership = alone("a");
    assert!(membership.learn_frozen("old", ms(0), at(100), FREEZING));
    assert!(membership.learn_frozen("new", ms(90), at(100), FREEZING));
    let listed = membership.frozen_nodes(at(100 + 250), FREEZING);
    assert_eq!(
        listed,
        vec![FrozenNode {
            node_id: "new".into(),
            frozen_at: ms(90)
        }],
        "the expired one leaves the list"
    );
}

fn two_members(a: &str, b: &str) -> Membership {
    let membership = Membership::new(TTL, 16);
    membership.install(MemberTable {
        version: 1,
        members: vec![member(a), member(b)],
        ..MemberTable::default()
    });
    membership
}

/// UC: UC-CNC-010
#[test]
fn an_exclusion_and_an_entry_that_cross_follow_the_same_rule_in_both_directions() {
    // This node coordinates the exclusion of "b" (op5): an older join (op3) displaces it.
    let membership = two_members("a", "b");
    membership
        .register_coordinating(
            "op5",
            Change::Exclude {
                node_id: "b".into(),
            },
            at(0),
        )
        .unwrap();
    membership
        .register("op3", Change::Add(member("c")), at(0))
        .unwrap();
    assert!(!membership.is_pending("op5"));
    assert!(membership.is_pending("op3"));

    // This node coordinates the join of "c" (op5): a newer exclusion (op7) waits, and an
    // older one (op3) displaces it.
    let membership = two_members("a", "b");
    membership
        .register_coordinating("op5", Change::Add(member("c")), at(0))
        .unwrap();
    assert!(matches!(
        membership.register(
            "op7",
            Change::Exclude {
                node_id: "b".into()
            },
            at(0)
        ),
        Err(MembershipError::Conflict(_))
    ));
    assert!(membership.is_pending("op5"));
    membership
        .register(
            "op3",
            Change::Exclude {
                node_id: "b".into(),
            },
            at(0),
        )
        .unwrap();
    assert!(!membership.is_pending("op5"));
    assert!(membership.is_pending("op3"));
}

/// UC: UC-FAL-032
#[test]
fn the_excluded_nodes_kept_for_the_reunion_are_the_last_64_and_leave_when_back_in_the_table() {
    let membership = Membership::new(TTL, 16);
    let ids: Vec<String> = (0..70).map(|n| format!("m{n:02}")).collect();
    let mut members = vec![member("a")];
    members.extend(ids.iter().map(|id| member(id)));
    membership.install(MemberTable {
        version: 1,
        members,
        ..MemberTable::default()
    });
    for (n, id) in ids.iter().enumerate() {
        let operation = format!("op{n}");
        membership
            .register(
                &operation,
                Change::Exclude {
                    node_id: id.clone(),
                },
                at(0),
            )
            .unwrap();
        membership.commit(&operation, at(0)).unwrap();
    }

    let kept: Vec<String> = membership
        .excluded_members()
        .into_iter()
        .map(|m| m.node_id)
        .collect();
    assert_eq!(kept.len(), 64, "at most 64");
    assert_eq!(kept, ids[6..], "the oldest leave first");

    // What returns to the table (with the same ID) stops being looked for.
    let mut table = membership.table().unwrap();
    table.members.push(member(&ids[69]));
    membership.install(table);
    let kept: Vec<String> = membership
        .excluded_members()
        .into_iter()
        .map(|m| m.node_id)
        .collect();
    assert_eq!(kept.len(), 63);
    assert!(!kept.contains(&ids[69]));
}

/// UC: UC-CNC-013, UC-CNC-014
#[test]
fn a_change_told_by_the_channel_is_applied_without_the_pending_one_and_the_late_commit_is_harmless()
{
    let membership = alone("a");
    // The node never saw the register: the change arrives already effected, at the next version.
    let applied = membership.learn("op1", 2, Change::Add(member("bb")), at(1));
    assert_eq!(applied.len(), 1);
    assert_eq!(applied[0].1, 2);
    assert_eq!(
        membership.ring().node_count(),
        2,
        "the ring gained the new one"
    );
    let table = membership.table().unwrap();
    assert_eq!(table.version, 2);
    assert!(membership.is_member("bb"));

    // The exclusion too: the node leaves the table, enters freezing and the ring loses it.
    let applied = membership.learn(
        "op2",
        3,
        Change::Exclude {
            node_id: "bb".into(),
        },
        at(2),
    );
    assert_eq!(applied.len(), 1);
    assert!(!membership.is_member("bb"));
    assert!(
        membership
            .frozen_remaining("bb", at(3), Duration::from_secs(300))
            .is_some()
    );
    assert_eq!(
        membership.ring().node_count(),
        1,
        "the ring lost the excluded one"
    );

    // The commit that arrives later, of the same operation, returns the table without applying twice.
    let table = membership.commit("op2", at(4)).unwrap();
    assert_eq!(table.version, 3);
}

/// UC: UC-CNC-013
#[test]
fn a_change_told_by_the_channel_clears_the_pending_one_it_stands_for() {
    let membership = alone("a");
    membership
        .register("op1", Change::Add(member("bb")), at(0))
        .unwrap();
    assert!(membership.has_pending(at(0)));
    membership.learn("op1", 2, Change::Add(member("bb")), at(1));
    assert!(
        !membership.has_pending(at(1)),
        "the register became the effected change"
    );
    assert_eq!(membership.table().unwrap().version, 2);
    assert_eq!(membership.commit("op1", at(2)).unwrap().version, 2);
}

/// UC: UC-CNC-013
#[test]
fn a_change_already_applied_or_older_is_ignored() {
    let membership = alone("a");
    membership.learn("op1", 2, Change::Add(member("bb")), at(1));
    assert!(
        membership
            .learn("op1", 2, Change::Add(member("bb")), at(2))
            .is_empty(),
        "the same version again"
    );
    assert!(
        membership
            .learn("op0", 1, Change::Add(member("cc")), at(2))
            .is_empty(),
        "an earlier version"
    );
    assert_eq!(membership.table().unwrap().version, 2);
    assert!(!membership.is_member("cc"));
    assert!(
        Membership::new(TTL, 16)
            .learn("op1", 2, Change::Add(member("bb")), at(0))
            .is_empty(),
        "without a table there is nothing to apply"
    );
}

/// UC: UC-CNC-015
#[test]
fn a_change_ahead_of_the_next_version_waits_for_the_ones_before_it() {
    let membership = alone("a");
    // Version 3 arrives before 2 (they came from different nodes' queues).
    assert!(
        membership
            .learn("op2", 3, Change::Add(member("cc")), at(1))
            .is_empty()
    );
    assert_eq!(membership.table().unwrap().version, 1);
    let applied = membership.learn("op1", 2, Change::Add(member("bb")), at(2));
    assert_eq!(
        applied.iter().map(|(_, v, _)| *v).collect::<Vec<_>>(),
        [2, 3],
        "applies both, in order"
    );
    assert!(membership.is_member("bb") && membership.is_member("cc"));

    // The normal commit advances the version while the next one waits: `learn_held` applies it.
    let membership = alone("a");
    membership.learn("op3", 4, Change::Add(member("dd")), at(1));
    membership.learn("op2", 3, Change::Add(member("cc")), at(1));
    membership
        .register("op1", Change::Add(member("bb")), at(1))
        .unwrap();
    membership.commit("op1", at(2)).unwrap();
    assert_eq!(membership.table().unwrap().version, 2);
    let applied = membership.learn_held(at(3));
    assert_eq!(applied.len(), 2);
    assert_eq!(membership.table().unwrap().version, 4);
}

/// UC: UC-CNC-015
#[test]
fn a_change_that_does_not_fit_the_table_is_dropped_and_the_held_ones_are_bounded() {
    let membership = alone("a");
    // Removing a non-member does not fit: it is discarded, without touching the table.
    let applied = membership.learn(
        "op1",
        2,
        Change::Remove {
            node_id: "zz".into(),
        },
        at(1),
    );
    assert!(applied.is_empty());
    assert_eq!(membership.table().unwrap().version, 1);

    // The held-ahead ones number at most 64, the closest to the table.
    for version in 3..=100u64 {
        membership.learn(
            &format!("op{version}"),
            version,
            Change::Add(member(&format!("n{version}"))),
            at(1),
        );
    }
    let applied = membership.learn("op2", 2, Change::Add(member("bb")), at(2));
    assert_eq!(
        applied.len(),
        1 + 63,
        "2 and the following 63 that fit in the 64 stored"
    );
}

/// The freezing of whoever learns the exclusion from the `membership` frame counts from the instant it
/// learned it (the frame does not carry the coordinator's instant) and ends at the deadline (D126).
/// UC: UC-CNC-013
#[test]
fn the_freezing_of_a_node_learned_by_the_membership_frame_counts_from_the_instant_it_was_learned() {
    let membership = alone("a");
    membership.learn("op1", 2, Change::Add(member("bb")), at(1));
    let freezing = Duration::from_secs(300);
    assert_eq!(membership.frozen_remaining("bb", at(2), freezing), None);

    membership.learn(
        "op2",
        3,
        Change::Exclude {
            node_id: "bb".into(),
        },
        at(10),
    );
    assert_eq!(
        membership.frozen_remaining("bb", at(10), freezing),
        Some(freezing),
        "the whole deadline, from when it learned"
    );
    assert_eq!(
        membership.frozen_remaining("bb", at(70), freezing),
        Some(Duration::from_secs(240))
    );
    assert_eq!(
        membership.frozen_remaining("bb", at(10 + 300), freezing),
        None,
        "ends at the deadline"
    );
    // A leave by drain (without a vote) leaves no freezing.
    membership.learn("op3", 4, Change::Add(member("cc")), at(11));
    membership.learn(
        "op4",
        5,
        Change::Remove {
            node_id: "cc".into(),
        },
        at(12),
    );
    assert_eq!(membership.frozen_remaining("cc", at(12), freezing), None);
}
