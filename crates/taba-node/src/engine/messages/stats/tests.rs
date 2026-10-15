use super::*;

const MINUTE: u64 = 60_000;

fn snapshot() -> TopicSnapshot {
    TopicSnapshot {
        counters: TopicCounters::new(),
        publish_rate: 0.0,
        consume_rate: 0.0,
        ack_rate: 0.0,
        visible: 0,
        invisible: 0,
        oldest_ms: None,
        memory_messages: 0,
        disk_messages: 0,
        memory_bytes: 0,
        disk_bytes: 0,
        groups: 0,
        largest_group: 0,
        busy_groups: 0,
        stalled_group: None,
        publish_latency: Percentiles::default(),
        consume_latency: Percentiles::default(),
    }
}

fn no_rules() -> StatsRules {
    StatsRules {
        top_groups: 10,
        backlog_depth: None,
        backlog_age_ms: None,
        noisy_group_share: 50,
        group_stalled_deliveries: None,
        stalled_after_ms: None,
        idle_after_ms: None,
        disk_free_min: None,
    }
}

/// UC: UC-OPS-028
#[test]
fn a_limit_that_was_not_set_turns_its_condition_off() {
    let mut s = snapshot();
    s.visible = 1_000_000;
    s.oldest_ms = Some(0);
    assert_eq!(evaluate(&s, &no_rules(), 100 * MINUTE, 0), []);
}

/// UC: UC-OPS-028
#[test]
fn backlog_fires_on_depth_or_on_age() {
    let rules = StatsRules {
        backlog_depth: Some(100),
        backlog_age_ms: Some(5 * MINUTE),
        ..no_rules()
    };
    let mut s = snapshot();
    s.visible = 101;
    assert_eq!(evaluate(&s, &rules, 0, 0), [Condition::Backlog]);

    s.visible = 3;
    s.oldest_ms = Some(10 * MINUTE);
    assert_eq!(evaluate(&s, &rules, 14 * MINUTE, 0), []);
    assert_eq!(evaluate(&s, &rules, 16 * MINUTE, 0), [Condition::Backlog]);
}

/// UC: UC-OPS-028
#[test]
fn stalled_needs_messages_and_no_consumption_for_the_whole_period() {
    let rules = StatsRules {
        stalled_after_ms: Some(MINUTE),
        ..no_rules()
    };
    let mut s = snapshot();
    assert_eq!(
        evaluate(&s, &rules, 10 * MINUTE, 0),
        [],
        "without messages there is nothing to consume"
    );
    s.invisible = 2;
    assert_eq!(evaluate(&s, &rules, 10 * MINUTE, 0), [Condition::Stalled]);
    s.counters.last_consumed_ms = Some(10 * MINUTE - 1_000);
    assert_eq!(evaluate(&s, &rules, 10 * MINUTE, 0), []);
    assert_eq!(
        evaluate(&s, &rules, 10 * MINUTE + 2 * MINUTE, 0),
        [Condition::Stalled]
    );
}

/// UC: UC-OPS-028
#[test]
fn idle_counts_from_the_last_publish_or_consume_or_the_baseline() {
    let rules = StatsRules {
        idle_after_ms: Some(10 * MINUTE),
        ..no_rules()
    };
    let mut s = snapshot();
    assert_eq!(evaluate(&s, &rules, 11 * MINUTE, 0), [Condition::Idle]);
    assert_eq!(
        evaluate(&s, &rules, 11 * MINUTE, 5 * MINUTE),
        [],
        "the node's start counts as activity"
    );
    s.counters.last_published_ms = Some(9 * MINUTE);
    assert_eq!(evaluate(&s, &rules, 11 * MINUTE, 0), []);
}

/// UC: UC-GRP-016, UC-OPS-028
#[test]
fn noisy_group_needs_two_groups_and_more_than_the_share() {
    let rules = no_rules();
    let mut s = snapshot();
    s.visible = 100;
    s.groups = 1;
    s.largest_group = 100;
    assert_eq!(
        evaluate(&s, &rules, 0, 0),
        [],
        "a single group is not noisy"
    );
    s.groups = 2;
    s.largest_group = 50;
    assert_eq!(evaluate(&s, &rules, 0, 0), [], "exactly 50% does not pass");
    s.largest_group = 51;
    assert_eq!(evaluate(&s, &rules, 0, 0), [Condition::NoisyGroup]);
    assert!((s.noisy_share() - 0.51).abs() < 1e-9);
}

/// UC: UC-OPS-028
#[test]
fn memory_pressure_is_most_of_the_topic_on_disk() {
    let mut s = snapshot();
    s.memory_messages = 10;
    s.disk_messages = 10;
    assert_eq!(evaluate(&s, &no_rules(), 0, 0), []);
    s.disk_messages = 11;
    assert_eq!(evaluate(&s, &no_rules(), 0, 0), [Condition::MemoryPressure]);
}

/// UC: UC-OPS-028
#[test]
fn group_stalled_follows_what_the_index_found() {
    let mut s = snapshot();
    s.stalled_group = Some("customer-1".into());
    assert_eq!(evaluate(&s, &no_rules(), 0, 0), [Condition::GroupStalled]);
}

/// UC: UC-OPS-028
#[test]
fn warnings_come_before_infos() {
    let rules = StatsRules {
        backlog_depth: Some(1),
        idle_after_ms: Some(1),
        ..no_rules()
    };
    let mut s = snapshot();
    s.visible = 10;
    s.memory_messages = 1;
    s.disk_messages = 9;
    let found = evaluate(&s, &rules, 10 * MINUTE, 0);
    assert_eq!(
        found,
        [
            Condition::Backlog,
            Condition::MemoryPressure,
            Condition::Idle
        ]
    );
    assert_eq!(found[0].severity(), Severity::Warning);
    assert_eq!(found[1].severity(), Severity::Info);
}

#[test]
fn condition_names_round_trip() {
    for condition in Condition::ALL {
        assert_eq!(Condition::parse(condition.as_str()), Some(condition));
    }
    assert_eq!(Condition::parse("ok"), None);
}

/// UC: UC-GRP-017
#[test]
fn the_wait_is_kept_per_band_and_the_last_band_is_15_or_more() {
    let counters = NodeCounters::new();
    counters.observe_wait(0, 0.002);
    counters.observe_wait(3, 4.0);
    counters.observe_wait(40, 4.0);
    counters.observe_wait(15, 4.0);
    let waits = counters.fair_wait();
    assert_eq!(waits.len(), BANDS);
    assert_eq!(waits[0].count(), 1);
    assert_eq!(waits[3].count(), 1);
    assert_eq!(waits[15].count(), 2, "40 and 15 fall in `15+`");
}
