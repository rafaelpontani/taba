use super::*;
use crate::core::id;

fn queue() -> TopicDefinition {
    TopicDefinition {
        id: id::new_at(1_791_288_000_000),
        name: "pedidos".into(),
        created_ms: 1_791_288_000_000,
        tags: BTreeMap::from([("time".into(), "financeiro".into())]),
        spec: Spec::Queue(QueueSpec {
            volatile: true,
            idle_timeout: Some(300),
            fifo: true,
            max_message_bytes: None,
            ttl: Some(86_400),
            max_deliveries: None,
        }),
    }
}

fn distributor() -> TopicDefinition {
    TopicDefinition {
        id: id::new_at(1_791_288_000_000),
        name: "dist".into(),
        created_ms: 1_791_288_000_000,
        tags: BTreeMap::new(),
        spec: Spec::Distributor {
            prefixes: vec!["pedidos".into(), "x".into()],
        },
    }
}

/// UC: UC-CNV-002
#[test]
fn queue_json_has_null_for_defaults_and_distributor_json_omits_queue_fields() {
    let q = queue().to_json();
    assert!(q["maxMessageBytes"].is_null() && q.get("maxMessageBytes").is_some());
    assert_eq!(q["ttl"], 86_400);
    assert_eq!(q["createdAt"], "2026-10-06T12:00:00.000Z");
    assert!(q.get("prefixes").is_none());

    let d = distributor().to_json();
    for absent in [
        "volatile",
        "fifo",
        "ttl",
        "maxDeliveries",
        "maxMessageBytes",
        "idleTimeout",
    ] {
        assert!(
            d.get(absent).is_none(),
            "{absent} does not apply to the distributor"
        );
    }
    assert_eq!(d["prefixes"], json!(["pedidos", "x"]));
}

#[test]
fn idle_timeout_only_appears_on_volatile_queues() {
    let mut q = queue();
    if let Spec::Queue(spec) = &mut q.spec {
        spec.volatile = false;
        spec.idle_timeout = None;
    }
    assert!(q.to_json().get("idleTimeout").is_none());
}

#[test]
fn json_round_trips_for_both_types() {
    for def in [queue(), distributor()] {
        assert_eq!(TopicDefinition::from_json(&def.to_json()).unwrap(), def);
    }
}

#[test]
fn from_json_reports_what_is_wrong() {
    assert!(TopicDefinition::from_json(&json!([])).is_err());
    assert!(
        TopicDefinition::from_json(&json!({ "id": "x" }))
            .unwrap_err()
            .contains("name")
            || TopicDefinition::from_json(&json!({ "id": "x" }))
                .unwrap_err()
                .contains("createdAt")
    );
    let mut bad = queue().to_json();
    bad["type"] = json!("broadcast");
    assert!(
        TopicDefinition::from_json(&bad)
            .unwrap_err()
            .contains("broadcast")
    );
}

#[test]
fn dead_letter_id_is_stable_distinct_and_still_a_guid() {
    let source = id::new_at(1_791_288_000_000);
    let derived = dead_letter_id(&source);
    assert_ne!(derived, source);
    assert_eq!(derived, dead_letter_id(&source));
    assert_eq!(derived.len(), source.len());
    assert_eq!(derived[..35], source[..35]);
    assert!(id::is_guid_v7(&derived));
}
