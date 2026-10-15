use super::*;

fn kind(raw: &str) -> BodyKind {
    ContentType::parse(raw).unwrap().kind()
}

/// UC: UC-MSG-004
#[test]
fn rejects_what_is_not_a_media_type() {
    for raw in [
        "",
        "json",
        "/json",
        "application/",
        "application/json;",
        "text/plain; charset",
        "text/plain; =utf-8",
        "a b/c",
        "text/plain; charset=\"open",
    ] {
        let err = ContentType::parse(raw).unwrap_err();
        assert_eq!(err.code, ErrorCode::ValidationInvalidContentType, "{raw:?}");
    }
    let long = format!("text/{}", "a".repeat(251));
    assert_eq!(long.len(), 256);
    assert!(ContentType::parse(&long).is_err());
    assert!(ContentType::parse(&long[..255]).is_ok());
}

#[test]
fn keeps_the_text_exactly_as_sent_with_its_parameters() {
    let ct = ContentType::parse("Text/Plain; Charset=\"UTF-8\"; x=y").unwrap();
    assert_eq!(ct.as_str(), "Text/Plain; Charset=\"UTF-8\"; x=y");
    assert_eq!(ct.kind(), BodyKind::Text);
}

/// UC: UC-CNS-011
#[test]
fn json_kinds_include_plus_json_suffixes() {
    assert_eq!(kind("application/json"), BodyKind::Json);
    assert_eq!(kind("APPLICATION/JSON; charset=utf-8"), BodyKind::Json);
    assert_eq!(kind("application/vnd.api+json"), BodyKind::Json);
    assert_eq!(kind("text/foo+json"), BodyKind::Json);
    assert_eq!(kind("text/json"), BodyKind::Text);
}

#[test]
fn text_is_utf8_only_and_everything_else_is_binary() {
    assert_eq!(kind("text/plain"), BodyKind::Text);
    assert_eq!(kind("text/plain; charset=utf-8"), BodyKind::Text);
    assert_eq!(kind("text/plain; charset=UTF-8"), BodyKind::Text);
    assert_eq!(kind("text/plain; charset=iso-8859-1"), BodyKind::Binary);
    assert_eq!(kind("image/png"), BodyKind::Binary);
    assert_eq!(kind("application/octet-stream"), BodyKind::Binary);
}

/// UC: UC-MSG-005, UC-MSG-006, UC-MSG-007
#[test]
fn only_json_and_utf8_text_are_validated() {
    let json = ContentType::parse("application/json").unwrap();
    assert!(json.validate_body(br#"{"a": [1, 2.50, null]}"#).is_ok());
    assert!(json.validate_body(b"\"text\"").is_ok());
    assert!(json.validate_body(b"42").is_ok());
    for bad in [&b"{ nao"[..], b"", b"{} {}"] {
        let err = json.validate_body(bad).unwrap_err();
        assert_eq!(err.code, ErrorCode::ValidationInvalidBody);
    }

    let text = ContentType::parse("text/plain").unwrap();
    assert!(text.validate_body("café".as_bytes()).is_ok());
    assert_eq!(
        text.validate_body(&[0xff, 0xfe]).unwrap_err().code,
        ErrorCode::ValidationInvalidBody
    );

    let png = ContentType::parse("image/png").unwrap();
    assert!(png.validate_body(&[0xff, 0xfe, 0x00]).is_ok());
    let latin1 = ContentType::parse("text/plain; charset=iso-8859-1").unwrap();
    assert!(latin1.validate_body(&[0xe9]).is_ok());
}

/// UC: UC-CNS-008, UC-CNS-009, UC-CNS-010
#[test]
fn renders_json_text_and_base64() {
    let json = ContentType::parse("application/json").unwrap();
    let raw = br#"{ "amount": 129.90,  "items": ["a"] }"#;
    let body = Body::render(&json, raw);
    assert_eq!(
        serde_json::to_string(&body).unwrap(),
        r#"{ "amount": 129.90,  "items": ["a"] }"#,
        "the JSON comes out exactly as it was sent"
    );
    assert_eq!(body.encoding(), None);

    let text = Body::render(
        &ContentType::parse("text/plain").unwrap(),
        "café".as_bytes(),
    );
    assert_eq!(serde_json::to_string(&text).unwrap(), "\"café\"");
    assert_eq!(text.encoding(), None);

    let png = Body::render(
        &ContentType::parse("image/png").unwrap(),
        &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a],
    );
    assert_eq!(serde_json::to_string(&png).unwrap(), "\"iVBORw0KGgo=\"");
    assert_eq!(png.encoding(), Some("base64"));
}
