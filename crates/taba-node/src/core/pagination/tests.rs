use serde::{Deserialize, Serialize};

use super::*;

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Key {
    n: String,
    i: u32,
}

#[test]
fn cursor_round_trips_and_is_url_safe() {
    let key = Key {
        n: "orders/ç?".into(),
        i: 7,
    };
    let cursor = encode_cursor(&key);
    assert!(
        cursor
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
        "{cursor}"
    );
    assert_eq!(decode_cursor::<Key>(&cursor).unwrap(), key);
}

/// UC: UC-PAG-002
#[test]
fn malformed_cursor_is_invalid_cursor() {
    for bad in ["", "!!!", "bm90LWpzb24", &encode_cursor(&"text")] {
        let err = decode_cursor::<Key>(bad).unwrap_err();
        assert_eq!(err.code, ErrorCode::ValidationInvalidCursor, "{bad:?}");
    }
}

/// UC: UC-PAG-002
#[test]
fn limit_must_be_between_one_and_the_maximum() {
    assert_eq!(parse_limit(None, 100, 1000).unwrap(), 100);
    assert_eq!(parse_limit(Some("1"), 100, 1000).unwrap(), 1);
    assert_eq!(parse_limit(Some("1000"), 100, 1000).unwrap(), 1000);
    for bad in ["0", "1001", "-1", "abc", "", "1.5"] {
        let err = parse_limit(Some(bad), 100, 1000).unwrap_err();
        assert_eq!(err.code, ErrorCode::ValidationOutOfRange, "{bad:?}");
    }
}
