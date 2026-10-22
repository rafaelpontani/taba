//! Statistics and events (doc 11): the counters follow each transition of the message, and the events
//! come out as *log records* with `event.name`.

use std::collections::BTreeMap;
use std::sync::Mutex;

use super::*;

/// A captured log event: the level and the fields as text.
#[derive(Debug, Clone)]
struct Captured {
    level: tracing::Level,
    fields: BTreeMap<String, String>,
}

impl Captured {
    fn name(&self) -> &str {
        self.fields.get("event.name").map_or("", String::as_str)
    }

    fn field(&self, name: &str) -> &str {
        self.fields.get(name).map_or("", String::as_str)
    }
}

#[derive(Default)]
struct FieldVisitor(BTreeMap<String, String>);

impl tracing::field::Visit for FieldVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.0.insert(field.name().to_owned(), format!("{value:?}"));
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        self.0.insert(field.name().to_owned(), value.to_owned());
    }
}

type Sink = Arc<Mutex<Vec<Captured>>>;

thread_local! {
    /// Where this thread's events go while a `capture` runs on it.
    static SINK: std::cell::RefCell<Option<Sink>> = const { std::cell::RefCell::new(None) };
}

/// The global collector: hands each event to the `capture` of the thread that emitted it and ignores the rest.
/// A single collector, installed once, avoids tracing's race between tests on different
/// threads that swap collectors (the log point's interest may end up turned off).
struct Router;

impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for Router {
    fn on_event(&self, event: &tracing::Event<'_>, _: tracing_subscriber::layer::Context<'_, S>) {
        SINK.with(|sink| {
            let Some(sink) = sink.borrow().clone() else {
                return;
            };
            let mut visitor = FieldVisitor::default();
            event.record(&mut visitor);
            sink.lock().unwrap().push(Captured {
                level: *event.metadata().level(),
                fields: visitor.0,
            });
        });
    }
}

fn install_router() {
    use tracing_subscriber::layer::SubscriberExt;
    static INSTALLED: std::sync::Once = std::sync::Once::new();
    INSTALLED.call_once(|| {
        let subscriber = tracing_subscriber::registry().with(Router);
        let _ = tracing::subscriber::set_global_default(subscriber);
    });
}

/// Runs `run` on this thread with a log event collector, of any level.
fn capture<T>(run: impl FnOnce() -> T) -> (T, Vec<Captured>) {
    install_router();
    let events: Sink = Arc::new(Mutex::new(Vec::new()));
    SINK.with(|sink| *sink.borrow_mut() = Some(events.clone()));
    let out = run();
    SINK.with(|sink| *sink.borrow_mut() = None);
    let captured = events.lock().unwrap().clone();
    (out, captured)
}

fn events_named<'a>(events: &'a [Captured], name: &str) -> Vec<&'a Captured> {
    events.iter().filter(|e| e.name() == name).collect()
}

fn stats_rules() -> StatsRules {
    let config = Config::parse(None, &MapEnv::new([("TABA_REPLICATION_FACTOR", "1")])).unwrap();
    StatsRules::from_config(&config)
}

/// UC: UC-OPS-033, UC-RET-018
#[tokio::test]
async fn a_dead_lettered_message_raises_the_event_and_the_counters() {
    let h = harness();
    h.topics
        .create(CreateTopic {
            max_deliveries: Some(1),
            ..queue("orders")
        })
        .await
        .unwrap();
    publish_ok(&h, "orders", "1");
    assert_eq!(
        consume(&h, "orders", q(None, Some("1"), None)).await.len(),
        1
    );
    h.clock.advance(Duration::from_secs(2));

    let ((), events) = capture(|| h.messages.sweep());
    let found = events_named(&events, "taba.message.dead_lettered");
    assert_eq!(found.len(), 1, "{events:?}");
    assert_eq!(found[0].field("messaging.destination.name"), "orders");
    assert_eq!(found[0].field("delivery_count"), "1");
    assert!(!found[0].field("message.id").is_empty());
    assert_eq!(found[0].level, tracing::Level::WARN);

    assert_eq!(h.messages.observe_node().dead_lettered, 1);
    let snapshot = h.messages.topic_snapshot("orders", &stats_rules()).unwrap();
    assert_eq!(snapshot.counters.dead_lettered, 1);
    assert_eq!(snapshot.depth(), 0);
    let dead = h
        .messages
        .topic_snapshot("orders-dead-letter", &stats_rules())
        .unwrap();
    assert_eq!(dead.visible, 1);
}

/// UC: UC-OPS-033, UC-RET-007
#[tokio::test]
async fn an_expired_message_raises_the_event_and_the_counters() {
    let h = harness();
    h.topics
        .create(CreateTopic {
            ttl: Some(60),
            ..queue("orders")
        })
        .await
        .unwrap();
    publish_ok(&h, "orders", "1");
    h.clock.advance(Duration::from_secs(61));

    let ((), events) = capture(|| h.messages.sweep());
    let found = events_named(&events, "taba.message.expired");
    assert_eq!(found.len(), 1, "{events:?}");
    assert_eq!(found[0].field("messaging.destination.name"), "orders");
    assert_eq!(h.messages.observe_node().expired, 1);
    let snapshot = h.messages.topic_snapshot("orders", &stats_rules()).unwrap();
    assert_eq!(snapshot.counters.expired, 1);
}

/// UC: UC-OPS-033, UC-FIF-044
#[tokio::test]
async fn a_deduplicated_publication_raises_the_event_and_the_counters() {
    let h = harness();
    h.topics
        .create(CreateTopic {
            fifo: Some(true),
            ..queue("orders")
        })
        .await
        .unwrap();
    let request = PublishRequest {
        body: b"1".to_vec(),
        group_id: Some("g".into()),
        deduplication_id: Some("k".into()),
        ..Default::default()
    };
    let (original, events) = capture(|| {
        let first = published_id(h.messages.publish("orders", request.clone()).unwrap());
        h.messages.publish("orders", request.clone()).unwrap();
        first
    });
    let found = events_named(&events, "taba.message.deduplicated");
    assert_eq!(found.len(), 1, "{events:?}");
    assert_eq!(found[0].field("messaging.destination.name"), "orders");
    assert_eq!(found[0].field("group_id"), "g");
    assert_eq!(found[0].field("deduplication_id"), "k");
    assert_eq!(found[0].field("message.id"), original);
    assert_eq!(h.messages.observe_node().deduplicated, 1);
    let snapshot = h.messages.topic_snapshot("orders", &stats_rules()).unwrap();
    assert_eq!(snapshot.counters.deduplicated, 1);
    assert_eq!(
        snapshot.counters.published, 1,
        "the duplicate is not a publication"
    );
}

/// UC: UC-OPS-033, UC-DST-018
#[tokio::test]
async fn a_distributor_raises_no_target_and_partial_and_counts_its_copies() {
    let h = harness();
    h.topics
        .create(distributor("none", &["zzz"]))
        .await
        .unwrap();
    h.topics
        .create(distributor("eventos", &["ped"]))
        .await
        .unwrap();
    h.topics.create(queue("ped-a")).await.unwrap();
    h.topics
        .create(CreateTopic {
            fifo: Some(true),
            ..queue("ped-b")
        })
        .await
        .unwrap();

    let (_, events) = capture(|| h.messages.publish("none", json_body("1")).unwrap());
    let found = events_named(&events, "taba.distributor.no_target");
    assert_eq!(found.len(), 1, "{events:?}");
    assert_eq!(found[0].field("messaging.destination.name"), "none");
    assert_eq!(found[0].level, tracing::Level::INFO);

    // The FIFO queue refuses the copy without a `groupId`: the publication is partial.
    let (_, events) = capture(|| h.messages.publish("eventos", json_body("2")).unwrap());
    let found = events_named(&events, "taba.distributor.partial");
    assert_eq!(found.len(), 1, "{events:?}");
    assert_eq!(found[0].field("failed"), "1");
    assert_eq!(found[0].level, tracing::Level::WARN);

    assert_eq!(
        h.messages.distributor_counters("none"),
        DistributorCounters {
            received: 1,
            copies: 0,
            copy_errors: 0
        }
    );
    assert_eq!(
        h.messages.distributor_counters("eventos"),
        DistributorCounters {
            received: 1,
            copies: 1,
            copy_errors: 1
        }
    );
    assert_eq!(
        h.messages.distributor_counters("desconhecido"),
        DistributorCounters::default()
    );
}

/// UC: UC-OPS-026, UC-OPS-024
#[tokio::test]
async fn the_counters_follow_each_transition_of_a_message() {
    let h = harness();
    h.topics.create(queue("orders")).await.unwrap();
    let rules = stats_rules();
    let snapshot = |h: &Harness| h.messages.topic_snapshot("orders", &rules).unwrap();

    let empty = snapshot(&h);
    assert_eq!(
        (empty.visible, empty.invisible, empty.oldest_ms),
        (0, 0, None)
    );

    publish_ok(&h, "orders", "1");
    h.clock.advance(Duration::from_secs(5));
    publish_ok(&h, "orders", "2");
    let published = snapshot(&h);
    assert_eq!((published.visible, published.invisible), (2, 0));
    let first_at = published.oldest_ms.unwrap();

    let delivered = consume(&h, "orders", q(Some("1"), Some("30"), None)).await;
    let delivering = snapshot(&h);
    assert_eq!(
        (delivering.visible, delivering.invisible),
        (1, 1),
        "delivery: -1 visible, +1 invisible"
    );
    assert_eq!(delivering.counters.consumed, 1);

    h.clock.advance(Duration::from_secs(31));
    let expired = snapshot(&h);
    assert_eq!(
        (expired.visible, expired.invisible),
        (2, 0),
        "expiry: -1 invisible, +1 visible"
    );
    assert_eq!(expired.counters.redelivered, 0, "not yet delivered again");

    let again = consume(&h, "orders", q(Some("2"), Some("30"), None)).await;
    assert_eq!(again.len(), 2);
    assert_eq!(snapshot(&h).counters.redelivered, 1);
    let first = again.iter().find(|m| m.id == delivered[0].id).unwrap();
    h.messages
        .ack("orders", &first.id, Some(&first.receipt_token))
        .unwrap();
    let acked = snapshot(&h);
    assert_eq!(acked.depth(), 1, "ack: -1 in the state it was in");
    assert_eq!(acked.counters.acked, 1);
    assert!(acked.oldest_ms.unwrap() > first_at);
    assert!(acked.consume_latency.p50.is_some());
}

/// UC: UC-GRP-015
#[tokio::test]
async fn group_snapshots_show_counts_bands_and_the_oldest_of_each_group() {
    let h = harness();
    h.topics.create(queue("orders")).await.unwrap();
    let group = |g: &str| PublishRequest {
        body: b"1".to_vec(),
        group_id: Some(g.into()),
        ..Default::default()
    };
    h.messages.publish("orders", group("a")).unwrap();
    h.clock.advance(Duration::from_secs(1));
    for _ in 0..3 {
        h.messages.publish("orders", group("b")).unwrap();
    }
    consume(&h, "orders", q(Some("1"), Some("600"), None)).await;

    let mut groups = h.messages.group_snapshots("orders").unwrap();
    groups.sort_by(|x, y| x.group_id.cmp(&y.group_id));
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0].group_id.as_deref(), Some("a"));
    assert_eq!(
        (groups[0].visible, groups[0].invisible),
        (0, 1),
        "the oldest came out first"
    );
    assert_eq!((groups[1].visible, groups[1].invisible), (3, 0));
    assert!(groups[1].oldest_ms.unwrap() > groups[0].oldest_ms.unwrap());
    assert!(!groups[0].overflow);
    assert_eq!(groups[1].band, 0);

    assert_eq!(
        h.messages.group_snapshots("nothing").unwrap_err().code,
        ErrorCode::TopicNotFound
    );
    h.topics.create(distributor("dist", &["x"])).await.unwrap();
    assert_eq!(
        h.messages.group_snapshots("dist").unwrap_err().code,
        ErrorCode::TopicWrongType
    );
}

/// UC: UC-OPS-016
#[tokio::test]
async fn publish_errors_are_counted_per_topic_and_not_for_missing_topics() {
    let h = harness();
    h.topics.create(queue("orders")).await.unwrap();
    publish_err(
        &h,
        "orders",
        PublishRequest {
            body: b"{".to_vec(),
            content_type: Some("application/json".into()),
            ..Default::default()
        },
    );
    publish_err(&h, "nothing", json_body("1"));
    publish_ok(&h, "orders", "1");
    let snapshot = h.messages.topic_snapshot("orders", &stats_rules()).unwrap();
    assert_eq!(snapshot.counters.publish_errors, 1);
    assert_eq!(snapshot.counters.published, 1);
    assert_eq!(snapshot.counters.published_bytes, 1);
    assert!(snapshot.publish_latency.p50.is_some());
}

/// UC: UC-OPS-034
#[tokio::test]
async fn the_overflow_goes_to_the_log_as_one_periodic_summary_and_not_per_message() {
    let h = tiny_memory();
    h.topics.create(queue("orders")).await.unwrap();
    // One fits in memory; the other four go to disk.
    let ((), during) = capture(|| {
        for body in ["\"a\"", "\"b\"", "\"c\"", "\"d\"", "\"e\""] {
            publish_ok(&h, "orders", body);
        }
    });
    assert!(
        events_named(&during, "taba.storage.overflow").is_empty(),
        "no log per message: {during:?}"
    );

    let (moved, events) = capture(|| h.messages.log_overflow_summary(Duration::from_secs(60)));
    assert_eq!(moved, 4);
    let found = events_named(&events, "taba.storage.overflow");
    assert_eq!(found.len(), 1, "a single summary: {events:?}");
    assert_eq!(found[0].level, tracing::Level::INFO);
    assert_eq!(found[0].field("messages"), "4");
    assert_eq!(found[0].field("window_secs"), "60");

    // Without a new overflow, the next window generates no log.
    let (moved, events) = capture(|| h.messages.log_overflow_summary(Duration::from_secs(60)));
    assert_eq!(moved, 0);
    assert!(events_named(&events, "taba.storage.overflow").is_empty());
}
