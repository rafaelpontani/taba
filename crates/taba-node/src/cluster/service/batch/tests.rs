use super::*;
use crate::cluster::members::MemberState;
use crate::cluster::protocol::{OperationRef, Request, Step};

fn member() -> Member {
    Member {
        node_id: "01900000-0000-7000-8000-000000000001".into(),
        addresses: vec!["10.0.0.1".into()],
        port: 8081,
        state: MemberState::Active,
    }
}

fn commit(n: u32) -> Request {
    Request::Write {
        topic: "orders".into(),
        id: "01900000-0000-7000-8000-000000000002".into(),
        step: Step::Commit(OperationRef {
            operation_id: format!("op-{n}"),
        }),
    }
}

/// A queue with `sizes.len()` items; the receivers are returned so the callers stay alive.
fn queue_of(
    sizes: &[usize],
) -> (
    NodeQueue,
    Vec<oneshot::Receiver<Result<Response, CallError>>>,
) {
    let mut receivers = Vec::new();
    let mut items = VecDeque::new();
    for (n, size) in sizes.iter().enumerate() {
        let (reply, receiver) = oneshot::channel();
        receivers.push(receiver);
        items.push_back(Pending {
            request: commit(n as u32),
            traceparent: None,
            operation_id: format!("op-{n}"),
            size: *size,
            queued: Instant::now(),
            reply: Reply::One(reply),
        });
    }
    (
        NodeQueue {
            state: Mutex::new(QueueState {
                member: member(),
                items,
            }),
            wake: Notify::new(),
        },
        receivers,
    )
}

fn ids(items: &[Pending]) -> Vec<&str> {
    items.iter().map(|i| i.operation_id.as_str()).collect()
}

/// UC: UC-ESC-040
#[test]
fn a_batch_that_reaches_neither_limit_leaves_with_everything_that_was_waiting() {
    let (queue, _callers) = queue_of(&[600, 600, 600]);
    let (_, taken) = take(&queue, 1000, 8 << 20);
    assert_eq!(ids(&taken), ["op-0", "op-1", "op-2"]);
    assert!(!has_items(&queue));
}

/// UC: UC-ESC-040
#[test]
fn the_item_limit_cuts_the_batch_and_the_rest_waits_for_the_next_slot() {
    let (queue, _callers) = queue_of(&[600; 5]);
    let (_, first) = take(&queue, 2, 8 << 20);
    assert_eq!(ids(&first), ["op-0", "op-1"]);
    assert!(has_items(&queue), "the rest stays in the queue");
    let (_, second) = take(&queue, 2, 8 << 20);
    assert_eq!(
        ids(&second),
        ["op-2", "op-3"],
        "in order, with none skipped"
    );
}

/// UC: UC-ESC-040
#[test]
fn the_byte_limit_is_soft_an_item_above_it_goes_alone_and_never_stays_behind() {
    // The first item always goes, however large; the next one only if it still fits.
    let (queue, _callers) = queue_of(&[5_000, 100, 100, 6_000, 100]);
    let (_, first) = take(&queue, 1000, 1_000);
    assert_eq!(ids(&first), ["op-0"], "larger than the limit: alone");
    let (_, second) = take(&queue, 1000, 1_000);
    assert_eq!(ids(&second), ["op-1", "op-2"], "stops before the large one");
    let (_, third) = take(&queue, 1000, 1_000);
    assert_eq!(ids(&third), ["op-3"], "the large one leaves alone");
    let (_, fourth) = take(&queue, 1000, 1_000);
    assert_eq!(ids(&fourth), ["op-4"]);
}

/// UC: UC-ESC-042
#[test]
fn a_group_is_answered_once_in_its_order_even_when_its_items_leave_in_different_batches() {
    let (reply, mut answered) = oneshot::channel();
    let group = Arc::new(Group {
        state: Mutex::new(GroupState {
            results: vec![None, None, None],
            missing: 3,
            reply: Some(reply),
        }),
    });
    let items = (0..3)
        .map(|n| Pending {
            request: commit(n),
            traceparent: None,
            operation_id: format!("op-{n}"),
            size: 600,
            queued: Instant::now(),
            reply: Reply::Slot(group.clone(), n as usize),
        })
        .collect();
    let queue = NodeQueue {
        state: Mutex::new(QueueState {
            member: member(),
            items,
        }),
        wake: Notify::new(),
    };
    let ack = |pending: Pending| {
        let operation_id = pending.operation_id.clone();
        pending
            .reply
            .send(Ok(Response::Ack(crate::cluster::protocol::Ack {
                operation_id,
                accepted: true,
            })));
    };
    let (_, first) = take(&queue, 2, 8 << 20);
    let (_, second) = take(&queue, 2, 8 << 20);
    assert_eq!(ids(&first), ["op-0", "op-1"]);
    assert_eq!(ids(&second), ["op-2"]);
    // The second batch comes back first: the group still waits for the rest.
    second.into_iter().for_each(ack);
    assert!(
        answered.try_recv().is_err(),
        "answered before its last item"
    );
    first.into_iter().for_each(ack);
    let results = answered.try_recv().expect("the group is answered");
    let order: Vec<String> = results
        .into_iter()
        .map(|result| match result {
            Ok(Response::Ack(ack)) => ack.operation_id,
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(order, ["op-0", "op-1", "op-2"]);
}

/// UC: UC-ESC-040
#[test]
fn an_operation_whose_caller_gave_up_is_not_sent() {
    let (queue, mut callers) = queue_of(&[600, 600, 600]);
    drop(callers.remove(1));
    let (_, taken) = take(&queue, 1000, 8 << 20);
    assert_eq!(ids(&taken), ["op-0", "op-2"]);
}
