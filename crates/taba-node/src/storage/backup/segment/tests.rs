use super::*;
use crate::storage::backup::store::{MemoryStore, ObjectStore};

fn meta(id: &str) -> MessageMeta {
    MessageMeta {
        id: id.into(),
        content_type: "application/octet-stream".into(),
        group_id: Some("g".into()),
        deduplication_id: None,
        published_ms: 1_000,
        delivery_count: 2,
        invisible_until_ms: 5,
        receipt_token: Some("r".into()),
        dead_lettered_ms: None,
        dead_letter_reason: None,
        position: 77,
    }
}

fn build(count: usize) -> (Vec<u8>, Vec<IndexEntry>) {
    let mut builder = SegmentBuilder::new("pedidos", "tid", "no-1");
    for i in 0..count {
        builder.push(
            &meta(&format!("id-{i:03}")),
            format!("corpo-{i}").as_bytes(),
        );
    }
    let entries = builder.entries().to_vec();
    (builder.finish(), entries)
}

/// UC: UC-BKP-006
#[test]
fn a_segment_round_trips_with_its_footer_index() {
    let (bytes, entries) = build(3);
    let (footer, messages) = decode_all(&bytes).unwrap();
    assert_eq!(footer.topic, "pedidos");
    assert_eq!(footer.topic_id, "tid");
    assert_eq!(footer.writer, "no-1");
    assert_eq!(footer.entries, entries);
    assert_eq!(messages.len(), 3);
    assert_eq!(messages[1].0.id, "id-001");
    assert_eq!(messages[1].0.position, 77);
    assert_eq!(messages[1].1, b"corpo-1");
}

/// UC: UC-BKP-006
#[test]
fn one_message_is_read_by_range_without_downloading_the_segment() {
    let (bytes, entries) = build(50);
    let store = MemoryStore::new();
    let entry = entries[37].clone();
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(async {
            store.put("seg", bytes.clone()).await.unwrap();
            let tail = store.get_suffix("seg", TAIL_GUESS).await.unwrap().unwrap();
            let footer = match read_footer(&tail).unwrap() {
                FooterRead::Done(f) => f,
                FooterRead::NeedMore(n) => {
                    match read_footer(&store.get_suffix("seg", n).await.unwrap().unwrap()).unwrap()
                    {
                        FooterRead::Done(f) => f,
                        FooterRead::NeedMore(_) => panic!("the whole footer was requested"),
                    }
                }
            };
            let found = footer.entries.iter().find(|e| e.id == entry.id).unwrap();
            let raw = store
                .get_range("seg", found.offset, found.len)
                .await
                .unwrap()
                .unwrap();
            let (meta, body) = decode_entry(&raw).unwrap();
            assert_eq!(meta.id, "id-037");
            assert_eq!(body, b"corpo-37");
        });
}

/// UC: UC-BKP-006
#[test]
fn a_short_tail_asks_for_the_whole_footer_and_garbage_is_refused() {
    let (bytes, _) = build(40);
    let tail = &bytes[bytes.len() - 20..];
    match read_footer(tail).unwrap() {
        FooterRead::NeedMore(n) => assert!(n > 20),
        FooterRead::Done(_) => panic!("20 bytes are not enough"),
    }
    assert!(read_footer(b"nothing to see").is_err());
    assert!(decode_all(b"TABASEG1").is_err());
    assert!(decode_entry(&[1, 0]).is_err());
}

/// UC: UC-BKP-006
#[test]
fn metadata_survives_the_conversion_both_ways() {
    let m = meta("a");
    let back: MessageMeta = BackupMessage::from(&m).into();
    assert_eq!(back, m);
}
