//! The bounded lists in the engine: the ready list (`delivery.ready-list.size`) and the invisible
//! list with the `expiry` table (`delivery.invisible-list.size`).

use super::*;
use crate::storage::messages::{TOPIC_DB_FILE, dir_name};

fn rows(h: &Harness, topic: &str) -> u64 {
    h.store.expiry_count(topic).unwrap()
}

/// UC: UC-INV-017, UC-INV-025, UC-INV-026, UC-ARM-012
#[tokio::test]
async fn pairs_over_the_ceiling_wait_in_the_expiry_table_and_come_back_on_time() {
    let h = harness_with(&[("TABA_DELIVERY_INVISIBLE_LIST_SIZE", "2")]);
    h.topics.create(queue("orders")).await.unwrap();
    for i in 0..6 {
        publish_ok(&h, "orders", &i.to_string());
    }
    let mut taken = Vec::new();
    for i in 0..6u64 {
        let visibility = (30 + i * 10).to_string();
        let got = consume(&h, "orders", q(Some("1"), Some(&visibility), None)).await;
        taken.push(got.into_iter().next().unwrap());
    }
    assert_eq!(rows(&h, "orders"), 4, "duas na lista, quatro na tabela");

    // The ack deletes the row in the same commit: no stale entry is left.
    h.messages
        .ack("orders", &taken[5].id, Some(&taken[5].receipt_token))
        .unwrap();
    assert_eq!(rows(&h, "orders"), 3);

    // The first deadline arrives: only that message comes back.
    h.clock.advance(Duration::from_secs(31));
    let back = consume(&h, "orders", q(Some("10"), Some("1000"), None)).await;
    assert_eq!(
        back.iter().map(|m| &m.id).collect::<Vec<_>>(),
        [&taken[0].id]
    );

    // Later the ones that waited in the table are brought back to the list and come back too.
    h.clock.advance(Duration::from_secs(100));
    let mut ids: Vec<String> = consume(&h, "orders", q(Some("10"), Some("1000"), None))
        .await
        .into_iter()
        .map(|m| m.id)
        .collect();
    ids.sort();
    let mut expected: Vec<String> = taken[1..5].iter().map(|m| m.id.clone()).collect();
    expected.sort();
    assert_eq!(ids, expected);
}

/// UC: UC-INV-027
#[tokio::test]
async fn a_failed_table_write_undoes_the_delivery_before_the_answer() {
    let h = harness_with(&[("TABA_DELIVERY_INVISIBLE_LIST_SIZE", "1")]);
    h.topics.create(queue("orders")).await.unwrap();
    for i in 0..3 {
        publish_ok(&h, "orders", &i.to_string());
    }
    consume(&h, "orders", q(Some("1"), Some("30"), None)).await;

    // The topic folder only exists once something goes to disk: create it, then break the table.
    h.store
        .expiry_apply("orders", &[(true, 1, "x".into()), (false, 1, "x".into())])
        .unwrap();
    let db = h.dir.path().join(dir_name("orders")).join(TOPIC_DB_FILE);
    let conn = rusqlite::Connection::open(&db).unwrap();
    conn.execute("DROP TABLE expiry", []).unwrap();

    // The second delivery falls after the first on the list, so it needs the table: it fails.
    h.messages
        .consume("orders", q(Some("1"), Some("60"), None))
        .await
        .unwrap_err();

    conn.execute(
        "CREATE TABLE expiry (invisible_until INTEGER NOT NULL, id TEXT NOT NULL, \
         PRIMARY KEY (invisible_until, id)) WITHOUT ROWID",
        [],
    )
    .unwrap();
    // The message was not left invisible: the next call delivers it as a first delivery.
    let again = consume(&h, "orders", q(Some("1"), Some("60"), None)).await;
    assert_eq!(again.len(), 1);
    assert_eq!(again[0].delivery_count, 1);
}

/// UC: UC-ENT-008, UC-ENT-015
#[tokio::test]
async fn a_small_ready_list_still_delivers_every_message_exactly_once() {
    let h = harness_with(&[("TABA_DELIVERY_READY_LIST_SIZE", "3")]);
    h.topics.create(queue("orders")).await.unwrap();
    let published: Vec<String> = (0..10)
        .map(|i| publish_ok(&h, "orders", &i.to_string()))
        .collect();
    let mut got = Vec::new();
    for max in ["4", "4", "4"] {
        got.extend(
            consume(&h, "orders", q(Some(max), Some("600"), None))
                .await
                .into_iter()
                .map(|m| m.id),
        );
    }
    got.sort();
    assert_eq!(got, published);
}
