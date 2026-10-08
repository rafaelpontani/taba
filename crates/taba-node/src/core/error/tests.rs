use super::*;
use std::collections::HashSet;

/// UC: UC-CNV-008
#[test]
fn every_code_has_the_documented_status() {
    // Tabela escrita à mão a partir de 06d, seção 8.3.6: não derivar da implementação.
    let expected: &[(&str, u16)] = &[
        ("validation.failed", 400),
        ("validation.required", 400),
        ("validation.unknown-field", 400),
        ("validation.out-of-range", 400),
        ("validation.invalid-name", 400),
        ("validation.reserved-name", 400),
        ("validation.immutable-field", 400),
        ("validation.not-applicable", 400),
        ("validation.invalid-body", 400),
        ("validation.invalid-content-type", 400),
        ("validation.group-id-required", 400),
        ("validation.invalid-cursor", 400),
        ("validation.max-bytes-above-hard-limit", 400),
        ("auth.unauthorized", 401),
        ("auth.forbidden", 403),
        ("auth.out-of-scope", 403),
        ("ops.ip-not-allowed", 403),
        ("cluster.not-member", 403),
        ("cluster.node-frozen", 403),
        ("topic.not-found", 404),
        ("message.not-found", 404),
        ("token.not-found", 404),
        ("route.not-found", 404),
        ("cluster.operation-unknown", 404),
        ("topic.already-exists", 409),
        ("topic.wrong-type", 409),
        ("topic.read-only", 409),
        ("message.stale-receipt", 409),
        ("drain.conflict", 409),
        ("cluster.conflict", 409),
        ("message.too-large", 413),
        ("media.unsupported", 415),
        ("fifo.misdirected", 421),
        ("cluster.wrong-node", 421),
        ("topic.fifo-needs-replication", 422),
        ("distributor.too-many-targets", 422),
        ("topic.volatile-limit", 422),
        ("cluster.join-unreachable", 422),
        ("rate-limit.exceeded", 429),
        ("internal.error", 500),
        ("forward.bad-gateway", 502),
        ("forward.timeout", 504),
        ("cluster.quorum-unavailable", 503),
        ("cluster.registration-failed", 503),
        ("cluster.commit-timeout", 503),
        ("node.failed", 503),
        ("cluster.node-syncing", 503),
        ("cluster.node-draining", 503),
        ("fifo.primary-unavailable", 503),
        ("delivery.unavailable", 503),
    ];

    let actual: Vec<(&str, u16)> = ErrorCode::ALL
        .iter()
        .map(|c| (c.as_str(), c.status()))
        .collect();
    for (code, status) in expected {
        assert!(
            actual.contains(&(*code, *status)),
            "{code} deveria ser {status}"
        );
    }
    assert_eq!(
        actual.len(),
        expected.len(),
        "o catálogo e a tabela do teste divergem"
    );
    let unique: HashSet<_> = actual.iter().map(|(c, _)| *c).collect();
    assert_eq!(unique.len(), actual.len(), "código repetido");
}

/// UC: UC-CNV-006
#[test]
fn codes_follow_group_dot_reason() {
    for code in ErrorCode::ALL {
        let (group, reason) = code.as_str().split_once('.').expect("grupo.motivo");
        assert!(!group.is_empty() && !reason.is_empty());
        assert!(
            code.as_str()
                .chars()
                .all(|c| c.is_ascii_lowercase() || c == '.' || c == '-')
        );
    }
}

/// UC: UC-CNV-006
#[test]
fn error_response_has_no_status_nor_retry_after() {
    let err =
        TabaError::new(ErrorCode::ClusterQuorumUnavailable, "Sem quórum.").with_retry_after(5);
    let json = serde_json::to_value(err.to_response("abc")).unwrap();
    assert_eq!(
        json,
        serde_json::json!({
            "code": "cluster.quorum-unavailable",
            "message": "Sem quórum.",
            "requestId": "abc"
        })
    );
    assert_eq!(err.status(), 503);
    assert_eq!(err.retry_after, Some(5));
}

/// UC: UC-CNV-006
#[test]
fn details_carry_field_code_and_message() {
    let err = TabaError::new(
        ErrorCode::ValidationFailed,
        "A requisição tem campos inválidos.",
    )
    .with_detail(
        "name",
        ErrorCode::ValidationInvalidName,
        "Use de 1 a 100 caracteres.",
    );
    let json = serde_json::to_value(err.to_response("id-1")).unwrap();
    assert_eq!(json["details"][0]["field"], "name");
    assert_eq!(json["details"][0]["code"], "validation.invalid-name");
    assert_eq!(json["details"][0]["message"], "Use de 1 a 100 caracteres.");
}
