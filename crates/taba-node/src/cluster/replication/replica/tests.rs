use std::sync::Arc;
use std::time::Duration;

use super::*;
use crate::cluster::protocol::MessageRecord;
use crate::config::{Config, MapEnv};
use crate::core::clock::ManualClock;
use crate::core::id;
use crate::engine::messages::MessageRules;
use crate::engine::topics::{CreateTopic, Kind, TopicRules};
use crate::storage::messages::MessageStore;
use crate::storage::topics::MemoryTopicStore;

struct Harness {
    replica: Arc<Replica>,
    messages: Arc<Messages>,
    _dir: tempfile::TempDir,
}

async fn harness() -> Harness {
    let config = Config::parse(None, &MapEnv::new([("TABA_REPLICATION_FACTOR", "1")])).unwrap();
    let clock = Arc::new(ManualClock::at_doc_epoch());
    let topics = Arc::new(Topics::new(
        TopicRules::from_config(&config),
        Arc::new(MemoryTopicStore::default()),
        clock.clone(),
        Vec::new(),
    ));
    topics
        .create(CreateTopic {
            name: "orders".into(),
            kind: Kind::Queue,
            tags: None,
            prefixes: None,
            volatile: None,
            idle_timeout: None,
            fifo: None,
            max_message_bytes: None,
            ttl: None,
            max_deliveries: None,
        })
        .await
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(MessageStore::new(dir.path(), config.storage.db_synchronous));
    let messages = Messages::new(
        topics.clone(),
        clock.clone(),
        MessageRules::from_config(&config),
        store,
    );
    let replica = Replica::new(
        messages.clone(),
        topics,
        clock,
        Arc::new(Telemetry::from_config(&config, "n")),
        Duration::from_secs(60),
    );
    Harness {
        replica,
        messages,
        _dir: dir,
    }
}

fn record(topic: &str, id: &str) -> MessageRecord {
    MessageRecord {
        id: id.to_owned(),
        topic: topic.to_owned(),
        body: b"hello".to_vec(),
        content_type: "text/plain".into(),
        group_id: None,
        deduplication_id: None,
        delivery_count: 0,
        invisible_until_ms: None,
        receipt_token: None,
        dead_lettered_ms: None,
        dead_letter_reason: None,
        delete_at_ms: None,
    }
}

fn register(operation_id: &str, topic: &str, message: &str) -> Step<MessageRegisterRequest> {
    Step::Register(MessageRegisterRequest {
        operation_id: operation_id.to_owned(),
        coordinator_id: "coordinator".into(),
        kind: WriteKind::Publish,
        message: record(topic, message),
        source_topic: None,
    })
}

fn commit(operation_id: &str) -> Step<MessageRegisterRequest> {
    Step::Commit(OperationRef {
        operation_id: operation_id.to_owned(),
    })
}

fn rollback(operation_id: &str) -> Step<MessageRegisterRequest> {
    Step::Rollback(OperationRef {
        operation_id: operation_id.to_owned(),
    })
}

fn expected(operation_id: &str) -> Ack {
    Ack {
        operation_id: operation_id.to_owned(),
        accepted: true,
    }
}

/// UC: UC-ESC-026
#[tokio::test]
async fn repeating_register_or_commit_returns_the_same_ack_without_a_new_effect() {
    let h = harness().await;
    let message = id::new_at(1_791_288_000_000);
    let op = id::new_at(1_791_288_000_001);
    let write = |step| h.replica.write("orders", &message, step);

    assert_eq!(
        write(register(&op, "orders", &message)).await.unwrap(),
        expected(&op)
    );
    assert_eq!(
        write(register(&op, "orders", &message)).await.unwrap(),
        expected(&op),
        "the repeated register confirms again"
    );
    assert_eq!(
        h.replica.pending(),
        1,
        "and does not open a second operation"
    );
    assert!(
        h.messages
            .message_info("orders", &message)
            .unwrap()
            .is_none(),
        "nothing is visible before the commit"
    );

    assert_eq!(write(commit(&op)).await.unwrap(), expected(&op));
    assert_eq!(write(commit(&op)).await.unwrap(), expected(&op));
    assert_eq!(h.replica.pending(), 0);
    assert!(
        h.messages
            .message_info("orders", &message)
            .unwrap()
            .is_some()
    );

    // Proof that the repeated commit applies nothing: the message goes away and a new commit or
    // register of the same `operationId` does not bring it back.
    h.messages.apply_ack("orders", &message).unwrap();
    assert!(
        h.messages
            .message_info("orders", &message)
            .unwrap()
            .is_none()
    );
    assert_eq!(write(commit(&op)).await.unwrap(), expected(&op));
    assert_eq!(
        write(register(&op, "orders", &message)).await.unwrap(),
        expected(&op),
        "a register after the commit is still the same Ack"
    );
    assert_eq!(h.replica.pending(), 0);
    assert!(
        h.messages
            .message_info("orders", &message)
            .unwrap()
            .is_none()
    );
}

/// UC: UC-ESC-026
#[tokio::test]
async fn repeating_rollback_returns_the_same_ack_and_the_message_never_appears() {
    let h = harness().await;
    let message = id::new_at(1_791_288_000_010);
    let op = id::new_at(1_791_288_000_011);
    let write = |step| h.replica.write("orders", &message, step);

    write(register(&op, "orders", &message)).await.unwrap();
    assert_eq!(h.replica.pending(), 1);
    assert_eq!(write(rollback(&op)).await.unwrap(), expected(&op));
    assert_eq!(
        write(rollback(&op)).await.unwrap(),
        expected(&op),
        "the repeated rollback confirms again"
    );
    assert_eq!(h.replica.pending(), 0);
    assert_eq!(
        write(register(&op, "orders", &message)).await.unwrap(),
        expected(&op),
        "the register of a decided operation opens nothing"
    );
    assert_eq!(h.replica.pending(), 0);
    assert!(
        h.messages
            .message_info("orders", &message)
            .unwrap()
            .is_none()
    );
}
