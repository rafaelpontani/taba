use serde_json::json;

use super::*;

/// UC: UC-GTW-025
#[test]
fn a_number_comes_as_a_json_number_or_as_text_and_an_empty_text_is_absent() {
    let params = json!({"A": 5, "B": " 7 ", "C": "", "D": "x", "E": 1.5});
    assert_eq!(number(&params, "A").unwrap(), Some(5));
    assert_eq!(number(&params, "B").unwrap(), Some(7));
    assert_eq!(number(&params, "C").unwrap(), None);
    assert_eq!(number(&params, "Z").unwrap(), None);
    assert!(number(&params, "D").is_err());
    assert!(number(&params, "E").is_err());
    assert!(number_in(&json!({"A": 11}), "A", 1, 10).is_err());
    assert_eq!(number_in(&json!({"A": 10}), "A", 1, 10).unwrap(), Some(10));
}

/// UC: UC-GTW-025
#[test]
fn texts_lists_and_maps_are_read_from_the_shape_both_protocols_give() {
    let params = json!({"Name": "n", "Empty": "", "Keys": ["a", "b"], "One": "x", "Map": {"k": "v", "n": 3}});
    assert_eq!(text(&params, "Name"), Some("n"));
    assert_eq!(text(&params, "Empty"), None);
    assert_eq!(
        required_text(&params, "Missing").unwrap_err().json_code,
        "MissingParameter"
    );
    assert_eq!(texts(&params, "Keys"), vec!["a", "b"]);
    assert_eq!(texts(&params, "One"), vec!["x"]);
    assert!(texts(&params, "Nothing").is_empty());
    assert_eq!(
        text_map(&params, "Map"),
        vec![
            ("k".to_owned(), "v".to_owned()),
            ("n".to_owned(), "3".to_owned())
        ]
    );
}

/// UC: UC-GTW-025
#[test]
fn a_path_segment_is_escaped_for_the_client_api() {
    assert_eq!(url_encode("orders-eu_1.v2~"), "orders-eu_1.v2~");
    assert_eq!(url_encode("a b/c"), "a%20b%2Fc");
}
