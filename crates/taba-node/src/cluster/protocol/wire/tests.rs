use bytes::Bytes;
use prost::Message;

use super::*;
use crate::cluster::protocol::{
    AckRegisterRequest, MessageRecord, MessageRegisterRequest, OperationRef, Request, Step,
    TakeRequest, VisibilityKind, VisibilityRegisterRequest, WriteKind,
};

const OPERATION: &str = "01900000-0000-7000-8000-000000000001";
const COORDINATOR: &str = "01900000-0000-7000-8000-0000000000c0";
const MESSAGE: &str = "01900000-0000-7000-8000-000000000002";

fn record(body: &[u8]) -> MessageRecord {
    MessageRecord {
        id: MESSAGE.to_owned(),
        topic: "orders".to_owned(),
        body: Bytes::copy_from_slice(body),
        content_type: "application/octet-stream".to_owned(),
        group_id: Some("g1".to_owned()),
        deduplication_id: None,
        delivery_count: 3,
        invisible_until_ms: Some(1_700_000_000_123),
        receipt_token: None,
        dead_lettered_ms: None,
        dead_letter_reason: Some("too many deliveries".to_owned()),
        delete_at_ms: None,
    }
}

fn write_register(body: &[u8]) -> Request {
    Request::Write {
        topic: "orders".to_owned(),
        id: MESSAGE.to_owned(),
        step: Step::Register(MessageRegisterRequest {
            operation_id: OPERATION.to_owned(),
            coordinator_id: COORDINATOR.to_owned(),
            kind: WriteKind::DeadLetter,
            message: record(body),
            source_topic: Some("orders-source".to_owned()),
        }),
    }
}

fn every_kind() -> Vec<Request> {
    let commit = || OperationRef {
        operation_id: OPERATION.to_owned(),
    };
    vec![
        write_register(b"hello"),
        Request::Write {
            topic: "orders".to_owned(),
            id: MESSAGE.to_owned(),
            step: Step::Commit(commit()),
        },
        Request::Write {
            topic: "orders".to_owned(),
            id: MESSAGE.to_owned(),
            step: Step::Rollback(commit()),
        },
        Request::Visibility {
            topic: "orders".to_owned(),
            id: MESSAGE.to_owned(),
            step: Step::Register(VisibilityRegisterRequest {
                operation_id: OPERATION.to_owned(),
                coordinator_id: COORDINATOR.to_owned(),
                kind: VisibilityKind::Delivery,
                invisible_until_ms: 1_700_000_030_000,
                receipt_token: "tok".to_owned(),
                delivery_count: Some(2),
            }),
        },
        Request::Visibility {
            topic: "orders".to_owned(),
            id: MESSAGE.to_owned(),
            step: Step::Commit(commit()),
        },
        Request::Ack {
            topic: "orders".to_owned(),
            id: MESSAGE.to_owned(),
            step: Step::Register(AckRegisterRequest {
                operation_id: OPERATION.to_owned(),
                coordinator_id: COORDINATOR.to_owned(),
                receipt_token: "tok".to_owned(),
                delete_at_ms: Some(1_700_000_999_000),
                discard: true,
            }),
        },
        Request::Ack {
            topic: "orders".to_owned(),
            id: MESSAGE.to_owned(),
            step: Step::Rollback(commit()),
        },
        Request::Take {
            topic: "orders".to_owned(),
            request: TakeRequest {
                max_messages: 10,
                visibility_timeout: 30,
            },
        },
        Request::FifoPublish {
            topic: "orders".to_owned(),
            record: record(b"fifo"),
        },
    ]
}

fn entries(requests: Vec<Request>) -> Vec<Entry> {
    requests
        .into_iter()
        .map(|request| {
            Ok(BatchItem {
                request,
                traceparent: None,
            })
        })
        .collect()
}

/// UC: UC-CLR-011
#[test]
fn every_operation_of_a_batch_comes_back_as_it_went() {
    let sent = entries(every_kind());
    let received = decode_batch(&encode_batch(&sent)).unwrap();
    assert_eq!(received, sent);
}

/// UC: UC-CLR-011
#[test]
fn a_body_with_every_byte_value_and_line_breaks_is_kept_byte_for_byte() {
    let body: Vec<u8> = (0..=255u8).chain(*b"\n\r\n{\"a\":1}\n").collect();
    let sent = entries(vec![write_register(&body)]);
    let received = decode_batch(&encode_batch(&sent)).unwrap();
    assert_eq!(received, sent);
}

/// UC: UC-CLR-011
#[test]
fn a_decoded_body_does_not_keep_the_batch_alive() {
    // The body is copied out of the batch: a message that stays in memory must not hold the whole batch with it.
    let sent = entries(vec![
        write_register(&[7u8; 1024]),
        write_register(&[8u8; 1024]),
    ]);
    let wire = encode_batch(&sent);
    let received = decode_batch(&wire).unwrap();
    let Ok(BatchItem {
        request:
            Request::Write {
                step: Step::Register(register),
                ..
            },
        ..
    }) = &received[0]
    else {
        panic!("a write register was expected");
    };
    let start = wire.as_ptr() as usize;
    let at = register.message.body.as_ptr() as usize;
    assert!(
        at < start || at >= start + wire.len(),
        "the body points into the batch buffer"
    );
}

/// UC: UC-CLR-011
#[test]
fn the_trace_context_of_each_item_travels_with_it() {
    let traced = BatchItem {
        request: every_kind().remove(1),
        traceparent: Some("00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01".to_owned()),
    };
    let sent = vec![Ok(traced), entries(every_kind()).remove(0)];
    assert_eq!(decode_batch(&encode_batch(&sent)).unwrap(), sent);
}

/// UC: UC-CLR-011
#[test]
fn an_empty_batch_is_valid_and_has_no_items() {
    assert!(decode_batch(&encode_batch(&[])).unwrap().is_empty());
}

/// UC: UC-CLR-011
#[test]
fn a_body_that_is_not_a_batch_is_an_error_of_the_whole_call() {
    let error = decode_batch(b"\x0a\xff\xff\xff\xff\x0f truncated").unwrap_err();
    assert_eq!(error.code, ErrorCode::ValidationFailed);
    let error = decode_batch(br#"{"items":[]}"#).unwrap_err();
    assert_eq!(
        error.code,
        ErrorCode::ValidationFailed,
        "JSON is not accepted"
    );
}

/// UC: UC-CLR-011
#[test]
fn an_item_of_an_unknown_kind_is_an_error_of_that_item_only() {
    let good = item_of(&entries(every_kind())[1].clone().unwrap()).unwrap();
    let batch = BatchRequest {
        items: vec![
            Item {
                kind: 99,
                ..good.clone()
            },
            good.clone(),
            Item {
                phase: 77,
                ..good.clone()
            },
            // A register with no body.
            Item {
                phase: Phase::Register as i32,
                ..good
            },
        ],
    };
    let decoded = decode_batch(&batch.encode_to_vec()).unwrap();
    assert_eq!(decoded.len(), 4);
    assert_eq!(
        decoded[0].as_ref().unwrap_err().code,
        ErrorCode::ValidationFailed
    );
    assert!(decoded[1].is_ok());
    assert_eq!(
        decoded[2].as_ref().unwrap_err().code,
        ErrorCode::ValidationFailed
    );
    assert_eq!(
        decoded[3].as_ref().unwrap_err().code,
        ErrorCode::ValidationRequired
    );
}

/// UC: UC-CLR-011
#[test]
fn a_field_a_later_version_adds_is_ignored() {
    let mut wire = encode_batch(&entries(every_kind()));
    // Field 15, varint, value 1, appended to the end of the (last item's) bytes does not exist in the schema; adding
    // it at the batch level (an unknown field of `BatchRequest`) must not break the reading.
    wire.extend_from_slice(&[0x78, 0x01]);
    assert_eq!(decode_batch(&wire).unwrap().len(), every_kind().len());
}

/// UC: UC-CLR-011
#[test]
fn the_results_come_back_in_order_with_the_error_of_each_item() {
    let mut conflict = TabaError::new(ErrorCode::ClusterConflict, "Another operation holds it.")
        .with_detail("id", ErrorCode::ClusterConflict, "Pending register.");
    conflict.retry_after = Some(5);
    let sent: Vec<Outcome> = vec![
        Ok(Response::Ack(Ack {
            operation_id: OPERATION.to_owned(),
            accepted: true,
        })),
        Ok(Response::Taken(TakeResponse {
            messages: vec![record(b"one"), record(b"")],
        })),
        Ok(Response::Taken(TakeResponse { messages: vec![] })),
        Ok(Response::FifoPublished(FifoPublished {
            id: MESSAGE.to_owned(),
            duplicate: true,
        })),
        Err(conflict),
        Err(TabaError::new(
            ErrorCode::MessageNotFound,
            "No such message.",
        )),
    ];
    let received = decode_results(&encode_results(&sent)).unwrap();
    assert_eq!(received, sent);
    assert_eq!(received[4].as_ref().unwrap_err().status(), 409);
}

/// UC: UC-CLR-011
#[test]
fn a_reply_that_is_not_a_batch_reply_is_refused() {
    assert!(decode_results(b"\x0a\xff\xff\xff\xff\x0f").is_err());
}

/// UC: UC-CLR-011
#[test]
fn a_request_that_is_not_a_message_operation_goes_out_as_an_unspecified_item() {
    let wire = encode_batch(&[Ok(BatchItem {
        request: Request::Status,
        traceparent: None,
    })]);
    let decoded = decode_batch(&wire).unwrap();
    assert_eq!(
        decoded[0].as_ref().unwrap_err().code,
        ErrorCode::ValidationRequired
    );
}

fn hex(bytes: &[u8]) -> String {
    let mut text: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    text.push('\n');
    text
}

/// UC: UC-CLR-011
#[test]
fn the_bytes_of_a_batch_are_the_golden_vector() {
    // The same bytes any implementation of `proto/taba-cluster.proto` must produce for this batch. To regenerate after
    // a deliberate change of the schema: `TABA_UPDATE_GOLDEN=1 cargo test --lib golden_vector`.
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/wire/batch.hex");
    let bytes = encode_batch(&entries(every_kind()));
    if std::env::var_os("TABA_UPDATE_GOLDEN").is_some() {
        std::fs::write(path, hex(&bytes)).unwrap();
    }
    let expected = std::fs::read_to_string(path).expect("tests/wire/batch.hex");
    assert_eq!(hex(&bytes), expected, "the encoding of the batch changed");
}
