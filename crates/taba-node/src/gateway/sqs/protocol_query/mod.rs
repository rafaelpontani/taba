//! The Query protocol of SQS (doc 39): a form (`Action=SendMessage&QueueUrl=...`) in the body or in the query, and an
//! XML answer. It is what the older AWS SDKs speak, and the one a client uses when it does not know the JSON one.
//!
//! Decoding turns the form into the **same** JSON shape the JSON protocol sends (lists of `Name`/`Value` become maps,
//! numbered lists become arrays, the `...RequestEntry` lists become `Entries`), so the actions are written once.
//! Encoding goes the other way: the result of an action, in the JSON shape, becomes the XML an SDK of this
//! protocol parses (lists are flat repeated elements; a map is a list of `Name`/`Value` or `Key`/`Value`).

use serde_json::{Map, Value};

use crate::gateway::aws::errors::AwsError;
use crate::gateway::aws::form;
use crate::gateway::aws::xml::Xml;

pub const CONTENT_TYPE: &str = "text/xml";
const NAMESPACE: &str = "http://queue.amazonaws.com/doc/2012-11-05/";

/// The actions that answer with data; the others only answer `ResponseMetadata`.
const WITH_RESULT: &[&str] = &[
    "CreateQueue",
    "GetQueueUrl",
    "ListQueues",
    "GetQueueAttributes",
    "ListQueueTags",
    "SendMessage",
    "SendMessageBatch",
    "ReceiveMessage",
    "DeleteMessageBatch",
    "ChangeMessageVisibilityBatch",
    "ListDeadLetterSourceQueues",
];

/// Reads the form of the query string and of the body together and returns the action and its parameters.
pub fn decode(query: &str, body: &str) -> Result<(String, Value), AwsError> {
    let mut pairs = form::pairs(query).map_err(form_error)?;
    pairs.extend(form::pairs(body).map_err(form_error)?);
    let mut root = form::to_value(&pairs).map_err(form_error)?;
    let Value::Object(object) = &mut root else {
        return Err(AwsError::invalid_action(""));
    };
    let action = match object.remove("Action") {
        Some(Value::String(action)) if !action.is_empty() => action,
        _ => return Err(AwsError::missing_parameter("Action")),
    };
    object.remove("Version");
    Ok((action, reshape(root)))
}

fn form_error(error: form::FormError) -> AwsError {
    match error {
        form::FormError::TooMany => {
            AwsError::invalid_parameter("The request has too many parameters.")
        }
        form::FormError::Invalid(why) => AwsError::invalid_parameter(why),
    }
}

/// `[{Name, Value}, ...]` as `{name: value}`; the pair names differ by list (`Name`/`Value`, `Key`/`Value`).
fn pairs_to_map(list: Value, name_key: &str, keep_value_object: bool) -> Value {
    let mut map = Map::new();
    if let Value::Array(items) = list {
        for item in items {
            let Value::Object(mut fields) = item else {
                continue;
            };
            let Some(Value::String(name)) = fields.remove(name_key) else {
                continue;
            };
            let value = fields.remove("Value").unwrap_or(Value::Null);
            map.insert(
                name,
                if keep_value_object {
                    reshape(value)
                } else {
                    value
                },
            );
        }
    }
    Value::Object(map)
}

/// The names the Query protocol gives to what the JSON protocol names differently.
fn reshape(value: Value) -> Value {
    match value {
        Value::Object(object) => {
            let mut out = Map::new();
            for (key, inner) in object {
                match key.as_str() {
                    // Some SDKs (the Java ones) name the numbered lists in the plural: `Tags.1.Key`.
                    "Attribute" | "Attributes" if inner.is_array() => {
                        out.insert("Attributes".into(), pairs_to_map(inner, "Name", false));
                    }
                    "Tag" | "Tags" if inner.is_array() => {
                        out.insert("Tags".into(), pairs_to_map(inner, "Key", false));
                    }
                    "MessageAttribute" | "MessageAttributes" if inner.is_array() => {
                        out.insert(
                            "MessageAttributes".into(),
                            pairs_to_map(inner, "Name", true),
                        );
                    }
                    "AttributeName" => {
                        out.insert("AttributeNames".into(), inner);
                    }
                    "MessageAttributeName" => {
                        out.insert("MessageAttributeNames".into(), inner);
                    }
                    "MessageSystemAttributeName" => {
                        out.insert("MessageSystemAttributeNames".into(), inner);
                    }
                    "TagKey" => {
                        out.insert("TagKeys".into(), inner);
                    }
                    "SendMessageBatchRequestEntry"
                    | "DeleteMessageBatchRequestEntry"
                    | "ChangeMessageVisibilityBatchRequestEntry" => {
                        out.insert("Entries".into(), reshape(inner));
                    }
                    _ => {
                        out.insert(key, reshape(inner));
                    }
                }
            }
            Value::Object(out)
        }
        Value::Array(items) => Value::Array(items.into_iter().map(reshape).collect()),
        other => other,
    }
}

/// The XML of a success.
pub fn encode_ok(action: &str, result: &Value, request_id: &str) -> String {
    let mut xml = Xml::new();
    xml.open(&format!("{action}Response"), Some(NAMESPACE));
    if WITH_RESULT.contains(&action) {
        let name = format!("{action}Result");
        xml.open(&name, None);
        if let Value::Object(fields) = result {
            write_fields(&mut xml, action, fields);
        }
        xml.close(&name);
    }
    xml.open("ResponseMetadata", None)
        .leaf("RequestId", request_id)
        .close("ResponseMetadata")
        .close(&format!("{action}Response"));
    xml.finish()
}

/// The XML of an error.
pub fn encode_error(error: &AwsError, request_id: &str) -> String {
    let mut xml = Xml::new();
    xml.open("ErrorResponse", Some(NAMESPACE))
        .open("Error", None)
        .leaf(
            "Type",
            if error.sender_fault {
                "Sender"
            } else {
                "Receiver"
            },
        )
        .leaf("Code", error.query_code)
        .leaf("Message", &error.message)
        .open("Detail", None)
        .close("Detail")
        .close("Error")
        .leaf("RequestId", request_id)
        .close("ErrorResponse");
    xml.finish()
}

fn scalar(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Bool(flag) => flag.to_string(),
        Value::Number(number) => number.to_string(),
        other => other.to_string(),
    }
}

/// The element a list is written with, by the field that holds it.
fn item_name(action: &str, field: &str) -> Option<String> {
    Some(match field {
        "QueueUrls" => "QueueUrl".to_owned(),
        "Messages" => "Message".to_owned(),
        "Failed" => "BatchResultErrorEntry".to_owned(),
        "Successful" => format!("{action}ResultEntry"),
        _ => return None,
    })
}

fn write_fields(xml: &mut Xml, action: &str, fields: &Map<String, Value>) {
    for (field, value) in fields {
        match (field.as_str(), value) {
            // A map written as a list of name and value.
            ("Attributes", Value::Object(map)) => {
                for (name, text) in map {
                    xml.open("Attribute", None)
                        .leaf("Name", name)
                        .leaf("Value", &scalar(text))
                        .close("Attribute");
                }
            }
            ("Tags", Value::Object(map)) => {
                for (key, text) in map {
                    xml.open("Tag", None)
                        .leaf("Key", key)
                        .leaf("Value", &scalar(text))
                        .close("Tag");
                }
            }
            ("MessageAttributes", Value::Object(map)) => {
                for (name, attribute) in map {
                    xml.open("MessageAttribute", None).leaf("Name", name);
                    xml.open("Value", None);
                    if let Value::Object(inner) = attribute {
                        write_fields(xml, action, inner);
                    }
                    xml.close("Value").close("MessageAttribute");
                }
            }
            (_, Value::Array(items)) => {
                let name = item_name(action, field).unwrap_or_else(|| field.clone());
                for item in items {
                    match item {
                        Value::Object(inner) => {
                            xml.open(&name, None);
                            write_fields(xml, action, inner);
                            xml.close(&name);
                        }
                        other => {
                            xml.leaf(&name, &scalar(other));
                        }
                    }
                }
            }
            (_, Value::Object(inner)) => {
                xml.open(field, None);
                write_fields(xml, action, inner);
                xml.close(field);
            }
            (_, Value::Null) => {}
            (_, other) => {
                xml.leaf(field, &scalar(other));
            }
        }
    }
}

#[cfg(test)]
mod tests;
