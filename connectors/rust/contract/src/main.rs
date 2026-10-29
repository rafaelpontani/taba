//! The Rust driver of the connector contract tests (connectors/contract/README.md): reads the scenario files and runs
//! each line against a real Taba through the connector, in the style of the language.
//!
//!     cargo run --release -- --url http://host:8080 --token TOKEN --prefix ct-rust-0101 --lang rust \
//!         --scenario ../../contract/scenarios/core.scn

use std::collections::HashMap;
use std::time::{Duration, Instant};

use taba_client::base64;
use taba_client::{
    BatchMessage, BatchResult, Client, Config, ConsumeError, ConsumeOptions, Error, Message,
    Nullable, PublishOptions, PublishResponse, Topic, TopicRequest, TopicUpdate,
};

type Fields = HashMap<String, String>;

struct Args {
    values: HashMap<String, String>,
    scenarios: Vec<String>,
}

impl Args {
    fn get(&self, key: &str) -> &str {
        self.values.get(key).map(String::as_str).unwrap_or("")
    }
}

// ---- the scenario language ------------------------------------------------------------------------------

/// Decodes the %XX runs as UTF-8 (a "+" stays as it is).
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(byte) = u8::from_str_radix(&text[i + 1..i + 3], 16) {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Literal parts are percent-decoded; {p}, {lang} and $name.field are replaced afterwards, untouched.
fn expand(value: &str, args: &Args, variables: &HashMap<String, Fields>) -> String {
    let mut out = String::new();
    let mut literal = String::new();
    let chars: Vec<char> = value.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let rest: String = chars[i..].iter().collect();
        if rest.starts_with("{p}") {
            out.push_str(&percent_decode(&std::mem::take(&mut literal)));
            out.push_str(args.get("prefix"));
            i += 3;
        } else if rest.starts_with("{lang}") {
            out.push_str(&percent_decode(&std::mem::take(&mut literal)));
            out.push_str(args.get("lang"));
            i += 6;
        } else if chars[i] == '$' && variable_at(&rest).is_some() {
            let (name, field, length) = variable_at(&rest).unwrap();
            out.push_str(&percent_decode(&std::mem::take(&mut literal)));
            if let Some(v) = variables.get(&name).and_then(|f| f.get(&field)) {
                out.push_str(v);
            }
            i += length;
        } else {
            literal.push(chars[i]);
            i += 1;
        }
    }
    out.push_str(&percent_decode(&literal));
    out
}

/// `$name.field` at the start of `text`: the name, the field and how many characters it takes.
fn variable_at(text: &str) -> Option<(String, String, usize)> {
    let body = text.strip_prefix('$')?;
    let name: String = body
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    let after = body[name.len()..].strip_prefix('.')?;
    let field: String = after
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    if name.is_empty() || field.is_empty() {
        return None;
    }
    let length = 1 + name.len() + 1 + field.len();
    Some((name, field, length))
}

struct Expect {
    key: String,
    op: char,
    value: String,
}

struct Line {
    op: String,
    params: Fields,
    expects: Vec<Expect>,
}

fn parse_line(text: &str, args: &Args, variables: &HashMap<String, Fields>) -> Line {
    let (head, tail) = match text.split_once(" => ") {
        Some((head, tail)) => (head, tail),
        None => (text, ""),
    };
    let mut parts = head.split_whitespace();
    let op = parts.next().unwrap_or("").to_owned();
    let mut params = Fields::new();
    for pair in parts {
        if let Some((key, value)) = pair.split_once('=') {
            params.insert(key.to_owned(), expand(value, args, variables));
        }
    }
    let mut expects = Vec::new();
    for token in tail.split_whitespace() {
        if let Some(at) = token.find(|c| c == '=' || c == '~') {
            expects.push(Expect {
                key: token[..at].to_owned(),
                op: token[at..].chars().next().unwrap(),
                value: expand(&token[at + 1..], args, variables),
            });
        }
    }
    Line {
        op,
        params,
        expects,
    }
}

// ---- helpers of the operations --------------------------------------------------------------------------

fn num<T: std::str::FromStr>(p: &Fields, key: &str) -> Option<T> {
    p.get(key).and_then(|v| v.parse().ok())
}

fn flag(p: &Fields, key: &str) -> Option<bool> {
    p.get(key).map(|v| v == "true")
}

fn items(p: &Fields, key: &str) -> Option<Vec<String>> {
    p.get(key).map(|v| {
        v.split(',')
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect()
    })
}

fn tags(p: &Fields, key: &str) -> Option<Vec<(String, String)>> {
    p.get(key).map(|v| {
        v.split(',')
            .filter_map(|t| t.split_once(':'))
            .map(|(k, v)| (k.to_owned(), v.to_owned()))
            .collect()
    })
}

/// "null" goes back to the server default; absent leaves the field out.
fn nullable(p: &Fields, key: &str) -> Nullable<u64> {
    match p.get(key) {
        None => Nullable::Keep,
        Some(v) if v == "null" => Nullable::Default,
        Some(v) => Nullable::Set(v.parse().unwrap_or(0)),
    }
}

fn text<T: ToString>(value: Option<T>) -> String {
    value.map(|v| v.to_string()).unwrap_or_default()
}

fn render(message: &Message) -> String {
    let media = message
        .content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if media == "application/octet-stream" {
        format!("b64:{}", base64::encode(&message.body))
    } else {
        String::from_utf8_lossy(&message.body).into_owned()
    }
}

fn fields(pairs: &[(&str, String)]) -> Fields {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), v.clone()))
        .collect()
}

fn topic_fields(t: &Topic) -> Fields {
    fields(&[
        ("id", t.id.clone()),
        ("name", t.name.clone()),
        ("type", t.topic_type.clone()),
        ("fifo", text(t.fifo)),
        ("volatile", text(t.volatile)),
        ("raw", t.raw.clone()),
    ])
}

fn publish_fields(r: &PublishResponse) -> Fields {
    fields(&[
        ("id", text(r.id.clone())),
        ("duplicate", text(r.duplicate)),
        ("matched", text(r.matched)),
    ])
}

fn batch_fields(results: &[BatchResult]) -> Fields {
    let statuses: Vec<String> = results.iter().map(|r| r.status.to_string()).collect();
    fields(&[
        ("count", results.len().to_string()),
        ("statuses", statuses.join(",")),
    ])
}

fn consume_options(p: &Fields) -> ConsumeOptions {
    ConsumeOptions {
        max_messages: num(p, "max"),
        wait_time: num(p, "wait"),
        visibility_timeout: num(p, "visibility"),
        ..ConsumeOptions::default()
    }
}

fn publish_options(p: &Fields, content_type: &str) -> PublishOptions {
    PublishOptions {
        content_type: Some(content_type.to_owned()),
        group_id: p.get("groupId").cloned(),
        deduplication_id: p.get("deduplicationId").cloned(),
        fifo: flag(p, "fifo").unwrap_or(false),
        ..PublishOptions::default()
    }
}

fn message_fields(messages: &[Message]) -> Fields {
    let bodies: Vec<String> = messages.iter().map(render).collect();
    let mut out = fields(&[
        ("count", messages.len().to_string()),
        ("bodies", bodies.join(",")),
    ]);
    if let Some(first) = messages.first() {
        out.extend(fields(&[
            ("id", first.id.clone()),
            ("receiptToken", first.receipt_token.clone()),
            ("deliveryCount", first.delivery_count.to_string()),
            ("contentType", first.content_type.clone()),
            ("body", render(first)),
        ]));
    }
    out
}

// ---- the operations -------------------------------------------------------------------------------------

/// Whether the connector has the operation. `cancelConsume` is not here: the Rust client is synchronous and has no
/// way to cancel a call in flight (doc 21), so the scenario lines that use it are skipped.
fn supported(op: &str) -> bool {
    matches!(
        op,
        "createTopic"
            | "getTopic"
            | "listTopics"
            | "updateTopic"
            | "deleteTopic"
            | "publishText"
            | "publishJson"
            | "publishBytes"
            | "publishBatch"
            | "settleBatch"
            | "consume"
            | "ack"
            | "changeVisibility"
            | "handle"
            | "subscribe"
            | "drain"
            | "sleep"
    )
}

fn run(op: &str, c: &Client, p: &Fields) -> Result<Fields, Error> {
    let get = |key: &str| p.get(key).cloned().unwrap_or_default();
    match op {
        "createTopic" => c
            .create_topic(&TopicRequest {
                name: get("name"),
                topic_type: get("type"),
                tags: tags(p, "tags").unwrap_or_default(),
                prefixes: items(p, "prefixes").unwrap_or_default(),
                volatile: flag(p, "volatile"),
                idle_timeout: num(p, "idleTimeout"),
                fifo: flag(p, "fifo"),
                max_message_bytes: num(p, "maxMessageBytes"),
                ttl: num(p, "ttl"),
                max_deliveries: num(p, "maxDeliveries"),
            })
            .map(|t| topic_fields(&t)),
        "getTopic" => c.get_topic(&get("name")).map(|t| topic_fields(&t)),
        "listTopics" => {
            let topics = c.list_all_topics(p.get("name").map(String::as_str))?;
            let mut names: Vec<String> = topics.iter().map(|t| t.name.clone()).collect();
            names.sort();
            Ok(fields(&[
                ("count", topics.len().to_string()),
                ("names", names.join(",")),
            ]))
        }
        "updateTopic" => c
            .update_topic(
                &get("name"),
                &TopicUpdate {
                    tags: tags(p, "tags"),
                    prefixes: items(p, "prefixes"),
                    idle_timeout: num(p, "idleTimeout"),
                    max_message_bytes: nullable(p, "maxMessageBytes"),
                    ttl: nullable(p, "ttl"),
                    max_deliveries: nullable(p, "maxDeliveries"),
                },
            )
            .map(|t| topic_fields(&t)),
        "deleteTopic" => c.delete_topic(&get("name")).map(|_| Fields::new()),

        "publishText" => c
            .publish(
                &get("topic"),
                get("body").as_bytes(),
                &publish_options(p, "text/plain; charset=utf-8"),
            )
            .map(|r| publish_fields(&r)),
        "publishJson" => c
            .publish(
                &get("topic"),
                get("body").as_bytes(),
                &publish_options(p, "application/json"),
            )
            .map(|r| publish_fields(&r)),
        "publishBytes" => {
            let body = base64::decode(&get("base64")).unwrap_or_default();
            let content_type = p
                .get("contentType")
                .map(String::as_str)
                .unwrap_or("application/octet-stream");
            c.publish(&get("topic"), &body, &publish_options(p, content_type))
                .map(|r| publish_fields(&r))
        }

        // A batch of texts (one item per text); bad=true adds an item with an invalid content type.
        "publishBatch" => {
            let mut batch: Vec<BatchMessage> = get("texts")
                .split(',')
                .map(|text| {
                    let message = BatchMessage::text(text);
                    match p.get("groupId") {
                        Some(group) => message.group_id(group),
                        None => message,
                    }
                })
                .collect();
            if flag(p, "bad") == Some(true) {
                batch.push(BatchMessage::text("x").content_type("not a type"));
            }
            c.publish_batch(&get("topic"), &batch)
                .map(|r| batch_fields(&r))
        }
        // Consumes and settles what came in one batch: action ack or visibility (seconds). stale=true makes the receipt
        // of the first one old before the batch: it is given back and taken again.
        "settleBatch" => {
            let topic = get("topic");
            let messages = c.consume(&topic, &consume_options(p))?;
            if flag(p, "stale") == Some(true) && !messages.is_empty() {
                c.change_visibility(&topic, &messages[0].id, &messages[0].receipt_token, 0)?;
                c.consume(
                    &topic,
                    &ConsumeOptions {
                        max_messages: Some(1),
                        wait_time: Some(2),
                        visibility_timeout: num(p, "visibility"),
                        ..ConsumeOptions::default()
                    },
                )?;
            }
            if get("action") == "visibility" {
                c.change_visibility_batch(&topic, &messages, num(p, "seconds").unwrap_or(0))
            } else {
                c.ack_batch(&topic, &messages)
            }
            .map(|r| batch_fields(&r))
        }

        "consume" => c
            .consume(&get("topic"), &consume_options(p))
            .map(|m| message_fields(&m)),
        "ack" => c
            .ack(&get("topic"), &get("id"), &get("receipt"))
            .map(|_| Fields::new()),
        "changeVisibility" => c
            .change_visibility(
                &get("topic"),
                &get("id"),
                &get("receipt"),
                num(p, "seconds").unwrap_or(0),
            )
            .map(|r| fields(&[("id", r.id), ("invisibleUntil", r.invisible_until)])),

        // Consuming with a handler: mode ok (deleted), fail (returns Err), keep (delete = false) or slow (holds the
        // message for `hold` seconds, which only works if the invisibility is renewed).
        "handle" => {
            let mut seen = Vec::new();
            let mut lease_lost = false;
            let mode = get("mode");
            let hold: f64 = num(p, "hold").unwrap_or(0.0);
            let result = c.consume_with(&get("topic"), &consume_options(p), |delivery| {
                seen.push(render(delivery.message));
                if mode == "fail" {
                    return Err("the handler failed".into());
                }
                if mode == "keep" {
                    delivery.delete = false;
                }
                if mode == "slow" {
                    std::thread::sleep(Duration::from_secs_f64(hold));
                }
                lease_lost = delivery.lease_lost();
                Ok(())
            });
            let (handled, failed) = match result {
                Ok(n) => (n, false),
                Err(ConsumeError::Handler(_)) => (0, true),
                Err(ConsumeError::Api(e)) => return Err(e),
            };
            Ok(fields(&[
                ("handled", handled.to_string()),
                ("failed", failed.to_string()),
                ("bodies", seen.join(",")),
                ("leaseLost", lease_lost.to_string()),
            ]))
        }

        // The consume loop: stops after `count` messages or after `timeout` seconds. failFirst makes the first fail.
        "subscribe" => {
            let count: usize = num(p, "count").unwrap_or(1);
            let deadline =
                Instant::now() + Duration::from_secs_f64(num(p, "timeout").unwrap_or(30.0));
            let fail_first = p.get("failFirst").map(String::as_str) == Some("true");
            let seen = std::cell::RefCell::new(Vec::new());
            // The stop is only looked at between long pollings, so they are short here.
            let options = ConsumeOptions {
                wait_time: Some(2),
                visibility_timeout: num(p, "visibility"),
                ..ConsumeOptions::default()
            };
            c.subscribe(
                &get("topic"),
                &options,
                || seen.borrow().len() >= count || Instant::now() >= deadline,
                |message| {
                    seen.borrow_mut().push(render(message));
                    if fail_first && seen.borrow().len() == 1 {
                        return Err("the handler failed".into());
                    }
                    Ok(())
                },
            )?;
            let seen = seen.into_inner();
            Ok(fields(&[
                ("count", seen.len().to_string()),
                ("bodies", seen.join(",")),
            ]))
        }

        // Consumes and acknowledges until `count` messages came or `timeout` seconds passed. The bodies come sorted.
        "drain" => {
            let count: usize = num(p, "count").unwrap_or(1);
            let deadline =
                Instant::now() + Duration::from_secs_f64(num(p, "timeout").unwrap_or(30.0));
            let options = ConsumeOptions {
                max_messages: Some(10),
                wait_time: Some(1),
                visibility_timeout: Some(60),
                ..ConsumeOptions::default()
            };
            let mut seen = Vec::new();
            while seen.len() < count && Instant::now() < deadline {
                for message in c.consume(&get("topic"), &options)? {
                    seen.push(render(&message));
                    c.ack(&get("topic"), &message.id, &message.receipt_token)?;
                }
            }
            seen.sort();
            Ok(fields(&[
                ("count", seen.len().to_string()),
                ("bodies", seen.join(",")),
            ]))
        }
        "sleep" => {
            std::thread::sleep(Duration::from_secs_f64(num(p, "seconds").unwrap_or(0.0)));
            Ok(Fields::new())
        }
        _ => Ok(Fields::new()),
    }
}

// ---- the run --------------------------------------------------------------------------------------------

fn check(result: &Fields, expects: &[Expect]) -> Vec<String> {
    let mut problems = Vec::new();
    let failure = result.contains_key("error");
    let expects_error = expects.iter().any(|e| e.key == "error");
    if failure && !expects_error {
        problems.push(format!(
            "unexpected error {} (status {}): {}",
            result["error"],
            result["status"],
            result.get("message").cloned().unwrap_or_default()
        ));
    }
    if !failure && expects_error {
        problems.push("expected an error, but the call succeeded".to_owned());
    }
    for e in expects {
        let actual = result.get(&e.key).cloned();
        let mismatch = if e.op == '~' {
            !actual.as_deref().unwrap_or("").contains(&e.value)
        } else if e.value == "*" {
            actual.as_deref().unwrap_or("").is_empty()
        } else {
            actual.as_deref() != Some(e.value.as_str())
        };
        if mismatch {
            problems.push(format!(
                "{} {} {:?}, got {:?}",
                e.key, e.op, e.value, actual
            ));
        }
    }
    problems
}

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let mut args = Args {
        values: HashMap::new(),
        scenarios: Vec::new(),
    };
    for pair in argv.chunks(2) {
        let key = pair[0].trim_start_matches("--").to_owned();
        if key == "scenario" {
            args.scenarios.push(pair[1].clone());
        } else {
            args.values.insert(key, pair[1].clone());
        }
    }

    let mut clients: HashMap<String, Client> = HashMap::new();
    let mut variables: HashMap<String, Fields> = HashMap::new();
    let (mut passed, mut failed, mut skipped) = (0, 0, 0);
    for path in args.scenarios.clone() {
        let content = std::fs::read_to_string(&path).expect("scenario file");
        let file = path.rsplit('/').next().unwrap_or(&path).to_owned();
        for (n, raw) in content.lines().enumerate() {
            let line_text = raw.trim();
            if line_text.is_empty() || line_text.starts_with('#') {
                continue;
            }
            let place = format!("{}:{}", file, n + 1);
            let line = parse_line(line_text, &args, &variables);
            if !supported(&line.op) {
                skipped += 1;
                println!("SKIP {place} {} (not supported by this connector)", line.op);
                continue;
            }
            let key = format!(
                "{}|{}|{}",
                line.params.get("url").cloned().unwrap_or_default(),
                line.params.get("token").cloned().unwrap_or_default(),
                line.params.get("attempts").cloned().unwrap_or_default()
            );
            let client = clients.entry(key).or_insert_with(|| {
                let mut config = Config::new(
                    line.params
                        .get("url")
                        .cloned()
                        .unwrap_or_else(|| args.get("url").to_owned()),
                );
                config.token = Some(
                    line.params
                        .get("token")
                        .cloned()
                        .unwrap_or_else(|| args.get("token").to_owned()),
                );
                if let Some(attempts) = num(&line.params, "attempts") {
                    config.max_attempts = attempts;
                }
                Client::new(config).expect("client")
            });
            let result = match run(&line.op, client, &line.params) {
                Ok(fields) => fields,
                Err(e) => fields(&[
                    ("error", e.code.clone()),
                    ("status", e.status.to_string()),
                    ("message", e.message.clone()),
                ]),
            };
            if let Some(name) = line.params.get("as") {
                variables.insert(name.clone(), result.clone());
            }
            let problems = check(&result, &line.expects);
            if problems.is_empty() {
                passed += 1;
                println!("PASS {place} {}", line.op);
            } else {
                failed += 1;
                println!("FAIL {place} {line_text}");
                for problem in problems {
                    println!("       {problem}");
                }
            }
        }
    }
    println!(
        "{}: {passed} passed, {failed} failed, {skipped} skipped",
        args.get("lang")
    );
    std::process::exit(if failed == 0 { 0 } else { 1 });
}
