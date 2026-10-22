//! What the engine gives to the restoration of the past lives (doc 07, sections 10.3 and 10.6).

use super::*;

/// UC: UC-RTN-027
#[tokio::test]
async fn the_topic_of_a_message_of_a_life_is_the_same_gone_or_recreated() {
    let h = harness();
    h.topics.create(queue("orders")).await.unwrap();
    let id = h.messages.current_topic_id("orders").unwrap();
    assert_eq!(
        h.messages.check_topic("orders", Some(&id)),
        TopicCheck::Alive
    );
    // A folder from before the record proves nothing: the topic counts as the same.
    assert_eq!(h.messages.check_topic("orders", None), TopicCheck::Alive);
    // A topic that does not exist, and one that is not a queue, hold no messages.
    assert_eq!(h.messages.check_topic("ghosts", None), TopicCheck::Gone);
    h.topics
        .create(distributor("events", &["a"]))
        .await
        .unwrap();
    assert_eq!(h.messages.check_topic("events", None), TopicCheck::Gone);

    // Deleted and created again with the same name: it has another `topicId`.
    h.topics.delete("orders").await.unwrap();
    assert_eq!(
        h.messages.check_topic("orders", Some(&id)),
        TopicCheck::Gone
    );
    h.topics.create(queue("orders")).await.unwrap();
    assert_ne!(h.messages.current_topic_id("orders").unwrap(), id);
    assert_eq!(
        h.messages.check_topic("orders", Some(&id)),
        TopicCheck::Recreated
    );
}

/// UC: UC-RTN-028
#[tokio::test]
async fn the_node_tells_which_ids_its_log_of_excluded_messages_has() {
    let h = harness();
    h.topics.create(queue("orders")).await.unwrap();
    h.store.record_excluded("orders", "a", 1, 5_000).unwrap();
    h.store.record_excluded("orders", "c", 2, 5_000).unwrap();
    let ids: Vec<String> = ["a", "b", "c"].iter().map(|s| (*s).to_owned()).collect();
    assert_eq!(h.messages.excluded_among("orders", &ids), ["a", "c"]);
    assert!(h.messages.excluded_among("other", &ids).is_empty());
}

/// UC: UC-RTN-026
#[tokio::test]
async fn the_log_of_a_life_is_added_to_the_nodes_and_the_start_goes_back_only_if_they_meet() {
    let h = harness();
    h.topics.create(queue("orders")).await.unwrap();
    let life_dir = h.dir.path().join("_lives").join("old");
    let life = h.messages.open_life(&life_dir);
    life.record_excluded("orders", "dead", 7, 2_000).unwrap();
    // A topic the cluster no longer has is not incorporated.
    life.record_excluded("ghosts", "ghost", 9, 2_000).unwrap();

    h.store.set_excluded_since(10_000).unwrap();
    // The life was seen at 4 s and the node's log starts at 10 s: there is a hole, the start stays.
    h.messages
        .import_life_log(&life, Some(1_000), 4_000)
        .unwrap();
    assert_eq!(h.messages.excluded_since(), 10_000);
    // The ids are evidence all the same.
    assert!(h.messages.is_excluded("orders", "dead"));
    assert!(!h.messages.is_excluded("ghosts", "ghost"));

    // Seen at 12 s, after the node's log started: they meet and the start goes back to the life's.
    h.messages
        .import_life_log(&life, Some(1_000), 12_000)
        .unwrap();
    assert_eq!(h.messages.excluded_since(), 1_000);
    // A manifest from before the field leaves the start alone.
    h.store.set_excluded_since(10_000).unwrap();
    h.messages.import_life_log(&life, None, 12_000).unwrap();
    assert_eq!(h.messages.excluded_since(), 10_000);
}
