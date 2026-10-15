use super::*;
use crate::cluster::protocol::DefinitionKind;

fn ready(topic: &str, has: bool) -> Signal {
    Signal::Ready {
        topic: topic.into(),
        has_ready: has,
    }
}

fn notify(topic: &str) -> Signal {
    Signal::Notify {
        topic: topic.into(),
        count: 1,
    }
}

/// UC: UC-CAN-009, UC-CAN-010
#[test]
fn frames_have_increasing_seq_and_leave_the_queue_only_when_confirmed() {
    let mut queue = OutQueue::new(100, Duration::from_secs(60));
    let now = Instant::now();
    let (a, _) = queue.push(ready("a", true), now);
    let (b, _) = queue.push(ready("b", true), now);
    let (c, _) = queue.push(notify("a"), now);
    assert!(a < b && b < c, "o seq cresce");

    // Without confirmation, the same frame goes out again, with the same seq.
    assert_eq!(
        queue.pending(10).iter().map(|f| f.seq).collect::<Vec<_>>(),
        [a, b, c]
    );
    assert_eq!(
        queue.pending(10).iter().map(|f| f.seq).collect::<Vec<_>>(),
        [a, b, c]
    );
    // The confirmation only takes what it confirms, even out of order; what was confirmed is not repeated.
    queue.confirm(&[b]);
    assert_eq!(
        queue.pending(10).iter().map(|f| f.seq).collect::<Vec<_>>(),
        [a, c]
    );
    assert!(queue.contains(a) && !queue.contains(b));
    queue.confirm(&[a, c, 99]);
    assert!(queue.is_empty());
    // The numbers do not go back.
    let (d, _) = queue.push(notify("z"), now);
    assert!(d > c);
}

/// UC: UC-CAN-019
#[test]
fn state_signals_of_the_same_topic_replace_each_other_and_notices_do_not() {
    let mut queue = OutQueue::new(100, Duration::from_secs(60));
    let now = Instant::now();
    queue.push(ready("a", true), now);
    queue.push(ready("b", true), now);
    queue.push(notify("a"), now);
    queue.push(notify("a"), now);
    let (_, dropped) = queue.push(ready("a", false), now);
    assert_eq!(dropped.coalesced, 1, "only the last state of `a` matters");
    let pending = queue.pending(10);
    assert_eq!(pending.len(), 4);
    assert!(pending.iter().any(|f| f.signal == ready("a", false)));
    assert!(!pending.iter().any(|f| f.signal == ready("a", true)));
    assert_eq!(
        pending
            .iter()
            .filter(|f| matches!(f.signal, Signal::Notify { .. }))
            .count(),
        2,
        "the notices do not replace each other"
    );
    // The node's state is also a single one.
    queue.push(
        Signal::NodeState {
            state: "draining".into(),
        },
        now,
    );
    let (_, dropped) = queue.push(
        Signal::NodeState {
            state: "active".into(),
        },
        now,
    );
    assert_eq!(dropped.coalesced, 1);
}

/// UC: UC-CAN-019
#[test]
fn old_notices_are_dropped_and_old_states_are_kept() {
    let mut queue = OutQueue::new(100, Duration::from_secs(600));
    let start = Instant::now();
    queue.push(notify("a"), start);
    queue.push(ready("a", true), start);
    let dropped = queue.expire(start + NOTIFY_TTL + Duration::from_secs(1));
    assert_eq!(dropped.expired, 1);
    assert_eq!(queue.len(), 1);
    assert!(matches!(queue.pending(1)[0].signal, Signal::Ready { .. }));
}

/// UC: UC-CAN-020
#[test]
fn the_queue_overflows_by_size_or_by_age() {
    let start = Instant::now();
    let mut by_size = OutQueue::new(3, Duration::from_secs(600));
    for n in 0..3 {
        by_size.push(notify(&format!("t{n}")), start);
    }
    assert!(!by_size.overflowed(start), "at the limit it still fits");
    by_size.push(notify("t3"), start);
    assert!(by_size.overflowed(start));

    let mut by_age = OutQueue::new(100, Duration::from_secs(30));
    by_age.push(ready("a", true), start);
    assert!(!by_age.overflowed(start + Duration::from_secs(30)));
    assert!(by_age.overflowed(start + Duration::from_secs(31)));
    // Emptied (the node left the table), it no longer overflows.
    assert_eq!(by_age.clear(), 1);
    assert!(!by_age.overflowed(start + Duration::from_secs(900)));
}

fn frozen(node_id: &str, secs: u64) -> Signal {
    Signal::Frozen {
        node_id: node_id.into(),
        frozen_at: secs,
    }
}

/// UC: UC-FRZ-013
#[test]
fn the_frozen_signal_never_expires_and_only_the_same_node_replaces_it() {
    let mut queue = OutQueue::new(100, Duration::from_secs(600));
    let start = Instant::now();
    queue.push(frozen("x", 1), start);
    let (_, dropped) = queue.push(frozen("y", 2), start);
    assert_eq!(dropped.coalesced, 0, "another node does not replace it");
    let (_, dropped) = queue.push(frozen("x", 3), start);
    assert_eq!(dropped.coalesced, 1, "the same node replaces it");
    let dropped = queue.expire(start + NOTIFY_TTL + Duration::from_secs(60));
    assert_eq!(dropped.expired, 0, "it does not expire like a notice");
    assert_eq!(queue.len(), 2);
}

fn membership(operation_id: &str, version: u64) -> Signal {
    Signal::Membership {
        operation_id: operation_id.into(),
        version,
        change: crate::cluster::members::Change::Exclude {
            node_id: "x".into(),
        },
    }
}

/// UC: UC-CAN-027
#[test]
fn a_membership_change_is_never_replaced_and_never_expires() {
    let mut queue = OutQueue::new(100, Duration::from_secs(600));
    let start = Instant::now();
    queue.push(membership("op1", 2), start);
    let (_, dropped) = queue.push(membership("op2", 3), start);
    assert_eq!(dropped.coalesced, 0, "another change does not replace it");
    let (_, dropped) = queue.push(membership("op1", 2), start);
    assert_eq!(dropped.coalesced, 0, "nor the same one again");
    assert_eq!(queue.len(), 3);
    let dropped = queue.expire(start + NOTIFY_TTL + Duration::from_secs(3600));
    assert_eq!(dropped.expired, 0, "time does not discard it");
    assert_eq!(queue.len(), 3);
}

fn definition(kind: DefinitionKind, key: &str, version: u64) -> Signal {
    Signal::Definition {
        kind,
        key: key.into(),
        version,
        deleted: false,
        body: serde_json::Value::Null,
    }
}

/// UC: UC-CAN-028
#[test]
fn a_definition_frame_is_replaced_by_the_newer_of_the_same_definition_and_never_expires() {
    let mut queue = OutQueue::new(100, Duration::from_secs(600));
    let start = Instant::now();
    queue.push(definition(DefinitionKind::Topic, "pedidos", 1), start);
    queue.push(definition(DefinitionKind::Topic, "pagamentos", 1), start);
    queue.push(definition(DefinitionKind::Token, "pedidos", 1), start);
    assert_eq!(
        queue.len(),
        3,
        "another name or another type does not replace it"
    );

    let (_, dropped) = queue.push(definition(DefinitionKind::Topic, "pedidos", 2), start);
    assert_eq!(
        dropped.coalesced, 1,
        "the same definition, only the last stays"
    );
    assert_eq!(queue.len(), 3);
    let pending = queue.pending(10);
    assert!(pending.iter().any(|f| matches!(
        &f.signal,
        Signal::Definition { kind: DefinitionKind::Topic, key, version: 2, .. } if key == "pedidos"
    )));
    assert!(!pending.iter().any(|f| matches!(
        &f.signal,
        Signal::Definition { version: 1, kind: DefinitionKind::Topic, key, .. } if key == "pedidos"
    )));

    let dropped = queue.expire(start + NOTIFY_TTL + Duration::from_secs(3600));
    assert_eq!(dropped.expired, 0, "time does not discard it");
    assert_eq!(queue.len(), 3);
}
