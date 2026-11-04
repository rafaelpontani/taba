//! The protocol of SNS (doc 39): the Query form in, XML out. SNS only speaks Query, so there is no JSON side.
//!
//! Decoding turns the form into the JSON shape the actions read: the `member` and `entry` levels SNS puts in every list
//! disappear (`TagKeys.member.1=a` is `["a"]`), a map of `key`/`value` entries becomes an object (`Attributes`), a list of
//! `Key`/`Value` becomes an object (`Tags`), and the message attributes become `{name: {DataType, StringValue, ...}}`, the
//! same shape the SQS actions read. Encoding goes the other way: the result of an action in that shape becomes the XML an
//! SDK parses.

use serde_json::{Map, Value};

use crate::gateway::aws::errors::AwsError;
use crate::gateway::aws::form;
use crate::gateway::aws::xml::Xml;

pub const CONTENT_TYPE: &str = "text/xml";
const NAMESPACE: &str = "http://sns.amazonaws.com/doc/2010-03-31/";

/// The actions that answer with nothing but `ResponseMetadata`.
const WITHOUT_RESULT: &[&str] = &[
    "DeleteTopic",
    "Unsubscribe",
    "SetTopicAttributes",
    "SetSubscriptionAttributes",
    "AddPermission",
    "RemovePermission",
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
            AwsError::sns_invalid_parameter("The request has too many parameters.")
        }
        form::FormError::Invalid(why) => AwsError::sns_invalid_parameter(why),
    }
}

/// `[{key, value}, ...]` (or `[{Key, Value}, ...]`) as `{key: value}`.
fn pairs_to_map(list: Vec<Value>, name_key: &str, keep_value_object: bool) -> Value {
    let mut map = Map::new();
    for item in list {
        let Value::Object(mut fields) = item else {
            continue;
        };
        let Some(Value::String(name)) = fields.remove(name_key) else {
            continue;
        };
        let value_key = if name_key == "key" { "value" } else { "Value" };
        let value = fields.remove(value_key).unwrap_or(Value::Null);
        map.insert(
            name,
            if keep_value_object {
                reshape(value)
            } else {
                value
            },
        );
    }
    Value::Object(map)
}

/// The list inside a `member` (or `entry`) level, if the value is one.
fn list_of(value: &Value, level: &str) -> Option<Vec<Value>> {
    match value {
        Value::Object(object) if object.len() == 1 => match object.get(level) {
            Some(Value::Array(items)) => Some(items.clone()),
            _ => None,
        },
        _ => None,
    }
}

fn reshape(value: Value) -> Value {
    match value {
        Value::Object(object) => {
            let mut out = Map::new();
            for (key, inner) in object {
                let reshaped = match key.as_str() {
                    "Attributes" => {
                        list_of(&inner, "entry").map(|list| pairs_to_map(list, "key", false))
                    }
                    "Tags" => {
                        list_of(&inner, "member").map(|list| pairs_to_map(list, "Key", false))
                    }
                    "MessageAttributes" => {
                        list_of(&inner, "entry").map(|list| pairs_to_map(list, "Name", true))
                    }
                    _ => list_of(&inner, "member")
                        .map(|list| Value::Array(list.into_iter().map(reshape).collect())),
                };
                out.insert(key, reshaped.unwrap_or_else(|| reshape(inner)));
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
    if !WITHOUT_RESULT.contains(&action) {
        let name = format!("{action}Result");
        xml.open(&name, None);
        if let Value::Object(fields) = result {
            write_fields(&mut xml, fields);
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

fn write_fields(xml: &mut Xml, fields: &Map<String, Value>) {
    for (field, value) in fields {
        match (field.as_str(), value) {
            // A map written as a list of key and value.
            ("Attributes", Value::Object(map)) => {
                xml.open("Attributes", None);
                for (name, text) in map {
                    xml.open("entry", None)
                        .leaf("key", name)
                        .leaf("value", &scalar(text))
                        .close("entry");
                }
                xml.close("Attributes");
            }
            // A list is a level of `member`s.
            (_, Value::Array(items)) => {
                xml.open(field, None);
                for item in items {
                    xml.open("member", None);
                    match item {
                        Value::Object(inner) => write_fields(xml, inner),
                        other => {
                            xml.text(&scalar(other));
                        }
                    }
                    xml.close("member");
                }
                xml.close(field);
            }
            (_, Value::Object(inner)) => {
                xml.open(field, None);
                write_fields(xml, inner);
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
