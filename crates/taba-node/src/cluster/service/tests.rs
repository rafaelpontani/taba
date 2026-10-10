use std::time::Duration;

use super::*;
use std::collections::BTreeMap;

use crate::config::MapEnv;

fn table(ids: &[&str]) -> MemberTable {
    MemberTable {
        version: 7,
        members: ids
            .iter()
            .map(|id| Member {
                node_id: (*id).to_owned(),
                addresses: vec!["10.0.0.1".into()],
                port: 8081,
                state: MemberState::Active,
            })
            .collect(),
        ..MemberTable::default()
    }
}

fn rules_with(extra: &[(&str, &str)]) -> ClusterRules {
    let mut env = BTreeMap::from([(
        "TABA_CLUSTER_ENTRY_ADDRESS".to_owned(),
        "taba-lb.interno:8081".to_owned(),
    )]);
    env.extend(
        extra
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned())),
    );
    ClusterRules::from_config(&Config::parse(None, &MapEnv(env)).unwrap())
}

/// UC: UC-FAL-031
#[test]
fn the_reunion_has_exactly_one_losing_side_the_smaller_and_in_a_tie_the_one_of_the_lowest_id() {
    // Menos membros perde, e só ele.
    let small = table(&["a", "b"]);
    let large = table(&["x", "y", "z"]);
    assert!(loses_reunion(&small, &large));
    assert!(!loses_reunion(&large, &small));

    // Empate: perde o lado do menor `nodeId`, e os dois lados chegam à mesma conclusão.
    let left = table(&["b", "d"]);
    let right = table(&["c", "e"]);
    assert!(loses_reunion(&left, &right), "o lado de `b` perde");
    assert!(!loses_reunion(&right, &left), "o lado de `c` fica");

    // Em qualquer par de tabelas diferentes há um perdedor, nunca dois nem nenhum.
    for (a, b) in [(&small, &large), (&left, &right), (&left, &large)] {
        assert_ne!(loses_reunion(a, b), loses_reunion(b, a));
    }
}

/// UC: UC-FAL-033
#[test]
fn the_reunion_and_recovery_parameters_default_to_what_was_decided_and_can_be_changed() {
    let rules = rules_with(&[]);
    assert_eq!(
        rules.reunion_interval,
        (rules.keepalive_interval * 10).max(Duration::from_secs(1))
    );
    assert_eq!(rules.message_loss_ttl, Duration::from_secs(14 * 24 * 3600));
    assert_eq!(rules.recovery_timeout, Duration::from_secs(300));

    // Com um sinal de vida curto, o reencontro nunca passa a rodar em menos de 1 s.
    let fast = rules_with(&[("TABA_CLUSTER_CHANNEL_KEEPALIVE_INTERVAL", "50ms")]);
    assert_eq!(fast.reunion_interval, Duration::from_secs(1));

    let changed = rules_with(&[
        ("TABA_CLUSTER_REUNION_INTERVAL", "7s"),
        ("TABA_CLUSTER_MESSAGE_LOSS_TTL", "2d"),
        ("TABA_CLUSTER_RECOVERY_TIMEOUT", "90s"),
    ]);
    assert_eq!(changed.reunion_interval, Duration::from_secs(7));
    assert_eq!(changed.message_loss_ttl, Duration::from_secs(2 * 24 * 3600));
    assert_eq!(changed.recovery_timeout, Duration::from_secs(90));
}
