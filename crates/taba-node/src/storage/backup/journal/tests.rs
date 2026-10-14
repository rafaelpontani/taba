use super::*;

fn meta() -> MessageMeta {
    MessageMeta {
        id: "m1".into(),
        content_type: "text/plain".into(),
        group_id: None,
        deduplication_id: None,
        published_ms: 1,
        delivery_count: 3,
        invisible_until_ms: 900,
        receipt_token: Some("tok".into()),
        dead_lettered_ms: Some(50),
        dead_letter_reason: Some("max-deliveries".into()),
        position: 0,
    }
}

/// UC: UC-BKP-007
#[test]
fn a_journal_round_trips_states_and_removals() {
    let header = Header {
        topic: "pedidos".into(),
        topic_id: "tid".into(),
        writer: "no-1".into(),
        at_ms: 60_000,
    };
    let records = vec![Record::state(&meta()), Record::removal("m2")];
    let (back_header, back) = decode(&encode(&header, &records)).unwrap();
    assert_eq!(back_header, header);
    assert_eq!(back, records);
    assert!(!back[0].removed);
    assert!(back[1].removed);
}

/// UC: UC-BKP-007
#[test]
fn a_state_record_overwrites_the_delivery_fields_of_the_segment_copy() {
    let mut fresh = meta();
    fresh.delivery_count = 0;
    fresh.invisible_until_ms = 0;
    fresh.receipt_token = None;
    fresh.dead_lettered_ms = None;
    fresh.dead_letter_reason = None;
    Record::state(&meta()).apply_to(&mut fresh);
    assert_eq!(fresh, meta());
}

/// UC: UC-BKP-007
#[test]
fn a_broken_journal_is_an_error_not_a_panic() {
    assert!(decode(b"").is_err());
    assert!(decode(b"isto nao e json\n").is_err());
}
