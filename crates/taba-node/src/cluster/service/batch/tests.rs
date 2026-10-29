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
            reply,
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

/// UC: UC-ESC-040
#[test]
fn an_operation_whose_caller_gave_up_is_not_sent() {
    let (queue, mut callers) = queue_of(&[600, 600, 600]);
    drop(callers.remove(1));
    let (_, taken) = take(&queue, 1000, 8 << 20);
    assert_eq!(ids(&taken), ["op-0", "op-2"]);
}
