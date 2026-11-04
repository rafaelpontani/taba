use serde_json::json;

use super::*;

fn string(value: &str) -> Attribute {
    Attribute {
        data_type: "String".into(),
        string_value: Some(value.into()),
        binary_value: None,
    }
}

/// UC: UC-GTW-007
#[test]
fn a_message_without_attributes_is_stored_as_its_plain_text() {
    let packed = pack("{\"a\": 1}", &Attributes::new());
    assert_eq!(packed.content_type, PLAIN_CONTENT_TYPE);
    assert_eq!(packed.bytes, b"{\"a\": 1}");
    assert_eq!(packed.value, json!("{\"a\": 1}"));
}

/// UC: UC-GTW-007
#[test]
fn a_message_with_attributes_is_an_envelope_that_comes_back_whole() {
    let mut attributes = Attributes::new();
    attributes.insert("color".into(), string("blue"));
    attributes.insert(
        "blob".into(),
        Attribute {
            data_type: "Binary".into(),
            string_value: None,
            binary_value: Some("AAEC/w==".into()),
        },
    );
    let packed = pack("café €", &attributes);
    assert_eq!(packed.content_type, ENVELOPE_CONTENT_TYPE);
    assert_eq!(packed.value["tabaSqs"], 1);

    let opened = unpack(&packed.value, None);
    assert_eq!(opened.body, "café €");
    assert_eq!(opened.attributes, attributes);
}

/// UC: UC-GTW-007
#[test]
fn a_message_that_was_not_published_through_the_gateway_is_read_as_text() {
    assert_eq!(unpack(&json!("plain"), None).body, "plain");
    // A native publisher's JSON, delivered as a value.
    let native = unpack(&json!({"order": 7}), None);
    assert_eq!(native.body, "{\"order\":7}");
    assert!(native.attributes.is_empty());
    // Bytes of another type arrive in base64.
    assert_eq!(unpack(&json!("aGVsbG8="), Some("base64")).body, "hello");
}

/// UC: UC-GTW-007
#[test]
fn the_md5_of_the_body_is_the_md5_of_its_bytes() {
    assert_eq!(
        md5_of_body("hello world"),
        "5eb63bbbe01eeed093cb22bb8f5acdc3"
    );
    assert_eq!(md5_of_body(""), "d41d8cd98f00b204e9800998ecf8427e");
}

/// UC: UC-GTW-007
#[test]
fn the_md5_of_the_attributes_follows_the_aws_encoding() {
    assert_eq!(md5_of_attributes(&Attributes::new()), None);
    let mut attributes = Attributes::new();
    attributes.insert("n".into(), string("v"));
    // 4-byte length + name, 4-byte length + type, kind 1, 4-byte length + value.
    let mut expected = Vec::new();
    expected.extend_from_slice(&[0, 0, 0, 1, b'n']);
    expected.extend_from_slice(&[0, 0, 0, 6]);
    expected.extend_from_slice(b"String");
    expected.push(1);
    expected.extend_from_slice(&[0, 0, 0, 1, b'v']);
    assert_eq!(
        md5_of_attributes(&attributes),
        Some(format!("{:x}", md5::compute(&expected)))
    );
}

/// UC: UC-GTW-007
#[test]
fn the_names_are_sorted_before_the_md5() {
    let mut forward = Attributes::new();
    forward.insert("a".into(), string("1"));
    forward.insert("b".into(), string("2"));
    // A BTreeMap sorts by itself, whatever the insertion order.
    let mut backward = Attributes::new();
    backward.insert("b".into(), string("2"));
    backward.insert("a".into(), string("1"));
    assert_eq!(md5_of_attributes(&forward), md5_of_attributes(&backward));
}

/// UC: UC-GTW-007
#[test]
fn the_attributes_of_a_request_are_checked() {
    let ok = parse_attributes(Some(&json!({
        "color": {"DataType": "String", "StringValue": "blue"},
        "n": {"DataType": "Number.int", "StringValue": "42"},
        "b": {"DataType": "Binary", "BinaryValue": "AAEC/w=="}
    })))
    .expect("valid");
    assert_eq!(ok.len(), 3);
    assert_eq!(ok["n"].data_type, "Number.int");

    let bad = |value: serde_json::Value| parse_attributes(Some(&value)).is_err();
    assert!(bad(
        json!({"x": {"DataType": "Boolean", "StringValue": "true"}})
    ));
    assert!(bad(json!({"x": {"DataType": "String"}})));
    assert!(bad(
        json!({"x": {"DataType": "Number", "StringValue": "abc"}})
    ));
    assert!(bad(
        json!({"x": {"DataType": "Binary", "BinaryValue": "!!"}})
    ));
    assert!(bad(
        json!({"a b": {"DataType": "String", "StringValue": "v"}})
    ));
    assert!(bad(
        json!({"AWS.x": {"DataType": "String", "StringValue": "v"}})
    ));
    assert!(bad(json!({"x": {"DataType": "String", "StringValue": ""}})));
    let eleven: serde_json::Map<String, serde_json::Value> = (0..11)
        .map(|i| {
            (
                format!("a{i}"),
                json!({"DataType": "String", "StringValue": "v"}),
            )
        })
        .collect();
    assert!(bad(serde_json::Value::Object(eleven)));
    assert!(parse_attributes(None).expect("none").is_empty());
}

/// UC: UC-GTW-020
#[test]
fn the_original_ids_of_a_fifo_message_travel_in_an_envelope_even_without_attributes() {
    let origin = Origin {
        group: Some("tenant/42!blue".into()),
        dedup: None,
    };
    let packed = pack_with("hello", &Attributes::new(), &origin);
    assert_eq!(packed.content_type, ENVELOPE_CONTENT_TYPE);
    let opened = unpack(&packed.value, None);
    assert_eq!(opened.body, "hello");
    assert!(opened.attributes.is_empty());
    assert_eq!(opened.origin, origin);

    let plain = pack_with("hello", &Attributes::new(), &Origin::default());
    assert_eq!(plain.content_type, PLAIN_CONTENT_TYPE);
    assert_eq!(unpack(&plain.value, None).origin, Origin::default());
}

/// UC: UC-GTW-020
#[test]
fn the_sha256_is_hexadecimal() {
    assert_eq!(
        sha256_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}
