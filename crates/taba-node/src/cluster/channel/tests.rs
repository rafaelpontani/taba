use super::keeps_new_link;

/// UC: UC-CAN-013
#[test]
fn of_two_connections_the_one_opened_by_the_node_with_the_larger_hash_stays() {
    // Este nó tem o maior hash: fica a que ele abriu.
    assert!(keeps_new_link(false, true, true), "a nova é minha: fica");
    assert!(!keeps_new_link(true, false, true), "a nova é do par: sai");
    // Este nó tem o menor hash: fica a que o par abriu.
    assert!(keeps_new_link(true, false, false), "a nova é do par: fica");
    assert!(!keeps_new_link(false, true, false), "a nova é minha: sai");
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
    // A conexão `ca` foi aberta por A e `cb` por B. O sobrevivente é a do nó de maior hash.
    for a_wins in [true, false] {
        let survivor_is_ca = a_wins;
        // No nó A (mine_wins = a_wins): `ca` é "minha", `cb` é do par.
        let at_a_ca_then_cb = !keeps_new_link(true, false, a_wins); // ca sobrevive se cb não entra
        let at_a_cb_then_ca = keeps_new_link(false, true, a_wins); // ca sobrevive se entra
        // No nó B (mine_wins = !a_wins): `cb` é "minha", `ca` é do par.
        let at_b_cb_then_ca = keeps_new_link(true, false, !a_wins); // ca sobrevive se entra
        let at_b_ca_then_cb = !keeps_new_link(false, true, !a_wins); // ca sobrevive se cb não entra
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
