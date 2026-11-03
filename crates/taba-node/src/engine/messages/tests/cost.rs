//! What the engine alone spends on a message (no HTTP, no cluster layer): publishing, taking 100 at a time and acking
//! one by one, timed apart. The times are printed (`cargo test --release -p taba-node engine::messages::tests::cost
//! -- --nocapture`); the assertions are about the result, since a time varies with the machine.

use std::time::Instant;

use super::*;

/// UC: UC-MSG-001, UC-CNS-001, UC-ACK-001, UC-BNC-006
#[tokio::test]
async fn publishing_taking_and_acking_a_deep_backlog_one_by_one() {
    let h = harness();
    h.topics.create(queue("deep")).await.unwrap();
    let count: usize = std::env::var("COST_N")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(20_000);
    let body = format!("\"{}\"", "x".repeat(1022));
    let started = Instant::now();
    for _ in 0..count {
        h.messages.publish("deep", json_body(&body)).unwrap();
    }
    let publish = started.elapsed();

    let started = Instant::now();
    let mut receipts = Vec::with_capacity(count);
    while receipts.len() < count {
        let page = consume(&h, "deep", q(Some("100"), Some("120"), None)).await;
        assert!(!page.is_empty(), "the queue emptied at {}", receipts.len());
        receipts.extend(page.into_iter().map(|m| (m.id, m.receipt_token)));
    }
    let take = started.elapsed();

    let started = Instant::now();
    for (id, token) in &receipts {
        h.messages.ack("deep", id, Some(token)).unwrap();
    }
    let ack = started.elapsed();
    let per = |d: std::time::Duration| d.as_secs_f64() * 1e6 / count as f64;
    eprintln!(
        "ENGINE per message: publish {:.1} µs, take (of 100) {:.1} µs, ack {:.1} µs",
        per(publish),
        per(take),
        per(ack)
    );
    assert!(
        consume(&h, "deep", q(Some("100"), None, None))
            .await
            .is_empty()
    );
}

/// UC: UC-MSG-001, UC-ACK-001, UC-BNC-006
#[tokio::test]
async fn a_message_is_in_memory_until_it_is_acked_or_demoted() {
    let h = harness();
    h.topics.create(queue("mem")).await.unwrap();
    let id = publish_ok(&h, "mem", "1");
    assert!(h.messages.is_in_memory("mem", &id));
    assert!(!h.messages.is_in_memory("mem", "no-such-id"));
    assert!(!h.messages.is_in_memory("no-such-topic", &id));

    let taken = consume(&h, "mem", q(Some("1"), Some("60"), None)).await;
    assert!(h.messages.is_in_memory("mem", &id));
    h.messages
        .ack("mem", &id, Some(&taken[0].receipt_token))
        .unwrap();
    assert!(!h.messages.is_in_memory("mem", &id));

    // With no room in memory the message goes to disk and the replica takes the blocking path.
    let small = harness_with(&[("TABA_MEMORY_LIMIT", "3")]);
    small.topics.create(queue("disk")).await.unwrap();
    let on_disk = publish_ok(&small, "disk", "1234");
    assert!(!small.messages.is_in_memory("disk", &on_disk));
}

/// The ack leaves the exclusion in the log even though its row is written in a batch: another reader of the log
/// (the synchronization, the vote) sees it at once.
/// UC: UC-ESC-037, UC-ACK-001
#[tokio::test]
async fn the_ack_of_a_message_in_memory_shows_in_the_log_of_excluded_messages() {
    let h = harness();
    h.topics.create(queue("log")).await.unwrap();
    let id = publish_ok(&h, "log", "1");
    let taken = consume(&h, "log", q(Some("1"), Some("60"), None)).await;
    h.messages
        .ack("log", &id, Some(&taken[0].receipt_token))
        .unwrap();
    assert!(h.messages.is_excluded("log", &id));
}
