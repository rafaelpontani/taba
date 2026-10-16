use super::*;

/// TUI: TUI-MSG-07
#[test]
fn json_is_re_indented_only_when_it_is_valid() {
    let text = formatted(br#"{"a":1,"b":[true,null],"c":{"d":"x y"}}"#).expect("valid JSON");
    assert_eq!(
        text,
        "{\n  \"a\": 1,\n  \"b\": [\n    true,\n    null\n  ],\n  \"c\": {\n    \"d\": \"x y\"\n  }\n}"
    );
    assert_eq!(
        formatted(br#"{"a":1,"#),
        None,
        "a cut JSON is not formatted"
    );
    assert_eq!(formatted(b"[1, 2"), None);
    assert_eq!(formatted(b"   "), None);
    assert_eq!(
        formatted(&[0xff, 0xfe, b'{']),
        None,
        "bytes that are not text"
    );
}

/// TUI: TUI-MSG-07
#[test]
fn simple_yaml_is_written_back_with_two_spaces_and_plain_text_is_left_alone() {
    let text = formatted(b"name: orders\nlimits:\n      ttl: 60\n      tries: 3\n").expect("YAML");
    assert_eq!(
        text,
        "name: orders
limits:
  ttl: 60
  tries: 3
"
    );
    assert_eq!(formatted(b"just some words"), None);
    assert_eq!(
        formatted(b"- a\n- b\n"),
        None,
        "lists are outside the simple YAML"
    );
}

/// TUI: TUI-MSG-07
#[test]
fn base64_is_the_bytes_in_lines_of_76() {
    assert_eq!(base64_lines(b"hello"), vec!["aGVsbG8=".to_owned()]);
    let long = base64_lines(&[7u8; 100]);
    assert_eq!(long.len(), 2);
    assert_eq!(long[0].len(), 76);
    // It goes back to the same bytes.
    let joined: String = long.concat();
    assert_eq!(taba_client::base64::decode(&joined), Some(vec![7u8; 100]));
}

/// TUI: TUI-MSG-07
#[test]
fn a_view_that_cannot_be_applied_shows_the_body_as_sent_and_says_so() {
    let (lines, applied) = lines(b"not json at all", BodyView::Formatted);
    assert_eq!(lines, vec!["not json at all".to_owned()]);
    assert!(!applied);
    let (lines, applied) = lines_of_sent();
    assert!(applied && lines == vec!["a".to_owned(), "b".to_owned()]);
}

fn lines_of_sent() -> (Vec<String>, bool) {
    lines(b"a\nb", BodyView::Sent)
}
