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
