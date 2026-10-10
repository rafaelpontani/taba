use serde_json::json;

use super::*;
use crate::cluster::members::MemberState;

fn guid(n: u64) -> String {
    id::new_at(1_791_288_000_000 + n)
}

/// UC: UC-ING-003
#[test]
fn the_join_request_has_the_documented_fields() {
    let node_id = guid(1);
    let request: JoinRequest = serde_json::from_value(json!({
        "nodeId": node_id,
        "addresses": ["10.0.0.5"],
        "port": 8081,
        "version": "0.9.0"
    }))
    .unwrap();
    assert_eq!(request.port, 8081);
    request.validate().unwrap();

    let extra = serde_json::from_value::<JoinRequest>(json!({
        "nodeId": guid(1), "addresses": ["10.0.0.5"], "port": 8081, "version": "1", "extra": 1
    }));
    assert!(extra.is_err(), "campo que a rota não define é recusado");
    let missing = serde_json::from_value::<JoinRequest>(json!({"nodeId": guid(1)}));
    assert!(missing.is_err());
}

#[test]
fn the_join_request_is_validated() {
    let ok = JoinRequest {
        node_id: guid(1),
        addresses: vec!["10.0.0.5".into()],
        port: 8081,
        version: "0.9.0".into(),
    };
    ok.validate().unwrap();

    let cases = [
        (
            JoinRequest {
                node_id: "x".into(),
                ..ok.clone()
            },
            "nodeId",
        ),
        (
            JoinRequest {
                addresses: vec![],
                ..ok.clone()
            },
            "addresses",
        ),
        (
            JoinRequest {
                addresses: vec![" ".into()],
                ..ok.clone()
            },
            "addresses",
        ),
        (
            JoinRequest {
                port: 0,
                ..ok.clone()
            },
            "port",
        ),
        (
            JoinRequest {
                version: "".into(),
                ..ok.clone()
            },
            "version",
        ),
    ];
    for (request, field) in cases {
        let error = request.validate().unwrap_err();
        assert_eq!(error.details[0].field, field, "{request:?}");
    }
}

#[test]
fn register_unregister_and_commit_use_camel_case() {
    let register = NodeRegisterRequest {
        operation_id: guid(1),
        coordinator_id: guid(2),
        node: Member {
            node_id: guid(3),
            addresses: vec!["10.0.0.3".into()],
            port: 8081,
            state: MemberState::Syncing,
        },
    };
    let value = serde_json::to_value(&register).unwrap();
    assert_eq!(value["coordinatorId"], json!(register.coordinator_id));
    assert_eq!(value["node"]["state"], json!("syncing"));
    assert_eq!(
        serde_json::from_value::<NodeRegisterRequest>(value).unwrap(),
        register
    );
    register.validate().unwrap();

    let unregister: NodeUnregisterRequest = serde_json::from_value(json!({
        "operationId": guid(4), "coordinatorId": guid(2), "nodeId": guid(3), "reason": "drain"
    }))
    .unwrap();
    assert_eq!(unregister.reason, UnregisterReason::Drain);
    unregister.validate().unwrap();

    let bad = OperationRef {
        operation_id: "nao-e-guid".into(),
    };
    assert_eq!(bad.validate().unwrap_err().details[0].field, "operationId");
}

#[test]
fn the_ack_carries_the_operation_id() {
    let ack = Ack {
        operation_id: guid(1),
        accepted: true,
    };
    let value = serde_json::to_value(&ack).unwrap();
    assert_eq!(
        value,
        json!({"operationId": ack.operation_id, "accepted": true})
    );
}

// ---- O codec HTTP e os corpos das mensagens ----

use serde_json::Value;

use super::http::{HttpReply, error_reply};

fn record(content_type: &str, body: &[u8]) -> MessageRecord {
    MessageRecord {
        id: guid(10),
        topic: "pedidos".into(),
        body: body.to_vec(),
        content_type: content_type.into(),
        group_id: Some("g1".into()),
        deduplication_id: None,
        delivery_count: 2,
        invisible_until_ms: Some(1_791_288_060_000),
        receipt_token: Some(guid(11)),
        dead_lettered_ms: None,
        dead_letter_reason: None,
        delete_at_ms: None,
    }
}

/// UC: UC-STR-012
#[test]
fn a_json_body_goes_native_and_the_rest_goes_in_base64() {
    let json = record("application/json", br#"{"a": 1}"#);
    let text = serde_json::to_string(&json).unwrap();
    assert!(text.contains(r#""body":{"a": 1}"#), "{text}");
    assert!(!text.contains("bodyEncoding"));
    assert_eq!(serde_json::from_str::<MessageRecord>(&text).unwrap(), json);

    let plus_json = record("application/vnd.api+json", b"[1,2]");
    let text = serde_json::to_string(&plus_json).unwrap();
    assert!(text.contains(r#""body":[1,2]"#), "{text}");

    // Texto e binário vão em base64, com `bodyEncoding`.
    for (content_type, body) in [
        ("text/plain", "olá".as_bytes()),
        ("application/octet-stream", &[0u8, 255, 7][..]),
    ] {
        let original = record(content_type, body);
        let text = serde_json::to_string(&original).unwrap();
        assert!(text.contains(r#""bodyEncoding":"base64""#), "{text}");
        assert_eq!(
            serde_json::from_str::<MessageRecord>(&text).unwrap(),
            original
        );
    }

    // JSON com espaços nas pontas não pode mudar os bytes: vai em base64 se não sobrevive intacto.
    let padded = record("application/json", b"  {\"a\":1}  ");
    let text = serde_json::to_string(&padded).unwrap();
    assert_eq!(
        serde_json::from_str::<MessageRecord>(&text).unwrap(),
        padded
    );
}

#[test]
fn the_dates_of_a_record_travel_as_text_and_come_back() {
    let mut original = record("application/json", b"1");
    original.dead_lettered_ms = Some(1_791_288_000_123);
    original.dead_letter_reason = Some("max-deliveries".into());
    original.invisible_until_ms = None;
    let text = serde_json::to_string(&original).unwrap();
    let value: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(value["deadLetteredAt"], "2026-10-06T12:00:00.123Z");
    assert_eq!(value["invisibleUntil"], Value::Null);
    let back: MessageRecord = serde_json::from_str(&text).unwrap();
    assert_eq!(back, original);
    assert_eq!(
        back.published_ms(),
        id::unix_millis_of(&original.id).unwrap()
    );
}

fn requests() -> Vec<Request> {
    let op = || OperationRef {
        operation_id: guid(1),
    };
    let message = MessageRegisterRequest {
        operation_id: guid(2),
        coordinator_id: guid(3),
        kind: WriteKind::Publish,
        message: record("application/json", br#"{"x":true}"#),
        source_topic: None,
    };
    let dead_letter = MessageRegisterRequest {
        kind: WriteKind::DeadLetter,
        source_topic: Some("pedidos".into()),
        ..message.clone()
    };
    let visibility = VisibilityRegisterRequest {
        operation_id: guid(4),
        coordinator_id: guid(3),
        kind: VisibilityKind::Delivery,
        invisible_until_ms: 1_791_288_120_000,
        receipt_token: guid(5),
        delivery_count: Some(1),
    };
    let ack = AckRegisterRequest {
        operation_id: guid(6),
        coordinator_id: guid(3),
        receipt_token: guid(5),
        delete_at_ms: Some(1_791_288_300_000),
    };
    let topic = DefinitionProposal {
        kind: DefinitionKind::Topic,
        operation: DefinitionOperation::Create,
        key: "pedidos".into(),
        body: serde_json::json!({"id": guid(8), "name": "pedidos", "type": "queue"}),
        expected_id: Some(guid(8)),
    };
    let token = DefinitionApply {
        kind: DefinitionKind::Token,
        key: guid(12),
        version: 3,
        deleted: true,
        body: serde_json::Value::Null,
    };
    let t = || "pedidos".to_owned();
    let id = || guid(10);
    vec![
        Request::ListNodes,
        Request::Join(JoinRequest {
            node_id: guid(1),
            addresses: vec!["10.0.0.1".into()],
            port: 8081,
            version: "0.9.0".into(),
        }),
        Request::Register(NodeRegisterRequest {
            operation_id: guid(1),
            coordinator_id: guid(2),
            node: Member {
                node_id: guid(3),
                addresses: vec!["10.0.0.3".into()],
                port: 8081,
                state: MemberState::Active,
            },
        }),
        Request::Unregister(NodeUnregisterRequest {
            operation_id: guid(1),
            coordinator_id: guid(2),
            node_id: guid(3),
            reason: UnregisterReason::Drain,
        }),
        Request::Commit(op()),
        Request::Rollback(op()),
        Request::DefinitionPropose(topic),
        Request::ListTopics(StreamQuery {
            from: Some(guid(1)),
            to: None,
            limit: Some(50),
        }),
        Request::DefinitionApply(token),
        Request::ListTokens(StreamQuery::default()),
        Request::ListTombstones(StreamQuery {
            from: Some("topic:pedidos".to_owned()),
            to: None,
            limit: Some(20),
        }),
        Request::Write {
            topic: t(),
            id: id(),
            step: Step::Register(message),
        },
        Request::Write {
            topic: t(),
            id: id(),
            step: Step::Register(dead_letter),
        },
        Request::Write {
            topic: t(),
            id: id(),
            step: Step::Commit(op()),
        },
        Request::Write {
            topic: t(),
            id: id(),
            step: Step::Rollback(op()),
        },
        Request::Visibility {
            topic: t(),
            id: id(),
            step: Step::Register(visibility),
        },
        Request::Visibility {
            topic: t(),
            id: id(),
            step: Step::Commit(op()),
        },
        Request::Ack {
            topic: t(),
            id: id(),
            step: Step::Register(ack),
        },
        Request::Ack {
            topic: t(),
            id: id(),
            step: Step::Rollback(op()),
        },
        Request::Take {
            topic: t(),
            request: TakeRequest {
                max_messages: 3,
                visibility_timeout: 120,
            },
        },
        Request::Vote(VoteRequest {
            vote_id: guid(20),
            candidate_node_id: guid(21),
            opened_by: guid(22),
            reason: VoteReason::QueueOverflow,
        }),
        Request::Status,
        Request::Forward(ForwardRequest {
            method: "POST".into(),
            path_and_query: "/topics/pedidos/messages?groupId=g1".into(),
            headers: vec![
                ("content-type".into(), "application/json".into()),
                ("authorization".into(), "Bearer x".into()),
            ],
            body: b"{\"a\":1}".to_vec(),
            client_ip: Some("203.0.113.9".into()),
        }),
        Request::BlockIds {
            block: "00000000000000ab".into(),
            query: StreamQuery {
                from: Some(guid(1)),
                to: Some(guid(9)),
                limit: Some(200),
            },
        },
        Request::BlockMessages {
            block: "00000000000000ab".into(),
            query: StreamQuery::default(),
        },
        Request::BlockMessage {
            block: "00000000000000ab".into(),
            id: guid(4),
        },
        Request::MessageLoss(StreamQuery {
            from: Some(guid(4)),
            to: None,
            limit: Some(10),
        }),
        Request::LossRegister(LossRecord {
            topic: "pedidos".into(),
            id: guid(4),
            block: "00000000000000ab".into(),
            detected_at: "2026-10-07T12:00:00.000Z".into(),
        }),
        Request::LossRemove {
            topic: "pedidos".into(),
            id: guid(4),
        },
        Request::Recover {
            records: vec![
                record("application/json", br#"{"a":1}"#),
                record("application/octet-stream", &[0, 1, 2]),
            ],
            replicate: true,
        },
        Request::Recover {
            records: vec![record("text/plain", b"oi")],
            replicate: false,
        },
        Request::FifoPublish {
            topic: "pedidos".into(),
            record: record("application/json", br#"{"a":1}"#),
        },
    ]
}

/// UC: UC-ESC-025, UC-INV-013, UC-DEF-033, UC-RTN-001, UC-RTN-003, UC-FIF-026
#[test]
fn every_request_survives_the_trip_to_http_and_back() {
    for request in requests() {
        let call = request.to_http();
        let back = Request::from_http(&call.method, &call.path, &call.headers, &call.body)
            .unwrap_or_else(|e| panic!("{}: {e}", request.name()));
        assert_eq!(
            back,
            request,
            "{} em {} {}",
            request.name(),
            call.method,
            call.path
        );
    }
}

/// UC: UC-DEF-033
#[test]
fn the_routes_are_the_ones_of_the_document() {
    let paths: Vec<(String, String)> = requests()
        .iter()
        .map(|r| {
            let call = r.to_http();
            (
                call.method,
                call.path
                    .split('?')
                    .next()
                    .unwrap()
                    .split('/')
                    .map(|s| if id::is_guid_v7(s) { "{id}" } else { s })
                    .collect::<Vec<_>>()
                    .join("/"),
            )
        })
        .collect();
    let has = |method: &str, path: &str| paths.iter().any(|(m, p)| m == method && p == path);
    let id = "{id}";
    assert!(has("GET", "/cluster/nodes"));
    assert!(has("POST", "/cluster/nodes/join"));
    assert!(has("POST", "/cluster/nodes/rollback"));
    assert!(has("POST", "/cluster/definitions/propose"));
    assert!(has("GET", "/cluster/topics"));
    assert!(has("GET", "/cluster/definitions/tombstones"));
    assert!(has("POST", "/cluster/definitions/apply"));
    assert!(has(
        "POST",
        &format!("/cluster/topics/pedidos/messages/{id}/register")
    ));
    assert!(has(
        "POST",
        &format!("/cluster/topics/pedidos/messages/{id}/commit")
    ));
    assert!(has(
        "POST",
        &format!("/cluster/topics/pedidos/messages/{id}/visibility/register")
    ));
    assert!(has(
        "POST",
        &format!("/cluster/topics/pedidos/messages/{id}/ack/rollback")
    ));
    assert!(has("POST", "/cluster/topics/pedidos/messages/take"));
    assert!(has("POST", "/cluster/vote-remove"));
    assert!(has("GET", "/cluster/status"));
    assert!(has("GET", "/cluster/blocks/00000000000000ab/ids"));
    assert!(has("GET", "/cluster/blocks/00000000000000ab/messages"));
    assert!(has(
        "GET",
        &format!("/cluster/blocks/00000000000000ab/messages/{id}")
    ));
    assert!(has("POST", "/cluster/forward/topics/pedidos/messages"));
    assert!(has("GET", "/cluster/message-loss"));
    assert!(has("POST", "/cluster/message-loss"));
    assert!(has(
        "DELETE",
        &format!("/cluster/message-loss/pedidos/{id}")
    ));
    assert!(has("POST", "/cluster/messages/recover"));
    assert!(has("POST", "/cluster/topics/pedidos/fifo/publish"));
}

/// A linha curta da FIFO (mensagem confirmada) viaja sem corpo e com `deleteAt`.
///
/// UC: UC-FIF-041
#[test]
fn the_short_line_of_a_confirmed_fifo_message_travels_without_a_body() {
    let mut line = record("application/json", b"");
    line.delete_at_ms = Some(1_791_288_300_000);
    let text = serde_json::to_string(&line).unwrap();
    assert!(!text.contains("\"body\""), "{text}");
    assert!(text.contains("\"deleteAt\""), "{text}");
    let back: MessageRecord = serde_json::from_str(&text).unwrap();
    assert_eq!(back, line);
    assert!(back.body.is_empty());
}

/// O corpo do `recover` é um stream NDJSON: sem a linha final, com a contagem errada ou com outro
/// `Content-Type`, a requisição é recusada e quem envia reenvia tudo.
///
/// UC: UC-RTN-003, UC-RTN-009
#[test]
fn the_recover_body_is_a_stream_with_a_mandatory_last_line() {
    let ndjson = vec![("content-type".to_owned(), "application/x-ndjson".to_owned())];
    let records = vec![record("text/plain", b"oi"), record("text/plain", b"ola")];
    let body = recover_body(&records);
    let text = String::from_utf8(body.clone()).unwrap();
    assert_eq!(text.lines().count(), 3, "duas linhas de dados e a final");
    assert!(text.lines().last().unwrap().contains(r#""end":true"#));
    assert_eq!(parse_recover_body(&body).unwrap(), records);

    let refused = |headers: &[(String, String)], path: &str, body: &[u8]| {
        Request::from_http("POST", path, headers, body)
            .unwrap_err()
            .code
    };
    let without_end: String = text.lines().take(2).collect::<Vec<_>>().join("\n");
    assert_eq!(
        refused(&ndjson, "/cluster/messages/recover", without_end.as_bytes()),
        ErrorCode::ValidationFailed,
        "um corte entre duas linhas não se distingue de um fim normal sem a linha final"
    );
    let wrong_count = text.replace(r#""count":2"#, r#""count":3"#);
    assert_eq!(
        refused(&ndjson, "/cluster/messages/recover", wrong_count.as_bytes()),
        ErrorCode::ValidationFailed
    );
    assert_eq!(
        refused(&[], "/cluster/messages/recover", &body),
        ErrorCode::MediaUnsupported
    );
    assert_eq!(
        refused(&ndjson, "/cluster/messages/recover?replicate=talvez", &body),
        ErrorCode::ValidationFailed
    );
    assert_eq!(
        refused(&ndjson, "/cluster/messages/recover?outro=1", &body),
        ErrorCode::ValidationUnknownField
    );
    let local = Request::from_http(
        "POST",
        "/cluster/messages/recover?replicate=false",
        &ndjson,
        &body,
    )
    .unwrap();
    assert!(matches!(
        local,
        Request::Recover {
            replicate: false,
            ..
        }
    ));
}

#[test]
fn unknown_routes_wrong_verbs_and_bad_bodies_are_refused() {
    let json = vec![("content-type".to_owned(), "application/json".to_owned())];
    let refused = |method: &str, path: &str, headers: &[(String, String)], body: &[u8]| {
        Request::from_http(method, path, headers, body)
            .unwrap_err()
            .code
    };
    assert_eq!(
        refused("GET", "/topics", &[], b""),
        ErrorCode::RouteNotFound
    );
    assert_eq!(
        refused("GET", "/cluster/desconhecida", &[], b""),
        ErrorCode::RouteNotFound
    );
    assert_eq!(
        refused("GET", "/cluster/nodes/join", &[], b""),
        ErrorCode::RouteNotFound
    );
    assert_eq!(
        refused("POST", "/cluster/nodes", &json, b"{}"),
        ErrorCode::RouteNotFound
    );
    assert_eq!(
        refused("POST", "/cluster/topics/pedidos", &json, b"{}"),
        ErrorCode::RouteNotFound
    );
    assert_eq!(
        refused("POST", "/cluster/nodes/join", &json, br#"{"nodeId":"x"}"#),
        ErrorCode::ValidationRequired
    );
    assert_eq!(
        refused(
            "POST",
            "/cluster/nodes/commit",
            &json,
            br#"{"operationId":"x","extra":1}"#
        ),
        ErrorCode::ValidationUnknownField
    );
    assert_eq!(
        refused(
            "POST",
            "/cluster/nodes/commit",
            &[],
            b"{\"operationId\":\"x\"}"
        ),
        ErrorCode::MediaUnsupported
    );
    assert_eq!(
        refused("GET", "/cluster/topics?cursor=1", &[], b""),
        ErrorCode::ValidationUnknownField
    );
    assert_eq!(
        refused("GET", "/cluster/topics?limit=abc", &[], b""),
        ErrorCode::ValidationFailed
    );
}

/// UC: UC-STR-004, UC-STR-001
#[test]
fn a_stream_ends_with_the_end_line_and_without_it_the_chunk_is_a_failure() {
    let items = vec![
        serde_json::json!({"id": guid(1), "name": "a"}),
        serde_json::json!({"id": guid(2), "name": "b"}),
    ];
    let page = StreamPage::new(items, true);
    assert_eq!(
        page.end.last.as_deref(),
        Some(page.items[1]["id"].as_str().unwrap())
    );
    let text = page.to_ndjson();
    assert_eq!(
        text.lines().count(),
        3,
        "um objeto por linha e a linha final"
    );
    assert_eq!(StreamPage::from_ndjson(&text).unwrap(), page);

    // Cortado antes da linha final: falho, mesmo que as linhas de dados estejam inteiras.
    let truncated: String = text.lines().take(2).map(|l| format!("{l}\n")).collect();
    assert!(StreamPage::from_ndjson(&truncated).is_err());
    // A contagem da linha final tem de conferir.
    let wrong = text.replace("\"count\":2", "\"count\":5");
    assert!(StreamPage::from_ndjson(&wrong).is_err());
    // Erro depois do `200`: o emissor escreve `{"error":...}`.
    let failed = format!(
        "{}\n{{\"error\":\"disco\"}}\n",
        text.lines().next().unwrap()
    );
    assert!(StreamPage::from_ndjson(&failed).is_err());
}

#[test]
fn the_replies_are_read_by_the_kind_of_the_request() {
    let ack = Response::Ack(Ack {
        operation_id: guid(1),
        accepted: true,
    });
    let commit = Request::Commit(OperationRef {
        operation_id: guid(1),
    });
    assert_eq!(commit.parse_reply(ack.clone().into_http()).unwrap(), ack);

    let page = Response::Stream(StreamPage::new(
        vec![serde_json::json!({"id": guid(1)})],
        false,
    ));
    let list = Request::ListTopics(StreamQuery::default());
    let reply = page.clone().into_http();
    assert_eq!(reply.headers[0].1, "application/x-ndjson");
    assert_eq!(list.parse_reply(reply).unwrap(), page);

    let taken = Response::Taken(TakeResponse {
        messages: vec![record("application/json", b"{}")],
    });
    let take = Request::Take {
        topic: "pedidos".into(),
        request: TakeRequest {
            max_messages: 1,
            visibility_timeout: 30,
        },
    };
    assert_eq!(take.parse_reply(taken.clone().into_http()).unwrap(), taken);

    let vote = Request::Vote(VoteRequest {
        vote_id: guid(20),
        candidate_node_id: guid(21),
        opened_by: guid(22),
        reason: VoteReason::ChannelDown,
    });
    let yes = Response::Vote(VoteResponse { vote: Ballot::Yes });
    assert_eq!(vote.parse_reply(yes.clone().into_http()).unwrap(), yes);

    // O estado do nó é o JSON do `NodeStatus`, sem forma própria no protocolo.
    let status = Response::Status(serde_json::json!({ "nodeId": guid(1), "state": "active" }));
    assert_eq!(
        Request::Status
            .parse_reply(status.clone().into_http())
            .unwrap(),
        status
    );

    // Uma mensagem do bloco volta inteira; as listas do bloco são streams.
    let single = Request::BlockMessage {
        block: "00000000000000ab".into(),
        id: guid(4),
    };
    let one = Response::Record(record("application/json", br#"{"a":1}"#));
    assert_eq!(single.parse_reply(one.clone().into_http()).unwrap(), one);
    let ids = Request::BlockIds {
        block: "00000000000000ab".into(),
        query: StreamQuery::default(),
    };
    assert_eq!(ids.parse_reply(page.clone().into_http()).unwrap(), page);

    // A lista de perdas é um stream; o registro e a baixa devolvem `changed`; o `recover`, a contagem.
    let loss = Request::MessageLoss(StreamQuery::default());
    assert_eq!(loss.parse_reply(page.clone().into_http()).unwrap(), page);
    let register = Request::LossRegister(LossRecord {
        topic: "pedidos".into(),
        id: guid(4),
        block: "00000000000000ab".into(),
        detected_at: "2026-10-07T12:00:00.000Z".into(),
    });
    let changed = Response::Loss(LossAck { changed: true });
    assert_eq!(
        register.parse_reply(changed.clone().into_http()).unwrap(),
        changed
    );
    let recover = Request::Recover {
        records: vec![],
        replicate: true,
    };
    let done = Response::Recovered(RecoverResult::new(2));
    assert_eq!(recover.parse_reply(done.clone().into_http()).unwrap(), done);
}

/// UC: UC-FWD-004
#[test]
fn a_forwarded_reply_comes_back_as_the_destination_gave_it() {
    let forward = Request::Forward(ForwardRequest {
        method: "GET".into(),
        path_and_query: "/topics".into(),
        headers: vec![],
        body: vec![],
        client_ip: None,
    });
    for status in [200u16, 409, 413, 429, 503] {
        let reply = HttpReply {
            status,
            headers: vec![("x-extra".into(), "1".into())],
            body: b"corpo".to_vec(),
        };
        let parsed = forward.parse_reply(reply).unwrap();
        assert_eq!(
            parsed,
            Response::Forwarded(ForwardResponse {
                status,
                headers: vec![("x-extra".into(), "1".into())],
                body: b"corpo".to_vec(),
            })
        );
    }
}

#[test]
fn an_error_reply_becomes_the_error_of_the_peer_with_retry_after() {
    let error = TabaError::new(ErrorCode::ClusterNodeSyncing, "sem tabela")
        .with_detail("x", ErrorCode::ValidationFailed, "y")
        .with_retry_after(7);
    let reply = error_reply(&error, "req-1");
    assert_eq!(reply.status, 503);
    let request = Request::ListNodes;
    let back = request.parse_reply(reply).unwrap_err();
    assert_eq!(back.code, ErrorCode::ClusterNodeSyncing);
    assert_eq!(back.message, "sem tabela");
    assert_eq!(back.retry_after, Some(7));
    assert_eq!(back.details[0].field, "x");

    // Uma resposta que não é o `ErrorResponse` não derruba a leitura.
    let odd = HttpReply {
        status: 500,
        headers: vec![],
        body: b"<html>".to_vec(),
    };
    assert_eq!(
        request.parse_reply(odd).unwrap_err().code,
        ErrorCode::InternalError
    );
}

#[test]
fn the_token_json_round_trips() {
    use crate::core::token::{Permission, TokenRecord};
    let token = TokenRecord {
        id: guid(1),
        name: "app".into(),
        permissions: vec![Permission::Publish, Permission::Consume],
        topic_prefixes: vec!["pedidos".into()],
        created_ms: 1_791_288_000_000,
        expires_ms: Some(1_791_374_400_000),
        salt: "c2FsdA".into(),
        secret_hash: "aGFzaA".into(),
    };
    assert_eq!(token_from_json(&token_to_json(&token)).unwrap(), token);
    let mut no_expiry = token.clone();
    no_expiry.expires_ms = None;
    assert_eq!(
        token_from_json(&token_to_json(&no_expiry)).unwrap(),
        no_expiry
    );
    assert!(token_from_json(&serde_json::json!({"id": "x"})).is_err());
}

/// UC: UC-CAN-002, UC-CAN-004
#[test]
fn channel_frames_are_small_json_signals_with_a_type_and_never_carry_message_content() {
    let signals = [
        (
            Signal::Listeners {
                topic: "pedidos".into(),
                has_listeners: true,
            },
            json!({"type": "listeners", "topic": "pedidos", "hasListeners": true}),
        ),
        (
            Signal::Ready {
                topic: "pedidos".into(),
                has_ready: false,
            },
            json!({"type": "ready", "topic": "pedidos", "hasReady": false}),
        ),
        (
            Signal::Notify {
                topic: "pedidos".into(),
                count: 3,
            },
            json!({"type": "notify", "topic": "pedidos", "count": 3}),
        ),
        (
            Signal::NodeState {
                state: "draining".into(),
            },
            json!({"type": "node-state", "state": "draining"}),
        ),
        (
            Signal::Frozen {
                node_id: "n3".into(),
                frozen_at: 1_791_288_000_000,
            },
            json!({"type": "frozen", "nodeId": "n3", "frozenAt": 1_791_288_000_000_u64}),
        ),
    ];
    for (signal, expected) in signals {
        // Só o `type` e os campos de contagem e de estado: nada de corpo nem de lista de IDs.
        assert_eq!(serde_json::to_value(&signal).unwrap(), expected);
        let back: Signal = serde_json::from_value(expected).unwrap();
        assert_eq!(back, signal);
    }
    // O sinal de vida e o `confirm` completam os seis tipos do doc 06c.
    let confirm = serde_json::to_value(Confirm::new(vec![1, 2])).unwrap();
    assert_eq!(confirm, json!({"type": "confirm", "seqs": [1, 2]}));
    let keepalive = serde_json::to_value(Keepalive {
        at: "2026-10-05T12:00:00.000Z".into(),
        load: 2,
        version: 7,
        state: "active".into(),
        ready: vec![ReadyCount {
            topic: "pedidos".into(),
            count: 4,
        }],
    })
    .unwrap();
    assert_eq!(
        keepalive,
        json!({
            "at": "2026-10-05T12:00:00.000Z", "load": 2, "version": 7, "state": "active",
            "ready": [{"topic": "pedidos", "count": 4}]
        })
    );
}

/// UC: UC-CAN-006
#[test]
fn channel_messages_are_json_text_and_the_confirm_is_told_by_its_type() {
    let batch = ChannelMessage::Batch(ChannelBatch {
        to: String::new(),
        from: guid(1),
        keepalive: Some(Keepalive {
            at: "2026-10-05T12:00:00.000Z".into(),
            load: 2,
            version: 7,
            state: "active".into(),
            ready: vec![ReadyCount {
                topic: "pedidos".into(),
                count: 4,
            }],
        }),
        frames: vec![
            ChannelFrame {
                seq: 1,
                signal: Signal::Ready {
                    topic: "pedidos".into(),
                    has_ready: true,
                },
            },
            ChannelFrame {
                seq: 2,
                signal: Signal::Notify {
                    topic: "pedidos".into(),
                    count: 4,
                },
            },
        ],
    });
    let empty = ChannelMessage::Batch(ChannelBatch {
        to: String::new(),
        from: guid(1),
        keepalive: None,
        frames: vec![],
    });
    let confirm = ChannelMessage::Confirm(Confirm::new(vec![3]));
    assert_eq!(confirm.to_text(), r#"{"type":"confirm","seqs":[3]}"#);
    for message in [batch, empty, confirm] {
        assert_eq!(ChannelMessage::from_text(&message.to_text()), Ok(message));
    }
    assert!(ChannelMessage::from_text("não é JSON").is_err());
}
