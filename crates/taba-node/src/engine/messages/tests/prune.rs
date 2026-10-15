//! Emptying a queue on one node (`Messages::prune`, doc 34, D-TUI-2): what it removes, what it counts and
//! how a repeated `pruneId` answers.

use super::*;

const PRUNE_A: &str = "01968000-0000-7000-8000-000000000001";
const PRUNE_B: &str = "01968000-0000-7000-8000-000000000002";

/// UC: UC-TOP-044
#[tokio::test]
async fn pruning_removes_the_visible_and_the_invisible_messages_and_counts_them() {
    let h = harness();
    h.topics.create(queue("orders")).await.unwrap();
    for n in 0..5 {
        publish_ok(&h, "orders", &format!("{n}"));
    }
    // Two of them become invisible: a prune takes them too.
    let delivered = consume(&h, "orders", q(Some("2"), Some("300"), None)).await;
    assert_eq!(delivered.len(), 2);

    assert_eq!(h.messages.prune("orders", PRUNE_A).unwrap(), 5);

    let after = consume(&h, "orders", q(Some("10"), Some("300"), None)).await;
    assert!(after.is_empty(), "the queue is empty");
    let snapshot = h.messages.queue_view("orders").unwrap();
    assert_eq!(snapshot.definition.name, "orders", "the topic stays");
    // A message published afterwards is delivered normally.
    publish_ok(&h, "orders", "\"again\"");
    assert_eq!(
        consume(&h, "orders", q(Some("10"), Some("300"), None))
            .await
            .len(),
        1
    );
}

/// UC: UC-TOP-045
#[tokio::test]
async fn a_repeated_prune_id_answers_the_same_and_removes_nothing_new() {
    let h = harness();
    h.topics.create(queue("orders")).await.unwrap();
    publish_ok(&h, "orders", "1");
    publish_ok(&h, "orders", "2");
    assert_eq!(h.messages.prune("orders", PRUNE_A).unwrap(), 2);

    publish_ok(&h, "orders", "3");
    assert_eq!(
        h.messages.prune("orders", PRUNE_A).unwrap(),
        2,
        "the same pruneId: the first answer, nothing removed"
    );
    assert_eq!(
        consume(&h, "orders", q(Some("10"), Some("300"), None))
            .await
            .len(),
        1,
        "the message published after the first prune stays"
    );
    assert_eq!(
        h.messages.prune("orders", PRUNE_B).unwrap(),
        1,
        "a new pruneId prunes again: the delivered (invisible) message is still there"
    );
}

/// UC: UC-TOP-046
#[tokio::test]
async fn pruning_an_unknown_topic_is_404_and_a_distributor_is_a_wrong_type() {
    let h = harness();
    assert_eq!(
        h.messages.prune("missing", PRUNE_A).unwrap_err().code,
        ErrorCode::TopicNotFound
    );
    h.topics.create(queue("orders")).await.unwrap();
    h.topics
        .create(distributor("dist", &["orders"]))
        .await
        .unwrap();
    assert_eq!(
        h.messages.prune("dist", PRUNE_A).unwrap_err().code,
        ErrorCode::TopicWrongType
    );
}
