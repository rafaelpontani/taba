use serde_json::{Map, Value};

use super::*;
use crate::core::time::format_ms;

fn queue(visible: u64, invisible: u64, oldest_ms: Option<u64>) -> RawQueue {
    RawQueue {
        visible,
        invisible,
        oldest_ms,
        published: 10,
        ..RawQueue::default()
    }
}

fn node(node_id: &str, queue: RawQueue) -> NodeStats {
    NodeStats {
        node_id: node_id.into(),
        memory_total_bytes: 100,
        topics: vec![RawTopic {
            name: "orders".into(),
            kind: RawKind::Queue(Box::new(queue)),
        }],
    }
}

/// Each node says how many messages it is the **owner** of, so the cluster's view is the sum: the
/// replicas are not in the numbers, and the replication factor does not multiply them.
///
/// UC: UC-OPS-023
#[test]
fn the_cluster_view_adds_what_each_primary_counts_and_takes_the_oldest_date() {
    let a = queue(3, 1, Some(2_000));
    let b = queue(4, 2, Some(1_000));
    let c = queue(0, 0, None);

    let mut joined = a.clone();
    joined.join(&b);
    joined.join(&c);
    assert_eq!((joined.visible, joined.invisible), (7, 3));
    assert_eq!(joined.snapshot().depth(), 10, "depth = visible + invisible");
    assert_eq!(
        joined.oldest_ms,
        Some(1_000),
        "the oldest date is the smallest; a node without messages does not count"
    );
    assert_eq!(joined.published, 30, "the counters add up too");

    // The order the nodes answer in changes nothing.
    let mut reversed = c;
    reversed.join(&b);
    reversed.join(&a);
    assert_eq!(
        (reversed.visible, reversed.invisible, reversed.oldest_ms),
        (joined.visible, joined.invisible, joined.oldest_ms)
    );

    // The same through what the node that answers does with the answers of every member.
    let source = super::super::Source::of(
        vec![
            node("n1", a.clone()),
            node("n2", b.clone()),
            node("n3", queue(0, 0, None)),
        ],
        Vec::new(),
    );
    let seen = &source.queues["orders"];
    assert_eq!((seen.visible, seen.invisible, seen.oldest_ms), (7, 3, Some(1_000)));
    assert_eq!(source.memory_total, 300);
    assert!(!source.partial());

    // `oldestAge = now - oldestMessageAt`, in seconds.
    let mut out = Map::new();
    super::super::oldest_fields(&mut out, joined.oldest_ms, 11_000);
    assert_eq!(out["oldestMessageAt"], Value::from(format_ms(1_000)));
    assert_eq!(out["oldestAge"], Value::from(10.0));
    let mut empty = Map::new();
    super::super::oldest_fields(&mut empty, None, 11_000);
    assert!(empty.is_empty(), "no message, no oldest date and no age");
}
