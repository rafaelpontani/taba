use std::time::Duration;

use super::*;
use crate::config::{Config, DbSynchronous, MapEnv};
use crate::core::clock::ManualClock;
use crate::engine::messages::{MessageRules, PublishRequest};
use crate::engine::topics::{CreateTopic, Kind, TopicRules, Topics};
use crate::storage::messages::MessageStore;
use crate::storage::topics::MemoryTopicStore;

struct Harness {
    controller: Arc<DrainController>,
    messages: Arc<Messages>,
    shared: Arc<NodeShared>,
    _dir: tempfile::TempDir,
}

async fn harness(timeout: Duration) -> Harness {
    let config = Config::parse(None, &MapEnv::new([("TABA_REPLICATION_FACTOR", "1")])).unwrap();
    let clock = Arc::new(ManualClock::at_doc_epoch());
    let topics = Arc::new(Topics::new(
        TopicRules::from_config(&config),
        Arc::new(MemoryTopicStore::default()),
        clock.clone(),
        Vec::new(),
    ));
    let dir = tempfile::tempdir().unwrap();
    let messages = Messages::new(
        topics.clone(),
        clock.clone(),
        MessageRules::from_config(&config),
        Arc::new(MessageStore::new(
            &dir.path().join("msgs"),
            DbSynchronous::Normal,
        )),
    );
    topics
        .create(CreateTopic {
            name: "pedidos".into(),
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
    for body in ["\"a\"", "\"b\""] {
        messages
            .publish(
                "pedidos",
                PublishRequest {
                    body: body.as_bytes().to_vec(),
                    ..Default::default()
                },
            )
            .unwrap();
    }
    let shared = Arc::new(NodeShared::new("no".into(), SystemTime::UNIX_EPOCH));
    let controller = DrainController::new(shared.clone(), messages.clone(), clock, timeout);
    Harness {
        controller,
        messages,
        shared,
        _dir: dir,
    }
}

async fn wait_for(h: &Harness, state: DrainState) {
    for _ in 0..200 {
        if h.shared.drain_state() == state {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("the drain did not reach {state:?}");
}

/// UC: UC-DRN-001, UC-DRN-004, UC-DRN-005, UC-DRN-010
#[tokio::test]
async fn the_api_drain_moves_memory_to_disk_and_ends_drained() {
    let h = harness(Duration::from_secs(25)).await;
    assert_eq!(h.controller.status().state, DrainState::Active);
    assert_eq!(h.controller.status().trigger, None);
    assert_eq!(h.controller.status().pending_messages, 2);

    let Started::Started(status) = h.controller.start_api() else {
        panic!("esperava Started");
    };
    assert_eq!(status.trigger, Some(Trigger::Api));
    assert!(status.started_at.is_some());
    wait_for(&h, DrainState::Drained).await;

    let status = h.controller.status();
    assert_eq!(status.pending_messages, 0, "reaches 0 when drained");
    assert_eq!(status.trigger, Some(Trigger::Api));
    assert_eq!(h.messages.usage("pedidos"), (0, 2));
}

/// UC: UC-DRN-004
#[tokio::test]
async fn starting_a_drain_twice_changes_nothing() {
    let h = harness(Duration::from_secs(25)).await;
    assert!(matches!(h.controller.start_api(), Started::Started(_)));
    assert!(matches!(
        h.controller.start_api(),
        Started::AlreadyDraining(_)
    ));
    wait_for(&h, DrainState::Drained).await;
    assert!(matches!(
        h.controller.start_api(),
        Started::AlreadyDraining(status) if status.state == DrainState::Drained
    ));
}

/// UC: UC-DRN-019
#[tokio::test]
async fn canceling_without_a_drain_has_nothing_to_cancel() {
    let h = harness(Duration::from_secs(25)).await;
    let Cancelled::Nothing(status) = h.controller.cancel().unwrap() else {
        panic!("esperava Nothing");
    };
    assert_eq!(status.state, DrainState::Active);
}

/// UC: UC-DRN-016
#[tokio::test]
async fn canceling_while_draining_stops_the_move() {
    let h = harness(Duration::from_secs(25)).await;
    // Holds the drain at the start: the node enters `draining` without the task that empties.
    let stop = h.controller.begin(Trigger::Api).unwrap();
    assert_eq!(h.shared.drain_state(), DrainState::Draining);

    let Cancelled::Cancelled(status) = h.controller.cancel().unwrap() else {
        panic!("esperava Cancelled");
    };
    assert_eq!(status.state, DrainState::Active);
    assert!(
        stop.load(Ordering::Relaxed),
        "the drain in progress was notified"
    );

    // The task that wakes up after the cancellation concludes nothing.
    h.controller.clone().run_drain(stop).await;
    assert_eq!(h.shared.drain_state(), DrainState::Active);
    assert!(!h.messages.is_draining(), "goes back to writing in memory");
}

/// UC: UC-DRN-018
#[tokio::test]
async fn leaving_the_drained_state_returns_as_a_new_node() {
    let h = harness(Duration::from_secs(25)).await;
    h.controller.start_api();
    wait_for(&h, DrainState::Drained).await;
    assert!(matches!(
        h.controller.cancel().unwrap(),
        Cancelled::Returned(status) if status.state == DrainState::Active
    ));
}

/// UC: UC-DRN-002, UC-DRN-012
#[tokio::test]
async fn the_signal_drain_waits_for_the_save_and_marks_the_trigger() {
    let h = harness(Duration::from_secs(25)).await;
    assert!(h.controller.stop_by_signal().await);
    let status = h.controller.status();
    assert_eq!(status.state, DrainState::Drained);
    assert_eq!(status.trigger, Some(Trigger::Signal));
    assert_eq!(h.messages.usage("pedidos"), (0, 2));
}

/// UC: UC-DRN-002
#[tokio::test]
async fn the_signal_waits_for_a_drain_the_api_already_started() {
    let h = harness(Duration::from_secs(25)).await;
    h.controller.start_api();
    assert!(h.controller.stop_by_signal().await);
    assert_eq!(h.controller.status().trigger, Some(Trigger::Api));
}

/// UC: UC-DRN-017
#[tokio::test]
async fn with_the_process_stopping_by_signal_cancel_is_a_conflict() {
    let h = harness(Duration::from_secs(25)).await;
    h.controller.stop_by_signal().await;
    let error = h.controller.cancel().unwrap_err();
    assert_eq!(error.code, ErrorCode::DrainConflict);
}

/// UC: UC-DRN-014
#[tokio::test]
async fn an_expired_timeout_leaves_the_node_draining() {
    let h = harness(Duration::ZERO).await;
    assert!(!h.controller.stop_by_signal().await);
    assert_eq!(h.shared.drain_state(), DrainState::Draining);
    assert_eq!(h.messages.pending(), 2, "what was left stays in memory");
}

/// UC: UC-DRN-007
#[tokio::test]
async fn a_long_poll_waiting_when_the_drain_starts_answers_empty() {
    let h = harness(Duration::from_secs(25)).await;
    // Empties the queue so the `GET` stays waiting.
    h.messages
        .consume(
            "pedidos",
            crate::engine::messages::ConsumeQuery {
                max_messages: Some("10".into()),
                visibility_timeout: Some("600".into()),
                wait_time: None,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let messages = h.messages.clone();
    let poll = tokio::spawn(async move {
        messages
            .consume(
                "pedidos",
                crate::engine::messages::ConsumeQuery {
                    max_messages: None,
                    visibility_timeout: None,
                    wait_time: Some("20".into()),
                    ..Default::default()
                },
            )
            .await
    });
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(!poll.is_finished(), "still waiting for a message");

    h.controller.start_api();
    let result = tokio::time::timeout(Duration::from_secs(5), poll)
        .await
        .expect("o drain acorda o long polling")
        .unwrap()
        .unwrap();
    assert!(result.is_empty());
}
