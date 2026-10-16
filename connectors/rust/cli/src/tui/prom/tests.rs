use super::*;

const TEXT: &str = r#"# HELP taba_ratelimit_rejected_total Requests refused.
# TYPE taba_ratelimit_rejected_total counter
taba_ratelimit_rejected_total{category="publish"} 3
taba_ratelimit_rejected_total{category="consume"} 4
# TYPE taba_storage_memory_messages gauge
taba_storage_memory_messages 12
http_server_request_duration_seconds_bucket{le="+Inf",route="/a{b}"} 7 1700000000
weird{msg="say \"hi\"\\ok"} NaN
"#;

/// TUI: TUI-CLU-04
#[test]
fn samples_are_read_with_labels_and_values() {
    let m = Metrics::parse(TEXT);
    assert_eq!(m.samples.len(), 5);
    assert_eq!(m.sum("taba_ratelimit_rejected_total"), Some(7.0));
    assert_eq!(m.sum("taba_storage_memory_messages"), Some(12.0));
    assert_eq!(m.sum("nothing"), None);
    let bucket = &m.samples[3];
    assert_eq!(bucket.label("le"), Some("+Inf"));
    assert_eq!(bucket.label("route"), Some("/a{b}"));
    assert_eq!(bucket.value, 7.0);
}

/// TUI: TUI-MON-06
#[test]
fn counters_are_told_from_gauges_by_the_type_line() {
    let m = Metrics::parse(TEXT);
    assert!(m.is_counter("taba_ratelimit_rejected_total"));
    assert!(!m.is_counter("taba_storage_memory_messages"));
    assert_eq!(m.kind("taba_storage_memory_messages"), Some("gauge"));
}

#[test]
fn escaped_label_values_are_decoded_and_nan_is_kept() {
    let m = Metrics::parse(TEXT);
    let weird = m.samples.last().unwrap();
    assert_eq!(weird.label("msg"), Some("say \"hi\"\\ok"));
    assert!(weird.value.is_nan());
}

/// TUI: TUI-MON-06
#[test]
fn a_rate_is_the_difference_over_the_time_and_a_restart_has_none() {
    assert_eq!(rate(10.0, 20.0, 5.0), Some(2.0));
    assert_eq!(rate(10.0, 10.0, 5.0), Some(0.0));
    assert_eq!(rate(20.0, 3.0, 5.0), None);
    assert_eq!(rate(1.0, 2.0, 0.0), None);
}

#[test]
fn garbage_lines_are_skipped() {
    let m = Metrics::parse("not a metric\n# comment\n\nok 1\n{broken 2\nname{a=\"b\" 3\n");
    assert_eq!(m.samples.len(), 1);
    assert_eq!(m.samples[0].series(), "ok");
}

#[test]
fn a_series_is_printed_with_its_labels() {
    let m = Metrics::parse(TEXT);
    assert_eq!(
        m.samples[0].series(),
        "taba_ratelimit_rejected_total{category=\"publish\"}"
    );
}
