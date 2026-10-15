use super::keeps_new_link;

/// UC: UC-CAN-013
#[test]
fn of_two_connections_the_one_opened_by_the_node_with_the_larger_hash_stays() {
    // This node has the highest hash: the one it opened stays.
    assert!(
        keeps_new_link(false, true, true),
        "the new one is mine: it stays"
    );
    assert!(
        !keeps_new_link(true, false, true),
        "the new one is the peer's: it goes"
    );
    // This node has the lowest hash: the one the peer opened stays.
    assert!(
        keeps_new_link(true, false, false),
        "the new one is the peer's: it stays"
    );
    assert!(
        !keeps_new_link(false, true, false),
        "the new one is mine: it goes"
    );
}

/// UC: UC-CAN-013
#[test]
fn the_same_side_opening_again_replaces_the_dead_connection() {
    for mine_wins in [true, false] {
        assert!(keeps_new_link(true, true, mine_wins));
        assert!(keeps_new_link(false, false, mine_wins));
    }
}

/// UC: UC-CAN-013
#[test]
fn both_nodes_keep_the_same_connection_in_any_order_of_arrival() {
    // Connection `ca` was opened by A and `cb` by B. The survivor is the one of the node with the highest hash.
    for a_wins in [true, false] {
        let survivor_is_ca = a_wins;
        // On node A (mine_wins = a_wins): `ca` is "mine", `cb` is the peer's.
        let at_a_ca_then_cb = !keeps_new_link(true, false, a_wins); // ca survives if cb does not come in
        let at_a_cb_then_ca = keeps_new_link(false, true, a_wins); // ca survives if it comes in
        // On node B (mine_wins = !a_wins): `cb` is "mine", `ca` is the peer's.
        let at_b_cb_then_ca = keeps_new_link(true, false, !a_wins); // ca survives if it comes in
        let at_b_ca_then_cb = !keeps_new_link(false, true, !a_wins); // ca survives if cb does not come in
        for ca_survives in [
            at_a_ca_then_cb,
            at_a_cb_then_ca,
            at_b_cb_then_ca,
            at_b_ca_then_cb,
        ] {
            assert_eq!(ca_survives, survivor_is_ca);
        }
    }
}
