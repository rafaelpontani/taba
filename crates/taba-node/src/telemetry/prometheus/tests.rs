use super::*;
use crate::telemetry::metrics::*;

fn text(registry: &Registry) -> String {
    render(&registry.snapshot(), &[], false)
}

/// UC: UC-OPS-011
#[test]
fn names_map_from_opentelemetry_to_prometheus() {
    let cases: [(&Def, &str); 7] = [
        (&STORAGE_MEMORY_LIMIT, "taba_storage_memory_limit_bytes"),
        (
            &HTTP_SERVER_DURATION,
            "http_server_request_duration_seconds",
        ),
        (&PROCESS_CPU_UTILIZATION, "process_cpu_utilization_ratio"),
        (&NODE_STATE, "taba_node_state"),
        (&STORAGE_MEMORY_MESSAGES, "taba_storage_memory_messages"),
        (
            &TOPIC_OLDEST_TIMESTAMP,
            "taba_topic_oldest_timestamp_seconds",
        ),
        (&PROCESS_UPTIME, "process_uptime_seconds"),
    ];
    for (def, expected) in cases {
        assert_eq!(family_name(def), expected);
    }
    assert_eq!(
        label_name("http.response.status_code"),
        "http_response_status_code"
    );
    assert_eq!(label_name("taba.topic.tag.time"), "taba_topic_tag_time");
}

/// UC: UC-OPS-011
#[test]
fn counters_get_total_and_the_unit_is_not_repeated() {
    let registry = Registry::default();
    registry.add(&DELIVERY_REDELIVERIES, &[], 3.0);
    registry.set(&STORAGE_MEMORY_USAGE, &[], 1024.0);
    let out = text(&registry);
    assert!(out.contains("# TYPE taba_delivery_redeliveries_total counter\n"));
    assert!(
        out.contains("taba_delivery_redeliveries_total 3\n"),
        "{out}"
    );
    assert!(out.contains("# TYPE taba_storage_memory_usage_bytes gauge\n"));
    assert!(out.contains("taba_storage_memory_usage_bytes 1024\n"));
    assert!(
        !out.contains("{message}"),
        "the unit in braces does not appear in the name"
    );
}

/// UC: UC-OPS-011, UC-OPS-041
#[test]
fn histograms_become_bucket_sum_and_count_with_attributes_as_labels() {
    let registry = Registry::default();
    let labels = [
        ("http.request.method", "GET"),
        ("http.route", "/topics/{topic}/messages"),
        ("http.response.status_code", "200"),
    ];
    registry.observe(&HTTP_SERVER_DURATION, &labels, 0.004);
    registry.observe(&HTTP_SERVER_DURATION, &labels, 0.2);
    let out = text(&registry);
    assert!(out.contains("# TYPE http_server_request_duration_seconds histogram\n"));
    assert!(
        out.contains(
            "http_server_request_duration_seconds_bucket{http_request_method=\"GET\",\
             http_response_status_code=\"200\",http_route=\"/topics/{topic}/messages\",\
             le=\"0.005\"} 1\n"
        ),
        "{out}"
    );
    assert!(out.contains("le=\"+Inf\"} 2\n"));
    assert!(out.contains("http_server_request_duration_seconds_count{http_request_method=\"GET\""));
    assert!(out.contains("_sum{"));
}

#[test]
fn label_values_are_escaped() {
    let registry = Registry::default();
    registry.set(&NODE_STATE, &[("taba.state", "a\"b\\c\nd")], 1.0);
    assert!(text(&registry).contains(r#"taba_node_state{taba_state="a\"b\\c\nd"} 1"#));
}

/// UC: UC-OPS-011, UC-OPS-040
#[test]
fn the_resource_becomes_target_info() {
    let registry = Registry::default();
    registry.set(&PROCESS_UPTIME, &[], 5.0);
    let resource = vec![
        ("service.name".to_owned(), "taba".to_owned()),
        ("service.instance.id".to_owned(), "n1".to_owned()),
    ];
    let out = render(&registry.snapshot(), &resource, false);
    assert!(out.contains("target_info{service_name=\"taba\",service_instance_id=\"n1\"} 1\n"));
    assert!(out.contains("# TYPE target_info gauge\n"));
}

/// UC: UC-OPS-006
#[test]
fn open_metrics_drops_total_from_the_family_and_ends_with_eof() {
    let registry = Registry::default();
    registry.add(&DELIVERY_REDELIVERIES, &[], 1.0);
    let out = render(
        &registry.snapshot(),
        &[("service.name".to_owned(), "taba".to_owned())],
        true,
    );
    assert!(
        out.contains("# TYPE taba_delivery_redeliveries counter\n"),
        "{out}"
    );
    assert!(out.contains("taba_delivery_redeliveries_total 1\n"));
    assert!(out.contains("# TYPE target info\n"));
    assert!(out.ends_with("# EOF\n"));
}

#[test]
fn families_without_series_are_left_out() {
    let registry = Registry::default();
    registry.add(&DELIVERY_REDELIVERIES, &[], 1.0);
    registry.clear(&DELIVERY_REDELIVERIES);
    assert_eq!(text(&registry), "");
}
