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
    // The command has a space and a parenthesis; utime = 150 and stime = 50 (14th and 15th fields).
    let text = "42 (ta ba) x) S 1 42 42 0 -1 4194560 100 0 0 0 150 50 0 0 20 0 3 0 1000 0 0";
    assert_eq!(parse_stat_cpu(text, 100.0), Some(2.0));
    assert_eq!(parse_stat_cpu("no parentheses", 100.0), None);
}

/// UC: UC-OPS-016
#[test]
fn utilization_is_measured_between_two_readings() {
    let meter = CpuMeter::default();
    let start = Instant::now();
    assert_eq!(
        meter.utilization(1.0, start, 4.0),
        None,
        "the first one only marks"
    );
    // 2 s of CPU in 2 s of clock, on 4 cores: 25%.
    let value = meter
        .utilization(3.0, start + Duration::from_secs(2), 4.0)
        .unwrap();
    assert!((value - 0.25).abs() < 1e-9, "{value}");
}
