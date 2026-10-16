use super::*;
use crate::tui::api::NodeStatus;

fn status(partial: bool, nodes: &[(&str, u64, f64)]) -> ClusterStatus {
    ClusterStatus {
        partial,
        nodes: nodes
            .iter()
            .map(|(id, used, rejected)| NodeStatus {
                node_id: (*id).to_owned(),
                reachable: true,
                memory_used: *used,
                memory_limit: 1000,
                rate_limited: *rejected,
                ..NodeStatus::default()
            })
            .collect(),
        ..ClusterStatus::default()
    }
}

/// TUI: TUI-MON-05
#[test]
fn the_default_keeps_an_hour_every_five_seconds() {
    let h = History::new(DEFAULT_RETENTION_SECS, DEFAULT_INTERVAL_SECS);
    assert_eq!(h.capacity(), 720);
    assert_eq!(h.retention_ms(), 3_600_000);
}

/// TUI: TUI-MON-05
#[test]
fn a_ring_drops_the_oldest_when_full() {
    let mut h = History::new(20, 5);
    assert_eq!(h.capacity(), 4);
    for i in 0..6u64 {
        h.sample(i * 5000, &status(false, &[("a", i, 0.0)]));
    }
    let ring = h.series("a", Series::MemoryUsed).unwrap();
    assert_eq!(ring.len(), 4);
    let values: Vec<f64> = ring
        .window(25_000, 100_000)
        .iter()
        .map(|p| p.value)
        .collect();
    assert_eq!(values, vec![2.0, 3.0, 4.0, 5.0]);
}

#[test]
fn samples_closer_than_the_interval_are_skipped() {
    let mut h = History::new(60, 5);
    assert!(h.sample(0, &status(false, &[("a", 1, 0.0)])));
    assert!(!h.sample(4_999, &status(false, &[("a", 2, 0.0)])));
    assert!(h.sample(5_000, &status(false, &[("a", 3, 0.0)])));
    assert_eq!(h.series("a", Series::MemoryUsed).unwrap().len(), 2);
}

/// TUI: TUI-MON-06
#[test]
fn a_counter_becomes_a_rate_and_a_restart_is_not_a_negative_one() {
    let mut h = History::new(60, 5);
    h.sample(0, &status(false, &[("a", 0, 10.0)]));
    h.sample(5_000, &status(false, &[("a", 0, 20.0)]));
    h.sample(10_000, &status(false, &[("a", 0, 2.0)]));
    h.sample(15_000, &status(false, &[("a", 0, 7.0)]));
    let rates: Vec<f64> = h
        .series("a", Series::RateLimited)
        .unwrap()
        .window(15_000, 100_000)
        .iter()
        .map(|p| p.value)
        .collect();
    assert_eq!(rates, vec![2.0, 1.0]);
}

/// TUI: TUI-FRM-08
#[test]
fn the_sample_of_a_partial_read_is_marked() {
    let mut h = History::new(60, 5);
    h.sample(0, &status(true, &[("a", 1, 0.0)]));
    assert!(
        h.series("a", Series::MemoryUsed)
            .unwrap()
            .last()
            .unwrap()
            .partial
    );
}

/// TUI: TUI-MON-05
#[test]
fn a_young_series_says_how_long_it_has_been_collected() {
    let mut h = History::new(3600, 5);
    h.sample(10_000, &status(false, &[("a", 1, 0.0)]));
    h.sample(15_000, &status(false, &[("a", 1, 0.0)]));
    let ring = h.series("a", Series::MemoryUsed).unwrap();
    assert_eq!(ring.collected_ms(70_000), 60_000);
}

/// TUI: TUI-HLP-05
#[test]
fn clearing_forgets_everything() {
    let mut h = History::new(60, 5);
    h.sample(0, &status(false, &[("a", 1, 0.0)]));
    h.clear();
    assert!(h.series("a", Series::MemoryUsed).is_none());
    assert!(h.sample(1, &status(false, &[("a", 1, 0.0)])));
}

#[test]
fn the_summary_has_min_max_and_last() {
    let p = |v| Point {
        at: 0,
        value: v,
        partial: false,
    };
    assert_eq!(summary(&[p(3.0), p(1.0), p(2.0)]), Some((1.0, 3.0, 2.0)));
    assert_eq!(summary(&[]), None);
}

#[test]
fn the_settings_are_validated() {
    assert!(validate(3600, 5).is_ok());
    assert!(validate(7 * 3600, 5).is_err());
    assert!(validate(3600, 0).is_err());
    assert!(validate(3, 5).is_err());
}

/// TUI: TUI-MON-03
#[test]
fn a_window_ends_at_the_given_instant_so_a_frozen_chart_does_not_move() {
    let mut h = History::new(600, 5);
    for i in 0..4u64 {
        h.sample(i * 5000, &status(false, &[("a", i, 0.0)]));
    }
    let ring = h.series("a", Series::MemoryUsed).unwrap();
    assert_eq!(ring.window(10_000, 60_000).len(), 3);
    assert_eq!(ring.window(15_000, 60_000).len(), 4);
    assert_eq!(ring.window(15_000, 5_000).len(), 2);
}
