use serde_json::json;

use super::*;

/// UC: UC-GTW-016
#[test]
fn only_the_characters_sqs_allows_are_accepted_in_a_body() {
    assert!(check_contents("café € 日本 \t\n\r 😀").is_ok());
    assert_eq!(
        check_contents("bad \u{1} char").unwrap_err().json_code,
        "InvalidMessageContents"
    );
    assert!(check_contents("\u{FFFE}").is_err());
}

/// UC: UC-GTW-016
#[test]
fn a_message_is_prepared_with_its_delay_md5_and_size_limit() {
    let prepared = prepare(
        &json!({"MessageBody": "hello world", "DelaySeconds": "7"}),
        0,
        1000,
    )
    .unwrap();
    assert_eq!(prepared.delay, 7);
    assert_eq!(prepared.md5_body, "5eb63bbbe01eeed093cb22bb8f5acdc3");
    assert!(prepared.md5_attributes.is_none());

    // The queue's delay applies when the message does not say.
    assert_eq!(
        prepare(&json!({"MessageBody": "x"}), 3, 1000)
            .unwrap()
            .delay,
        3
    );
    // And an explicit 0 wins over the queue's.
    assert_eq!(
        prepare(&json!({"MessageBody": "x", "DelaySeconds": 0}), 3, 1000)
            .unwrap()
            .delay,
        0
    );
    assert!(prepare(&json!({"MessageBody": "x", "DelaySeconds": 901}), 0, 1000).is_err());
    assert!(prepare(&json!({"MessageBody": "x".repeat(1001)}), 0, 1000).is_err());
    assert_eq!(
        prepare(&json!({"MessageBody": ""}), 0, 1000)
            .err()
            .unwrap()
            .json_code,
        "MissingParameter"
    );
}

/// UC: UC-GTW-016
#[test]
fn a_batch_has_one_to_ten_entries_with_distinct_valid_ids() {
    let ok = json!({"Entries": [{"Id": "a"}, {"Id": "b-1"}, {"Id": "c_2"}]});
    assert_eq!(entries(&ok).unwrap().len(), 3);
    let code = |v: serde_json::Value| entries(&v).err().unwrap().json_code;
    assert_eq!(code(json!({})), "EmptyBatchRequest");
    assert_eq!(code(json!({"Entries": []})), "EmptyBatchRequest");
    let eleven: Vec<_> = (0..11).map(|i| json!({"Id": format!("e{i}")})).collect();
    assert_eq!(
        code(json!({"Entries": eleven})),
        "TooManyEntriesInBatchRequest"
    );
    assert_eq!(
        code(json!({"Entries": [{"Id": "a"}, {"Id": "a"}]})),
        "BatchEntryIdsNotDistinct"
    );
    assert_eq!(
        code(json!({"Entries": [{"Id": "has space"}]})),
        "InvalidBatchEntryId"
    );
    assert_eq!(
        code(json!({"Entries": [{"Id": "a".repeat(81)}]})),
        "InvalidBatchEntryId"
    );
}

/// UC: UC-GTW-016
#[test]
fn the_attributes_a_receive_asks_for_are_matched_by_name_all_or_prefix() {
    let names = |list: &[&str]| list.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    assert!(wanted(&names(&["All"]), "anything"));
    assert!(wanted(&names(&[".*"]), "anything"));
    assert!(wanted(&names(&["color"]), "color"));
    assert!(!wanted(&names(&["color"]), "size"));
    assert!(wanted(&names(&["app.*"]), "app.id"));
    assert!(!wanted(&names(&["app.*"]), "other.id"));
    assert!(!wanted(&names(&[]), "color"));
}

/// UC: UC-GTW-016
#[test]
fn the_items_of_a_batch_answer_become_successes_or_aws_errors() {
    assert!(item_error(&json!({"status": 201, "id": "m1"})).is_none());
    assert!(item_error(&json!({"status": 204})).is_none());
    let stale = item_error(
        &json!({"status": 409, "error": {"code": "message.stale-receipt", "message": "old"}}),
    )
    .unwrap();
    assert_eq!(stale.json_code, "MessageNotInflight");
    let large = item_error(
        &json!({"status": 413, "error": {"code": "message.too-large", "message": "big"}}),
    )
    .unwrap();
    assert_eq!(large.json_code, "InvalidParameterValue");
    // An item with no answer is a failure, not a success.
    assert!(item_error(&Value::Null).is_some());
}

/// UC: UC-GTW-016
#[test]
fn deleting_what_is_already_gone_is_not_an_error() {
    assert!(gone("message.not-found"));
    assert!(gone("message.stale-receipt"));
    assert!(!gone("topic.not-found"));
    assert!(!gone("auth.forbidden"));
}

/// UC: UC-GTW-016
#[test]
fn a_failed_entry_says_who_is_to_blame_and_the_code() {
    let failed = failed_entry("e1", &AwsError::receipt_handle_is_invalid());
    assert_eq!(failed["Id"], "e1");
    assert_eq!(failed["Code"], "ReceiptHandleIsInvalid");
    assert_eq!(failed["SenderFault"], true);
    let result = batch_result(vec![json!({"Id": "ok"})], vec![]);
    assert!(
        result.get("Failed").is_none(),
        "an empty list of failures is not written"
    );
}

fn fifo(content_dedup: bool) -> Mode {
    Mode::Fifo { content_dedup }
}

/// UC: UC-GTW-022
#[test]
fn a_fifo_message_needs_a_group_and_a_deduplication_id() {
    let send = |entry: Value, content_dedup: bool| prepare_in(&entry, &fifo(content_dedup), 1000);
    let ok = send(
        json!({"MessageBody": "b", "MessageGroupId": "g1", "MessageDeduplicationId": "d1"}),
        false,
    )
    .unwrap();
    let ids = ok.fifo.expect("fifo ids");
    assert_eq!((ids.group.as_str(), ids.dedup.as_str()), ("g1", "d1"));
    assert_eq!(ok.delay, 0);
    assert_eq!(
        ok.packed.content_type,
        envelope::PLAIN_CONTENT_TYPE,
        "ids Taba accepts need no envelope"
    );

    assert_eq!(
        send(
            json!({"MessageBody": "b", "MessageDeduplicationId": "d1"}),
            false
        )
        .err()
        .unwrap()
        .json_code,
        "MissingParameter"
    );
    assert_eq!(
        send(json!({"MessageBody": "b", "MessageGroupId": "g1"}), false)
            .err()
            .unwrap()
            .json_code,
        "InvalidParameterValue",
        "no id and no content-based deduplication"
    );
    for bad in ["has space", "ação", &"a".repeat(129)] {
        assert!(
            send(
                json!({"MessageBody": "b", "MessageGroupId": bad, "MessageDeduplicationId": "d"}),
                false
            )
            .is_err(),
            "{bad}"
        );
    }
}

/// UC: UC-GTW-022
#[test]
fn with_content_based_deduplication_the_id_is_the_hash_of_the_body() {
    let entry = json!({"MessageBody": "abc", "MessageGroupId": "g1"});
    let ids = prepare_in(&entry, &fifo(true), 1000).unwrap().fifo.unwrap();
    assert_eq!(
        ids.dedup,
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    let explicit =
        json!({"MessageBody": "abc", "MessageGroupId": "g1", "MessageDeduplicationId": "mine"});
    assert_eq!(
        prepare_in(&explicit, &fifo(true), 1000)
            .unwrap()
            .fifo
            .unwrap()
            .dedup,
        "mine",
        "an id the application sends wins"
    );
}

/// UC: UC-GTW-022
#[test]
fn a_fifo_message_has_no_delay() {
    let entry = json!({"MessageBody": "b", "MessageGroupId": "g", "MessageDeduplicationId": "d", "DelaySeconds": 5});
    assert_eq!(
        prepare_in(&entry, &fifo(false), 1000)
            .err()
            .unwrap()
            .json_code,
        "InvalidParameterValue"
    );
    // The queue's own delay is 0 on a FIFO queue, so it adds nothing; a standard queue ignores the FIFO fields.
    let standard = json!({"MessageBody": "b", "MessageGroupId": "g", "DelaySeconds": 5});
    let prepared = prepare(&standard, 0, 1000).unwrap();
    assert_eq!(prepared.delay, 5);
    assert!(prepared.fifo.is_none());
}

/// UC: UC-GTW-020
#[test]
fn ids_taba_does_not_accept_go_as_hashes_and_the_original_travels_in_the_envelope() {
    let entry = json!({
        "MessageBody": "b",
        "MessageGroupId": "tenant/42!blue",
        "MessageDeduplicationId": "order#7"
    });
    let prepared = prepare_in(&entry, &fifo(false), 1000).unwrap();
    let ids = prepared.fifo.as_ref().unwrap();
    assert!(ids.group.starts_with("h.") && ids.dedup.starts_with("h."));
    assert_eq!(
        prepared.packed.content_type,
        envelope::ENVELOPE_CONTENT_TYPE
    );
    let opened = envelope::unpack(&prepared.packed.value, None);
    assert_eq!(opened.body, "b");
    assert_eq!(opened.origin.group.as_deref(), Some("tenant/42!blue"));
    assert_eq!(opened.origin.dedup.as_deref(), Some("order#7"));
}

/// UC: UC-GTW-022
#[test]
fn the_sequence_number_grows_with_the_message_id_inside_a_group() {
    let first = sequence_number("01a1363a-5434-7000-8000-000000000001").unwrap();
    let second = sequence_number("01a1363a-5434-7000-8000-000000000002").unwrap();
    assert!(first.parse::<u128>().unwrap() < second.parse::<u128>().unwrap());
    assert_eq!(sequence_number("not-an-id"), None);
    let prepared = prepare_in(
        &json!({"MessageBody": "b", "MessageGroupId": "g", "MessageDeduplicationId": "d"}),
        &fifo(false),
        1000,
    )
    .unwrap();
    assert!(sent("01a1363a-5434-7000-8000-000000000001", &prepared).contains_key("SequenceNumber"));
    let standard = prepare(&json!({"MessageBody": "b"}), 0, 1000).unwrap();
    assert!(
        !sent("01a1363a-5434-7000-8000-000000000001", &standard).contains_key("SequenceNumber")
    );
}
