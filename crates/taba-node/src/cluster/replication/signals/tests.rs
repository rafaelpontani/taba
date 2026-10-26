use super::draw_order;

fn nodes(list: &[(&str, Option<u64>)]) -> Vec<(String, Option<u64>)> {
    list.iter().map(|(n, c)| ((*n).to_owned(), *c)).collect()
}

/// Which node comes first for each possible point of the draw.
fn firsts(list: &[(&str, Option<u64>)], total: u64) -> Vec<String> {
    (0..total)
        .map(|point| draw_order(nodes(list), |_| point).remove(0))
        .collect()
}

/// UC: UC-ENT-019
#[test]
fn the_peer_is_drawn_with_a_weight_proportional_to_the_ready_ones_it_said() {
    let list = [("a", Some(1)), ("b", Some(9)), ("c", Some(90))];
    let first = firsts(&list, 100);
    let count = |who: &str| first.iter().filter(|n| *n == who).count();
    assert_eq!((count("a"), count("b"), count("c")), (1, 9, 90));
}

/// UC: UC-ENT-019
#[test]
fn without_the_quantity_of_some_peer_the_draw_is_uniform() {
    let list = [("a", Some(1_000)), ("b", None), ("c", Some(5))];
    let first = firsts(&list, 3);
    assert_eq!(first, ["a", "b", "c"]);
}

/// UC: UC-ENT-019
#[test]
fn a_peer_that_said_zero_still_has_a_chance_and_every_peer_comes_once() {
    let list = [("a", Some(0)), ("b", Some(0)), ("c", Some(4))];
    let first = firsts(&list, 6);
    assert_eq!(first.iter().filter(|n| *n == "a").count(), 1);
    assert_eq!(first.iter().filter(|n| *n == "c").count(), 4);

    // After the first is drawn the others are drawn among those left, so nobody repeats or is lost.
    let mut order = draw_order(nodes(&list), |total| total - 1);
    assert_eq!(order, ["c", "b", "a"]);
    order.sort();
    assert_eq!(order, ["a", "b", "c"]);
    assert!(draw_order(Vec::new(), |_| 0).is_empty());
}
