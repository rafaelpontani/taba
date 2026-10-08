use std::time::{Duration, Instant};

use super::*;

/// UC: UC-OPS-016
#[test]
fn status_gives_the_resident_memory_and_the_threads() {
    let text = "Name:\ttaba\nVmPeak:\t  900000 kB\nVmRSS:\t   20480 kB\nThreads:\t12\n";
    assert_eq!(parse_status(text), (Some(20480 * 1024), Some(12)));
    assert_eq!(parse_status("Name:\ttaba\n"), (None, None));
}

/// UC: UC-OPS-016
#[test]
fn stat_cpu_counts_from_the_last_parenthesis() {
    // O comando tem espaço e parêntese; utime = 150 e stime = 50 (14º e 15º campos).
    let text = "42 (ta ba) x) S 1 42 42 0 -1 4194560 100 0 0 0 150 50 0 0 20 0 3 0 1000 0 0";
    assert_eq!(parse_stat_cpu(text, 100.0), Some(2.0));
    assert_eq!(parse_stat_cpu("sem parênteses", 100.0), None);
}

/// UC: UC-OPS-016
#[test]
fn utilization_is_measured_between_two_readings() {
    let meter = CpuMeter::default();
    let start = Instant::now();
    assert_eq!(
        meter.utilization(1.0, start, 4.0),
        None,
        "a primeira só marca"
    );
    // 2 s de CPU em 2 s de relógio, em 4 núcleos: 25%.
    let value = meter
        .utilization(3.0, start + Duration::from_secs(2), 4.0)
        .unwrap();
    assert!((value - 0.25).abs() < 1e-9, "{value}");
}
