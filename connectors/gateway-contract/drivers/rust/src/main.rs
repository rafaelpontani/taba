//! The Rust driver of the gateway contract tests (connectors/gateway-contract/README.md): reads the scenario files and
//! runs each line through aws-sdk-sqs and aws-sdk-sns, the way an application would. Prints one RESULT line per
//! scenario line.
use std::collections::HashMap;
use std::fmt::{Debug, Display};
use std::time::{Duration, Instant};

use aws_config::retry::RetryConfig;
use aws_config::{BehaviorVersion, Region};
use aws_sdk_sns::types::{MessageAttributeValue as SnsAttr, PublishBatchRequestEntry};
use aws_sdk_sqs::config::Credentials;
use aws_sdk_sqs::error::{ProvideErrorMetadata, SdkError};
use aws_sdk_sqs::primitives::Blob;
use aws_sdk_sqs::types::{
    ChangeMessageVisibilityBatchRequestEntry, DeleteMessageBatchRequestEntry, MessageAttributeValue as SqsAttr, MessageSystemAttributeName,
    QueueAttributeName, SendMessageBatchRequestEntry,
};
use base64::Engine;
use md5::{Digest, Md5};

#[derive(Default, Clone)]
struct Res {
    m: HashMap<String, String>,
    handles: Vec<String>,
}

impl Res {
    fn with(kv: &[(&str, String)]) -> Res {
        let mut r = Res::default();
        for (k, v) in kv {
            r.m.insert((*k).to_string(), v.clone());
        }
        r
    }
}

enum Failure {
    /// An answer from the service: the AWS error code and message.
    Service(String, String),
    /// Anything else: no answer (connection), or the driver itself.
    Other(String),
}

fn fail<E, R>(e: SdkError<E, R>) -> Failure
where
    E: std::error::Error + Send + Sync + 'static,
    R: Debug,
    SdkError<E, R>: ProvideErrorMetadata,
{
    match e.code() {
        Some(code) => Failure::Service(code.to_string(), e.message().unwrap_or("").to_string()),
        None => Failure::Other(format!("{}", aws_sdk_sqs::error::DisplayErrorContext(&e))),
    }
}

fn other<T: Display>(e: T) -> Failure {
    Failure::Other(e.to_string())
}

struct Ctx {
    args: HashMap<String, String>,
    creds: HashMap<String, (String, String)>,
    sqs: aws_sdk_sqs::Client,
    sns: aws_sdk_sns::Client,
}

async fn make_clients(args: &HashMap<String, String>, credentials: Option<(String, String)>) -> (aws_sdk_sqs::Client, aws_sdk_sns::Client) {
    let region = args.get("region").cloned().unwrap_or_else(|| "us-east-1".into());
    let mut loader = aws_config::defaults(BehaviorVersion::latest()).region(Region::new(region)).retry_config(RetryConfig::disabled());
    if let Some((ak, sk)) = credentials {
        loader = loader.credentials_provider(Credentials::new(ak, sk, None, None, "gateway-contract"));
    }
    let config = loader.load().await;
    let mut sqs = aws_sdk_sqs::config::Builder::from(&config);
    let mut sns = aws_sdk_sns::config::Builder::from(&config);
    if args.get("endpoint-mode").map(String::as_str) == Some("code") {
        sqs = sqs.endpoint_url(args["sqs-endpoint"].clone());
        sns = sns.endpoint_url(args["sns-endpoint"].clone());
    }
    (aws_sdk_sqs::Client::from_conf(sqs.build()), aws_sdk_sns::Client::from_conf(sns.build()))
}

// ---- the scenario language ----------------------------------------------------------------------------------------

fn unquote(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Some(v) = std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

fn expand(value: &str, args: &HashMap<String, String>, vars: &HashMap<String, Res>) -> String {
    let chars: Vec<char> = value.chars().collect();
    let mut out = String::new();
    let mut literal = String::new();
    let mut i = 0;
    let flush = |literal: &mut String, out: &mut String| {
        out.push_str(&unquote(literal));
        literal.clear();
    };
    while i < chars.len() {
        let rest: String = chars[i..].iter().collect();
        if rest.starts_with("{p}") {
            flush(&mut literal, &mut out);
            out.push_str(args.get("prefix").map(String::as_str).unwrap_or(""));
            i += 3;
        } else if rest.starts_with("{lang}") {
            flush(&mut literal, &mut out);
            out.push_str(args.get("lang").map(String::as_str).unwrap_or(""));
            i += 6;
        } else if rest.starts_with("{prev}") {
            flush(&mut literal, &mut out);
            out.push_str(args.get("prev").map(String::as_str).unwrap_or(""));
            i += 6;
        } else if chars[i] == '$' {
            let mut j = i + 1;
            while j < chars.len() && is_word(chars[j]) {
                j += 1;
            }
            let name: String = chars[i + 1..j].iter().collect();
            if j < chars.len() && chars[j] == '.' && !name.is_empty() {
                let mut k = j + 1;
                while k < chars.len() && is_word(chars[k]) {
                    k += 1;
                }
                let field: String = chars[j + 1..k].iter().collect();
                if !field.is_empty() {
                    flush(&mut literal, &mut out);
                    if let Some(v) = vars.get(&name).and_then(|r| r.m.get(&field)) {
                        out.push_str(v);
                    }
                    i = k;
                    continue;
                }
            }
            literal.push('$');
            i += 1;
        } else {
            literal.push(chars[i]);
            i += 1;
        }
    }
    flush(&mut literal, &mut out);
    out
}

struct Line {
    op: String,
    p: HashMap<String, String>,
    expects: Vec<(String, String, String)>,
}

fn parse_line(text: &str, args: &HashMap<String, String>, vars: &HashMap<String, Res>) -> Result<Line, String> {
    let (head, tail) = match text.find(" => ") {
        Some(at) => (&text[..at], &text[at + 4..]),
        None => (text, ""),
    };
    let mut parts = head.split_whitespace();
    let op = parts.next().ok_or("empty line")?.to_string();
    let mut p = HashMap::new();
    for part in parts {
        let (k, v) = part.split_once('=').unwrap_or((part, ""));
        p.insert(k.to_string(), expand(v, args, vars));
    }
    let mut expects = Vec::new();
    for token in tail.split_whitespace() {
        let at = token.find(|c| c == '=' || c == '~' || c == '!' || c == '>' || c == '<').ok_or("bad expectation")?;
        let key = &token[..at];
        let rest = &token[at..];
        let (opr, value) = if rest.starts_with(">=") || rest.starts_with("<=") || rest.starts_with("!=") {
            (&rest[..2], &rest[2..])
        } else {
            (&rest[..1], &rest[1..])
        };
        expects.push((key.to_string(), opr.to_string(), expand(value, args, vars)));
    }
    Ok(Line { op, p, expects })
}

fn check(r: &Res, expects: &[(String, String, String)]) -> Option<String> {
    for (key, op, want) in expects {
        let got = r.m.get(key).cloned().unwrap_or_default();
        let ok = match op.as_str() {
            "=" if want == "*" => !got.is_empty(),
            "=" => &got == want,
            "!=" => &got != want,
            "~" => got.contains(want.as_str()),
            ">=" => matches!((got.parse::<f64>(), want.parse::<f64>()), (Ok(a), Ok(b)) if a >= b),
            _ => matches!((got.parse::<f64>(), want.parse::<f64>()), (Ok(a), Ok(b)) if a <= b),
        };
        if !ok {
            let shown: String = got.chars().take(200).collect();
            return Some(format!("{key}{op}{want} but was '{shown}'"));
        }
    }
    None
}

// ---- the operations ---------------------------------------------------------------------------------------------------

fn md5hex(s: &str) -> String {
    Md5::digest(s.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

fn pairs(t: &str) -> HashMap<String, String> {
    let mut d = HashMap::new();
    if t.is_empty() {
        return d;
    }
    for item in t.split(',') {
        let (k, v) = item.split_once(':').unwrap_or((item, ""));
        d.insert(k.to_string(), v.to_string());
    }
    d
}

/// "name:Type:value" items.
fn attr_items(t: &str) -> Vec<(String, String, String)> {
    if t.is_empty() {
        return vec![];
    }
    t.split(',')
        .map(|item| {
            let mut it = item.splitn(3, ':');
            (it.next().unwrap_or("").to_string(), it.next().unwrap_or("").to_string(), it.next().unwrap_or("").to_string())
        })
        .collect()
}

fn binary(v: &str) -> Vec<u8> {
    base64::engine::general_purpose::STANDARD.decode(v.trim_start_matches("b64:")).unwrap_or_default()
}

fn b64(bytes: &[u8]) -> String {
    format!("b64:{}", base64::engine::general_purpose::STANDARD.encode(bytes))
}

fn last_segment(url: &str) -> String {
    url.trim_end_matches('/').rsplit('/').next().unwrap_or("").to_string()
}

fn sqs_attrs(t: &str) -> HashMap<String, SqsAttr> {
    attr_items(t)
        .into_iter()
        .map(|(n, ty, v)| {
            let b = SqsAttr::builder().data_type(ty.clone());
            let b = if ty == "Binary" { b.binary_value(Blob::new(binary(&v))) } else { b.string_value(v) };
            (n, b.build().expect("attribute"))
        })
        .collect()
}

fn sns_attrs(t: &str) -> HashMap<String, SnsAttr> {
    attr_items(t)
        .into_iter()
        .map(|(n, ty, v)| {
            let b = SnsAttr::builder().data_type(ty.clone());
            let b = if ty == "Binary" { b.binary_value(Blob::new(binary(&v))) } else { b.string_value(v) };
            (n, b.build().expect("attribute"))
        })
        .collect()
}

fn received(msgs: &[aws_sdk_sqs::types::Message], elapsed: u128) -> Res {
    let mut r = Res::default();
    let bodies: Vec<String> = msgs.iter().map(|m| m.body().unwrap_or("").to_string()).collect();
    r.m.insert("count".into(), msgs.len().to_string());
    r.m.insert("elapsedMs".into(), elapsed.to_string());
    r.m.insert("bodies".into(), bodies.join(","));
    let mut sorted = bodies.clone();
    sorted.sort();
    r.m.insert("sorted".into(), sorted.join(","));
    r.handles = msgs.iter().map(|m| m.receipt_handle().unwrap_or("").to_string()).collect();
    if let Some(m) = msgs.first() {
        let body = m.body().unwrap_or("");
        r.m.insert("body".into(), body.to_string());
        r.m.insert("id".into(), m.message_id().unwrap_or("").to_string());
        r.m.insert("handle".into(), m.receipt_handle().unwrap_or("").to_string());
        r.m.insert("md5".into(), m.md5_of_body().unwrap_or("").to_string());
        r.m.insert("md5Ok".into(), (md5hex(body) == m.md5_of_body().unwrap_or("")).to_string());
        r.m.insert("size".into(), body.chars().count().to_string());
        for (k, v) in m.attributes().into_iter().flatten() {
            r.m.insert(format!("attr_{}", k.as_str()), v.clone());
        }
        message_attributes(m, &mut r.m);
        if let Ok(serde_json::Value::Object(doc)) = serde_json::from_str::<serde_json::Value>(body) {
            for (k, v) in doc {
                if let serde_json::Value::String(s) = v {
                    r.m.insert(format!("json_{k}"), s);
                }
            }
        }
    }
    r
}

fn message_attributes(m: &aws_sdk_sqs::types::Message, into: &mut HashMap<String, String>) {
    for (name, a) in m.message_attributes().into_iter().flatten() {
        let ty = a.data_type().to_string();
        let value = if ty.starts_with("Binary") { b64(a.binary_value().map(|b| b.as_ref()).unwrap_or(&[])) } else { a.string_value().unwrap_or("").to_string() };
        into.insert(format!("msgattr_{name}"), value);
        into.insert(format!("msgattrtype_{name}"), ty);
    }
}

fn counts(ok: usize, failed: usize) -> Res {
    Res::with(&[("ok", ok.to_string()), ("failed", failed.to_string())])
}

fn num(p: &HashMap<String, String>, k: &str) -> i32 {
    p.get(k).and_then(|v| v.parse().ok()).unwrap_or(0)
}

async fn op(ctx: &mut Ctx, name: &str, p: &HashMap<String, String>, vars: &HashMap<String, Res>) -> Result<Res, Failure> {
    let g = |k: &str| p.get(k).cloned().unwrap_or_default();
    let has = |k: &str| p.get(k).map(|v| !v.is_empty()).unwrap_or(false);
    let sqs = ctx.sqs.clone();
    let sns = ctx.sns.clone();
    match name {
        "createQueue" => {
            let mut a = pairs(&g("attrs"));
            if g("fifo") == "true" {
                a.insert("FifoQueue".into(), "true".into());
            }
            if has("redriveArn") {
                a.insert("RedrivePolicy".into(), format!("{{\"deadLetterTargetArn\":\"{}\",\"maxReceiveCount\":{}}}", g("redriveArn"), g("redriveMax")));
            }
            let mut req = sqs.create_queue().queue_name(g("name"));
            for (k, v) in a {
                req = req.attributes(QueueAttributeName::from(k.as_str()), v);
            }
            let o = req.send().await.map_err(fail)?;
            Ok(Res::with(&[("url", o.queue_url().unwrap_or("").to_string()), ("name", g("name"))]))
        }
        "getQueueUrl" => {
            let o = sqs.get_queue_url().queue_name(g("name")).send().await.map_err(fail)?;
            Ok(Res::with(&[("url", o.queue_url().unwrap_or("").to_string())]))
        }
        "deleteQueue" => sqs.delete_queue().queue_url(g("queue")).send().await.map(|_| Res::default()).map_err(fail),
        "purgeQueue" => sqs.purge_queue().queue_url(g("queue")).send().await.map(|_| Res::default()).map_err(fail),
        "listQueues" => {
            let o = sqs.list_queues().queue_name_prefix(g("prefix")).send().await.map_err(fail)?;
            let mut names: Vec<String> = o.queue_urls().iter().map(|u| last_segment(u)).collect();
            names.sort();
            Ok(Res::with(&[("count", names.len().to_string()), ("names", names.join(","))]))
        }
        "getQueueAttributes" => {
            let mut req = sqs.get_queue_attributes().queue_url(g("queue"));
            for n in g("names").split(',') {
                req = req.attribute_names(QueueAttributeName::from(n));
            }
            let o = req.send().await.map_err(fail)?;
            let mut r = Res::default();
            for (k, v) in o.attributes().into_iter().flatten() {
                r.m.insert(format!("attr_{}", k.as_str()), v.clone());
            }
            Ok(r)
        }
        "setQueueAttributes" => {
            let mut req = sqs.set_queue_attributes().queue_url(g("queue"));
            for (k, v) in pairs(&g("attrs")) {
                req = req.attributes(QueueAttributeName::from(k.as_str()), v);
            }
            req.send().await.map(|_| Res::default()).map_err(fail)
        }
        "tagQueue" => {
            let mut req = sqs.tag_queue().queue_url(g("queue"));
            for (k, v) in pairs(&g("tags")) {
                req = req.tags(k, v);
            }
            req.send().await.map(|_| Res::default()).map_err(fail)
        }
        "untagQueue" => {
            let mut req = sqs.untag_queue().queue_url(g("queue"));
            for k in g("keys").split(',') {
                req = req.tag_keys(k);
            }
            req.send().await.map(|_| Res::default()).map_err(fail)
        }
        "listQueueTags" => {
            let o = sqs.list_queue_tags().queue_url(g("queue")).send().await.map_err(fail)?;
            let mut r = Res::default();
            for (k, v) in o.tags().into_iter().flatten() {
                r.m.insert(format!("tag_{k}"), v.clone());
            }
            Ok(r)
        }
        "send" | "sendBig" => {
            let body = if name == "sendBig" { "x".repeat(num(p, "size") as usize) } else { g("body") };
            let mut req = sqs.send_message().queue_url(g("queue")).message_body(body.clone());
            if name == "send" {
                if has("delay") {
                    req = req.delay_seconds(num(p, "delay"));
                }
                if has("msgattrs") {
                    for (k, v) in sqs_attrs(&g("msgattrs")) {
                        req = req.message_attributes(k, v);
                    }
                }
                if has("group") {
                    req = req.message_group_id(g("group"));
                }
                if has("dedup") {
                    req = req.message_deduplication_id(g("dedup"));
                }
            }
            let o = req.send().await.map_err(fail)?;
            let md5 = o.md5_of_message_body().unwrap_or("").to_string();
            Ok(Res::with(&[
                ("id", o.message_id().unwrap_or("").to_string()),
                ("md5Ok", (md5hex(&body) == md5).to_string()),
                ("md5", md5),
                ("sequence", o.sequence_number().unwrap_or("").to_string()),
            ]))
        }
        "sendBatch" | "sendBatchTooMany" => {
            let bodies: Vec<String> = if name == "sendBatch" { g("bodies").split(',').map(String::from).collect() } else { (0..num(p, "count")).map(|i| format!("m{i}")).collect() };
            let delays: Vec<String> = if has("delays") { g("delays").split(',').map(String::from).collect() } else { vec![] };
            let mut req = sqs.send_message_batch().queue_url(g("queue"));
            for (i, b) in bodies.iter().enumerate() {
                let mut e = SendMessageBatchRequestEntry::builder().id(i.to_string()).message_body(b.clone());
                if !delays.is_empty() {
                    e = e.delay_seconds(delays[i].parse().unwrap_or(0));
                }
                if has("group") {
                    e = e.message_group_id(g("group"));
                }
                if has("dedupPrefix") {
                    e = e.message_deduplication_id(format!("{}{}", g("dedupPrefix"), i));
                }
                req = req.entries(e.build().map_err(other)?);
            }
            let o = req.send().await.map_err(fail)?;
            Ok(counts(o.successful().len(), o.failed().len()))
        }
        "receive" => {
            let mut req = sqs.receive_message().queue_url(g("queue")).max_number_of_messages(if has("max") { num(p, "max") } else { 1 }).wait_time_seconds(num(p, "wait"));
            if has("visibility") {
                req = req.visibility_timeout(num(p, "visibility"));
            }
            if has("attrs") {
                for n in g("attrs").split(',') {
                    req = req.message_system_attribute_names(MessageSystemAttributeName::from(n));
                }
            }
            if has("msgattrs") {
                for n in g("msgattrs").split(',') {
                    req = req.message_attribute_names(n);
                }
            }
            let t0 = Instant::now();
            let o = req.send().await.map_err(fail)?;
            Ok(received(o.messages(), t0.elapsed().as_millis()))
        }
        "drainAll" => {
            let want = num(p, "count") as usize;
            let deadline = Instant::now() + Duration::from_secs(num(p, "timeout") as u64);
            let mut r = Res::default();
            let mut bodies = Vec::new();
            while bodies.len() < want && Instant::now() < deadline {
                let mut req = sqs.receive_message().queue_url(g("queue")).max_number_of_messages(10).wait_time_seconds(1);
                if has("msgattrs") {
                    for n in g("msgattrs").split(',') {
                        req = req.message_attribute_names(n);
                    }
                }
                let o = req.send().await.map_err(fail)?;
                for m in o.messages() {
                    bodies.push(m.body().unwrap_or("").to_string());
                    message_attributes(m, &mut r.m);
                    sqs.delete_message().queue_url(g("queue")).receipt_handle(m.receipt_handle().unwrap_or("")).send().await.map_err(fail)?;
                }
            }
            r.m.insert("count".into(), bodies.len().to_string());
            r.m.insert("bodies".into(), bodies.join(","));
            Ok(r)
        }
        "deleteMessage" => sqs.delete_message().queue_url(g("queue")).receipt_handle(g("handle")).send().await.map(|_| Res::default()).map_err(fail),
        "changeVisibility" => sqs
            .change_message_visibility()
            .queue_url(g("queue"))
            .receipt_handle(g("handle"))
            .visibility_timeout(num(p, "seconds"))
            .send()
            .await
            .map(|_| Res::default())
            .map_err(fail),
        "deleteAll" => {
            let handles = vars.get(g("from").trim_start_matches('$')).map(|r| r.handles.clone()).unwrap_or_default();
            let mut req = sqs.delete_message_batch().queue_url(g("queue"));
            for (i, h) in handles.iter().enumerate() {
                req = req.entries(DeleteMessageBatchRequestEntry::builder().id(i.to_string()).receipt_handle(h.clone()).build().map_err(other)?);
            }
            let o = req.send().await.map_err(fail)?;
            Ok(counts(o.successful().len(), o.failed().len()))
        }
        "changeVisibilityAll" => {
            let handles = vars.get(g("from").trim_start_matches('$')).map(|r| r.handles.clone()).unwrap_or_default();
            let mut req = sqs.change_message_visibility_batch().queue_url(g("queue"));
            for (i, h) in handles.iter().enumerate() {
                req = req.entries(
                    ChangeMessageVisibilityBatchRequestEntry::builder().id(i.to_string()).receipt_handle(h.clone()).visibility_timeout(num(p, "seconds")).build().map_err(other)?,
                );
            }
            let o = req.send().await.map_err(fail)?;
            Ok(counts(o.successful().len(), o.failed().len()))
        }
        "allowSnsToSend" => {
            let policy = format!(
                "{{\"Version\":\"2012-10-17\",\"Statement\":[{{\"Effect\":\"Allow\",\"Principal\":{{\"Service\":\"sns.amazonaws.com\"}},\"Action\":\"sqs:SendMessage\",\"Resource\":\"{}\",\"Condition\":{{\"ArnEquals\":{{\"aws:SourceArn\":\"{}\"}}}}}}]}}",
                g("queueArn"),
                g("topic")
            );
            sqs.set_queue_attributes().queue_url(g("queue")).attributes(QueueAttributeName::Policy, policy).send().await.map(|_| Res::default()).map_err(fail)
        }
        "sleep" => {
            tokio::time::sleep(Duration::from_millis((g("seconds").parse::<f64>().unwrap_or(0.0) * 1000.0) as u64)).await;
            Ok(Res::default())
        }
        "useCredentials" => {
            let c = ctx.creds.get(&g("name")).cloned().ok_or_else(|| Failure::Other("unknown credentials".into()))?;
            let (a, b) = make_clients(&ctx.args, Some(c)).await;
            ctx.sqs = a;
            ctx.sns = b;
            Ok(Res::default())
        }
        "createTopic" => {
            let mut req = sns.create_topic().name(g("name"));
            if g("fifo") == "true" {
                req = req.attributes("FifoTopic", "true");
            }
            let o = req.send().await.map_err(fail)?;
            Ok(Res::with(&[("arn", o.topic_arn().unwrap_or("").to_string())]))
        }
        "deleteTopic" => sns.delete_topic().topic_arn(g("topic")).send().await.map(|_| Res::default()).map_err(fail),
        "listTopics" => {
            let mut names = Vec::new();
            let mut token: Option<String> = None;
            loop {
                let o = sns.list_topics().set_next_token(token.clone()).send().await.map_err(fail)?;
                for t in o.topics() {
                    let arn = t.topic_arn().unwrap_or("");
                    let n = arn.rsplit(':').next().unwrap_or("");
                    if n.starts_with(&g("prefix")) {
                        names.push(n.to_string());
                    }
                }
                token = o.next_token().filter(|t| !t.is_empty()).map(String::from);
                if token.is_none() {
                    break;
                }
            }
            names.sort();
            Ok(Res::with(&[("count", names.len().to_string()), ("names", names.join(","))]))
        }
        "getTopicAttributes" => {
            let o = sns.get_topic_attributes().topic_arn(g("topic")).send().await.map_err(fail)?;
            let mut r = Res::default();
            for (k, v) in o.attributes().into_iter().flatten() {
                r.m.insert(format!("attr_{k}"), v.clone());
            }
            Ok(r)
        }
        "subscribe" => {
            let mut req = sns.subscribe().topic_arn(g("topic")).protocol(g("protocol")).endpoint(g("endpoint")).return_subscription_arn(true);
            if has("attrs") {
                for (k, v) in pairs(&g("attrs")) {
                    req = req.attributes(k, v);
                }
            }
            let o = req.send().await.map_err(fail)?;
            Ok(Res::with(&[("arn", o.subscription_arn().unwrap_or("").to_string())]))
        }
        "unsubscribe" => sns.unsubscribe().subscription_arn(g("sub")).send().await.map(|_| Res::default()).map_err(fail),
        "listSubscriptions" => {
            let o = sns.list_subscriptions_by_topic().topic_arn(g("topic")).send().await.map_err(fail)?;
            let subs = o.subscriptions();
            let mut endpoints: Vec<String> = subs.iter().map(|s| s.endpoint().unwrap_or("").to_string()).collect();
            endpoints.sort();
            let mut protocols: Vec<String> = subs.iter().map(|s| s.protocol().unwrap_or("").to_string()).collect();
            protocols.sort();
            protocols.dedup();
            Ok(Res::with(&[("count", subs.len().to_string()), ("endpoints", endpoints.join(",")), ("protocols", protocols.join(","))]))
        }
        "getSubscriptionAttributes" => {
            let o = sns.get_subscription_attributes().subscription_arn(g("sub")).send().await.map_err(fail)?;
            let mut r = Res::default();
            for (k, v) in o.attributes().into_iter().flatten() {
                r.m.insert(format!("attr_{k}"), v.clone());
            }
            Ok(r)
        }
        "setSubscriptionAttributes" => {
            for (k, v) in pairs(&g("attrs")) {
                sns.set_subscription_attributes().subscription_arn(g("sub")).attribute_name(k).attribute_value(v).send().await.map_err(fail)?;
            }
            Ok(Res::default())
        }
        "publish" => {
            let mut req = sns.publish().topic_arn(g("topic")).message(g("message"));
            if has("subject") {
                req = req.subject(g("subject"));
            }
            if has("msgattrs") {
                for (k, v) in sns_attrs(&g("msgattrs")) {
                    req = req.message_attributes(k, v);
                }
            }
            if has("group") {
                req = req.message_group_id(g("group"));
            }
            if has("dedup") {
                req = req.message_deduplication_id(g("dedup"));
            }
            let o = req.send().await.map_err(fail)?;
            Ok(Res::with(&[("messageId", o.message_id().unwrap_or("").to_string())]))
        }
        "publishBatch" => {
            let mut req = sns.publish_batch().topic_arn(g("topic"));
            for (i, m) in g("messages").split(',').enumerate() {
                req = req.publish_batch_request_entries(PublishBatchRequestEntry::builder().id(i.to_string()).message(m).build().map_err(other)?);
            }
            let o = req.send().await.map_err(fail)?;
            Ok(counts(o.successful().len(), o.failed().len()))
        }
        _ => Err(Failure::Other(format!("operation {name} is not implemented"))),
    }
}

fn reachable(url: &str) -> bool {
    use std::net::ToSocketAddrs;
    let rest = url.trim_start_matches("http://").trim_start_matches("https://");
    let hostport = rest.split('/').next().unwrap_or("");
    let hostport = if hostport.contains(':') { hostport.to_string() } else { format!("{hostport}:80") };
    match hostport.to_socket_addrs() {
        Ok(mut addrs) => addrs.any(|a| std::net::TcpStream::connect_timeout(&a, Duration::from_secs(3)).is_ok()),
        Err(_) => false,
    }
}

#[tokio::main]
async fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let mut args = HashMap::new();
    let mut scenarios = Vec::new();
    let mut creds = HashMap::new();
    for pair in argv.chunks(2) {
        if pair.len() < 2 {
            break;
        }
        let key = pair[0].trim_start_matches("--").to_string();
        match key.as_str() {
            "scenario" => scenarios.push(pair[1].clone()),
            "cred" => {
                let (name, kv) = pair[1].split_once('=').unwrap_or((&pair[1], ""));
                let (ak, sk) = kv.split_once(':').unwrap_or((kv, ""));
                creds.insert(name.to_string(), (ak.to_string(), sk.to_string()));
            }
            _ => {
                args.insert(key, pair[1].clone());
            }
        }
    }
    let (sqs, sns) = make_clients(&args, None).await;
    let mut ctx = Ctx { args: args.clone(), creds, sqs, sns };
    let unreachable = !(reachable(&args["sqs-endpoint"]) && reachable(&args["sns-endpoint"]));
    let mut vars: HashMap<String, Res> = HashMap::new();
    let (mut pass, mut failed, mut connect) = (0, 0, 0);
    for path in &scenarios {
        let file = path.rsplit(['/', '\\']).next().unwrap_or(path).to_string();
        let text = std::fs::read_to_string(path).expect("scenario");
        for (n, raw) in text.lines().enumerate() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (mut status, mut detail) = ("PASS", String::new());
            let mut op_name = line.split_whitespace().next().unwrap_or("").to_string();
            match parse_line(line, &args, &vars) {
                Err(e) => (status, detail) = ("FAIL", e),
                Ok(parsed) => {
                    op_name = parsed.op.clone();
                    let expected = parsed.expects.iter().find(|e| e.0 == "error").map(|e| e.2.clone());
                    match op(&mut ctx, &parsed.op, &parsed.p, &vars).await {
                        Ok(r) => {
                            if let Some(e) = &expected {
                                (status, detail) = ("FAIL", format!("expected error {e} but the call worked"));
                            } else if let Some(problem) = check(&r, &parsed.expects) {
                                (status, detail) = ("FAIL", problem);
                            }
                            if let Some(as_name) = parsed.p.get("as") {
                                vars.insert(as_name.clone(), r);
                            }
                        }
                        Err(Failure::Service(code, message)) => {
                            let code = code.rsplit('#').next().unwrap_or("").to_string();
                            if !expected.as_deref().map(|e| e.split('|').any(|c| c == code)).unwrap_or(false) {
                                (status, detail) = ("FAIL", format!("error {code}: {}", message.chars().take(160).collect::<String>()));
                            }
                        }
                        Err(Failure::Other(message)) => {
                            if unreachable {
                                (status, detail) = ("CONNECT", "unreachable".to_string());
                            } else {
                                (status, detail) = ("FAIL", message.chars().take(200).collect());
                            }
                        }
                    }
                }
            }
            match status {
                "PASS" => pass += 1,
                "FAIL" => failed += 1,
                _ => connect += 1,
            }
            println!("RESULT {status} {file}:{} {op_name}{}", n + 1, if detail.is_empty() { String::new() } else { format!(" :: {detail}") });
        }
    }
    println!(
        "SUMMARY lang={} variant={} sdk={} pass={pass} fail={failed} connect={connect}",
        args.get("lang").cloned().unwrap_or_default(),
        args.get("variant").cloned().unwrap_or_default(),
        args.get("sdk").cloned().unwrap_or_default()
    );
}
