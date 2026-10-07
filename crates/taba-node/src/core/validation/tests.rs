use super::*;

const SUFFIX: &str = "-dead-letter";

/// UC: UC-TOP-009
#[test]
fn topic_name_accepts_the_documented_charset_and_length() {
    assert!(topic_name("pedidos", SUFFIX).is_ok());
    assert!(topic_name("Pedidos_2-a", SUFFIX).is_ok());
    assert!(topic_name(&"a".repeat(100), SUFFIX).is_ok());
}

/// UC: UC-TOP-009
#[test]
fn topic_name_rejects_empty_long_and_foreign_characters() {
    for bad in [
        "",
        &"a".repeat(101),
        "com espaço",
        "acentuação",
        "a/b",
        "a.b",
    ] {
        let err = topic_name(bad, SUFFIX).unwrap_err();
        assert_eq!(err.code, ErrorCode::ValidationInvalidName, "{bad:?}");
    }
}

/// UC: UC-TOP-010
#[test]
fn topic_name_ending_in_the_dead_letter_suffix_is_reserved() {
    let err = topic_name("pedidos-dead-letter", SUFFIX).unwrap_err();
    assert_eq!(err.code, ErrorCode::ValidationReservedName);
    assert!(topic_name("pedidos-dead", SUFFIX).is_ok());
    // O sufixo é configurável (`topics.dead-letter.suffix`).
    assert!(topic_name("pedidos-dead-letter", "-dlq").is_ok());
    assert_eq!(
        topic_name("pedidos-dlq", "-dlq").unwrap_err().code,
        ErrorCode::ValidationReservedName
    );
}

#[test]
fn topic_name_is_not_normalised_so_case_is_kept() {
    assert!(topic_name("Pedidos", SUFFIX).is_ok());
    assert!(topic_name("pedidos", SUFFIX).is_ok());
}

fn tag_map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect()
}

#[test]
fn valid_tags_pass_including_empty_value_and_the_limits() {
    assert!(
        tags(&tag_map(&[
            ("time", "financeiro"),
            ("ambiente", "prod"),
            ("v", "")
        ]))
        .is_ok()
    );
    let longest = tag_map(&[(&"k".repeat(64), &"v".repeat(128))]);
    assert!(tags(&longest).is_ok());
    let twenty: BTreeMap<_, _> = (0..20).map(|i| (format!("k{i}"), String::new())).collect();
    assert!(tags(&twenty).is_ok());
}

/// UC: UC-TOP-008
#[test]
fn invalid_tags_fail_with_the_field_in_details() {
    let twenty_one: BTreeMap<_, _> = (0..21).map(|i| (format!("k{i}"), String::new())).collect();
    let cases = [
        twenty_one,
        tag_map(&[("", "v")]),
        tag_map(&[(&"k".repeat(65), "v")]),
        tag_map(&[("k", &"v".repeat(129))]),
        tag_map(&[("k", "com espaço")]),
        tag_map(&[("c@ve", "v")]),
    ];
    for case in cases {
        let err = tags(&case).unwrap_err();
        assert_eq!(err.code, ErrorCode::ValidationFailed);
        assert!(!err.details.is_empty());
    }
    let err = tags(&tag_map(&[("k", "a b")])).unwrap_err();
    assert_eq!(err.details[0].field, "tags.k");
}

/// UC: UC-TOP-016
#[test]
fn distributor_prefixes_follow_the_documented_limits() {
    let ok: Vec<String> = (0..20).map(|i| format!("p{i}")).collect();
    assert!(prefixes(&ok).is_ok());
    assert!(prefixes(&[]).is_ok(), "lista vazia é aceita (UC-TOP-017)");

    let too_many: Vec<String> = (0..21).map(|i| format!("p{i}")).collect();
    assert!(prefixes(&too_many).is_err());
    assert!(
        prefixes(&[String::new()]).is_err(),
        "prefixo vazio casaria com tudo"
    );
    assert!(prefixes(&["a".repeat(101)]).is_err());
    assert!(prefixes(&["a".repeat(100)]).is_ok());
    assert!(prefixes(&["com espaço".to_owned()]).is_err());
    let err = prefixes(&["ok".to_owned(), "a.b".to_owned()]).unwrap_err();
    assert_eq!(err.details[0].field, "prefixes[1]");
}

/// UC: UC-TOP-014
#[test]
fn ranges_are_inclusive_at_both_ends() {
    for (limits, lo, hi) in [
        (IDLE_TIMEOUT_SECS, 60, 604_800),
        (TTL_SECS, 60, 31_536_000),
        (MAX_DELIVERIES, 1, 1000),
    ] {
        assert!(in_range("campo", lo, limits).is_ok());
        assert!(in_range("campo", hi, limits).is_ok());
        assert_eq!(
            in_range("campo", lo - 1, limits).unwrap_err().code,
            ErrorCode::ValidationOutOfRange
        );
        assert_eq!(
            in_range("campo", hi + 1, limits).unwrap_err().code,
            ErrorCode::ValidationOutOfRange
        );
    }
}
