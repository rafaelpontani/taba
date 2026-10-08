//! Estatística e eventos (doc 11): os contadores seguem cada transição da mensagem, e os eventos
//! saem como *log records* com `event.name`.

use std::collections::BTreeMap;
use std::sync::Mutex;

use super::*;

/// Um evento de log capturado: o nível e os campos como texto.
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

struct Collector(Arc<Mutex<Vec<Captured>>>);

impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for Collector {
    fn on_event(&self, event: &tracing::Event<'_>, _: tracing_subscriber::layer::Context<'_, S>) {
        let mut visitor = FieldVisitor::default();
        event.record(&mut visitor);
        self.0.lock().unwrap().push(Captured {
            level: *event.metadata().level(),
            fields: visitor.0,
        });
    }
}

/// Roda `run` nesta thread com um coletor de eventos de log, de qualquer nível.
fn capture<T>(run: impl FnOnce() -> T) -> (T, Vec<Captured>) {
    use tracing_subscriber::layer::SubscriberExt;
    let events = Arc::new(Mutex::new(Vec::new()));
    let subscriber = tracing_subscriber::registry().with(Collector(events.clone()));
    let out = tracing::subscriber::with_default(subscriber, run);
    let captured = events.lock().unwrap().clone();
    (out, captured)
}

fn events_named<'a>(events: &'a [Captured], name: &str) -> Vec<&'a Captured> {
    events.iter().filter(|e| e.name() == name).collect()
}

fn stats_rules() -> StatsRules {
    let config = Config::parse(None, &MapEnv::new([("TABA_MODE", "development")])).unwrap();
    StatsRules::from_config(&config)
}

/// UC: UC-OPS-033, UC-RET-018
#[tokio::test]
async fn a_dead_lettered_message_raises_the_event_and_the_counters() {
    let h = harness();
    h.topics
        .create(CreateTopic {
            max_deliveries: Some(1),
            ..queue("pedidos")
        })
        .await
        .unwrap();
    publish_ok(&h, "pedidos", "1");
    assert_eq!(
        consume(&h, "pedidos", q(None, Some("1"), None)).await.len(),
        1
    );
    h.clock.advance(Duration::from_secs(2));

    let ((), events) = capture(|| h.messages.sweep());
    let found = events_named(&events, "taba.message.dead_lettered");
    assert_eq!(found.len(), 1, "{events:?}");
    assert_eq!(found[0].field("messaging.destination.name"), "pedidos");
    assert_eq!(found[0].field("delivery_count"), "1");
    assert!(!found[0].field("message.id").is_empty());
    assert_eq!(found[0].level, tracing::Level::WARN);

    assert_eq!(h.messages.observe_node().dead_lettered, 1);
    let snapshot = h
        .messages
        .topic_snapshot("pedidos", &stats_rules())
        .unwrap();
    assert_eq!(snapshot.counters.dead_lettered, 1);
    assert_eq!(snapshot.depth(), 0);
    let dead = h
        .messages
        .topic_snapshot("pedidos-dead-letter", &stats_rules())
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
            ..queue("pedidos")
        })
        .await
        .unwrap();
    publish_ok(&h, "pedidos", "1");
    h.clock.advance(Duration::from_secs(61));

    let ((), events) = capture(|| h.messages.sweep());
    let found = events_named(&events, "taba.message.expired");
    assert_eq!(found.len(), 1, "{events:?}");
    assert_eq!(found[0].field("messaging.destination.name"), "pedidos");
    assert_eq!(h.messages.observe_node().expired, 1);
    let snapshot = h
        .messages
        .topic_snapshot("pedidos", &stats_rules())
        .unwrap();
    assert_eq!(snapshot.counters.expired, 1);
}

/// UC: UC-OPS-033, UC-FIF-044
#[tokio::test]
async fn a_deduplicated_publication_raises_the_event_and_the_counters() {
    let h = harness();
    h.topics
        .create(CreateTopic {
            fifo: Some(true),
            ..queue("pedidos")
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
        let first = published_id(h.messages.publish("pedidos", request.clone()).unwrap());
        h.messages.publish("pedidos", request.clone()).unwrap();
        first
    });
    let found = events_named(&events, "taba.message.deduplicated");
    assert_eq!(found.len(), 1, "{events:?}");
    assert_eq!(found[0].field("messaging.destination.name"), "pedidos");
    assert_eq!(found[0].field("group_id"), "g");
    assert_eq!(found[0].field("deduplication_id"), "k");
    assert_eq!(found[0].field("message.id"), original);
    assert_eq!(h.messages.observe_node().deduplicated, 1);
    let snapshot = h
        .messages
        .topic_snapshot("pedidos", &stats_rules())
        .unwrap();
    assert_eq!(snapshot.counters.deduplicated, 1);
    assert_eq!(
        snapshot.counters.published, 1,
        "a duplicata não é publicação"
    );
}

/// UC: UC-OPS-033, UC-DST-018
#[tokio::test]
async fn a_distributor_raises_no_target_and_partial_and_counts_its_copies() {
    let h = harness();
    h.topics
        .create(distributor("nenhum", &["zzz"]))
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

    let (_, events) = capture(|| h.messages.publish("nenhum", json_body("1")).unwrap());
    let found = events_named(&events, "taba.distributor.no_target");
    assert_eq!(found.len(), 1, "{events:?}");
    assert_eq!(found[0].field("messaging.destination.name"), "nenhum");
    assert_eq!(found[0].level, tracing::Level::INFO);

    // A fila FIFO recusa a cópia sem `groupId`: a publicação fica parcial.
    let (_, events) = capture(|| h.messages.publish("eventos", json_body("2")).unwrap());
    let found = events_named(&events, "taba.distributor.partial");
    assert_eq!(found.len(), 1, "{events:?}");
    assert_eq!(found[0].field("failed"), "1");
    assert_eq!(found[0].level, tracing::Level::WARN);

    assert_eq!(
        h.messages.distributor_counters("nenhum"),
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
    h.topics.create(queue("pedidos")).await.unwrap();
    let rules = stats_rules();
    let snapshot = |h: &Harness| h.messages.topic_snapshot("pedidos", &rules).unwrap();

    let empty = snapshot(&h);
    assert_eq!(
        (empty.visible, empty.invisible, empty.oldest_ms),
        (0, 0, None)
    );

    publish_ok(&h, "pedidos", "1");
    h.clock.advance(Duration::from_secs(5));
    publish_ok(&h, "pedidos", "2");
    let published = snapshot(&h);
    assert_eq!((published.visible, published.invisible), (2, 0));
    let first_at = published.oldest_ms.unwrap();

    let delivered = consume(&h, "pedidos", q(Some("1"), Some("30"), None)).await;
    let delivering = snapshot(&h);
    assert_eq!(
        (delivering.visible, delivering.invisible),
        (1, 1),
        "entrega: -1 visível, +1 invisível"
    );
    assert_eq!(delivering.counters.consumed, 1);

    h.clock.advance(Duration::from_secs(31));
    let expired = snapshot(&h);
    assert_eq!(
        (expired.visible, expired.invisible),
        (2, 0),
        "vencimento: -1 invisível, +1 visível"
    );
    assert_eq!(
        expired.counters.redelivered, 0,
        "ainda não foi entregue de novo"
    );

    let again = consume(&h, "pedidos", q(Some("2"), Some("30"), None)).await;
    assert_eq!(again.len(), 2);
    assert_eq!(snapshot(&h).counters.redelivered, 1);
    let first = again.iter().find(|m| m.id == delivered[0].id).unwrap();
    h.messages
        .ack("pedidos", &first.id, Some(&first.receipt_token))
        .unwrap();
    let acked = snapshot(&h);
    assert_eq!(acked.depth(), 1, "ack: -1 no estado em que estava");
    assert_eq!(acked.counters.acked, 1);
    assert!(acked.oldest_ms.unwrap() > first_at);
    assert!(acked.consume_latency.p50.is_some());
}

/// UC: UC-GRP-015
#[tokio::test]
async fn group_snapshots_show_counts_bands_and_the_oldest_of_each_group() {
    let h = harness();
    h.topics.create(queue("pedidos")).await.unwrap();
    let group = |g: &str| PublishRequest {
        body: b"1".to_vec(),
        group_id: Some(g.into()),
        ..Default::default()
    };
    h.messages.publish("pedidos", group("a")).unwrap();
    h.clock.advance(Duration::from_secs(1));
    for _ in 0..3 {
        h.messages.publish("pedidos", group("b")).unwrap();
    }
    consume(&h, "pedidos", q(Some("1"), Some("600"), None)).await;

    let mut groups = h.messages.group_snapshots("pedidos").unwrap();
    groups.sort_by(|x, y| x.group_id.cmp(&y.group_id));
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0].group_id.as_deref(), Some("a"));
    assert_eq!(
        (groups[0].visible, groups[0].invisible),
        (0, 1),
        "a mais antiga saiu primeiro"
    );
    assert_eq!((groups[1].visible, groups[1].invisible), (3, 0));
    assert!(groups[1].oldest_ms.unwrap() > groups[0].oldest_ms.unwrap());
    assert!(!groups[0].overflow);
    assert_eq!(groups[1].band, 0);

    assert_eq!(
        h.messages.group_snapshots("nada").unwrap_err().code,
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
    h.topics.create(queue("pedidos")).await.unwrap();
    publish_err(
        &h,
        "pedidos",
        PublishRequest {
            body: b"{".to_vec(),
            content_type: Some("application/json".into()),
            ..Default::default()
        },
    );
    publish_err(&h, "nada", json_body("1"));
    publish_ok(&h, "pedidos", "1");
    let snapshot = h
        .messages
        .topic_snapshot("pedidos", &stats_rules())
        .unwrap();
    assert_eq!(snapshot.counters.publish_errors, 1);
    assert_eq!(snapshot.counters.published, 1);
    assert_eq!(snapshot.counters.published_bytes, 1);
    assert!(snapshot.publish_latency.p50.is_some());
}
