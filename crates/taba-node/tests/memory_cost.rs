//! What a message costs in heap, measured with a counting allocator on top of the same allocator the
//! `taba` binary uses (mimalloc), through the HTTP API of a real node: the live bytes of a waiting
//! message (asked for, and really reserved, because the size classes round up), the live bytes that stay
//! after the ack, and the **heap calls the node makes** to publish, take and ack one message. The calls
//! of the client (another runtime, its threads marked) are left out. It is the unit-level way to see
//! where the cost of a backlog goes, with no container or heap dump (PRB-086).

mod common;

use std::alloc::{GlobalAlloc, Layout};
use std::backtrace::Backtrace;
use std::cell::Cell;
use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};

use common::TestNode;
use futures_util::stream::{self, StreamExt};
use mimalloc::MiMalloc;
use serde_json::{Value, json};

static INNER: MiMalloc = MiMalloc;
static ASKED: AtomicI64 = AtomicI64::new(0);
static RESERVED: AtomicI64 = AtomicI64::new(0);
/// Heap calls of the threads of the node (not of the client).
static CALLS: AtomicI64 = AtomicI64::new(0);
/// Sampling of the call stacks of the allocations, for the profile phase.
static SAMPLING: AtomicBool = AtomicBool::new(false);
static SAMPLES: Mutex<Vec<Backtrace>> = Mutex::new(Vec::new());
const SAMPLE_EVERY: i64 = 10;
const SIZES: usize = 4097;
/// Live blocks by exact requested size (the last slot takes everything bigger).
static BY_SIZE: [AtomicI64; SIZES + 1] = [const { AtomicI64::new(0) }; SIZES + 1];

thread_local! {
    static CLIENT: Cell<bool> = const { Cell::new(false) };
    static CAPTURING: Cell<bool> = const { Cell::new(false) };
}

struct Counting;

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { INNER.alloc(layout) };
        Self::add(ptr, layout.size());
        ptr
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { INNER.alloc_zeroed(layout) };
        Self::add(ptr, layout.size());
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        Self::sub(ptr, layout.size());
        unsafe { INNER.dealloc(ptr, layout) };
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        Self::sub(ptr, layout.size());
        let new = unsafe { INNER.realloc(ptr, layout, new_size) };
        Self::add(new, new_size);
        new
    }
}

impl Counting {
    fn add(ptr: *mut u8, size: usize) {
        if ptr.is_null() {
            return;
        }
        ASKED.fetch_add(size as i64, Ordering::Relaxed);
        RESERVED.fetch_add(unsafe { INNER.usable_size(ptr) } as i64, Ordering::Relaxed);
        BY_SIZE[size.min(SIZES)].fetch_add(1, Ordering::Relaxed);
        if !CLIENT.with(Cell::get) && !CAPTURING.with(Cell::get) {
            let n = CALLS.fetch_add(1, Ordering::Relaxed);
            if SAMPLING.load(Ordering::Relaxed) && n % SAMPLE_EVERY == 0 {
                CAPTURING.with(|c| c.set(true));
                let stack = Backtrace::force_capture();
                SAMPLES.lock().unwrap().push(stack);
                CAPTURING.with(|c| c.set(false));
            }
        }
    }

    fn sub(ptr: *mut u8, size: usize) {
        ASKED.fetch_sub(size as i64, Ordering::Relaxed);
        RESERVED.fetch_sub(unsafe { INNER.usable_size(ptr) } as i64, Ordering::Relaxed);
        BY_SIZE[size.min(SIZES)].fetch_sub(1, Ordering::Relaxed);
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

struct Snapshot {
    asked: i64,
    reserved: i64,
    calls: i64,
    blocks: Vec<i64>,
}

fn snapshot() -> Snapshot {
    Snapshot {
        asked: ASKED.load(Ordering::Relaxed),
        reserved: RESERVED.load(Ordering::Relaxed),
        calls: CALLS.load(Ordering::Relaxed),
        blocks: BY_SIZE.iter().map(|c| c.load(Ordering::Relaxed)).collect(),
    }
}

/// The client works in a runtime of its own, whose threads are marked, so its heap calls do not count.
fn client_runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .on_thread_start(|| CLIENT.with(|c| c.set(true)))
        .build()
        .unwrap()
}

async fn publish_many(client: reqwest::Client, url: String, count: usize, body: Vec<u8>) {
    for _ in 0..count {
        let response = client
            .post(&url)
            .header("content-type", "application/octet-stream")
            .body(body.clone())
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 201);
    }
}

/// Takes 100 at a time and acks each one, as a consumer does.
async fn consume_all(client: reqwest::Client, base: String, topic: String, count: usize) {
    let mut acked = 0;
    while acked < count {
        let response = client
            .get(format!(
                "{base}/topics/{topic}/messages?maxMessages=100&visibilityTimeout=120"
            ))
            .send()
            .await
            .unwrap();
        let page: Value = response.json().await.unwrap();
        let messages = page["messages"].as_array().unwrap();
        assert!(!messages.is_empty(), "the queue emptied at {acked}");
        for message in messages {
            let url = format!(
                "{base}/topics/{topic}/messages/{}?receiptToken={}",
                message["id"].as_str().unwrap(),
                message["receiptToken"].as_str().unwrap()
            );
            let response = client.delete(url).send().await.unwrap();
            assert_eq!(response.status(), 204);
            acked += 1;
        }
    }
}

fn blocks_report(label: &str, now: &Snapshot, before: &Snapshot, n: f64) {
    for (size, live) in now.blocks.iter().enumerate() {
        let per_message = (live - before.blocks[size]) as f64 / n;
        if per_message.abs() >= 0.01 {
            eprintln!("HEAP   {label}: blocks of {size} bytes: {per_message:.2} live per message");
        }
    }
}

/// The heap of a node per message of 1 KB: waiting in a queue, after the ack, and the calls to the allocator.
/// UC: UC-BNC-006
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn heap_cost_of_a_message() {
    let node = TestNode::single().await;
    for name in ["warm", "cost"] {
        let response = node
            .client
            .post(node.client_url("/topics"))
            .json(&json!({ "name": name, "type": "queue" }))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 201);
    }
    let rt = client_runtime();
    let client = node.client.clone();
    let base = node.client_url("").trim_end_matches('/').to_owned();
    let body = vec![b'x'; 1024];

    // The first ones allocate the buffers and the caches that do not grow with the backlog.
    let warm = rt.spawn(publish_many(
        client.clone(),
        format!("{base}/topics/warm/messages"),
        500,
        body.clone(),
    ));
    warm.await.unwrap();

    let count = 5_000;
    let n = count as f64;
    let before = snapshot();
    let published = rt.spawn(publish_many(
        client.clone(),
        format!("{base}/topics/cost/messages"),
        count,
        body,
    ));
    published.await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let waiting = snapshot();
    eprintln!(
        "HEAP publish: heap calls/msg={:.0}; waiting: asked/msg={:.0} reserved/msg={:.0} (over the 1024-byte body: {:.0} and {:.0})",
        (waiting.calls - before.calls) as f64 / n,
        (waiting.asked - before.asked) as f64 / n,
        (waiting.reserved - before.reserved) as f64 / n,
        (waiting.asked - before.asked) as f64 / n - 1024.0,
        (waiting.reserved - before.reserved) as f64 / n - 1024.0,
    );
    blocks_report("waiting", &waiting, &before, n);

    let consumed = rt.spawn(consume_all(client, base, "cost".to_owned(), count));
    consumed.await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let after = snapshot();
    eprintln!(
        "HEAP consume+ack: heap calls/msg={:.0}; still live per acked message: asked {:.0}, reserved {:.0}",
        (after.calls - waiting.calls) as f64 / n,
        (after.asked - before.asked) as f64 / n,
        (after.reserved - before.reserved) as f64 / n,
    );
    blocks_report("left after the ack", &after, &before, n);
    if std::env::var_os("TABA_ALLOC_PROFILE").is_some() {
        profile(&rt, &node).await;
    }
    rt.shutdown_background();
}

/// The function of the node that asked for each sampled heap block: the first frame of `taba_node`, or else
/// of whatever library it was.
fn owner(stack: &str) -> String {
    let frames: Vec<&str> = stack
        .lines()
        .filter_map(|line| {
            let line = line.trim_start();
            let (index, name) = line.split_once(": ")?;
            index.chars().all(|c| c.is_ascii_digit()).then_some(name)
        })
        .collect();
    let ours = frames
        .iter()
        .find(|name| name.starts_with("taba_node::") && !name.contains("memory_cost"));
    let other = frames.iter().find(|name| {
        !name.starts_with("std::")
            && !name.starts_with("core::")
            && !name.starts_with("alloc::")
            && !name.contains("memory_cost")
            && !name.starts_with("__rust")
            && !name.starts_with("_")
    });
    ours.or(other).map_or("?", |s| s).to_owned()
}

/// Where the heap calls of the node come from: publishing, then taking and acking, by function.
async fn profile(rt: &tokio::runtime::Runtime, node: &TestNode) {
    let client = node.client.clone();
    let base = node.client_url("").trim_end_matches('/').to_owned();
    let response = client
        .post(format!("{base}/topics"))
        .json(&json!({ "name": "profile", "type": "queue" }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 201);
    let count = 300;
    for phase in ["publish", "consume+ack"] {
        SAMPLES.lock().unwrap().clear();
        let calls_before = CALLS.load(Ordering::Relaxed);
        SAMPLING.store(true, Ordering::Relaxed);
        if phase == "publish" {
            rt.spawn(publish_many(
                client.clone(),
                format!("{base}/topics/profile/messages"),
                count,
                vec![b'x'; 1024],
            ))
            .await
            .unwrap();
        } else {
            rt.spawn(consume_all(
                client.clone(),
                base.clone(),
                "profile".to_owned(),
                count,
            ))
            .await
            .unwrap();
        }
        SAMPLING.store(false, Ordering::Relaxed);
        let calls = CALLS.load(Ordering::Relaxed) - calls_before;
        let stacks = std::mem::take(&mut *SAMPLES.lock().unwrap());
        let mut by_owner: HashMap<String, usize> = HashMap::new();
        for stack in &stacks {
            *by_owner.entry(owner(&stack.to_string())).or_default() += 1;
        }
        let mut rows: Vec<_> = by_owner.into_iter().collect();
        rows.sort_by_key(|row| std::cmp::Reverse(row.1));
        eprintln!(
            "PROFILE {phase}: {:.0} heap calls/msg, {} samples; heap calls per message by function:",
            calls as f64 / count as f64,
            stacks.len()
        );
        for (name, hits) in rows.iter().take(22) {
            eprintln!(
                "PROFILE   {:5.1}  {name}",
                *hits as f64 * SAMPLE_EVERY as f64 / count as f64
            );
        }
    }
}

/// UC: UC-BNC-006
/// How many requests of each kind the node answers per second with 16 clients at once (read it in a
/// `--release` run: `cargo test --release -p taba-node --test memory_cost throughput -- --nocapture`).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn throughput_of_publish_take_and_ack() {
    use std::time::Instant;
    let node = TestNode::single().await;
    let response = node
        .client
        .post(node.client_url("/topics"))
        .json(&json!({ "name": "t", "type": "queue" }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 201);
    let rt = client_runtime();
    let base = node.client_url("").trim_end_matches('/').to_owned();
    let per_client = 2_000;
    let clients = 16;
    let started = Instant::now();
    let tasks: Vec<_> = (0..clients)
        .map(|_| {
            rt.spawn(publish_many(
                node.client.clone(),
                format!("{base}/topics/t/messages"),
                per_client,
                vec![b'x'; 1024],
            ))
        })
        .collect();
    for task in tasks {
        task.await.unwrap();
    }
    let total = (per_client * clients) as f64;
    eprintln!(
        "THROUGHPUT publish: {:.0} msg/s",
        total / started.elapsed().as_secs_f64()
    );
    let started = Instant::now();
    let tasks: Vec<_> = (0..clients)
        .map(|_| {
            rt.spawn(consume_all(
                node.client.clone(),
                base.clone(),
                "t".to_owned(),
                per_client,
            ))
        })
        .collect();
    for task in tasks {
        task.await.unwrap();
    }
    eprintln!(
        "THROUGHPUT take(100)+ack each: {:.0} msg/s",
        total / started.elapsed().as_secs_f64()
    );
    rt.shutdown_background();
}

/// Takes everything (100 at a time) and returns `(id, receiptToken)` of each message.
async fn take_all(
    client: reqwest::Client,
    base: String,
    topic: String,
    count: usize,
) -> Vec<(String, String)> {
    let mut taken = Vec::with_capacity(count);
    while taken.len() < count {
        let response = client
            .get(format!(
                "{base}/topics/{topic}/messages?maxMessages=100&visibilityTimeout=120"
            ))
            .send()
            .await
            .unwrap();
        let page: Value = response.json().await.unwrap();
        let messages = page["messages"].as_array().unwrap();
        assert!(!messages.is_empty(), "the queue emptied at {}", taken.len());
        for message in messages {
            taken.push((
                message["id"].as_str().unwrap().to_owned(),
                message["receiptToken"].as_str().unwrap().to_owned(),
            ));
        }
    }
    taken
}

async fn ack_all(
    client: reqwest::Client,
    base: String,
    topic: String,
    receipts: Vec<(String, String)>,
) {
    for (id, token) in receipts {
        let response = client
            .delete(format!(
                "{base}/topics/{topic}/messages/{id}?receiptToken={token}"
            ))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 204);
    }
}

/// The rate of publishing and of acking with 1, 4, 16 and 64 requests at once: if it grows with the
/// concurrency, the ceiling of a client that sends one request per message is the client, not the node.
/// UC: UC-BNC-006
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn throughput_by_concurrency() {
    use std::time::Instant;
    let node = TestNode::single().await;
    let rt = client_runtime();
    let base = node.client_url("").trim_end_matches('/').to_owned();
    let total = 16_000;
    for clients in [1usize, 4, 16, 64] {
        let topic = format!("c{clients}");
        let response = node
            .client
            .post(node.client_url("/topics"))
            .json(&json!({ "name": topic, "type": "queue" }))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 201);
        let per_client = total / clients;
        let started = Instant::now();
        let tasks: Vec<_> = (0..clients)
            .map(|_| {
                rt.spawn(publish_many(
                    node.client.clone(),
                    format!("{base}/topics/{topic}/messages"),
                    per_client,
                    vec![b'x'; 100],
                ))
            })
            .collect();
        for task in tasks {
            task.await.unwrap();
        }
        let publish = total as f64 / started.elapsed().as_secs_f64();
        // One client takes everything, so no client empties the queue of another, and the acks are split.
        let taken = rt
            .spawn(take_all(
                node.client.clone(),
                base.clone(),
                topic.clone(),
                total,
            ))
            .await
            .unwrap();
        let started = Instant::now();
        let tasks: Vec<_> = taken
            .chunks(per_client)
            .map(|r| {
                rt.spawn(ack_all(
                    node.client.clone(),
                    base.clone(),
                    topic.clone(),
                    r.to_vec(),
                ))
            })
            .collect();
        for task in tasks {
            task.await.unwrap();
        }
        let ack = total as f64 / started.elapsed().as_secs_f64();
        eprintln!(
            "THROUGHPUT {clients:>2} at once: publish {publish:.0} msg/s, ack {ack:.0} msg/s"
        );
    }
    rt.shutdown_background();
}

/// The same load as [`throughput_by_concurrency`] with the batch routes, 100 messages per request: what a client
/// that publishes and acks in batch gets from the node.
/// UC: UC-BNC-006
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn throughput_in_batches() {
    use std::time::Instant;
    let node = TestNode::single().await;
    let rt = client_runtime();
    let base = node.client_url("").trim_end_matches('/').to_owned();
    let total = 64_000;
    let per_request = 100;
    for clients in [1usize, 4, 16] {
        let topic = format!("b{clients}");
        let response = node
            .client
            .post(node.client_url("/topics"))
            .json(&json!({ "name": topic, "type": "queue" }))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 201);
        let requests = total / per_request / clients;
        let body = json!({ "messages": vec![json!({ "body": "x".repeat(98) }); per_request] });
        let started = Instant::now();
        let tasks: Vec<_> = (0..clients)
            .map(|_| {
                let (client, url, body) = (
                    node.client.clone(),
                    format!("{base}/topics/{topic}/messages/batch"),
                    body.clone(),
                );
                rt.spawn(async move {
                    for _ in 0..requests {
                        let response = client.post(&url).json(&body).send().await.unwrap();
                        assert_eq!(response.status(), 200);
                    }
                })
            })
            .collect();
        for task in tasks {
            task.await.unwrap();
        }
        let publish = total as f64 / started.elapsed().as_secs_f64();
        let taken = rt
            .spawn(take_all(
                node.client.clone(),
                base.clone(),
                topic.clone(),
                total,
            ))
            .await
            .unwrap();
        let started = Instant::now();
        let tasks: Vec<_> = taken
            .chunks(total / clients)
            .map(|receipts| {
                let (client, url) = (
                    node.client.clone(),
                    format!("{base}/topics/{topic}/messages/ack"),
                );
                let receipts = receipts.to_vec();
                rt.spawn(async move {
                    for chunk in receipts.chunks(per_request) {
                        let items: Vec<Value> = chunk
                            .iter()
                            .map(|(id, token)| json!({ "id": id, "receiptToken": token }))
                            .collect();
                        let response = client
                            .post(&url)
                            .json(&json!({ "messages": items }))
                            .send()
                            .await
                            .unwrap();
                        assert_eq!(response.status(), 200);
                    }
                })
            })
            .collect();
        for task in tasks {
            task.await.unwrap();
        }
        let ack = total as f64 / started.elapsed().as_secs_f64();
        eprintln!(
            "THROUGHPUT batches of {per_request}, {clients:>2} at once: publish {publish:.0} msg/s, ack {ack:.0} msg/s"
        );
    }
    rt.shutdown_background();
}

/// The time per message of the cluster path without HTTP (`ClusterMessages`: the plan, the register and the commit on
/// the owner) and of the engine alone, one at a time and 128 at once: if the cluster path costs much more than the
/// engine, the cost of a batch is there and not in the requests.
/// UC: UC-BNC-006
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cost_of_the_cluster_path() {
    use std::time::Instant;
    use taba_node::engine::messages::PublishRequest;
    let node = TestNode::single().await;
    let cluster = common::cluster::replication_of(&node).messages.clone();
    let rt = client_runtime();
    let base = node.client_url("").trim_end_matches('/').to_owned();
    let total = 20_000;
    let request = || PublishRequest {
        body: bytes::Bytes::from(vec![b'x'; 100]),
        content_type: Some("application/octet-stream".to_owned()),
        group_id: None,
        deduplication_id: None,
    };
    for (round, at_once) in [(0, 1usize), (1, 128)] {
        let topic = format!("p{round}");
        let response = node
            .client
            .post(node.client_url("/topics"))
            .json(&json!({ "name": topic, "type": "queue" }))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 201);
        let started = Instant::now();
        stream::iter(0..total)
            .map(|_| {
                let (cluster, topic) = (cluster.clone(), topic.clone());
                async move { cluster.publish(&topic, request()).await.unwrap() }
            })
            .buffer_unordered(at_once)
            .collect::<Vec<_>>()
            .await;
        let publish = started.elapsed().as_secs_f64() * 1e6 / total as f64;
        let taken = rt
            .spawn(take_all(
                node.client.clone(),
                base.clone(),
                topic.clone(),
                total,
            ))
            .await
            .unwrap();
        let started = Instant::now();
        stream::iter(taken)
            .map(|(id, token)| {
                let (cluster, topic) = (cluster.clone(), topic.clone());
                async move { cluster.ack(&topic, &id, Some(&token)).await.unwrap() }
            })
            .buffer_unordered(at_once)
            .collect::<Vec<_>>()
            .await;
        let ack = started.elapsed().as_secs_f64() * 1e6 / total as f64;
        eprintln!(
            "COST cluster path, {at_once:>3} at once: publish {publish:.1} µs/msg, ack {ack:.1} µs/msg"
        );
    }
    // The engine alone, for comparison, in the same node.
    let messages = cluster.messages().clone();
    let topic = "engine";
    let response = node
        .client
        .post(node.client_url("/topics"))
        .json(&json!({ "name": topic, "type": "queue" }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 201);
    let started = Instant::now();
    for _ in 0..total {
        messages.publish(topic, request()).unwrap();
    }
    let publish = started.elapsed().as_secs_f64() * 1e6 / total as f64;
    let taken = rt
        .spawn(take_all(
            node.client.clone(),
            base.clone(),
            topic.to_owned(),
            total,
        ))
        .await
        .unwrap();
    let started = Instant::now();
    for (id, token) in &taken {
        messages.ack(topic, id, Some(token)).unwrap();
    }
    let ack = started.elapsed().as_secs_f64() * 1e6 / total as f64;
    eprintln!("COST engine alone, one at a time: publish {publish:.1} µs/msg, ack {ack:.1} µs/msg");

    // Four tasks on four workers: on one topic they share its lock; on four topics only what is of the whole node.
    for topics in [1usize, 4] {
        let names: Vec<String> = (0..topics).map(|n| format!("par{topics}-{n}")).collect();
        for name in &names {
            let response = node
                .client
                .post(node.client_url("/topics"))
                .json(&json!({ "name": name, "type": "queue" }))
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), 201);
        }
        let started = Instant::now();
        let tasks: Vec<_> = (0..4)
            .map(|n| {
                let (cluster, topic) = (cluster.clone(), names[n % topics].clone());
                tokio::spawn(async move {
                    for _ in 0..total / 4 {
                        cluster.publish(&topic, request()).await.unwrap();
                    }
                })
            })
            .collect();
        for task in tasks {
            task.await.unwrap();
        }
        let rate = total as f64 / started.elapsed().as_secs_f64();
        eprintln!("COST cluster path, 4 tasks on {topics} topic(s): publish {rate:.0} msg/s");
    }
    rt.shutdown_background();
}

/// Batches of 100 to a distributor with one queue behind it, the shape of a topic of the OMB driver.
/// UC: UC-BNC-006
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn throughput_in_batches_to_a_distributor() {
    use std::time::Instant;
    let node = TestNode::single().await;
    for body in [
        json!({ "name": "omb-sub", "type": "queue", "volatile": true, "idleTimeout": 86400 }),
        json!({ "name": "omb", "type": "distributor", "prefixes": ["omb-"] }),
    ] {
        let response = node
            .client
            .post(node.client_url("/topics"))
            .json(&body)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 201, "{body}");
    }
    let rt = client_runtime();
    let url = node.client_url("/topics/omb/messages/batch");
    let body = json!({ "messages": vec![json!({ "body": "eHh4".repeat(333), "bodyEncoding": "base64", "contentType": "application/octet-stream" }); 100] });
    let clients: usize = std::env::var("BATCH_CLIENTS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(16);
    let requests = 320 / clients.min(320);
    // A backlog already waiting (BATCH_PREFILL batches of 100): the publications after it are timed.
    let prefill: usize = std::env::var("BATCH_PREFILL")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let filled = rt.spawn({
        let (client, url, body) = (node.client.clone(), url.clone(), body.clone());
        async move {
            let chunks: Vec<_> = (0..16)
                .map(|_| {
                    let (client, url, body) = (client.clone(), url.clone(), body.clone());
                    tokio::spawn(async move {
                        for _ in 0..prefill / 16 {
                            assert_eq!(
                                client.post(&url).json(&body).send().await.unwrap().status(),
                                200
                            );
                        }
                    })
                })
                .collect();
            for chunk in chunks {
                chunk.await.unwrap();
            }
        }
    });
    filled.await.unwrap();
    let started = Instant::now();
    let tasks: Vec<_> = (0..clients)
        .map(|_| {
            let (client, url, body) = (node.client.clone(), url.clone(), body.clone());
            rt.spawn(async move {
                for _ in 0..requests {
                    let response = client.post(&url).json(&body).send().await.unwrap();
                    assert_eq!(response.status(), 200);
                    let page: Value = response.json().await.unwrap();
                    assert!(
                        page["results"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .all(|r| r["status"] == 200 && r["matched"] == 1),
                        "{page}"
                    );
                }
            })
        })
        .collect();
    for task in tasks {
        task.await.unwrap();
    }
    let rate = (clients * requests * 100) as f64 / started.elapsed().as_secs_f64();
    eprintln!("THROUGHPUT batches of 100 to a distributor, {clients} at once: {rate:.0} msg/s");
    rt.shutdown_background();
}

/// The same load in two steps, to see which one costs more: only taking (no ack), then only acking.
/// UC: UC-BNC-006
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn throughput_of_take_and_of_ack_apart() {
    use std::time::Instant;
    let node = TestNode::single().await;
    let response = node
        .client
        .post(node.client_url("/topics"))
        .json(&json!({ "name": "t", "type": "queue" }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 201);
    let rt = client_runtime();
    let base = node.client_url("").trim_end_matches('/').to_owned();
    let per_client = 2_000;
    let clients = 16;
    let total = (per_client * clients) as f64;
    let tasks: Vec<_> = (0..clients)
        .map(|_| {
            rt.spawn(publish_many(
                node.client.clone(),
                format!("{base}/topics/t/messages"),
                per_client,
                vec![b'x'; 1024],
            ))
        })
        .collect();
    for task in tasks {
        task.await.unwrap();
    }
    let started = Instant::now();
    let tasks: Vec<_> = (0..clients)
        .map(|_| {
            rt.spawn(take_all(
                node.client.clone(),
                base.clone(),
                "t".to_owned(),
                per_client,
            ))
        })
        .collect();
    let mut receipts = Vec::new();
    for task in tasks {
        receipts.push(task.await.unwrap());
    }
    eprintln!(
        "THROUGHPUT take(100) alone: {:.0} msg/s",
        total / started.elapsed().as_secs_f64()
    );
    let started = Instant::now();
    let tasks: Vec<_> = receipts
        .into_iter()
        .map(|r| {
            rt.spawn(ack_all(
                node.client.clone(),
                base.clone(),
                "t".to_owned(),
                r,
            ))
        })
        .collect();
    for task in tasks {
        task.await.unwrap();
    }
    eprintln!(
        "THROUGHPUT ack alone (one DELETE per message): {:.0} msg/s",
        total / started.elapsed().as_secs_f64()
    );
    rt.shutdown_background();
}
