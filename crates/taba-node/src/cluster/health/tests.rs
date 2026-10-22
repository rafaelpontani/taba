use super::*;

fn signals() -> HealthSignals {
    HealthSignals {
        members_version: 3,
        quorum_available: true,
        channels: BTreeMap::from([("a".to_owned(), true), ("b".to_owned(), true)]),
        ..HealthSignals::default()
    }
}

#[test]
fn a_healthy_node_shows_no_condition() {
    assert!(signals().conditions(false).is_empty());
}

#[test]
fn each_signal_raises_its_condition_with_the_severity_of_the_document() {
    let mut s = signals();
    s.channels.insert("b".into(), false);
    assert_eq!(s.conditions(false), [Condition::MemberUnreachable]);

    let mut s = signals();
    s.frozen_nodes = 1;
    s.syncing_blocks = 4;
    s.blocks_below_replication = 4;
    assert_eq!(
        s.conditions(true),
        [
            Condition::NodeFrozen,
            Condition::Syncing,
            Condition::UnderReplicated,
            Condition::StoragePressure
        ]
    );

    let mut s = signals();
    s.quorum_available = false;
    s.blocks_below_replication = 9;
    s.blocks_below_quorum = 9;
    // Blocks below the quorum are `write-quorum-at-risk`, not `under-replicated`.
    assert_eq!(
        s.conditions(false),
        [Condition::WriteQuorumAtRisk, Condition::QuorumLost]
    );
    assert_eq!(Condition::QuorumLost.severity(), Severity::Critical);
    assert_eq!(Condition::WriteQuorumAtRisk.severity(), Severity::Critical);
    assert_eq!(
        Condition::MembershipDivergent.severity(),
        Severity::Critical
    );
    assert_eq!(Condition::UnderReplicated.severity(), Severity::Warning);
    assert_eq!(Condition::NodeFrozen.severity(), Severity::Warning);
}

#[test]
fn blocks_below_the_replication_but_not_below_the_quorum_are_only_under_replicated() {
    let mut s = signals();
    s.blocks_below_replication = 5;
    s.blocks_below_quorum = 2;
    assert_eq!(
        s.conditions(false),
        [Condition::WriteQuorumAtRisk, Condition::UnderReplicated]
    );
}

#[test]
fn the_condition_entry_carries_the_name_and_the_severity() {
    assert_eq!(
        Condition::QuorumLost.entry(),
        json!({ "condition": "quorum-lost", "severity": "critical" })
    );
}

/// UC: UC-FAL-024
#[test]
fn merging_nodes_unions_the_conditions_and_flags_divergent_tables() {
    let same = merge(
        [
            (7, vec![Condition::Syncing]),
            (7, vec![Condition::Syncing, Condition::NodeFrozen]),
        ],
        false,
    );
    assert_eq!(same, [Condition::NodeFrozen, Condition::Syncing]);

    let divergent = merge([(7, vec![]), (8, vec![])], false);
    assert_eq!(divergent, [Condition::MembershipDivergent]);

    let silent = merge([(7, vec![])], true);
    assert_eq!(silent, [Condition::MemberUnreachable]);

    assert!(merge([(7, vec![]), (7, vec![])], false).is_empty());
}

/// UC: UC-OPS-053
#[test]
fn a_restoration_in_progress_raises_previous_life_pending_as_a_warning() {
    let mut s = signals();
    s.restore_pending = true;
    assert_eq!(s.conditions(false), [Condition::PreviousLifePending]);
    assert_eq!(Condition::PreviousLifePending.severity(), Severity::Warning);
    assert_eq!(
        Condition::PreviousLifePending.entry(),
        json!({ "condition": "previous-life-pending", "severity": "warning" })
    );
    assert!(Condition::ALL.contains(&Condition::PreviousLifePending));
}

/// UC: UC-ING-031
#[test]
fn a_stalled_upgrade_raises_its_condition_as_a_warning() {
    let mut s = signals();
    s.upgrade_stalled = true;
    assert_eq!(s.conditions(false), [Condition::UpgradeStalled]);
    assert_eq!(Condition::UpgradeStalled.severity(), Severity::Warning);
    assert_eq!(
        Condition::UpgradeStalled.entry(),
        json!({ "condition": "upgrade-stalled", "severity": "warning" })
    );
}

/// UC: UC-OPS-054
#[test]
fn a_probe_that_ended_with_nodes_missing_raises_members_missing_as_a_warning() {
    let mut s = signals();
    s.members_missing = true;
    assert_eq!(s.conditions(false), [Condition::MembersMissing]);
    assert_eq!(Condition::MembersMissing.severity(), Severity::Warning);
    assert_eq!(
        Condition::MembersMissing.entry(),
        json!({ "condition": "members-missing", "severity": "warning" })
    );
    assert!(Condition::ALL.contains(&Condition::MembersMissing));
}

/// UC: UC-OPS-055
#[test]
fn the_reduced_profile_is_an_informative_warning() {
    let mut s = signals();
    s.reduced_profile = true;
    assert_eq!(s.conditions(false), [Condition::ReducedProfile]);
    assert_eq!(
        Condition::ReducedProfile.entry(),
        json!({ "condition": "reduced-profile", "severity": "warning" })
    );
    assert!(Condition::ALL.contains(&Condition::ReducedProfile));
}
