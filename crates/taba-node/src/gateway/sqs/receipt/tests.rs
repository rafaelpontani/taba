use super::*;

/// UC: UC-GTW-009
#[test]
fn a_handle_gives_back_the_id_and_the_receipt_token() {
    let handle = encode(
        "0194f6a0-7b2c-7a11-9c3d-0123456789ab",
        "tok/en+with=chars:and:colons",
    );
    let (id, token) = decode(&handle).expect("decode");
    assert_eq!(id, "0194f6a0-7b2c-7a11-9c3d-0123456789ab");
    assert_eq!(token, "tok/en+with=chars:and:colons");
}

/// UC: UC-GTW-009
#[test]
fn a_handle_is_safe_in_a_url_and_in_a_form() {
    let handle = encode("id", "a/b+c==");
    assert!(
        handle
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
        "{handle}"
    );
}

/// UC: UC-GTW-009
#[test]
fn a_text_that_is_not_a_handle_is_invalid() {
    for bad in [
        "",
        "not-a-valid-handle",
        "!!!!",
        // base64 of "no-colon"
        "bm8tY29sb24",
        // base64 of ":token" and "id:"
        "OnRva2Vu",
        "aWQ6",
    ] {
        assert_eq!(
            decode(bad).unwrap_err().json_code,
            "ReceiptHandleIsInvalid",
            "{bad}"
        );
    }
    assert!(decode(&"A".repeat(2000)).is_err());
}
