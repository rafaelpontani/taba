//! Chaos with a verified history (doc 37; level **C**): a three-node cluster is hit by a seeded sequence of faults
//! (isolation, symmetric and one-way partitions, lost replies, lossy links, delay, a mute node, the clock
//! jumping) while clients publish, consume and ack. Every operation goes to a history, and at the end the checker
//! compares it with the guarantees I1 to I6 of doc 37.
//!
//! Each fault is applied alone (the previous one is healed first), so at any instant at most one node or one
//! link is impaired: inside what the default cluster promises to survive (`write.quorum − 1` failures).
//!
//! The faults come from the seed, so the same seed applies the same faults in the same order; the interleaving
//! with the clients runs in real time and is not bit-for-bit repeatable. A seed that fails becomes a test of its
//! own (`regression_seed_*`). `TABA_CHAOS_SEEDS=from..to` (for example `1..200`) sweeps a range of seeds, as the
//! nightly run does; without it that test does nothing.

mod common;

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde_json::{Value, json};
use taba_node::core::clock::Clock;

use common::cluster::{NO_VOTE, TestCluster, settled};
use common::history::{Ack, Delivery, History, Outcome, Publish, Rng};

const STD: &str = "chaos";
const FIFO: &str = "chaos-fifo";
/// The queue with a dead-letter: its consumers never ack the poison messages (`seq % 3 == 0`), which must end in
/// the dead-letter queue after `MAX_DELIVERIES` deliveries.
const DLQ: &str = "chaos-dlq";
const DEAD: &str = "chaos-dlq-dead-letter";
const MAX_DELIVERIES: u64 = 2;
const GROUPS: [&str; 3] = ["g0", "g1", "g2"];
/// How long the faults run (real time) in a run of the pull request.
const CHAOS: Duration = Duration::from_millis(2500);
const CALL_TIMEOUT: Duration = Duration::from_secs(3);

fn host(number: usize) -> String {
    common::cluster::host_of(number)
}

async fn create_dead_letter_queue(node: &common::TestNode, name: &str) {
    let response = node
        .client
        .post(node.client_url("/topics"))
        .json(&json!({"name": name, "type": "queue", "maxDeliveries": MAX_DELIVERIES}))
        .send()
        .await
        .expect("request");
    assert_eq!(response.status(), 201);
}

fn is_poison(queue: &str, seq: u64) -> bool {
    queue == DLQ && seq.is_multiple_of(3)
}

async fn create_fifo(node: &common::TestNode, name: &str) {
    let response = node
        .client
        .post(node.client_url("/topics"))
        .json(&json!({"name": name, "type": "queue", "fifo": true}))
        .send()
        .await
        .expect("request");
    assert_eq!(response.status(), 201);
}

fn outcome_of(status: u16) -> Outcome {
    match status {
        200..=299 => Outcome::Ok,
        400..=499 if status != 408 && status != 429 => Outcome::Fail,
        _ => Outcome::Indeterminate,
    }
}

/// Publishes until `stop`: each client sends one message at a time, through the nodes in turn.
async fn publisher(
    cluster: Arc<TestCluster>,
    history: History,
    stop: Arc<AtomicBool>,
    queue: &'static str,
    group: Option<&'static str>,
    first_node: usize,
) {
    let mut turn = first_node;
    while !stop.load(Ordering::SeqCst) {
        turn += 1;
        let node = cluster.node(turn % 3 + 1);
        let seq = history.next_seq();
        let query = group.map(|g| format!("?groupId={g}")).unwrap_or_default();
        let start = history.tick();
        let response = node
            .client
            .post(node.client_url(&format!("/topics/{queue}/messages{query}")))
            .json(&json!({"seq": seq}))
            .timeout(CALL_TIMEOUT)
            .send()
            .await;
        let (outcome, id) = match response {
            Ok(response) => {
                let outcome = outcome_of(response.status().as_u16());
                let id = if outcome == Outcome::Ok {
                    response
                        .json::<Value>()
                        .await
                        .ok()
                        .and_then(|v| v["id"].as_str().map(str::to_owned))
                } else {
                    None
                };
                (outcome, id)
            }
            Err(_) => (Outcome::Indeterminate, None),
        };
        history.record_publish(Publish {
            seq,
            group: group.map(str::to_owned),
            start,
            end: history.tick(),
            outcome,
            id,
            poison: is_poison(queue, seq),
        });
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

/// Consumes and acks until `stop`, recording every delivery and every ack attempt.
async fn consumer(
    cluster: Arc<TestCluster>,
    history: History,
    stop: Arc<AtomicBool>,
    queue: &'static str,
    first_node: usize,
) {
    let mut turn = first_node;
    while !stop.load(Ordering::SeqCst) {
        turn += 1;
        let node = cluster.node(turn % 3 + 1);
        let start = history.tick();
        let response = node
            .client
            .get(node.client_url(&format!(
                "/topics/{queue}/messages?maxMessages=5&visibilityTimeout=5"
            )))
            .timeout(CALL_TIMEOUT)
            .send()
            .await;
        let messages = match response {
            Ok(response) if response.status() == 200 => response
                .json::<Value>()
                .await
                .ok()
                .and_then(|v| v["messages"].as_array().cloned())
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        let end = history.tick();
        for message in &messages {
            let (Some(seq), Some(id), Some(token)) = (
                message["body"]["seq"].as_u64(),
                message["id"].as_str(),
                message["receiptToken"].as_str(),
            ) else {
                continue;
            };
            history.record_delivery(Delivery {
                seq,
                id: id.to_owned(),
                group: message["groupId"].as_str().map(str::to_owned),
                dead: queue == DEAD,
                limited: queue == DLQ,
                count: message["deliveryCount"].as_u64().unwrap_or(1),
                start,
                end,
            });
            if is_poison(queue, seq) {
                continue;
            }
            let ack_start = history.tick();
            let outcome = match node
                .client
                .delete(node.client_url(&format!(
                    "/topics/{queue}/messages/{id}?receiptToken={token}"
                )))
                .timeout(CALL_TIMEOUT)
                .send()
                .await
            {
                Ok(response) => outcome_of(response.status().as_u16()),
                Err(_) => Outcome::Indeterminate,
            };
            history.record_ack(Ack {
                seq,
                dead: queue == DEAD,
                id: id.to_owned(),
                start: ack_start,
                end: history.tick(),
                outcome,
            });
        }
        let pause = if messages.is_empty() { 20 } else { 2 };
        tokio::time::sleep(Duration::from_millis(pause)).await;
    }
}

/// One fault drawn from the seed. Returns what it did, for the log of a failure.
fn fault(rng: &mut Rng, cluster: &TestCluster) -> String {
    let network = &cluster.network;
    let a = rng.below(3) as usize + 1;
    let b = (a + rng.below(2) as usize) % 3 + 1;
    let (ha, hb) = (host(a), host(b));
    let action = rng.below(9);
    if action != 7 {
        network.heal_all();
    }
    match action {
        0 => "heal everything".to_owned(),
        1 => {
            network.isolate(&ha);
            format!("isolate node {a}")
        }
        2 => {
            network.partition(&ha, &hb);
            format!("partition {a} | {b}")
        }
        3 => {
            network.cut_one_way(&ha, &hb);
            format!("one-way cut {a} -> {b}")
        }
        4 => {
            network.lose_replies(&ha, &hb, true);
            format!("lost replies {a} -> {b}")
        }
        5 => {
            network.set_link_loss(&ha, &hb, 40);
            format!("40% loss {a} -> {b}")
        }
        6 => {
            network.set_delay(Duration::from_millis(15));
            "15 ms delay on every call".to_owned()
        }
        7 => {
            cluster.clock.advance(Duration::from_secs(8));
            "clock +8 s".to_owned()
        }
        _ => {
            network.set_silent(&ha, true);
            format!("mute node {a}")
        }
    }
}

/// The IDs every node declared lost.
fn declared_losses(cluster: &TestCluster) -> HashSet<String> {
    cluster
        .nodes
        .iter()
        .filter_map(|node| {
            node.node
                .cluster()?
                .replication()
                .map(|r| r.recovery.clone())
        })
        .flat_map(|recovery| recovery.losses().list(None, 100_000).unwrap_or_default())
        .map(|loss| loss.id)
        .collect()
}

/// Where the messages nobody received are, and what may be holding their group: printed with a failure.
fn diagnosis(cluster: &TestCluster, history: &History, violated: &[u64]) -> String {
    let mut out = String::new();
    let state = |queue: &str, id: &str, out: &mut String| {
        for number in 1..=3 {
            let messages = cluster
                .node(number)
                .node
                .cluster()
                .and_then(|c| c.replication())
                .map(|r| r.messages.messages().clone())
                .expect("replication");
            if let Ok(Some(info)) = messages.message_info(queue, id) {
                out.push_str(&format!(
                    "\n    {id} in {queue} on node {number}: owned {} count {} invisible_until {} (clock {})",
                    info.owned,
                    info.meta.delivery_count,
                    info.meta.invisible_until_ms,
                    cluster.clock.now_millis()
                ));
            }
        }
    };
    for id in history.undelivered_ids().iter().take(3) {
        for queue in [STD, FIFO, DLQ, DEAD] {
            state(queue, id, &mut out);
        }
    }
    // The dump of a message that broke I3 or I5: where it is now on every node, in every queue (PRB-077).
    for seq in violated {
        let Some(id) = history.id_of(*seq) else {
            continue;
        };
        out.push_str(&format!("\n  violated seq {seq} (id {id}):"));
        for queue in [STD, FIFO, DLQ, DEAD] {
            state(queue, &id, &mut out);
        }
    }
    for (seq, id) in history.blockers() {
        out.push_str(&format!(
            "\n  blocker seq {seq}: {}",
            history.trail(seq).replace('\n', " |")
        ));
        state(FIFO, &id, &mut out);
    }
    out
}

/// One run: the cluster, the clients, the faults of the seed, the heal and the check.
async fn run(seed: u64) {
    let cluster = Arc::new(TestCluster::start_with(3, NO_VOTE).await);
    settled(&cluster).await;
    common::cluster::create_queue(cluster.node(1), STD).await;
    create_fifo(cluster.node(1), FIFO).await;
    create_dead_letter_queue(cluster.node(1), DLQ).await;
    cluster.network.seed_links(seed);

    let history = History::new().with_max_deliveries(MAX_DELIVERIES);
    let stop_publishing = Arc::new(AtomicBool::new(false));
    let stop_consuming = Arc::new(AtomicBool::new(false));
    let mut publishers = Vec::new();
    for client in 0..2 {
        publishers.push(tokio::spawn(publisher(
            cluster.clone(),
            history.clone(),
            stop_publishing.clone(),
            STD,
            None,
            client,
        )));
    }
    publishers.push(tokio::spawn(publisher(
        cluster.clone(),
        history.clone(),
        stop_publishing.clone(),
        DLQ,
        None,
        2,
    )));
    for (client, group) in GROUPS.iter().enumerate() {
        publishers.push(tokio::spawn(publisher(
            cluster.clone(),
            history.clone(),
            stop_publishing.clone(),
            FIFO,
            Some(group),
            client,
        )));
    }
    let mut consumers = Vec::new();
    for (client, queue) in [STD, STD, FIFO, FIFO, DLQ, DEAD].into_iter().enumerate() {
        consumers.push(tokio::spawn(consumer(
            cluster.clone(),
            history.clone(),
            stop_consuming.clone(),
            queue,
            client,
        )));
    }

    // The nemesis.
    let mut rng = Rng::new(seed);
    let mut faults = Vec::new();
    let deadline = tokio::time::Instant::now() + CHAOS;
    while tokio::time::Instant::now() < deadline {
        faults.push(fault(&mut rng, &cluster));
        tokio::time::sleep(Duration::from_millis(100 + rng.below(200))).await;
    }

    // The end of the faults: the clients stop publishing, the network heals and the consumers get everything.
    stop_publishing.store(true, Ordering::SeqCst);
    for task in publishers {
        task.await.expect("publisher");
    }
    cluster.network.heal_all();
    let settle = tokio::time::Instant::now() + Duration::from_secs(common::cluster::WAIT);
    while !history.all_confirmed_delivered() && tokio::time::Instant::now() < settle {
        // What was handed out during a fault and never acked comes back when its invisibility ends.
        cluster.clock.advance(Duration::from_secs(8));
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    stop_consuming.store(true, Ordering::SeqCst);
    for task in consumers {
        task.await.expect("consumer");
    }

    let summary = history.summary();
    let violations = history.check(&declared_losses(&cluster));
    let violated: Vec<u64> = violations
        .iter()
        .take(3)
        .filter_map(|v| v.split_whitespace().find_map(|w| w.parse::<u64>().ok()))
        .collect();
    let where_are_they = diagnosis(&cluster, &history, &violated);
    eprintln!("seed {seed}: {summary:?}, {} faults", faults.len());
    let cluster = Arc::into_inner(cluster).expect("the clients ended");
    cluster.shutdown().await;
    assert!(
        summary.published_ok > 0 && summary.delivered > 0,
        "seed {seed}: the run did nothing: {summary:?}"
    );
    assert!(
        violations.is_empty(),
        "seed {seed}: {} violations
{}
trails:
{}
where:{where_are_they}
summary: {summary:?}
faults: {faults:#?}",
        violations.len(),
        violations.join(
            "
"
        ),
        violations
            .iter()
            .take(2)
            .filter_map(|v| v.split_whitespace().find_map(|w| w.parse::<u64>().ok()))
            .map(|seq| format!(
                "seq {seq}
{}",
                history.trail(seq)
            ))
            .collect::<Vec<_>>()
            .join(
                "
"
            )
    );
}

/// UC: UC-CHS-001, UC-CHS-004
#[tokio::test]
async fn seed_1_keeps_the_guarantees() {
    run(1).await;
}

/// UC: UC-CHS-001, UC-CHS-004
#[tokio::test]
async fn seed_2_keeps_the_guarantees() {
    run(2).await;
}

/// UC: UC-CHS-001, UC-CHS-004
#[tokio::test]
async fn seed_3_keeps_the_guarantees() {
    run(3).await;
}

/// UC: UC-CHS-001, UC-CHS-004
#[tokio::test]
async fn seed_4_keeps_the_guarantees() {
    run(4).await;
}

/// The nightly sweep: `TABA_CHAOS_SEEDS=1..200`. Without the variable it does nothing.
///
/// UC: UC-CHS-001, UC-CHS-004
#[tokio::test]
async fn a_range_of_seeds_keeps_the_guarantees() {
    let Ok(range) = std::env::var("TABA_CHAOS_SEEDS") else {
        return;
    };
    let (from, to) = range
        .split_once("..")
        .and_then(|(a, b)| Some((a.parse::<u64>().ok()?, b.parse::<u64>().ok()?)))
        .expect("TABA_CHAOS_SEEDS is `from..to`");
    for seed in from..to {
        run(seed).await;
    }
}

// ---- The checker must be able to fail: a checker that never complains proves nothing ----

fn publish(history: &History, seq: u64, group: Option<&str>, outcome: Outcome, id: &str) {
    let start = history.tick();
    history.record_publish(Publish {
        seq,
        group: group.map(str::to_owned),
        start,
        end: history.tick(),
        outcome,
        id: (outcome == Outcome::Ok).then(|| id.to_owned()),
        poison: false,
    });
}

fn deliver(history: &History, seq: u64, id: &str, group: Option<&str>) {
    let start = history.tick();
    history.record_delivery(Delivery {
        seq,
        id: id.to_owned(),
        group: group.map(str::to_owned),
        dead: false,
        limited: false,
        count: 1,
        start,
        end: history.tick(),
    });
}

fn ack(history: &History, seq: u64, id: &str) {
    let start = history.tick();
    history.record_ack(Ack {
        seq,
        dead: false,
        id: id.to_owned(),
        start,
        end: history.tick(),
        outcome: Outcome::Ok,
    });
}

fn none() -> HashSet<String> {
    HashSet::new()
}

/// UC: UC-CHS-002
#[test]
fn a_clean_history_has_no_violation() {
    let history = History::new();
    publish(&history, 1, None, Outcome::Ok, "a");
    publish(&history, 2, None, Outcome::Indeterminate, "b");
    deliver(&history, 1, "a", None);
    deliver(&history, 2, "b", None);
    ack(&history, 1, "a");
    assert_eq!(history.check(&none()), Vec::<String>::new());
}

/// UC: UC-CHS-002
#[test]
fn a_confirmed_publication_never_delivered_is_a_loss_unless_declared() {
    let history = History::new();
    publish(&history, 1, None, Outcome::Ok, "a");
    let violations = history.check(&none());
    assert!(
        violations.iter().any(|v| v.starts_with("I1/I6")),
        "{violations:?}"
    );
    let declared: HashSet<String> = ["a".to_owned()].into();
    assert!(
        history.check(&declared).is_empty(),
        "a declared loss is not silent"
    );
}

/// UC: UC-CHS-002
#[test]
fn a_delivery_without_a_publication_is_a_phantom() {
    let history = History::new();
    deliver(&history, 9, "x", None);
    assert!(history.check(&none()).iter().any(|v| v.starts_with("I2")));
}

/// UC: UC-CHS-002
#[test]
fn a_message_delivered_after_its_ack_is_a_violation() {
    let history = History::new();
    publish(&history, 1, None, Outcome::Ok, "a");
    deliver(&history, 1, "a", None);
    ack(&history, 1, "a");
    deliver(&history, 1, "a", None);
    assert!(history.check(&none()).iter().any(|v| v.starts_with("I3")));
}

/// UC: UC-CHS-002
#[test]
fn a_group_delivered_out_of_order_or_stored_twice_is_a_violation() {
    let history = History::new();
    publish(&history, 1, Some("g"), Outcome::Ok, "a");
    publish(&history, 2, Some("g"), Outcome::Ok, "b");
    deliver(&history, 2, "b", Some("g"));
    deliver(&history, 1, "a", Some("g"));
    assert!(history.check(&none()).iter().any(|v| v.starts_with("I4")));

    let twice = History::new();
    publish(&twice, 1, Some("g"), Outcome::Ok, "a");
    deliver(&twice, 1, "a", Some("g"));
    deliver(&twice, 1, "other", Some("g"));
    assert!(twice.check(&none()).iter().any(|v| v.starts_with("I4")));
}

/// UC: UC-CHS-002
#[test]
fn the_same_seed_draws_the_same_faults() {
    let draw = |seed| {
        let mut rng = Rng::new(seed);
        (0..20).map(|_| rng.below(9)).collect::<Vec<_>>()
    };
    assert_eq!(draw(5), draw(5));
    assert_ne!(draw(5), draw(6));
}

fn poison(history: &History, seq: u64, id: &str) {
    let start = history.tick();
    history.record_publish(Publish {
        seq,
        group: None,
        start,
        end: history.tick(),
        outcome: Outcome::Ok,
        id: Some(id.to_owned()),
        poison: true,
    });
}

fn deliver_counted(history: &History, seq: u64, id: &str, dead: bool, count: u64) {
    let start = history.tick();
    history.record_delivery(Delivery {
        seq,
        id: id.to_owned(),
        group: None,
        dead,
        limited: !dead,
        count,
        start,
        end: history.tick(),
    });
}

/// UC: UC-CHS-004
#[test]
fn a_poison_message_that_reaches_the_dead_letter_after_its_deliveries_is_clean() {
    let history = History::new().with_max_deliveries(2);
    poison(&history, 1, "a");
    deliver_counted(&history, 1, "a", false, 1);
    deliver_counted(&history, 1, "a", false, 2);
    deliver_counted(&history, 1, "a", true, 1);
    assert_eq!(history.check(&none()), Vec::<String>::new());
}

/// UC: UC-CHS-004
#[test]
fn a_poison_message_that_never_reaches_the_dead_letter_is_a_violation() {
    let history = History::new().with_max_deliveries(2);
    poison(&history, 1, "a");
    deliver_counted(&history, 1, "a", false, 1);
    assert!(history.check(&none()).iter().any(|v| v.starts_with("I5")));
    assert!(!history.all_confirmed_delivered());
}

/// UC: UC-CHS-004
#[test]
fn a_message_delivered_over_its_limit_is_a_violation() {
    let history = History::new().with_max_deliveries(2);
    poison(&history, 1, "a");
    deliver_counted(&history, 1, "a", false, 3);
    deliver_counted(&history, 1, "a", true, 1);
    assert!(
        history
            .check(&none())
            .iter()
            .any(|v| v.contains("over maxDeliveries"))
    );
}

/// UC: UC-CHS-004
#[test]
fn a_message_in_both_places_is_a_violation() {
    let history = History::new().with_max_deliveries(2);
    poison(&history, 1, "a");
    deliver_counted(&history, 1, "a", true, 1);
    deliver_counted(&history, 1, "a", false, 2);
    assert!(history.check(&none()).iter().any(|v| v.starts_with("I5")));

    let acked = History::new().with_max_deliveries(2);
    publish(&acked, 1, None, Outcome::Ok, "a");
    deliver(&acked, 1, "a", None);
    ack(&acked, 1, "a");
    deliver_counted(&acked, 1, "a", true, 1);
    assert!(acked.check(&none()).iter().any(|v| v.starts_with("I5")));
}
