//! The bounded lists: the ready window (cursors, refill, middle rule) and the invisible list
//! (ceiling, inclusion rule, `expiry` table, reload).

use super::*;
use std::collections::BTreeSet;

fn bounded(ready_cap: usize, invisible_cap: usize, threshold: usize) -> TopicIndex {
    TopicIndex::new(IndexRules {
        ready_cap,
        invisible_cap,
        reload_threshold: threshold,
        ..IndexRules::default()
    })
}

fn id(n: u32) -> String {
    format!("m{n:03}")
}

/// `count` ready messages of a single group, `m001`..., and the window rebuilt from both ends.
fn ready_index(ready_cap: usize, count: u32) -> TopicIndex {
    let mut index = bounded(ready_cap, usize::MAX, 0);
    for n in 1..=count {
        index.insert(&id(n), None, u64::from(n), 0, 0, 100);
    }
    index.reset_window();
    index
}

/// The `expiry` table, kept in memory for the tests: applies what the index queued.
#[derive(Default)]
struct Table(BTreeSet<(u64, String)>);

impl Table {
    fn apply(&mut self, index: &mut TopicIndex) {
        for op in index.take_spill_ops() {
            match op {
                SpillOp::Put(until, id) => {
                    self.0.insert((until, id));
                }
                SpillOp::Delete(until, id) => {
                    self.0.remove(&(until, id));
                }
            }
        }
    }

    fn reload(&mut self, index: &mut TopicIndex) -> usize {
        self.apply(index);
        let wanted = index.reload_wanted();
        let rows: Vec<_> = self.0.iter().take(wanted).cloned().collect();
        let loaded = index.reload(rows);
        self.apply(index);
        loaded
    }
}

/// UC: UC-ENT-008
#[test]
fn the_ready_list_holds_at_most_the_configured_size_and_the_counters_stay_exact() {
    let index = ready_index(5, 50);
    assert_eq!(index.window_len(), 5);
    assert_eq!(index.ready_ids().len(), 5);
    assert_eq!(index.counts(), (50, 0));
    assert_eq!(
        index.group_counts(&GroupKey::Named(String::new())),
        Some((50, 0))
    );
}

/// UC: UC-ENT-009
#[test]
fn only_the_room_that_opened_is_refilled_and_a_full_list_costs_nothing() {
    let mut index = ready_index(5, 50);
    let steps = index.scan_steps();
    assert_eq!(index.refill(), 0, "lista cheia: nada a repor");
    assert_eq!(index.scan_steps(), steps);
    for n in [1, 2, 50] {
        index.deliver(&id(n), 1_000_000, 100, 1);
    }
    assert_eq!(index.window_len(), 2);
    assert_eq!(index.refill(), 3, "repõe a quantidade que abriu");
    assert_eq!(index.window_len(), 5);
    assert_eq!(index.refill(), 0);
}

/// UC: UC-ENT-010
#[test]
fn the_refill_walks_two_cursors_alternating_the_oldest_and_the_newest_end() {
    let index = ready_index(4, 20);
    assert_eq!(index.ready_ids(), [id(1), id(2), id(19), id(20)]);

    let mut index = ready_index(4, 20);
    for n in [1, 2, 19, 20] {
        index.remove(&id(n));
    }
    assert_eq!(index.refill(), 4);
    assert_eq!(index.ready_ids(), [id(3), id(4), id(17), id(18)]);
    // The cursors only move forward: the next refill continues where they stopped.
    for n in [3, 4, 17, 18] {
        index.remove(&id(n));
    }
    index.refill();
    assert_eq!(index.ready_ids(), [id(5), id(6), id(15), id(16)]);
}

/// UC: UC-ENT-012
#[test]
fn a_new_message_enters_the_list_if_there_is_room_and_nothing_waits_else_the_cursor_finds_it() {
    let mut index = bounded(3, usize::MAX, 0);
    for n in 1..=2 {
        index.insert(&id(n), None, u64::from(n), 0, 0, 100);
    }
    index.insert(&id(3), None, 3, 0, 0, 100);
    assert_eq!(
        index.ready_ids(),
        [id(1), id(2), id(3)],
        "lugar e nada esperando: entra direto"
    );

    // With messages waiting in the gap, a new one is left for the thread to find.
    let mut index = ready_index(1, 10);
    index.insert(&id(11), None, 11, 0, 0, 100);
    assert!(!index.ready_ids().contains(&id(11)));
    assert_eq!(index.counts().0, 11);
    // Nothing is lost: delivering everything brings all of them, the new one included.
    let mut delivered = BTreeSet::new();
    while let Some(next) = index.pick() {
        index.deliver(&next, 1_000_000, 100, 1);
        delivered.insert(next);
    }
    assert!(delivered.contains(&id(11)));
    assert_eq!(delivered.len(), 11);
}

/// UC: UC-ENT-014
#[test]
fn a_returning_message_enters_by_its_position_and_the_middle_id_leaves_when_full() {
    let mut index = ready_index(4, 10);
    index.deliver(&id(1), 5_000, 100, 1);
    index.refill();
    assert_eq!(index.ready_ids(), [id(2), id(3), id(9), id(10)]);
    let steps = index.scan_steps();

    // The invisibility ends: the message returns by the position of its ID, with no scan.
    assert_eq!(index.release_due(5_000), [id(1)]);
    assert_eq!(index.scan_steps(), steps);
    assert_eq!(
        index.ready_ids(),
        [id(1), id(2), id(9), id(10)],
        "o ID do meio (m003) saiu"
    );
    assert_eq!(index.counts(), (10, 0));

    // The one that left is found again: no message is lost.
    let mut delivered = BTreeSet::new();
    while let Some(next) = index.pick() {
        index.deliver(&next, 1_000_000, 5_000, 2);
        delivered.insert(next);
    }
    assert_eq!(delivered.len(), 10);
    assert!(delivered.contains(&id(3)));
}

/// UC: UC-ENT-015
#[test]
fn choosing_the_next_message_only_reads_the_list_and_scans_nothing() {
    let mut index = ready_index(5, 200);
    let steps = index.scan_steps();
    for _ in 0..100 {
        assert!(index.pick().is_some());
    }
    assert_eq!(index.scan_steps(), steps);
}

/// The ready window always holds the oldest ready message: the `normal` pick (the oldest of all)
/// is right whatever happened before.
#[test]
fn the_window_always_holds_the_oldest_ready_message() {
    let mut index = bounded(6, usize::MAX, 0);
    let mut ready: BTreeSet<String> = BTreeSet::new();
    let mut busy: Vec<(String, u64)> = Vec::new();
    let mut seed = 7u64;
    let mut next_id = 0u32;
    let mut now = 100u64;
    for _ in 0..3000 {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        match (seed >> 33) % 5 {
            0 | 1 => {
                next_id += 1;
                index.insert(&id(next_id), None, u64::from(next_id), 0, 0, now);
                ready.insert(id(next_id));
            }
            2 => {
                if let Some(picked) = index.pick() {
                    assert!(ready.remove(&picked));
                    index.deliver(&picked, now + 50, now, 1);
                    busy.push((picked, now + 50));
                }
            }
            3 => {
                now += 60;
                for back in index.release_due(now) {
                    ready.insert(back);
                }
                busy.retain(|(_, until)| *until > now);
            }
            _ => {
                if let Some(oldest) = ready.first().cloned() {
                    index.remove(&oldest);
                    ready.remove(&oldest);
                }
            }
        }
        index.refill();
        assert_eq!(index.counts().0, ready.len());
        assert!(index.window_len() <= 6);
        if let Some(oldest) = ready.first() {
            assert_eq!(
                index.ready_ids().first(),
                Some(oldest),
                "o mais antigo está na lista"
            );
        }
    }
}

/// UC: UC-INV-022
#[test]
fn the_invisible_list_is_ordered_by_deadline_with_a_ceiling_and_found_by_id() {
    let mut index = bounded(usize::MAX, 3, 0);
    for (n, until) in [(1, 500), (2, 100), (3, 300), (4, 200), (5, 400)] {
        index.insert(&id(n), None, 1, until, 0, 0);
    }
    assert_eq!(index.invisible_list_len(), 3);
    assert_eq!(
        index.invisible_pairs(),
        [(100, id(2)), (200, id(4)), (300, id(3))],
        "as três mais próximas, por prazo"
    );
    assert_eq!(
        index.counts().1,
        5,
        "o contador conta também as que estão na tabela"
    );
    assert!(index.is_processing(&id(1)) && index.is_processing(&id(2)));
    assert_eq!(index.next_visible(), Some(100));
}

/// UC: UC-INV-023
#[test]
fn with_a_full_list_an_earlier_deadline_enters_and_the_last_goes_to_the_table() {
    let mut index = bounded(usize::MAX, 3, 0);
    for (n, until) in [(1, 10), (2, 20), (3, 30)] {
        index.insert(&id(n), None, 1, until, 0, 0);
    }
    assert!(index.take_spill_ops().is_empty());

    // Later than the last: straight to the table.
    index.insert(&id(4), None, 1, 40, 0, 0);
    assert_eq!(index.take_spill_ops(), [SpillOp::Put(40, id(4))]);
    assert_eq!(
        index.invisible_pairs(),
        [(10, id(1)), (20, id(2)), (30, id(3))]
    );

    // Earlier than the last: it enters and the last goes to the table.
    index.insert(&id(5), None, 1, 15, 0, 0);
    assert_eq!(index.take_spill_ops(), [SpillOp::Put(30, id(3))]);
    assert_eq!(
        index.invisible_pairs(),
        [(10, id(1)), (15, id(5)), (20, id(2))]
    );
}

/// UC: UC-INV-021
#[test]
fn when_the_list_empties_with_pairs_in_the_table_the_reload_is_asked_for_before_sleeping() {
    let mut index = bounded(usize::MAX, 2, 0);
    let mut table = Table::default();
    for (n, until) in [(1, 10), (2, 20), (3, 30), (4, 40)] {
        index.insert(&id(n), None, 1, until, 0, 0);
    }
    table.apply(&mut index);
    assert_eq!(
        index.reload_wanted(),
        0,
        "a lista tem entradas: não consulta a tabela"
    );
    assert_eq!(index.release_due(25), [id(1), id(2)]);
    assert_eq!(index.next_visible(), None, "a lista esvaziou");
    assert_eq!(
        index.reload_wanted(),
        2,
        "ainda há linhas na tabela: recarrega antes de dormir"
    );
    assert_eq!(table.reload(&mut index), 2);
    assert_eq!(index.next_visible(), Some(30));
}

/// UC: UC-INV-017
#[test]
fn the_head_is_taken_out_and_checked_and_a_pair_that_is_not_the_messages_is_dropped() {
    let mut index = bounded(usize::MAX, 4, 0);
    index.insert(&id(1), None, 1, 100, 0, 0);
    index.insert(&id(2), None, 1, 200, 0, 0);
    assert_eq!(index.next_visible(), Some(100));
    assert!(
        index.release_due(99).is_empty(),
        "antes do prazo nada acontece"
    );
    assert_eq!(index.release_due(100), [id(1)]);
    assert_eq!(index.next_visible(), Some(200));
    assert!(!index.is_processing(&id(1)));

    // Pairs in the table that no longer match the metadata are discarded on the reload.
    let mut index = bounded(usize::MAX, 4, 0);
    index.insert(&id(3), None, 1, 300, 0, 0);
    index.insert(&id(4), None, 1, 400, 0, 0);
    index.remove(&id(4));
    index.set_until(&id(3), 350, 0);
    let loaded = index.reload(vec![(300, id(3)), (400, id(4))]);
    assert_eq!(loaded, 0, "o prazo mudou, ou a mensagem acabou");
    assert_eq!(
        index.take_spill_ops(),
        [SpillOp::Delete(300, id(3)), SpillOp::Delete(400, id(4))],
        "as linhas lidas são apagadas mesmo quando descartadas"
    );
}

/// UC: UC-INV-018
#[test]
fn after_the_check_the_message_joins_the_ready_list_by_its_position_and_the_middle_leaves() {
    let mut index = ready_index(4, 10);
    index.deliver(&id(2), 7_000, 100, 1);
    index.refill();
    assert_eq!(index.ready_ids(), [id(1), id(3), id(9), id(10)]);
    index.deliver(&id(1), 8_000, 100, 1);
    index.refill();
    assert_eq!(index.release_due(7_000), [id(2)]);
    // Near the oldest end: it takes its place among the oldest and the border of the side leaves.
    assert!(index.ready_ids().contains(&id(2)));
    assert_eq!(index.ready_ids().len(), 4);
    assert_eq!(index.counts().0, 9);
}

/// UC: UC-INV-024
/// UC: UC-INV-025
/// UC: UC-INV-026
#[test]
fn the_list_always_expires_before_the_table_and_the_table_follows_every_change() {
    let mut index = bounded(usize::MAX, 5, 2);
    let mut table = Table::default();
    let mut seed = 99u64;
    let mut live: Vec<(String, u64)> = Vec::new();
    let mut next_id = 0u32;
    let mut now = 0u64;
    for step in 0..4000 {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let roll = (seed >> 33) % 6;
        let until = now + 1 + (seed >> 40) % 500;
        match roll {
            0..=2 => {
                next_id += 1;
                index.insert(&id(next_id), None, 1, until, 0, now);
                live.push((id(next_id), until));
            }
            3 if !live.is_empty() => {
                let at = (seed >> 20) as usize % live.len();
                let (who, _) = live.swap_remove(at);
                index.remove(&who);
            }
            4 if !live.is_empty() => {
                let at = (seed >> 20) as usize % live.len();
                live[at].1 = until;
                index.set_until(&live[at].0, until, now);
            }
            _ => {
                now += 40;
                for back in index.release_due(now) {
                    live.retain(|(who, _)| *who != back);
                }
                while table.reload(&mut index) > 0 {
                    for back in index.release_due(now) {
                        live.retain(|(who, _)| *who != back);
                    }
                }
            }
        }
        table.apply(&mut index);
        let list = index.invisible_pairs();
        assert!(list.len() <= 5, "passo {step}: teto");
        // Everything in the list expires before everything in the table.
        if let (Some(last), Some(first)) = (list.last(), table.0.first()) {
            assert!(
                last.0 <= first.0,
                "passo {step}: a lista vence antes da tabela"
            );
        }
        // The table holds exactly the pairs that are not in the list: no stale entry, no lazy removal.
        let mut expected: BTreeSet<(u64, String)> = live
            .iter()
            .map(|(who, until)| (*until, who.clone()))
            .collect();
        for pair in &list {
            assert!(
                expected.remove(pair),
                "passo {step}: par da lista sem mensagem"
            );
        }
        assert_eq!(table.0, expected, "passo {step}: tabela = o que não coube");
        assert_eq!(index.counts().1, live.len());
    }
}

/// UC: UC-INV-025
#[test]
fn the_reload_starts_below_the_threshold_reads_the_earliest_and_deletes_the_rows_read() {
    let mut index = bounded(usize::MAX, 4, 2);
    let mut table = Table::default();
    for n in 1..=8 {
        index.insert(&id(n), None, 1, u64::from(n) * 10, 0, 0);
    }
    table.apply(&mut index);
    assert_eq!(table.0.len(), 4);
    index.remove(&id(1));
    index.remove(&id(2));
    assert_eq!(
        index.reload_wanted(),
        0,
        "duas entradas: não está abaixo do limite de 2"
    );
    index.remove(&id(3));
    assert_eq!(
        index.reload_wanted(),
        3,
        "abaixo do limite: lê até encher a lista"
    );
    table.apply(&mut index);
    assert_eq!(table.reload(&mut index), 3);
    assert_eq!(
        index.invisible_pairs(),
        [(40, id(4)), (50, id(5)), (60, id(6)), (70, id(7))]
    );
    assert_eq!(table.0.len(), 1, "as linhas lidas foram apagadas");
}

/// UC: UC-INV-028
#[test]
fn with_room_in_the_list_nothing_is_written_beyond_the_metadata() {
    let mut index = bounded(usize::MAX, 10, 0);
    for n in 1..=5 {
        index.insert(&id(n), None, 1, u64::from(n) * 10, 0, 0);
    }
    index.set_until(&id(1), 99, 0);
    index.remove(&id(2));
    index.set_until(&id(3), 0, 0);
    assert!(index.take_spill_ops().is_empty());
}

/// UC: UC-INV-020
#[test]
fn on_a_fifo_queue_the_ready_list_has_no_ceiling_and_no_middle_rule() {
    let mut index = TopicIndex::new(IndexRules {
        fifo: true,
        ready_cap: 1,
        ..IndexRules::default()
    });
    for (n, group) in [(1, "a"), (2, "b"), (3, "c"), (4, "d")] {
        index.insert(&id(n), Some(group), u64::from(n), 0, 0, 100);
    }
    assert_eq!(index.counts().0, 4);
    index.deliver(&id(1), 5_000, 100, 1);
    assert_eq!(index.release_due(5_000), [id(1)]);
    // Every free group keeps its head, whatever the size configured for the list.
    let mut heads = Vec::new();
    while let Some(next) = index.pick() {
        index.deliver(&next, 1_000_000, 5_000, 1);
        heads.push(next);
    }
    assert_eq!(heads, [id(1), id(2), id(3), id(4)]);
}

/// A bounded index chooses exactly what a complete one chooses: the lists are only an
/// implementation detail of the same fair delivery.
#[test]
fn a_bounded_index_chooses_the_same_messages_as_a_complete_one() {
    for seed in 1..=32u64 {
        let ready_cap = [1, 2, 4, 9][seed as usize % 4];
        let invisible_cap = [1, 3, 5][seed as usize % 3];
        let rules = IndexRules {
            normal_every: 3,
            ..IndexRules::default()
        };
        let mut full = TopicIndex::new(rules);
        let mut small = TopicIndex::new(IndexRules {
            ready_cap,
            invisible_cap,
            reload_threshold: invisible_cap / 2,
            ..rules
        });
        let mut table = Table::default();
        let mut state = seed;
        let mut next_id = 0u32;
        let mut now = 100u64;
        for step in 0..1500 {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let roll = (state >> 33) % 7;
            let group = format!("g{}", (state >> 45) % 5);
            match roll {
                0..=2 => {
                    next_id += 1;
                    for index in [&mut full, &mut small] {
                        index.insert(&id(next_id), Some(&group), u64::from(next_id), 0, 0, now);
                    }
                }
                3 | 4 => {
                    let (a, b) = (full.pick(), small.pick());
                    assert_eq!(a, b, "semente {seed}, passo {step}");
                    if let Some(picked) = a {
                        let until = now + 30 + (state >> 40) % 200;
                        for index in [&mut full, &mut small] {
                            index.deliver(&picked, until, now, 1);
                        }
                    }
                }
                5 => {
                    now += 70;
                    let mut a = full.release_due(now);
                    let mut b = small.release_due(now);
                    while table.reload(&mut small) > 0 {
                        b.extend(small.release_due(now));
                    }
                    a.sort();
                    b.sort();
                    assert_eq!(a, b, "semente {seed}, passo {step}");
                }
                _ => {
                    let ids = full.ids();
                    if !ids.is_empty() {
                        let victim = &ids[(state >> 20) as usize % ids.len()];
                        full.remove(victim);
                        small.remove(victim);
                    }
                }
            }
            table.apply(&mut small);
            small.refill();
            assert_eq!(full.counts(), small.counts());
            assert!(small.window_len() <= ready_cap && small.invisible_list_len() <= invisible_cap);
        }
    }
}
