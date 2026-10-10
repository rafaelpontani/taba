use super::*;

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

    // Sem confirmação, o mesmo quadro sai de novo, com o mesmo seq.
    assert_eq!(
        queue.pending(10).iter().map(|f| f.seq).collect::<Vec<_>>(),
        [a, b, c]
    );
    assert_eq!(
        queue.pending(10).iter().map(|f| f.seq).collect::<Vec<_>>(),
        [a, b, c]
    );
    // A confirmação tira só o que confirma, mesmo fora de ordem; o confirmado não se repete.
    queue.confirm(&[b]);
    assert_eq!(
        queue.pending(10).iter().map(|f| f.seq).collect::<Vec<_>>(),
        [a, c]
    );
    assert!(queue.contains(a) && !queue.contains(b));
    queue.confirm(&[a, c, 99]);
    assert!(queue.is_empty());
    // Os números não voltam atrás.
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
    assert_eq!(dropped.coalesced, 1, "só o último estado de `a` importa");
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
        "os avisos não se substituem"
    );
    // O estado do nó também é um só.
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
    assert!(!by_size.overflowed(start), "no limite ainda cabe");
    by_size.push(notify("t3"), start);
    assert!(by_size.overflowed(start));

    let mut by_age = OutQueue::new(100, Duration::from_secs(30));
    by_age.push(ready("a", true), start);
    assert!(!by_age.overflowed(start + Duration::from_secs(30)));
    assert!(by_age.overflowed(start + Duration::from_secs(31)));
    // Esvaziada (o nó saiu da tabela), não estoura mais.
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
    assert_eq!(dropped.coalesced, 0, "outro nó não o substitui");
    let (_, dropped) = queue.push(frozen("x", 3), start);
    assert_eq!(dropped.coalesced, 1, "o mesmo nó o substitui");
    let dropped = queue.expire(start + NOTIFY_TTL + Duration::from_secs(60));
    assert_eq!(dropped.expired, 0, "não expira como um aviso");
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
    assert_eq!(dropped.coalesced, 0, "outra mudança não a substitui");
    let (_, dropped) = queue.push(membership("op1", 2), start);
    assert_eq!(dropped.coalesced, 0, "nem a mesma de novo");
    assert_eq!(queue.len(), 3);
    let dropped = queue.expire(start + NOTIFY_TTL + Duration::from_secs(3600));
    assert_eq!(dropped.expired, 0, "o tempo não a descarta");
    assert_eq!(queue.len(), 3);
}
