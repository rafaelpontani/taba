use super::jitter_in;

/// UC: UC-VOL-019
#[test]
fn the_takeover_delay_is_stable_per_queue_and_spreads_the_queries_inside_the_window() {
    let window = 2_000;
    let delays: Vec<u64> = (0..200)
        .map(|n| jitter_in(window, &format!("01a11115-be00-7000-8000-{n:012}")))
        .collect();
    assert!(delays.iter().all(|d| *d <= window), "within the window");
    // The same IDs always give the same delays (it is not a draw on each round).
    let again: Vec<u64> = (0..200)
        .map(|n| jitter_in(window, &format!("01a11115-be00-7000-8000-{n:012}")))
        .collect();
    assert_eq!(delays, again);
    // Spread out: the 200 queues fall in at least 15 of the 20 bands of 100 ms.
    let mut bands = std::collections::BTreeSet::new();
    for delay in &delays {
        bands.insert(delay / 100);
    }
    assert!(bands.len() >= 15, "faixas ocupadas: {}", bands.len());
    // Without a window, the query is immediate.
    assert_eq!(jitter_in(0, "any"), 0);
}
