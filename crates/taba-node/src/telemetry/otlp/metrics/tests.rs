use std::time::{Duration, UNIX_EPOCH};

use opentelemetry_proto::tonic::metrics::v1::metric::Data;
use opentelemetry_proto::tonic::metrics::v1::number_data_point::Value as Number;

use super::*;
use crate::telemetry::metrics::{
    HTTP_SERVER_DURATION, NODE_CONNECTIONS, NODE_STATE, Registry, WRITE_ROLLBACKS,
};

fn resource() -> Vec<(String, String)> {
    vec![
        ("service.name".to_owned(), "taba".to_owned()),
        ("service.instance.id".to_owned(), "no-1".to_owned()),
    ]
}

fn request(registry: &Registry) -> ExportMetricsServiceRequest {
    build_request(
        &registry.snapshot(),
        &resource(),
        UNIX_EPOCH + Duration::from_secs(100),
        UNIX_EPOCH + Duration::from_secs(160),
    )
}

fn find<'a>(request: &'a ExportMetricsServiceRequest, name: &str) -> &'a Metric {
    request.resource_metrics[0].scope_metrics[0]
        .metrics
        .iter()
        .find(|m| m.name == name)
        .unwrap_or_else(|| panic!("sem a métrica {name}"))
}

/// UC: UC-OPS-013
#[test]
fn each_instrument_kind_becomes_the_matching_otlp_aggregation() {
    let registry = Registry::default();
    registry.add(&WRITE_ROLLBACKS, &[], 2.0);
    registry.add(&NODE_CONNECTIONS, &[("server.port", "8080")], 3.0);
    registry.set(&NODE_STATE, &[("taba.state", "active")], 1.0);
    registry.observe(&HTTP_SERVER_DURATION, &[("http.route", "/x")], 0.02);
    registry.observe(&HTTP_SERVER_DURATION, &[("http.route", "/x")], 20.0);
    let request = request(&registry);

    let Some(Data::Sum(sum)) = &find(&request, "taba.write.rollbacks").data else {
        panic!("contador é Sum");
    };
    assert!(sum.is_monotonic);
    assert_eq!(
        sum.aggregation_temporality,
        AggregationTemporality::Cumulative as i32
    );
    assert_eq!(sum.data_points[0].value, Some(Number::AsDouble(2.0)));
    assert_eq!(sum.data_points[0].start_time_unix_nano, 100_000_000_000);
    assert_eq!(sum.data_points[0].time_unix_nano, 160_000_000_000);

    let Some(Data::Sum(updown)) = &find(&request, "taba.node.connections").data else {
        panic!("up-down é Sum");
    };
    assert!(!updown.is_monotonic);
    assert_eq!(
        updown.data_points[0].attributes[0].key, "server.port",
        "os atributos da série vão no ponto"
    );

    let Some(Data::Gauge(gauge)) = &find(&request, "taba.node.state").data else {
        panic!("medidor é Gauge");
    };
    assert_eq!(gauge.data_points[0].value, Some(Number::AsDouble(1.0)));

    let histogram_metric = find(&request, "http.server.request.duration");
    assert_eq!(histogram_metric.unit, "s");
    let Some(Data::Histogram(histogram)) = &histogram_metric.data else {
        panic!("histograma é Histogram");
    };
    let point = &histogram.data_points[0];
    assert_eq!(point.count, 2);
    assert_eq!(point.sum, Some(20.02));
    assert_eq!(
        point.bucket_counts.len(),
        point.explicit_bounds.len() + 1,
        "um balde a mais: o +Inf"
    );
    assert_eq!(point.bucket_counts.iter().sum::<u64>(), 2);
    assert_eq!(
        *point.bucket_counts.last().unwrap(),
        1,
        "20 s passa de todos os limites"
    );
}

/// UC: UC-OPS-013, UC-OPS-040
#[test]
fn the_request_carries_the_resource_the_scope_and_skips_families_without_series() {
    let registry = Registry::default();
    registry.add(&WRITE_ROLLBACKS, &[], 1.0);
    // Uma métrica apagada deixa a família vazia.
    registry.set(&NODE_STATE, &[("taba.state", "active")], 1.0);
    registry.clear(&NODE_STATE);
    let request = request(&registry);

    let resource = request.resource_metrics[0].resource.as_ref().unwrap();
    let keys: Vec<(&str, Option<&str>)> = resource
        .attributes
        .iter()
        .map(|kv| {
            (
                kv.key.as_str(),
                match kv.value.as_ref().and_then(|v| v.value.as_ref()) {
                    Some(any_value::Value::StringValue(s)) => Some(s.as_str()),
                    _ => None,
                },
            )
        })
        .collect();
    assert_eq!(
        keys,
        vec![
            ("service.name", Some("taba")),
            ("service.instance.id", Some("no-1"))
        ]
    );
    let scope = request.resource_metrics[0].scope_metrics[0]
        .scope
        .as_ref()
        .unwrap();
    assert_eq!(scope.name, "taba");
    let names: Vec<&str> = request.resource_metrics[0].scope_metrics[0]
        .metrics
        .iter()
        .map(|m| m.name.as_str())
        .collect();
    assert_eq!(names, vec!["taba.write.rollbacks"]);
}
