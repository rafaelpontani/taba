use super::*;

/// UC: UC-OPS-012, UC-OPS-041
#[test]
fn counters_add_up_per_series() {
    let registry = Registry::default();
    let labels = [("taba.category", "publish"), ("taba.key", "ip")];
    registry.add(&RATELIMIT_REJECTED, &labels, 1.0);
    registry.add(&RATELIMIT_REJECTED, &labels, 2.0);
    registry.add(
        &RATELIMIT_REJECTED,
        &[("taba.category", "consume"), ("taba.key", "token")],
        1.0,
    );
    assert_eq!(registry.get(&RATELIMIT_REJECTED, &labels), Some(3.0));
    assert_eq!(
        registry.get(
            &RATELIMIT_REJECTED,
            &[("taba.key", "token"), ("taba.category", "consume")]
        ),
        Some(1.0),
        "the order of the attributes does not matter"
    );
    assert_eq!(registry.get(&RATELIMIT_REJECTED, &[]), None);
}

#[test]
fn gauges_replace_and_clear_drops_every_series() {
    let registry = Registry::default();
    registry.set(&NODE_STATE, &[("taba.state", "active")], 1.0);
    registry.set(&NODE_STATE, &[("taba.state", "active")], 0.0);
    registry.set(&NODE_STATE, &[("taba.state", "syncing")], 1.0);
    assert_eq!(
        registry.get(&NODE_STATE, &[("taba.state", "active")]),
        Some(0.0)
    );
    registry.clear(&NODE_STATE);
    assert!(registry.snapshot().is_empty());
}

#[test]
fn histograms_observe_and_can_be_replaced() {
    let registry = Registry::default();
    registry.observe(&HTTP_SERVER_DURATION, &[("http.route", "/x")], 0.02);
    registry.observe(&HTTP_SERVER_DURATION, &[("http.route", "/x")], 3.0);
    let h = registry
        .histogram(&HTTP_SERVER_DURATION, &[("http.route", "/x")])
        .unwrap();
    assert_eq!(h.count(), 2);

    let mut other = Histogram::new(HTTP_SERVER_DURATION.buckets);
    other.record(0.001);
    registry.set_histogram(&HTTP_SERVER_DURATION, &[("http.route", "/x")], other);
    let h = registry
        .histogram(&HTTP_SERVER_DURATION, &[("http.route", "/x")])
        .unwrap();
    assert_eq!(h.count(), 1);
}

/// No metric uses the old `msg.` prefix, and each name is unique.
///
/// UC: UC-OPS-041
#[test]
fn names_follow_the_opentelemetry_vocabulary() {
    let all: Vec<&Def> = vec![
        &PROCESS_UPTIME,
        &PROCESS_CPU_UTILIZATION,
        &PROCESS_MEMORY_USAGE,
        &PROCESS_THREAD_COUNT,
        &PROCESS_OPEN_FILES,
        &NODE_CONNECTIONS,
        &STORAGE_MEMORY_USAGE,
        &STORAGE_MEMORY_LIMIT,
        &STORAGE_MEMORY_MESSAGES,
        &STORAGE_DISK_MESSAGES,
        &STORAGE_DISK_FREE,
        &STORAGE_OVERFLOW,
        &STORAGE_READS,
        &STORAGE_DISK_ERRORS,
        &HTTP_SERVER_DURATION,
        &HTTP_SERVER_ACTIVE,
        &HTTP_SERVER_REQUEST_BODY,
        &HTTP_SERVER_RESPONSE_BODY,
        &RATELIMIT_REJECTED,
        &AUTH_REQUESTS,
        &AUTH_TOKENS,
        &DELIVERY_READY,
        &DELIVERY_FAIR_WAIT,
        &DELIVERY_REDELIVERIES,
        &DELIVERY_DEAD_LETTERED,
        &RETENTION_EXPIRED,
        &FIFO_DEDUPLICATED,
        &FIFO_GROUPS_BUSY,
        &LONGPOLL_WAITING,
        &LONGPOLL_OUTCOME,
        &NODE_STATE,
        &NODE_DRAIN_STATE,
        &NODE_DRAIN_PENDING,
    ];
    let mut seen = std::collections::BTreeSet::new();
    for def in all.iter().copied().chain(TOPIC_DEFS.iter().copied()) {
        assert!(!def.name.starts_with("msg."), "{}", def.name);
        assert!(
            def.name.starts_with("taba.")
                || def.name.starts_with("process.")
                || def.name.starts_with("http."),
            "{}",
            def.name
        );
        assert!(seen.insert(def.name), "repeated name: {}", def.name);
        assert!(
            matches!(def.unit, "s" | "By" | "1") || def.unit.starts_with('{'),
            "unit `{}` of {}",
            def.unit,
            def.name
        );
        assert_eq!(
            def.kind == Kind::Histogram,
            !def.buckets.is_empty(),
            "{}",
            def.name
        );
    }
}

#[test]
fn series_updated_by_many_threads_at_once_lose_nothing() {
    let registry = std::sync::Arc::new(Registry::default());
    let threads: Vec<_> = (0..8)
        .map(|t| {
            let registry = registry.clone();
            std::thread::spawn(move || {
                let route = if t % 2 == 0 { "/a" } else { "/b" };
                for _ in 0..1000 {
                    registry.add(&RATELIMIT_REJECTED, &[("taba.key", route)], 1.0);
                    registry.observe(&HTTP_SERVER_DURATION, &[("http.route", route)], 0.001);
                }
            })
        })
        .collect();
    for thread in threads {
        thread.join().unwrap();
    }
    for route in ["/a", "/b"] {
        assert_eq!(
            registry.get(&RATELIMIT_REJECTED, &[("taba.key", route)]),
            Some(4000.0)
        );
        let h = registry
            .histogram(&HTTP_SERVER_DURATION, &[("http.route", route)])
            .unwrap();
        assert_eq!(h.count(), 4000);
        assert!((h.sum() - 4.0).abs() < 1e-9);
    }
}

#[test]
fn series_with_many_attributes_and_similar_values_stay_apart() {
    let registry = Registry::default();
    let many: Vec<(String, String)> = (0..12)
        .map(|i| (format!("k{i}"), format!("v{i}")))
        .collect();
    let labels: Vec<(&str, &str)> = many.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    registry.add(&RATELIMIT_REJECTED, &labels, 2.0);
    let mut reversed = labels.clone();
    reversed.reverse();
    assert_eq!(registry.get(&RATELIMIT_REJECTED, &reversed), Some(2.0));
    // The separator keeps ("ab", "c") apart from ("a", "bc").
    registry.add(&RATELIMIT_REJECTED, &[("ab", "c")], 1.0);
    registry.add(&RATELIMIT_REJECTED, &[("a", "bc")], 5.0);
    assert_eq!(registry.get(&RATELIMIT_REJECTED, &[("ab", "c")]), Some(1.0));
    assert_eq!(registry.get(&RATELIMIT_REJECTED, &[("a", "bc")]), Some(5.0));
    let snapshot = registry.snapshot();
    let series = &snapshot
        .iter()
        .find(|s| s.def.name == RATELIMIT_REJECTED.name)
        .unwrap()
        .series;
    assert_eq!(series.len(), 3);
    assert!(
        series.windows(2).all(|w| w[0].0 < w[1].0),
        "attribute order"
    );
}
