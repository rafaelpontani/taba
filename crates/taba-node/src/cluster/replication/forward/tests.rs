use super::freest;

/// UC: UC-FWD-003
#[test]
fn the_freest_destination_is_the_one_with_the_lowest_load() {
    // load of the heartbeat + calls already forwarded and in progress, by destination
    assert_eq!(freest(&[5, 2, 9]), [1]);
    assert_eq!(freest(&[0, 3 + 4, 6]), [0]);
    // The load of the heartbeat counts even when nothing was forwarded there yet.
    assert_eq!(freest(&[7, 1]), [1]);
}

/// UC: UC-FWD-003
#[test]
fn a_tie_leaves_every_tied_destination_to_be_drawn_and_no_candidate_leaves_none() {
    assert_eq!(freest(&[3, 1, 4, 1]), [1, 3]);
    assert_eq!(freest(&[0, 0, 0]), [0, 1, 2]);
    assert!(freest(&[]).is_empty());
}
