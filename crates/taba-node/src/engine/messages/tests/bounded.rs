//! The index keeps the entry of a message only while it is in the ready window or in the invisible
//! list; the rest is counted, and read from the metadata when something happens to it.

use super::*;

/// What the index of the topic keeps and counts: `(kept entries, ready, in processing)`.
fn index_of(h: &Harness, topic: &str) -> (usize, usize, usize) {
    let state = h.messages.state_of(topic);
    let state = lock(&state);
    (
        state.index.kept(),
        state.index.ready_count(),
        state.index.processing_count(),
    )
}

fn grouped(body: &str, group: &str) -> PublishRequest {
    let mut request = raw(body.as_bytes(), "application/json");
    request.group_id = Some(group.into());
    request
}

/// UC: UC-ENT-008, UC-INV-022
#[tokio::test]
async fn the_index_keeps_no_more_entries_than_the_window_and_the_list_hold() {
    for env in [
        vec![
            ("TABA_DELIVERY_READY_LIST_SIZE", "4"),
            ("TABA_DELIVERY_INVISIBLE_LIST_SIZE", "3"),
        ],
        vec![
            ("TABA_DELIVERY_READY_LIST_SIZE", "4"),
            ("TABA_DELIVERY_INVISIBLE_LIST_SIZE", "3"),
            ("TABA_MEMORY_LIMIT", "3"),
        ],
    ] {
        let h = harness_with(&env);
        h.topics.create(queue("orders")).await.unwrap();
        for i in 0..60 {
            publish_ok(&h, "orders", &i.to_string());
        }
        let taken = consume(&h, "orders", q(Some("10"), Some("600"), None)).await;
        assert_eq!(taken.len(), 10);
        let (kept, ready, processing) = index_of(&h, "orders");
        assert_eq!((ready, processing), (50, 10), "the counters stay exact");
        assert!(kept <= 4 + 3, "kept {kept}: only the window and the list");

        // Acknowledging every delivery keeps the bound and the counters.
        for m in &taken {
            h.messages
                .ack("orders", &m.id, Some(&m.receipt_token))
                .unwrap();
        }
        let (kept, ready, processing) = index_of(&h, "orders");
        assert_eq!((ready, processing), (50, 0));
        assert!(kept <= 4 + 3, "kept {kept} after the acks");
    }
}

/// UC: UC-ENT-009, UC-INV-025
#[tokio::test]
async fn ack_and_visibility_work_on_a_message_the_index_only_counts() {
    let h = harness_with(&[
        ("TABA_DELIVERY_READY_LIST_SIZE", "2"),
        ("TABA_DELIVERY_INVISIBLE_LIST_SIZE", "1"),
    ]);
    h.topics.create(queue("orders")).await.unwrap();
    for i in 0..5 {
        publish_ok(&h, "orders", &i.to_string());
    }
    let mut taken = Vec::new();
    for i in 0..3u64 {
        let visibility = (60 + i * 60).to_string();
        let got = consume(&h, "orders", q(Some("1"), Some(&visibility), None)).await;
        taken.push(got.into_iter().next().unwrap());
    }
    assert_eq!(h.store.expiry_count("orders").unwrap(), 2);
    assert_eq!(index_of(&h, "orders").2, 3);

    // The ack of a message that only the table knows.
    h.messages
        .ack("orders", &taken[2].id, Some(&taken[2].receipt_token))
        .unwrap();
    assert_eq!(h.store.expiry_count("orders").unwrap(), 1);

    // A new deadline for another one: it comes back when that deadline arrives, not before.
    let up = h
        .messages
        .set_visibility(
            "orders",
            &taken[1].id,
            Some(&taken[1].receipt_token),
            Some(500),
        )
        .unwrap();
    assert_eq!(up.invisible_until_ms, h.clock.now_millis() + 500_000);
    h.clock.advance(Duration::from_secs(120));
    let back = consume(&h, "orders", q(Some("10"), Some("1000"), None)).await;
    let back: Vec<_> = back.iter().map(|m| m.id.as_str()).collect();
    assert!(back.contains(&taken[0].id.as_str()), "the first one is due");
    assert!(
        !back.contains(&taken[1].id.as_str()),
        "the changed deadline holds"
    );
    assert!(
        !back.contains(&taken[2].id.as_str()),
        "the acknowledged one is gone"
    );
    h.clock.advance(Duration::from_secs(600));
    let late = consume(&h, "orders", q(Some("10"), Some("1000"), None)).await;
    assert!(late.iter().any(|m| m.id == taken[1].id));
}

/// UC: UC-RET-001
#[tokio::test]
async fn the_ttl_removes_the_ready_messages_that_wait_outside_the_window() {
    for env in [
        vec![("TABA_DELIVERY_READY_LIST_SIZE", "2")],
        vec![
            ("TABA_DELIVERY_READY_LIST_SIZE", "2"),
            ("TABA_MEMORY_LIMIT", "3"),
        ],
    ] {
        let h = harness_with(&env);
        let mut topic = queue("orders");
        topic.ttl = Some(600);
        h.topics.create(topic).await.unwrap();
        for i in 0..12 {
            publish_ok(&h, "orders", &i.to_string());
        }
        h.clock.advance(Duration::from_secs(300));
        let kept_young = publish_ok(&h, "orders", "\"young\"");
        h.clock.advance(Duration::from_secs(400));
        h.messages.sweep();
        let (_, ready, processing) = index_of(&h, "orders");
        assert_eq!((ready, processing), (1, 0), "only the young one is left");
        let left = consume(&h, "orders", q(Some("10"), Some("30"), None)).await;
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].id, kept_young);
    }
}

/// UC: UC-ENT-008, UC-INV-017
#[tokio::test]
async fn reopening_a_topic_rebuilds_the_counts_and_the_window_from_the_disk() {
    let h = harness_with(&[
        ("TABA_DELIVERY_READY_LIST_SIZE", "3"),
        ("TABA_DELIVERY_INVISIBLE_LIST_SIZE", "2"),
        ("TABA_MEMORY_LIMIT", "3"),
    ]);
    h.topics.create(queue("orders")).await.unwrap();
    let published: Vec<String> = (0..30)
        .map(|i| publish_ok(&h, "orders", &i.to_string()))
        .collect();
    let taken = consume(&h, "orders", q(Some("5"), Some("600"), None)).await;
    assert_eq!(taken.len(), 5);
    h.messages.drain();

    // The state of the topic is forgotten: the next use rebuilds it from the database rows.
    h.messages.states.write().unwrap().clear();
    let (kept, ready, processing) = index_of(&h, "orders");
    assert_eq!((ready, processing), (25, 5));
    assert!(kept <= 3 + 2, "kept {kept} after the rebuild");
    assert!(
        h.store.expiry_count("orders").unwrap() >= 3,
        "the table was rebuilt"
    );

    // The five in processing stay out; the rest comes, each one once.
    let mut rest: Vec<String> = Vec::new();
    loop {
        let got = consume(&h, "orders", q(Some("7"), Some("6000"), None)).await;
        if got.is_empty() {
            break;
        }
        rest.extend(got.into_iter().map(|m| m.id));
    }
    rest.sort();
    let mut expected: Vec<String> = published
        .iter()
        .filter(|id| !taken.iter().any(|m| &m.id == *id))
        .cloned()
        .collect();
    expected.sort();
    assert_eq!(rest, expected);

    // And the five come back on time.
    h.clock.advance(Duration::from_secs(700));
    let mut back: Vec<String> = consume(&h, "orders", q(Some("10"), Some("600"), None))
        .await
        .into_iter()
        .map(|m| m.id)
        .collect();
    back.sort();
    let mut was: Vec<String> = taken.iter().map(|m| m.id.clone()).collect();
    was.sort();
    assert_eq!(back, was);
}

/// UC: UC-ENT-015, UC-INV-020
#[tokio::test]
async fn small_lists_choose_the_same_messages_as_large_ones() {
    let small = harness_with(&[
        ("TABA_DELIVERY_READY_LIST_SIZE", "2"),
        ("TABA_DELIVERY_INVISIBLE_LIST_SIZE", "2"),
        ("TABA_DELIVERY_FAIR_NORMAL_EVERY", "3"),
    ]);
    let large = harness_with(&[("TABA_DELIVERY_FAIR_NORMAL_EVERY", "3")]);
    let mut sequences: Vec<Vec<String>> = Vec::new();
    for h in [&small, &large] {
        h.topics.create(queue("orders")).await.unwrap();
        // A big group, a few small ones, published interleaved.
        for i in 0..40u32 {
            let group = match i % 7 {
                0..=3 => "big".to_owned(),
                n => format!("g{n}"),
            };
            h.messages
                .publish("orders", grouped(&format!("\"{i}\""), &group))
                .unwrap();
        }
        let mut order = Vec::new();
        for step in 0..60u64 {
            let got = consume(h, "orders", q(Some("1"), Some("30"), None)).await;
            if let Some(m) = got.into_iter().next() {
                // The first delivery of a message is what is compared; a return is compared too.
                order.push(serde_json::to_string(&m.body).unwrap());
                if step % 4 == 0 {
                    h.messages
                        .ack("orders", &m.id, Some(&m.receipt_token))
                        .unwrap();
                }
            }
            h.clock.advance(Duration::from_secs(5));
        }
        sequences.push(order);
    }
    assert!(!sequences[0].is_empty());
    assert_eq!(sequences[0], sequences[1]);
}

/// UC: UC-ENT-008
#[tokio::test]
async fn the_oldest_message_comes_from_the_age_order_of_memory_and_disk() {
    let h = harness_with(&[("TABA_MEMORY_LIMIT", "3")]);
    h.topics.create(queue("orders")).await.unwrap();
    let first = publish_ok(&h, "orders", "\"first\"");
    h.clock.advance(Duration::from_secs(10));
    for i in 0..5 {
        publish_ok(&h, "orders", &i.to_string());
    }
    let oldest = |h: &Harness| {
        let state = h.messages.state_of("orders");
        let state = lock(&state);
        h.messages.oldest_owned_age("orders", &state)
    };
    let published = h.clock.now_millis() - 10_000;
    assert_eq!(oldest(&h), Some(published));

    // The oldest leaves: the next one is the oldest, in memory or on disk.
    let taken = consume(&h, "orders", q(Some("1"), Some("60"), None)).await;
    assert_eq!(taken[0].id, first);
    h.messages
        .ack("orders", &first, Some(&taken[0].receipt_token))
        .unwrap();
    assert_eq!(oldest(&h), Some(published + 10_000));
}
