use super::*;

/// UC: UC-CNT-021
#[test]
fn skips_every_kind_of_value() {
    for doc in [
        "null",
        "true",
        "false",
        "0",
        "-12",
        "3.14",
        "1e10",
        "-2.5E-3",
        "\"x\"",
        "[]",
        "{}",
        "[1, [2, [3]]]",
        "{\"a\": {\"b\": [true, null]}}",
        " \n\t 7 ",
    ] {
        let end = skip_value(doc, 0).unwrap_or_else(|e| panic!("{doc}: {e}"));
        assert_eq!(doc[end..].trim(), "", "{doc}");
    }
}

/// UC: UC-CNT-021
#[test]
fn rejects_malformed_documents() {
    for doc in [
        "",
        "{",
        "[1,",
        "{\"a\"}",
        "{\"a\":}",
        "tru",
        "01",
        "1.",
        "-",
        "\"abc",
        "\"\\x\"",
        "\"\\u12G4\"",
        "[1 2]",
        "{\"a\":1 \"b\":2}",
        "\"a\nb\"",
    ] {
        assert!(validate(doc).is_err(), "should refuse {doc:?}");
    }
}

/// UC: UC-CNT-021
#[test]
fn deep_nesting_is_bounded_not_a_stack_overflow() {
    let ok = format!(
        "{}1{}",
        "[".repeat(MAX_DEPTH - 1),
        "]".repeat(MAX_DEPTH - 1)
    );
    assert!(skip_value(&ok, 0).is_ok());
    let deep = format!("{}1{}", "[".repeat(100_000), "]".repeat(100_000));
    assert!(skip_value(&deep, 0).is_err());
}

/// UC: UC-CNT-021
#[test]
fn object_fields_give_the_raw_text_of_each_value() {
    let doc = "{ \"id\" : \"a\\nb\", \"body\" : { \"x\": [1, 2] } ,\"n\":12345678901234567890, \"e\":{} }\n";
    let fields = object_fields(doc).unwrap();
    let names: Vec<&str> = fields.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(names, ["id", "body", "n", "e"]);
    assert_eq!(fields[0].1, "\"a\\nb\"");
    assert_eq!(fields[1].1, "{ \"x\": [1, 2] }");
    assert_eq!(fields[2].1, "12345678901234567890");
    assert_eq!(fields[3].1, "{}");
    assert!(object_fields("{} x").is_err());
    assert!(object_fields("[]").is_err());
}

/// UC: UC-CNT-021
#[test]
fn decodes_escapes_unicode_and_surrogate_pairs() {
    assert_eq!(decode_string("\"a\\\"b\\\\c\\/d\"").unwrap(), "a\"b\\c/d");
    assert_eq!(
        decode_string("\"\\b\\f\\n\\r\\t\"").unwrap(),
        "\u{8}\u{c}\n\r\t"
    );
    assert_eq!(decode_string("\"\\u00e9\\u4e2d\"").unwrap(), "é中");
    assert_eq!(decode_string("\"\\ud83d\\ude00\"").unwrap(), "😀");
    assert_eq!(decode_string("\"😀 direto\"").unwrap(), "😀 direto");
    // A lone surrogate does not bring it down: it becomes the replacement character.
    assert_eq!(decode_string("\"\\ud83d x\"").unwrap(), "\u{FFFD} x");
    assert_eq!(decode_string("\"\\ude00\"").unwrap(), "\u{FFFD}");
    assert!(decode_string("abc").is_err());
}

/// UC: UC-CNT-021, UC-CNT-025
#[test]
fn quote_round_trips_through_decode() {
    for s in [
        "",
        "simple",
        "aspas \" e barra \\",
        "new\nline\ttab",
        "\u{1}ctrl",
        "çã 中 😀",
    ] {
        assert_eq!(decode_string(&quote(s)).unwrap(), s);
    }
    assert_eq!(quote("a\u{1f}"), "\"a\\u001f\"");
}

/// UC: UC-CNT-021
#[test]
fn array_items_split_values_with_nested_commas() {
    let items = array_items(" [ \"a,b\", [1,2], {\"k\": [3,4]}, 5 ] ").unwrap();
    assert_eq!(items, ["\"a,b\"", "[1,2]", "{\"k\": [3,4]}", "5"]);
    assert!(array_items("[]").unwrap().is_empty());
    assert!(array_items("[1,]").is_err());
}

/// UC: UC-CNT-028
#[test]
fn fields_look_up_typed_values_and_ignore_unknown_ones() {
    let f = Fields::parse(
        "{\"name\":\"x\",\"count\":7,\"ok\":true,\"none\":null,\"future\":{\"a\":1}}",
    )
    .unwrap();
    assert_eq!(f.string("name").as_deref(), Some("x"));
    assert_eq!(f.unsigned("count"), Some(7));
    assert_eq!(f.boolean("ok"), Some(true));
    assert_eq!(f.string("none"), None);
    assert_eq!(f.string("missing"), None);
    assert_eq!(f.raw("future"), Some("{\"a\":1}"));
}

/// UC: UC-CLI-004
#[test]
fn pretty_reindents_without_touching_scalars() {
    let out = pretty("{\"a\":1.50,\"b\":[true,{\"c\":\"x\\ny\"}],\"e\":{},\"f\":[]}").unwrap();
    assert_eq!(
        out,
        "{\n  \"a\": 1.50,\n  \"b\": [\n    true,\n    {\n      \"c\": \"x\\ny\"\n    }\n  ],\n  \"e\": {},\n  \"f\": []\n}"
    );
    assert_eq!(pretty("  \"s\" ").unwrap(), "\"s\"");
    assert!(pretty("{").is_err());
}
