use super::*;

/// UC: UC-ARM-005
#[test]
fn the_rate_is_the_moving_average_over_the_window() {
    let mut rate = RateWindow::default();
    assert_eq!(rate.per_second(0, 300), 0.0);

    // 600 mensagens espalhadas pela janela de 300 s: 2 por segundo.
    for second in 0..300 {
        rate.record(second * 1000, 2, 300);
    }
    assert!((rate.per_second(299_000, 300) - 2.0).abs() < 1e-9);
}

/// UC: UC-ARM-005
#[test]
fn what_left_the_window_stops_counting() {
    let mut rate = RateWindow::default();
    rate.record(0, 300, 300);
    assert!((rate.per_second(0, 300) - 1.0).abs() < 1e-9);
    assert!(
        (rate.per_second(299_000, 300) - 1.0).abs() < 1e-9,
        "ainda dentro"
    );
    assert_eq!(rate.per_second(300_000, 300), 0.0, "saiu da janela");

    // Registrar de novo descarta os baldes velhos.
    rate.record(1_000_000, 3, 300);
    assert_eq!(rate.buckets.len(), 1);
    assert!((rate.per_second(1_000_000, 300) - 0.01).abs() < 1e-9);
}

/// UC: UC-ARM-005
#[test]
fn several_events_in_one_second_share_a_bucket() {
    let mut rate = RateWindow::default();
    rate.record(5_100, 1, 60);
    rate.record(5_900, 4, 60);
    assert_eq!(rate.buckets.len(), 1);
    assert!((rate.per_second(5_900, 60) - 5.0 / 60.0).abs() < 1e-9);
}

/// UC: UC-ARM-005
#[test]
fn publish_and_consume_are_counted_apart() {
    let mut rates = Rates::default();
    rates.published.record(0, 10, 10);
    rates.consumed.record(0, 2, 10);
    assert!((rates.published.per_second(0, 10) - 1.0).abs() < 1e-9);
    assert!((rates.consumed.per_second(0, 10) - 0.2).abs() < 1e-9);
}
