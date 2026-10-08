use super::*;

fn rules(weight: f64, max_groups: Option<usize>, normal_every: u32) -> IndexRules {
    IndexRules {
        ready_weight: weight,
        max_groups,
        normal_every,
        fifo: false,
    }
}

fn index() -> TopicIndex {
    TopicIndex::new(rules(0.1, None, 0))
}

fn named(name: &str) -> GroupKey {
    GroupKey::Named(name.to_owned())
}

/// Entrega o que o `pick` escolher, como o `GET` faz, até acabar.
fn drain_order(index: &mut TopicIndex, now: u64) -> Vec<String> {
    let mut order = Vec::new();
    while let Some(id) = index.pick() {
        let deliveries = index.deliveries(&id).unwrap() + 1;
        index.deliver(&id, now + 1_000_000, now, deliveries);
        order.push(id);
    }
    order
}

/// UC: UC-GRP-006
#[test]
fn the_band_is_the_floor_of_log2_with_a_minimum_size_of_one() {
    for (size, expected) in [
        (0.0, 0),
        (0.5, 0),
        (1.0, 0),
        (1.9, 0),
        (2.0, 1),
        (3.9, 1),
        (4.0, 2),
        (7.9, 2),
        (8.0, 3),
        (1000.0, 9),
    ] {
        assert_eq!(band(size), expected, "tamanho {size}");
    }
}

/// UC: UC-GRP-001
#[test]
fn messages_without_a_group_are_one_group_of_the_empty_name() {
    let mut index = index();
    index.insert("a", None, 1, 0, 0, 100);
    index.insert("b", None, 2, 0, 0, 100);
    assert_eq!(index.group_count(), 1);
    assert_eq!(index.group_counts(&named("")), Some((2, 0)));
}

/// UC: UC-GRP-004
#[test]
fn the_counters_follow_publish_delivery_expiry_ack_and_dead_letter() {
    let mut index = index();
    let g = named("g");
    index.insert("a", Some("g"), 1, 0, 0, 100); // publicação: +1 pronta
    index.insert("b", Some("g"), 2, 0, 0, 100);
    assert_eq!(index.group_counts(&g), Some((2, 0)));
    assert_eq!(index.counts(), (2, 0));

    index.deliver("a", 5_000, 100, 1); // entrega: pronta vira em processamento
    assert_eq!(index.group_counts(&g), Some((1, 1)));
    assert_eq!(index.counts(), (1, 1));
    assert_eq!(index.next_visible(), Some(5_000));

    assert_eq!(index.release_due(4_999), Vec::<String>::new());
    assert_eq!(
        index.release_due(5_000),
        ["a"],
        "vencimento: volta a pronta"
    );
    assert_eq!(index.group_counts(&g), Some((2, 0)));
    assert_eq!(index.next_visible(), None);

    index.deliver("a", 9_000, 5_000, 2);
    index.set_until("a", 5_000, 5_000); // `PUT visibility` com 0: volta a pronta
    assert_eq!(index.group_counts(&g), Some((2, 0)));
    index.set_until("b", 8_000, 5_000); // `PUT` com prazo: fica em processamento de novo
    assert_eq!(index.group_counts(&g), Some((1, 1)));

    assert!(
        index.remove("b"),
        "ack: desconta o que estava em processamento"
    );
    assert_eq!(index.group_counts(&g), Some((1, 0)));
    assert!(index.remove("a"), "ttl ou dead-letter: desconta a pronta");
    assert_eq!(index.group_counts(&g), None, "grupo vazio some");
    assert!(index.is_empty());
    assert!(!index.remove("a"));
}

/// UC: UC-GRP-005
#[test]
fn the_group_size_weighs_ready_messages_by_the_configured_weight() {
    for (weight, expected_band) in [(0.0, 0), (0.1, 3), (1.0, 6)] {
        let mut index = TopicIndex::new(rules(weight, None, 0));
        for n in 0..100 {
            index.insert(&format!("{n:03}"), Some("g"), n, 0, 0, 0);
        }
        // 100 prontas: tamanho 0 (peso 0), 10 (peso 0,1) ou 100 (peso 1).
        assert_eq!(
            index.group_band(&named("g")),
            Some(expected_band),
            "peso {weight}"
        );
    }
    // Cada mensagem entregue sobe o tamanho em `1 − peso`.
    let mut index = TopicIndex::new(rules(0.5, None, 0));
    for n in 0..8 {
        index.insert(&format!("{n}"), Some("g"), n, 0, 0, 0);
    }
    assert_eq!(index.group_band(&named("g")), Some(2)); // 0,5 × 8 = 4
    for n in 0..4 {
        index.deliver(&format!("{n}"), 1_000, 0, 1);
    }
    // 4 em processamento + 0,5 × 4 prontas = 6.
    assert_eq!(index.group_band(&named("g")), Some(2));
    index.deliver("4", 1_000, 0, 1);
    index.deliver("5", 1_000, 0, 1);
    // 6 + 0,5 × 2 = 7.
    assert_eq!(index.group_band(&named("g")), Some(2));
    index.deliver("6", 1_000, 0, 1);
    // 7 + 0,5 = 7,5.
    index.deliver("7", 1_000, 0, 1);
    assert_eq!(
        index.group_band(&named("g")),
        Some(3),
        "8 em processamento: faixa 3"
    );
    assert_eq!(index.group_counts(&named("g")), Some((0, 8)));
}

/// UC: UC-GRP-007, UC-GRP-008, UC-GRP-010
#[test]
fn the_small_group_is_served_before_the_big_one_is_exhausted() {
    let mut index = index();
    // Os 100 do barulhento chegam primeiro, depois os 5 do pequeno.
    for n in 0..100 {
        index.insert(&format!("a{n:03}"), Some("barulhento"), n, 0, 0, 0);
    }
    for n in 0..5 {
        index.insert(&format!("b{n}"), Some("pequeno"), 1000 + n, 0, 0, 0);
    }
    let first_five: Vec<String> = (0..5)
        .map(|_| {
            let id = index.pick().unwrap();
            index.deliver(&id, 1_000_000, 0, 1);
            id
        })
        .collect();
    assert!(
        first_five.iter().all(|id| id.starts_with('b')),
        "as 5 do grupo pequeno saem antes de qualquer do grande: {first_five:?}"
    );
    assert_eq!(index.group_counts(&named("barulhento")), Some((100, 0)));
}

/// UC: UC-GRP-009
#[test]
fn the_band_is_checked_again_at_every_pick() {
    let mut index = TopicIndex::new(rules(1.0, None, 0));
    for n in 0..4 {
        index.insert(&format!("a{n}"), Some("a"), n, 0, 0, 0);
        index.insert(&format!("b{n}"), Some("b"), 10 + n, 0, 0, 0);
    }
    // Os dois grupos empatam; entregando de um, a faixa dele sobe e o outro passa à frente.
    let mut groups = Vec::new();
    for _ in 0..8 {
        let id = index.pick().unwrap();
        groups.push(id[..1].to_owned());
        index.deliver(&id, 1_000_000, 0, 1);
    }
    let a = groups.iter().filter(|g| *g == "a").count();
    assert_eq!(a, 4);
    let changes = groups.windows(2).filter(|w| w[0] != w[1]).count();
    assert!(
        changes >= 3,
        "alterna entre os dois à medida que a faixa muda: {groups:?}"
    );
}

/// UC: UC-CNS-013
#[test]
fn inside_a_band_the_oldest_and_the_newest_alternate_like_an_accordion() {
    let mut index = index();
    for (n, id) in ["a", "b", "c", "d", "e"].into_iter().enumerate() {
        index.insert(id, None, n as u64, 0, 0, 0);
    }
    assert_eq!(drain_order(&mut index, 0), ["a", "e", "b", "d", "c"]);
}

/// UC: UC-GRP-011
#[test]
fn every_nth_delivery_takes_the_oldest_of_any_group() {
    let mut index = TopicIndex::new(rules(0.1, None, 3));
    for n in 0..100 {
        index.insert(&format!("a{n:02}"), Some("grande"), n, 0, 0, 0);
    }
    for n in 0..30 {
        index.insert(&format!("b{n:02}"), Some("pequeno"), 100 + n, 0, 0, 0);
    }
    let mut order = Vec::new();
    for _ in 0..9 {
        let id = index.pick().unwrap();
        index.deliver(&id, 1_000_000, 0, 1);
        order.push(id);
    }
    // A 3ª, 6ª e 9ª entregas ignoram a faixa e levam as mais antigas da lista (o grupo grande).
    for (position, expected) in [(2, "a00"), (5, "a01"), (8, "a02")] {
        assert_eq!(order[position], expected, "{order:?}");
    }
    assert!(order.iter().filter(|id| id.starts_with('b')).count() >= 5);
}

/// UC: UC-GRP-011
#[test]
fn with_normal_every_zero_the_guarantee_is_off() {
    let mut index = TopicIndex::new(rules(0.1, None, 0));
    for n in 0..100 {
        index.insert(&format!("a{n:02}"), Some("grande"), n, 0, 0, 0);
    }
    for n in 0..20 {
        index.insert(&format!("b{n:02}"), Some("pequeno"), 100 + n, 0, 0, 0);
    }
    let order: Vec<String> = (0..5)
        .map(|_| {
            let id = index.pick().unwrap();
            index.deliver(&id, 1_000_000, 0, 1);
            id
        })
        .collect();
    assert!(order.iter().all(|id| id.starts_with('b')), "{order:?}");
}

/// UC: UC-GRP-012
#[test]
fn groups_above_the_ceiling_share_one_counter() {
    let mut index = TopicIndex::new(rules(0.1, Some(2), 0));
    for (n, group) in ["g1", "g2", "g3", "g4", "g1"].into_iter().enumerate() {
        index.insert(&format!("m{n}"), Some(group), n as u64, 0, 0, 0);
    }
    assert_eq!(index.group_count(), 3, "g1, g2 e o contador compartilhado");
    assert_eq!(index.group_counts(&named("g1")), Some((2, 0)));
    assert_eq!(index.group_counts(&GroupKey::Overflow), Some((2, 0)));
    assert_eq!(index.entry_group("m2"), Some(&GroupKey::Overflow));
    assert_eq!(index.entry_group("m3"), Some(&GroupKey::Overflow));

    // Quando um grupo acompanhado esvazia, o lugar volta a ficar livre.
    index.remove("m1");
    assert_eq!(index.group_counts(&named("g2")), None);
    index.insert("m5", Some("g5"), 5, 0, 0, 0);
    assert_eq!(index.entry_group("m5"), Some(&named("g5")));
}

/// UC: UC-RET-001
#[test]
fn messages_past_their_ttl_are_found_by_age() {
    let mut index = index();
    index.insert("velha", None, 1_000, 0, 0, 5_000);
    index.insert("nova", None, 4_000, 0, 0, 5_000);
    index.insert("em-voo", None, 500, 9_000, 1, 5_000);
    assert_eq!(index.expired(5_000, 10_000), Vec::<String>::new());
    assert_eq!(
        index.expired(6_000, 5_000),
        ["em-voo", "velha"],
        "qualquer estado, por idade"
    );
    assert_eq!(index.expired(9_000, 5_000), ["em-voo", "velha", "nova"]);
    assert_eq!(
        index.expired(100, 5_000),
        Vec::<String>::new(),
        "sem estourar a conta"
    );
}

/// UC: UC-INV-004
#[test]
fn an_inserted_message_with_a_future_deadline_starts_in_processing() {
    let mut index = index();
    index.insert("a", Some("g"), 1, 10_000, 3, 5_000);
    assert_eq!(index.counts(), (0, 1));
    assert_eq!(index.deliveries("a"), Some(3));
    assert!(!index.has_ready());
    assert_eq!(index.pick(), None);
    assert_eq!(index.release_due(10_000), ["a"]);
    assert!(index.has_ready());
    assert_eq!(index.ready_ids(), ["a"]);

    // Inserir de novo o mesmo ID troca o que havia, sem contar duas vezes.
    index.insert("a", Some("h"), 1, 0, 0, 10_000);
    assert_eq!(index.len(), 1);
    assert_eq!(index.group_counts(&named("g")), None);
    assert_eq!(index.group_counts(&named("h")), Some((1, 0)));
}

fn fifo_index() -> TopicIndex {
    TopicIndex::new(IndexRules {
        ready_weight: 0.1,
        max_groups: Some(1),
        normal_every: 3,
        fifo: true,
    })
}

/// UC: UC-FIF-020, UC-FIF-019
#[test]
fn the_fifo_list_has_one_entry_per_free_group_the_oldest_head_first() {
    let mut index = fifo_index();
    // Os grupos "a" e "b" intercalados; "c" tem muitas mensagens. Faixas e `normal-every` não
    // valem, e o teto de grupos também não (um grupo por contador só os misturaria).
    index.insert("1-a", Some("a"), 1, 0, 0, 0);
    index.insert("2-b", Some("b"), 2, 0, 0, 0);
    index.insert("3-a", Some("a"), 3, 0, 0, 0);
    for n in 0..50 {
        index.insert(&format!("5-c{n:02}"), Some("c"), 5 + n, 0, 0, 0);
    }
    assert_eq!(index.group_count(), 3);

    assert_eq!(index.pick().as_deref(), Some("1-a"));
    index.deliver("1-a", 1_000, 0, 1);
    // O grupo "a" ficou ocupado: some da lista, mesmo com "3-a" pronta.
    assert_eq!(index.pick().as_deref(), Some("2-b"));
    index.deliver("2-b", 1_000, 0, 1);
    assert_eq!(index.pick().as_deref(), Some("5-c00"));
    index.deliver("5-c00", 1_000, 0, 1);
    assert_eq!(index.pick(), None, "todos os grupos estão ocupados");
    assert!(!index.has_ready());
}

/// UC: UC-FIF-016, UC-FIF-018, UC-FIF-022
#[test]
fn a_group_is_freed_by_the_end_of_the_invisibility_the_ack_or_the_expiry_of_the_head() {
    let mut index = fifo_index();
    index.insert("1", Some("g"), 1, 0, 0, 0);
    index.insert("2", Some("g"), 2, 0, 0, 0);
    index.deliver("1", 100, 0, 1);
    assert_eq!(index.pick(), None);

    // Vencida a invisibilidade, a mesma cabeça volta.
    assert_eq!(index.release_due(100), ["1"]);
    assert_eq!(index.pick().as_deref(), Some("1"));
    index.deliver("1", 200, 100, 2);
    assert_eq!(index.pick(), None);

    // O ack da cabeça libera a seguinte.
    index.remove("1");
    assert_eq!(index.pick().as_deref(), Some("2"));
    index.deliver("2", 300, 200, 1);
    // A cabeça que vence por `ttl` também libera o grupo.
    index.insert("3", Some("g"), 3, 0, 0, 200);
    assert_eq!(index.pick(), None);
    index.remove("2");
    assert_eq!(index.pick().as_deref(), Some("3"));
}

/// UC: UC-FIF-014
#[test]
fn with_a_zero_visibility_the_group_is_free_again_after_the_batch() {
    let mut index = fifo_index();
    index.insert("1", Some("g"), 1, 0, 0, 0);
    index.insert("2", Some("g"), 2, 0, 0, 0);
    let id = index.pick().unwrap();
    // Dentro do lote a mensagem fica em processamento ao menos até o instante seguinte...
    index.deliver(&id, 1, 0, 1);
    assert_eq!(index.pick(), None, "nunca duas do mesmo grupo no lote");
    // ...e, com a invisibilidade zero, volta a pronta no fim.
    index.set_until(&id, 0, 0);
    assert_eq!(index.pick().as_deref(), Some("1"));
}
