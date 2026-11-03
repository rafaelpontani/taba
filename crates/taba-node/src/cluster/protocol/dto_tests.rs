//! The cluster DTOs and the contract of the synchronization streams, seen from the wire (doc 06c,
//! doc 06d section 8.3.7): field names, types, the refusal of invalid bodies and the query of a stream.

use std::fmt::Debug;

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use super::http::HttpCall;
use super::*;
use crate::cluster::members::MemberState;
use crate::cluster::state::NodeShared;
use crate::core::time::format_ms;
use crate::core::token::{Permission, TokenRecord};
use crate::core::topic::TopicDefinition;

const BASE_MS: u64 = 1_791_288_000_000;

fn guid(n: u64) -> String {
    id::new_at(BASE_MS + n)
}

fn json_headers() -> Vec<(String, String)> {
    vec![("content-type".to_owned(), "application/json".to_owned())]
}

/// Reads the wire JSON as the DTO and writes it back: nothing may be lost, added or renamed.
fn round_trip<T: Serialize + DeserializeOwned + Debug>(wire: Value) {
    // Through text, not `from_value`: the `MessageRecord` keeps the body as raw JSON.
    let parsed: T = serde_json::from_str(&wire.to_string())
        .unwrap_or_else(|e| panic!("{wire} is not read as {}: {e}", std::any::type_name::<T>()));
    assert_eq!(
        serde_json::to_value(&parsed).unwrap(),
        wire,
        "{}",
        std::any::type_name::<T>()
    );
}

fn member_wire(n: u64) -> Value {
    json!({
        "nodeId": guid(n),
        "addresses": ["10.0.0.5", "10.0.0.6"],
        "port": 8081,
        "state": "active"
    })
}

fn record_wire() -> Value {
    json!({
        "id": guid(10),
        "topic": "pedidos",
        "body": {"x": true},
        "contentType": "application/json",
        "groupId": "g1",
        "deliveryCount": 1,
        "invisibleUntil": format_ms(BASE_MS + 60_000),
        "receiptToken": guid(11)
    })
}

/// UC: UC-CNV-017
#[test]
fn the_ack_and_the_stream_end_have_the_fields_and_types_of_the_document() {
    round_trip::<Ack>(json!({"operationId": guid(1), "accepted": true}));
    round_trip::<Ack>(json!({"operationId": guid(1), "accepted": false}));
    for wrong in [
        json!({"operationId": guid(1)}),
        json!({"accepted": true}),
        json!({"operationId": guid(1), "accepted": "yes"}),
        json!({"operationId": 7, "accepted": true}),
        json!({"operationId": guid(1), "accepted": true, "extra": 1}),
    ] {
        assert!(
            serde_json::from_value::<Ack>(wrong.clone()).is_err(),
            "{wrong}"
        );
    }

    round_trip::<StreamEnd>(json!({"end": true, "count": 2, "last": guid(2), "hasMore": true}));
    // `last` is absent, not `null`, when the stream had no line.
    round_trip::<StreamEnd>(json!({"end": true, "count": 0, "hasMore": false}));
    for wrong in [
        json!({"end": true, "count": 2}),
        json!({"end": true, "hasMore": false}),
        json!({"end": true, "count": "2", "hasMore": false}),
        json!({"end": "yes", "count": 2, "hasMore": false}),
        json!({"end": true, "count": 2, "hasMore": false, "extra": 1}),
    ] {
        assert!(
            serde_json::from_value::<StreamEnd>(wrong.clone()).is_err(),
            "{wrong}"
        );
    }
}

/// UC: UC-CLR-002
#[test]
fn every_phase_of_every_two_phase_call_carries_the_operation_id() {
    let operation_id = guid(1);
    let op = || OperationRef {
        operation_id: operation_id.clone(),
    };
    let message = MessageRegisterRequest {
        operation_id: operation_id.clone(),
        coordinator_id: guid(2),
        kind: WriteKind::Publish,
        message: MessageRecord {
            id: guid(10),
            topic: "pedidos".into(),
            body: b"{}".to_vec().into(),
            content_type: "application/json".into(),
            group_id: None,
            deduplication_id: None,
            delivery_count: 0,
            invisible_until_ms: None,
            receipt_token: None,
            dead_lettered_ms: None,
            dead_letter_reason: None,
            delete_at_ms: None,
        },
        source_topic: None,
    };
    let visibility = VisibilityRegisterRequest {
        operation_id: operation_id.clone(),
        coordinator_id: guid(2),
        kind: VisibilityKind::Change,
        invisible_until_ms: BASE_MS + 60_000,
        receipt_token: guid(5),
        delivery_count: None,
    };
    let ack = AckRegisterRequest {
        operation_id: operation_id.clone(),
        coordinator_id: guid(2),
        receipt_token: guid(5),
        delete_at_ms: None,
        discard: false,
    };
    let (topic, id) = ("pedidos".to_owned(), guid(10));
    let calls: Vec<(Request, &str)> = vec![
        (
            Request::Register(NodeRegisterRequest {
                operation_id: operation_id.clone(),
                coordinator_id: guid(2),
                node: Member {
                    node_id: guid(3),
                    addresses: vec!["10.0.0.5".into()],
                    port: 8081,
                    state: MemberState::Syncing,
                },
            }),
            "register",
        ),
        (Request::Commit(op()), "commit"),
        (Request::Rollback(op()), "rollback"),
        (
            Request::Write {
                topic: topic.clone(),
                id: id.clone(),
                step: Step::Register(message),
            },
            "register",
        ),
        (
            Request::Write {
                topic: topic.clone(),
                id: id.clone(),
                step: Step::Commit(op()),
            },
            "commit",
        ),
        (
            Request::Write {
                topic: topic.clone(),
                id: id.clone(),
                step: Step::Rollback(op()),
            },
            "rollback",
        ),
        (
            Request::Visibility {
                topic: topic.clone(),
                id: id.clone(),
                step: Step::Register(visibility),
            },
            "register",
        ),
        (
            Request::Visibility {
                topic: topic.clone(),
                id: id.clone(),
                step: Step::Commit(op()),
            },
            "commit",
        ),
        (
            Request::Ack {
                topic: topic.clone(),
                id: id.clone(),
                step: Step::Register(ack),
            },
            "register",
        ),
        (
            Request::Ack {
                topic,
                id,
                step: Step::Rollback(op()),
            },
            "rollback",
        ),
    ];
    for (request, phase) in calls {
        let call: HttpCall = request.to_http();
        assert!(
            call.path.ends_with(&format!("/{phase}")),
            "{} goes to the {phase} route: {}",
            request.name(),
            call.path
        );
        let body: Value = serde_json::from_slice(&call.body).unwrap();
        assert_eq!(
            body["operationId"],
            operation_id.as_str(),
            "{} carries the operationId",
            request.name()
        );
        // The commit and the rollback say nothing but the operation.
        if phase != "register" {
            assert_eq!(body, json!({"operationId": operation_id}));
        }
    }
}

/// UC: UC-CLR-004
#[test]
fn an_invalid_body_on_a_cluster_route_is_a_400_with_a_validation_code() {
    let json = json_headers();
    let refused = |path: &str, body: &str| {
        Request::from_http("POST", path, &json, body.as_bytes())
            .expect_err(body)
            .code
    };
    let join = |extra: Value| {
        let mut body = json!({
            "nodeId": guid(1), "addresses": ["10.0.0.5"], "port": 8081, "version": "0.9.0"
        });
        if let (Some(object), Some(add)) = (body.as_object_mut(), extra.as_object()) {
            object.extend(add.clone());
        }
        body.to_string()
    };

    let cases = [
        (
            "/cluster/nodes/join",
            join(json!({"port": "oito"})),
            ErrorCode::ValidationFailed,
        ),
        (
            "/cluster/nodes/join",
            "{".to_owned(),
            ErrorCode::ValidationFailed,
        ),
        (
            "/cluster/nodes/join",
            r#"{"nodeId":"x"}"#.to_owned(),
            ErrorCode::ValidationRequired,
        ),
        (
            "/cluster/nodes/join",
            join(json!({"surplus": true})),
            ErrorCode::ValidationUnknownField,
        ),
        (
            "/cluster/nodes/commit",
            r#"{"operationId":5}"#.to_owned(),
            ErrorCode::ValidationFailed,
        ),
        (
            "/cluster/nodes/rollback",
            r#"{}"#.to_owned(),
            ErrorCode::ValidationRequired,
        ),
        (
            "/cluster/vote-remove",
            r#"{"voteId":"x","candidateNodeId":"y","openedBy":"z","reason":"node-failed","more":1}"#
                .to_owned(),
            ErrorCode::ValidationUnknownField,
        ),
    ];
    for (path, body, expected) in cases {
        let code = refused(path, &body);
        assert_eq!(code, expected, "{path} {body}");
        assert_eq!(code.status(), 400, "{path} {body}");
    }

    // The same for the fields that parse but are not valid values.
    let not_a_guid = JoinRequest {
        node_id: "nao-e-guid".into(),
        addresses: vec!["10.0.0.5".into()],
        port: 8081,
        version: "0.9.0".into(),
    };
    let failed = not_a_guid.validate().unwrap_err();
    assert_eq!(failed.code, ErrorCode::ValidationFailed);
    assert_eq!(failed.code.status(), 400);
    assert_eq!(failed.details[0].field, "nodeId");
    let blank = JoinRequest {
        version: "  ".into(),
        node_id: guid(1),
        ..not_a_guid
    };
    let required = blank.validate().unwrap_err();
    assert_eq!(required.code, ErrorCode::ValidationRequired);
    assert_eq!(required.code.status(), 400);

    // The query of a stream is validated the same way.
    let query = |path: &str| Request::from_http("GET", path, &[], b"").unwrap_err().code;
    assert_eq!(
        query("/cluster/topics?limit=abc"),
        ErrorCode::ValidationFailed
    );
    assert_eq!(
        query("/cluster/topics?sort=id"),
        ErrorCode::ValidationUnknownField
    );
}

/// UC: UC-CLR-005
#[test]
fn the_cluster_dtos_have_the_documented_fields_and_types() {
    let when = format_ms(BASE_MS + 60_000);

    round_trip::<Member>(member_wire(1));
    round_trip::<MemberTable>(json!({"version": 3, "members": [member_wire(1), member_wire(2)]}));
    round_trip::<JoinRequest>(json!({
        "nodeId": guid(1), "addresses": ["10.0.0.5"], "port": 8081, "version": "0.9.0"
    }));
    round_trip::<NodeRegisterRequest>(json!({
        "operationId": guid(1), "coordinatorId": guid(2), "node": member_wire(3)
    }));
    round_trip::<NodeUnregisterRequest>(json!({
        "operationId": guid(1), "coordinatorId": guid(2), "nodeId": guid(3), "reason": "drain"
    }));
    round_trip::<OperationRef>(json!({"operationId": guid(1)}));
    round_trip::<VoteRequest>(json!({
        "voteId": guid(1), "candidateNodeId": guid(2), "openedBy": guid(3),
        "reason": "channel-down"
    }));
    round_trip::<VoteResponse>(json!({"vote": "yes"}));
    round_trip::<VoteResponse>(json!({"vote": "no"}));
    round_trip::<DefinitionProposal>(json!({
        "kind": "topic", "operation": "update", "key": "pedidos",
        "body": {"id": guid(4), "name": "pedidos"}, "expectedId": guid(4)
    }));
    round_trip::<DefinitionApply>(json!({
        "kind": "token", "key": guid(5), "version": 4, "deleted": false,
        "body": {"id": guid(5)}, "relay": true
    }));
    round_trip::<MessageRegisterRequest>(json!({
        "operationId": guid(1), "coordinatorId": guid(2), "kind": "dead-letter",
        "message": record_wire(), "sourceTopic": "pedidos"
    }));
    round_trip::<VisibilityRegisterRequest>(json!({
        "operationId": guid(1), "coordinatorId": guid(2), "kind": "delivery",
        "invisibleUntil": when, "receiptToken": guid(5), "deliveryCount": 2
    }));
    round_trip::<AckRegisterRequest>(json!({
        "operationId": guid(1), "coordinatorId": guid(2), "receiptToken": guid(5),
        "deleteAt": when, "discard": true
    }));
    round_trip::<TakeRequest>(json!({"maxMessages": 5, "visibilityTimeout": 30}));
    round_trip::<TakeResponse>(json!({"messages": [record_wire()]}));
    round_trip::<MessageRecord>(record_wire());
    round_trip::<IdleResponse>(json!({
        "topicId": guid(6), "idleSeconds": 12, "sinceStart": false
    }));
    round_trip::<LossRecord>(json!({
        "topic": "pedidos", "id": guid(10), "block": "00000000000000ab", "detectedAt": when
    }));
    round_trip::<LossAck>(json!({"changed": true}));
    round_trip::<RecoverResult>(json!({"end": true, "count": 3}));
    round_trip::<StreamEnd>(json!({"end": true, "count": 1, "last": guid(10), "hasMore": false}));

    // The optional fields of a body do not travel when they have no value.
    let apply: DefinitionApply = serde_json::from_value(json!({
        "kind": "topic", "key": "pedidos", "version": 1, "deleted": true
    }))
    .unwrap();
    assert_eq!(apply.body, Value::Null);
    assert!(!apply.relay);
    assert!(serde_json::to_value(&apply).unwrap().get("relay").is_none());

    // A field the DTO does not define is refused, whatever the DTO.
    assert!(
        serde_json::from_value::<TakeRequest>(
            json!({"maxMessages": 5, "visibilityTimeout": 30, "extra": 1})
        )
        .is_err()
    );
    assert!(
        serde_json::from_value::<Member>(json!({
            "nodeId": guid(1), "addresses": [], "port": 1, "state": "dormant"
        }))
        .is_err()
    );

    // `TokenRecord`: the salt and the salted hash, never the secret.
    let token = TokenRecord {
        id: guid(7),
        name: "app".into(),
        permissions: vec![Permission::Publish, Permission::Consume],
        topic_prefixes: vec!["pedidos".into()],
        created_ms: BASE_MS,
        expires_ms: None,
        salt: "c2FsdA".into(),
        secret_hash: "aGFzaA".into(),
    };
    let wire = token_to_json(&token);
    assert_eq!(
        wire,
        json!({
            "id": token.id,
            "name": "app",
            "permissions": ["publish", "consume"],
            "permissionsSchema": 2,
            "topicPrefixes": ["pedidos"],
            "createdAt": format_ms(BASE_MS),
            "expiresAt": null,
            "salt": "c2FsdA",
            "secretHash": "aGFzaA"
        })
    );
    assert_eq!(token_from_json(&wire).unwrap(), token);
    assert!(wire.get("secret").is_none());

    // `TopicDefinition`: `null` in `ttl`, `maxDeliveries` and `maxMessageBytes` travels as `null`.
    let topic_wire = json!({
        "id": guid(8),
        "name": "pedidos",
        "type": "queue",
        "tags": {"time": "financeiro"},
        "createdAt": format_ms(BASE_MS),
        "volatile": false,
        "fifo": false,
        "maxMessageBytes": null,
        "ttl": null,
        "maxDeliveries": null
    });
    let topic = TopicDefinition::from_json(&topic_wire).unwrap();
    let back = topic.to_json();
    assert_eq!(back, topic_wire);
    for field in ["maxMessageBytes", "ttl", "maxDeliveries"] {
        assert!(
            back.get(field).is_some_and(Value::is_null),
            "{field} is sent as null, not left out"
        );
    }
}

/// UC: UC-STR-003
#[test]
fn the_stream_query_carries_from_to_and_limit_and_all_three_are_optional() {
    let block = "00000000000000ab".to_owned();
    let (first, last) = (guid(1), guid(9));

    let full = Request::from_http(
        "GET",
        &format!("/cluster/blocks/{block}/ids?from={first}&to={last}&limit=7"),
        &[],
        b"",
    )
    .unwrap();
    assert_eq!(
        full,
        Request::BlockIds {
            block: block.clone(),
            query: StreamQuery {
                from: Some(first.clone()),
                to: Some(last.clone()),
                limit: Some(7),
            },
        }
    );

    let bare = Request::from_http(
        "GET",
        &format!("/cluster/blocks/{block}/messages"),
        &[],
        b"",
    )
    .unwrap();
    assert_eq!(
        bare,
        Request::BlockMessages {
            block: block.clone(),
            query: StreamQuery::default(),
        }
    );

    // Each of the three alone, and the way back to the query string.
    for query in [
        StreamQuery {
            from: Some(first.clone()),
            ..StreamQuery::default()
        },
        StreamQuery {
            to: Some(last.clone()),
            ..StreamQuery::default()
        },
        StreamQuery {
            limit: Some(2),
            ..StreamQuery::default()
        },
    ] {
        let request = Request::BlockMessages {
            block: block.clone(),
            query,
        };
        let call = request.to_http();
        assert_eq!(
            Request::from_http(&call.method, &call.path, &call.headers, &call.body).unwrap(),
            request,
            "{}",
            call.path
        );
    }
    let call = Request::ListTopics(StreamQuery {
        from: Some(first.clone()),
        to: Some(last.clone()),
        limit: Some(3),
    })
    .to_http();
    assert_eq!(
        call.path,
        format!("/cluster/topics?from={first}&to={last}&limit=3")
    );
}

/// UC: UC-STR-006
#[test]
fn a_stretch_names_the_last_id_to_resume_from_and_the_resumption_asks_for_it_again() {
    let (a, b, c) = (guid(1), guid(2), guid(3));
    let page = StreamPage::new(
        vec![
            json!({"id": a, "topic": "t"}),
            json!({"id": b, "topic": "t"}),
        ],
        true,
    );
    assert_eq!(page.end.last.as_deref(), Some(b.as_str()));
    assert!(page.end.has_more);

    // The sender closed on the ceiling: the receiver reads the stretch back from the text...
    let read = StreamPage::from_ndjson(&page.to_ndjson()).unwrap();
    assert_eq!(read, page);
    // ... and asks for the next one from the last line it read (the line comes back; it is ignored there).
    let next = Request::BlockIds {
        block: "00000000000000ab".into(),
        query: StreamQuery {
            from: read.end.last.clone(),
            ..StreamQuery::default()
        },
    }
    .to_http();
    assert_eq!(
        next.path,
        format!("/cluster/blocks/00000000000000ab/ids?from={b}")
    );

    // The next stretch repeats the last line and goes on.
    let resumed = StreamPage::new(
        vec![
            json!({"id": b, "topic": "t"}),
            json!({"id": c, "topic": "t"}),
        ],
        false,
    );
    assert_eq!(resumed.items[0], read.items[1]);
    assert_eq!(resumed.end.last.as_deref(), Some(c.as_str()));

    // An empty stretch has nothing to resume from.
    let empty = StreamPage::new(Vec::new(), false);
    assert_eq!((empty.end.count, empty.end.last), (0, None));
}

/// UC: UC-ING-007
#[test]
fn the_node_names_itself_with_a_new_guid_v7_at_each_start_and_the_join_request_carries_it() {
    let start = |at_ms: u64| {
        NodeShared::new(
            id::new_at(at_ms),
            std::time::UNIX_EPOCH + std::time::Duration::from_millis(at_ms),
        )
    };
    let first_life = start(BASE_MS);
    let second_life = start(BASE_MS + 5_000);

    for node in [&first_life, &second_life] {
        assert!(id::is_guid_v7(&node.node_id()), "{}", node.node_id());
    }
    assert_eq!(id::unix_millis_of(&first_life.node_id()), Some(BASE_MS));
    assert_ne!(
        first_life.node_id(),
        second_life.node_id(),
        "a restart is a new identity"
    );

    // The identity is what the join request says (and what the receiver validates).
    let request = JoinRequest {
        node_id: second_life.node_id(),
        addresses: vec!["10.0.0.5".into()],
        port: 8081,
        version: "0.9.0".into(),
    };
    request.validate().unwrap();
    let wire = serde_json::to_value(&request).unwrap();
    assert_eq!(wire["nodeId"], second_life.node_id().as_str());
}
