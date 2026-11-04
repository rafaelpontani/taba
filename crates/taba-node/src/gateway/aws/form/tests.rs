use super::*;
use serde_json::json;

fn parse(text: &str) -> Value {
    to_value(&pairs(text).expect("pairs")).expect("value")
}

/// UC: UC-GTW-005
#[test]
fn plain_pairs_are_decoded_with_plus_as_space() {
    assert_eq!(
        parse("Action=SendMessage&MessageBody=hello+world%21&QueueUrl=http%3A%2F%2Fh%2F1%2Fq"),
        json!({"Action": "SendMessage", "MessageBody": "hello world!", "QueueUrl": "http://h/1/q"})
    );
}

/// UC: UC-GTW-005
#[test]
fn numbered_segments_become_lists_in_numeric_order() {
    assert_eq!(
        parse("AttributeName.2=B&AttributeName.1=A&AttributeName.10=C"),
        json!({"AttributeName": ["A", "B", "C"]})
    );
}

/// UC: UC-GTW-005
#[test]
fn lists_of_objects_keep_their_fields() {
    assert_eq!(
        parse(
            "Attribute.1.Name=VisibilityTimeout&Attribute.1.Value=45&Attribute.2.Name=DelaySeconds&Attribute.2.Value=0"
        ),
        json!({"Attribute": [
            {"Name": "VisibilityTimeout", "Value": "45"},
            {"Name": "DelaySeconds", "Value": "0"}
        ]})
    );
    assert_eq!(
        parse(
            "E.1.Id=a&E.1.MessageAttribute.1.Name=n&E.1.MessageAttribute.1.Value.DataType=String&E.1.MessageAttribute.1.Value.StringValue=v"
        ),
        json!({"E": [{"Id": "a", "MessageAttribute": [
            {"Name": "n", "Value": {"DataType": "String", "StringValue": "v"}}
        ]}]})
    );
}

/// UC: UC-GTW-005
#[test]
fn a_repeated_or_conflicting_name_is_refused() {
    let repeated = to_value(&pairs("A=1&A=2").expect("pairs"));
    assert!(matches!(repeated, Err(FormError::Invalid(_))));
    let conflict = to_value(&pairs("A=1&A.1=2").expect("pairs"));
    assert!(matches!(conflict, Err(FormError::Invalid(_))));
}

/// UC: UC-GTW-005
#[test]
fn limits_bound_what_a_form_can_ask_for() {
    let huge = (0..=MAX_PAIRS)
        .map(|i| format!("k{i}=v"))
        .collect::<Vec<_>>()
        .join("&");
    assert_eq!(pairs(&huge), Err(FormError::TooMany));
    let far = to_value(&pairs("A.99999999=x").expect("pairs"));
    assert!(matches!(far, Err(FormError::Invalid(_))));
    let deep = to_value(&pairs("a.b.c.d.e.f.g.h.i.j=x").expect("pairs"));
    assert!(matches!(deep, Err(FormError::Invalid(_))));
    let empty_segment = to_value(&pairs("A..B=x").expect("pairs"));
    assert!(matches!(empty_segment, Err(FormError::Invalid(_))));
}

/// UC: UC-GTW-005
#[test]
fn text_that_is_not_utf8_is_refused() {
    assert!(matches!(pairs("A=%FF%FE"), Err(FormError::Invalid(_))));
}
