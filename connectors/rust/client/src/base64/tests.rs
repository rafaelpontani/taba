use super::*;

/// UC: UC-CNT-022
#[test]
fn matches_the_rfc_4648_vectors() {
    for (plain, encoded) in [
        ("", ""),
        ("f", "Zg=="),
        ("fo", "Zm8="),
        ("foo", "Zm9v"),
        ("foob", "Zm9vYg=="),
        ("fooba", "Zm9vYmE="),
        ("foobar", "Zm9vYmFy"),
    ] {
        assert_eq!(encode(plain.as_bytes()), encoded);
        assert_eq!(decode(encoded).unwrap(), plain.as_bytes());
    }
}

/// UC: UC-CNT-022
#[test]
fn round_trips_arbitrary_bytes_and_the_png_header() {
    let all: Vec<u8> = (0..=255).collect();
    assert_eq!(decode(&encode(&all)).unwrap(), all);
    assert_eq!(
        decode("iVBORw0KGgo=").unwrap(),
        [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]
    );
}

/// UC: UC-CNT-022
#[test]
fn accepts_missing_padding_and_rejects_garbage() {
    assert_eq!(decode("Zg").unwrap(), b"f");
    assert!(decode("Z").is_none());
    assert!(decode("Zg===").is_none());
    assert!(decode("Zm9v!").is_none());
    assert!(decode("Zm 9v").is_none());
}
