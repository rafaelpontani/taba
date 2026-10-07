use super::*;

#[test]
fn manual_clock_only_moves_when_advanced() {
    let clock = ManualClock::at_doc_epoch();
    let t0 = clock.now();
    assert_eq!(clock.now(), t0);
    clock.advance(Duration::from_secs(120));
    assert_eq!(clock.now(), t0 + Duration::from_secs(120));
    assert_eq!(clock.now_millis(), 1_791_288_120_000);
}

#[test]
fn system_clock_is_after_2026() {
    assert!(SystemClock.now_millis() > 1_767_225_600_000);
}
