//! Reads the form of the AWS Query protocol (doc 39): `application/x-www-form-urlencoded` pairs in the body (or in the
//! query of a GET) whose names are paths: `Attribute.1.Name=a&Attribute.1.Value=b` is a list of objects.
//!
//! The result is a JSON value with the same shape the JSON protocol sends for the same request, so a service module
//! decodes the request once, whichever protocol it came by. A segment made of digits is a list position; a map that
//! has only such segments becomes a list, in numeric order.

use std::collections::BTreeMap;

use serde_json::{Map, Value};

/// The most pairs a request may carry (a batch of 10 entries with their attributes is far below it).
pub const MAX_PAIRS: usize = 2000;
/// The deepest path (`Entry.1.MessageAttribute.1.Value.StringValue` has 6 segments).
const MAX_DEPTH: usize = 8;
/// The largest list position accepted: it bounds the list a path like `A.99999999` could ask for.
const MAX_INDEX: usize = 1000;

#[derive(Debug, PartialEq, Eq)]
pub enum FormError {
    TooMany,
    Invalid(String),
}

/// `a=1&b=%2F+x` as decoded pairs. `+` is a space.
pub fn pairs(text: &str) -> Result<Vec<(String, String)>, FormError> {
    let mut out = Vec::new();
    for pair in text.split('&').filter(|p| !p.is_empty()) {
        if out.len() >= MAX_PAIRS {
            return Err(FormError::TooMany);
        }
        let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
        out.push((decode(name)?, decode(value)?));
    }
    Ok(out)
}

fn decode(text: &str) -> Result<String, FormError> {
    let bytes = super::sigv4::percent_decode(&text.replace('+', " "));
    String::from_utf8(bytes).map_err(|_| FormError::Invalid("the form is not valid UTF-8".into()))
}

#[derive(Debug)]
enum Node {
    Leaf(String),
    Branch(BTreeMap<String, Node>),
}

/// Builds the value from the decoded pairs.
pub fn to_value(pairs: &[(String, String)]) -> Result<Value, FormError> {
    let mut root = BTreeMap::new();
    for (name, value) in pairs {
        let segments: Vec<&str> = name.split('.').collect();
        if segments.len() > MAX_DEPTH || segments.iter().any(|s| s.is_empty()) {
            return Err(FormError::Invalid(format!(
                "the parameter `{name}` is not valid"
            )));
        }
        insert(&mut root, &segments, value, name)?;
    }
    Ok(branch_to_value(root))
}

fn insert(
    level: &mut BTreeMap<String, Node>,
    segments: &[&str],
    value: &str,
    full: &str,
) -> Result<(), FormError> {
    let (head, rest) = segments.split_first().expect("a path has a segment");
    if head.chars().all(|c| c.is_ascii_digit())
        && head.parse::<usize>().is_ok_and(|n| n > MAX_INDEX)
    {
        return Err(FormError::Invalid(format!(
            "the position in `{full}` is too large"
        )));
    }
    if rest.is_empty() {
        if level.contains_key(*head) {
            return Err(FormError::Invalid(format!(
                "the parameter `{full}` is repeated"
            )));
        }
        level.insert((*head).to_owned(), Node::Leaf(value.to_owned()));
        return Ok(());
    }
    match level
        .entry((*head).to_owned())
        .or_insert_with(|| Node::Branch(BTreeMap::new()))
    {
        Node::Branch(next) => insert(next, rest, value, full),
        Node::Leaf(_) => Err(FormError::Invalid(format!(
            "the parameter `{full}` conflicts with another"
        ))),
    }
}

fn node_to_value(node: Node) -> Value {
    match node {
        Node::Leaf(text) => Value::String(text),
        Node::Branch(map) => branch_to_value(map),
    }
}

fn branch_to_value(map: BTreeMap<String, Node>) -> Value {
    let numbered = !map.is_empty() && map.keys().all(|k| k.chars().all(|c| c.is_ascii_digit()));
    if numbered {
        let mut items: Vec<(usize, Node)> = map
            .into_iter()
            .map(|(k, v)| (k.parse::<usize>().unwrap_or(0), v))
            .collect();
        items.sort_by_key(|(n, _)| *n);
        return Value::Array(items.into_iter().map(|(_, v)| node_to_value(v)).collect());
    }
    let mut object = Map::new();
    for (key, node) in map {
        object.insert(key, node_to_value(node));
    }
    Value::Object(object)
}

#[cfg(test)]
mod tests;
