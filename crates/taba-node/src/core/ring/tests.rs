use std::collections::{BTreeMap, HashSet};

use super::*;
use crate::core::id;

fn node(n: u32) -> String {
    id::new_at(1_791_288_000_000 + u64::from(n))
}

fn ring_of(count: u32, virtual_nodes: u32) -> (Vec<String>, Ring) {
    let ids: Vec<String> = (0..count).map(node).collect();
    let ring = Ring::new(ids.iter().map(String::as_str), virtual_nodes);
    (ids, ring)
}

/// IDs de mensagem criados em milissegundos seguidos, como os de uma rajada de publicações.
fn message_ids(count: u64) -> Vec<String> {
    (0..count)
        .map(|i| id::new_at(1_791_288_000_000 + i))
        .collect()
}

/// A fração das posições de `ids` que cada nó tem como primário.
fn primary_shares(ring: &Ring, ids: &[String]) -> BTreeMap<String, f64> {
    let mut counts: BTreeMap<String, usize> = ring.nodes().map(|n| (n.to_owned(), 0)).collect();
    for message in ids {
        let primary = ring.primary(message_position(message)).unwrap();
        *counts.get_mut(primary).unwrap() += 1;
    }
    counts
        .into_iter()
        .map(|(n, c)| (n, c as f64 / ids.len() as f64))
        .collect()
}

/// UC: UC-ANE-001
#[test]
fn a_message_id_becomes_a_position_that_belongs_to_a_block_with_ordered_owners() {
    let (_, ring) = ring_of(4, 16);
    let message = id::new_at(1_791_288_000_123);
    let position = message_position(&message);
    assert_eq!(position, message_position(&message), "o hash é estável");

    let block = ring.block_of(position).unwrap();
    let owners = ring.owners(position, 3);
    assert_eq!(owners.len(), 3);
    assert_eq!(
        ring.block_owners(block, 3).unwrap(),
        owners,
        "o bloco da posição tem os mesmos responsáveis, na mesma ordem"
    );
    assert_eq!(ring.primary(position), Some(owners[0]));
}

/// UC: UC-ANE-002
#[test]
fn any_node_reaches_the_same_owners_from_the_ids_alone() {
    let (ids, ring) = ring_of(5, 32);
    // Outro nó recebe os membros em outra ordem (e com uma repetição).
    let shuffled: Vec<&str> = [4, 2, 0, 3, 1, 2]
        .iter()
        .map(|&i| ids[i].as_str())
        .collect();
    let other = Ring::new(shuffled, 32);
    assert_eq!(other.node_count(), 5);
    for message in message_ids(200) {
        let position = message_position(&message);
        assert_eq!(ring.owners(position, 3), other.owners(position, 3));
        assert_eq!(ring.block_of(position), other.block_of(position));
    }
}

/// UC: UC-ANE-003
#[test]
fn owners_are_distinct_physical_nodes() {
    // Poucos nós e muitos nós virtuais: as posições seguintes são quase sempre do mesmo nó.
    let (_, ring) = ring_of(4, 64);
    for message in message_ids(300) {
        let owners = ring.owners(message_position(&message), 3);
        let distinct: HashSet<&str> = owners.iter().copied().collect();
        assert_eq!(owners.len(), 3);
        assert_eq!(distinct.len(), 3, "{owners:?}");
    }
    // Pedir mais responsáveis que nós devolve todos os nós, uma vez cada.
    let (_, small) = ring_of(2, 64);
    assert_eq!(small.owners(0, 3).len(), 2);
}

/// UC: UC-ANE-004
#[test]
fn more_virtual_nodes_spread_the_blocks_more_evenly() {
    let ids = message_ids(6000);
    let spread = |virtual_nodes: u32| {
        let (_, ring) = ring_of(5, virtual_nodes);
        let shares = primary_shares(&ring, &ids);
        let max = shares.values().cloned().fold(f64::MIN, f64::max);
        let min = shares.values().cloned().fold(f64::MAX, f64::min);
        max - min
    };
    let (coarse, fine) = (spread(1), spread(256));
    assert!(
        fine < coarse,
        "com 256 nós virtuais a variação ({fine:.3}) deve ser menor que com 1 ({coarse:.3})"
    );
    assert!(fine < 0.10, "variação com 256 nós virtuais: {fine:.3}");
}

/// UC: UC-ANE-005
#[test]
fn the_hash_spreads_time_ordered_ids_over_the_whole_ring() {
    let (_, ring) = ring_of(3, 128);
    let ids = message_ids(6000);
    for (node, share) in primary_shares(&ring, &ids) {
        assert!(
            (0.20..0.47).contains(&share),
            "o nó {node} ficou com {share:.3} das mensagens"
        );
    }
    // O mesmo vale para o `id` do tópico (nó gestor de uma fila volátil).
    let topics: Vec<String> = (0..3000u64)
        .map(|i| id::new_at(1_791_288_000_000 + i * 7))
        .collect();
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for topic in &topics {
        *counts
            .entry(ring.primary(topic_position(topic)).unwrap())
            .or_default() += 1;
    }
    assert_eq!(counts.len(), 3, "todos os nós gerenciam alguma fila");
}

/// UC: UC-ANE-009, UC-FIF-023
#[test]
fn a_fifo_group_has_one_position_for_all_its_messages() {
    let topic = id::new_at(1_791_288_000_000);
    let other_topic = id::new_at(1_791_288_000_001);
    assert_eq!(fifo_position(&topic, "g1"), fifo_position(&topic, "g1"));
    assert_ne!(fifo_position(&topic, "g1"), fifo_position(&topic, "g2"));
    assert_ne!(
        fifo_position(&topic, "g1"),
        fifo_position(&other_topic, "g1")
    );
    // A chave é `<id do tópico>:<groupId>`.
    assert_eq!(
        fifo_position(&topic, "g1"),
        position(format!("{topic}:g1").as_bytes())
    );

    let (_, ring) = ring_of(4, 32);
    let owners = ring.owners(fifo_position(&topic, "g1"), 3);
    assert_eq!(
        owners,
        ring.owners(fifo_position(&topic, "g1"), 3),
        "o mesmo grupo tem sempre o mesmo primário e as mesmas réplicas"
    );
}

/// UC: UC-ANE-010
#[test]
fn the_ring_is_recalculated_from_the_members_and_goes_back_without_the_failed_node() {
    let (ids, before) = ring_of(3, 64);
    let newcomer = node(99);
    let with_new = Ring::new(
        ids.iter().map(String::as_str).chain([newcomer.as_str()]),
        64,
    );
    let sample = message_ids(1000);
    let moved: Vec<&String> = sample
        .iter()
        .filter(|m| before.primary(message_position(m)) != with_new.primary(message_position(m)))
        .collect();
    assert!(!moved.is_empty(), "o nó novo assume alguns blocos");
    assert!(moved.len() < sample.len() / 2, "mas não o anel inteiro");
    for message in &moved {
        assert_eq!(
            with_new.primary(message_position(message)),
            Some(newcomer.as_str()),
            "o que muda de primário vai para o nó novo"
        );
    }

    // Sem o nó que falhou, o anel volta a ser exatamente o anterior.
    let without = Ring::new(ids.iter().map(String::as_str), 64);
    assert_eq!(
        before.blocks().collect::<Vec<_>>(),
        without.blocks().collect::<Vec<_>>()
    );
    for message in &sample {
        let position = message_position(message);
        assert_eq!(before.owners(position, 3), without.owners(position, 3));
    }
}

#[test]
fn a_position_past_the_last_point_wraps_to_the_first() {
    let (_, ring) = ring_of(3, 4);
    let blocks: Vec<BlockId> = ring.blocks().collect();
    let (first, last) = (*blocks.first().unwrap(), *blocks.last().unwrap());
    assert_eq!(ring.block_of(0), Some(first));
    assert_eq!(ring.block_of(first.0), Some(first), "o ponto fecha o bloco");
    assert_eq!(ring.block_of(first.0 + 1), Some(blocks[1]));
    assert_eq!(ring.block_of(last.0 + 1), Some(first), "dá a volta ao anel");
}

#[test]
fn block_ids_are_sixteen_lowercase_hex_digits() {
    let block = BlockId(0xAB);
    assert_eq!(block.to_string(), "00000000000000ab");
    assert_eq!("00000000000000ab".parse::<BlockId>(), Ok(block));
    for bad in [
        "",
        "ab",
        "00000000000000AB",
        "0000000000000zzz",
        "00000000000000abc",
    ] {
        assert!(bad.parse::<BlockId>().is_err(), "{bad}");
    }
    let (_, ring) = ring_of(2, 8);
    let block = ring.blocks().next().unwrap();
    assert!(ring.block_owners(block, 2).is_some());
    assert!(ring.block_owners(BlockId(block.0 + 1), 2).is_none());
}

#[test]
fn a_single_node_owns_every_block_and_an_empty_ring_owns_nothing() {
    let (ids, ring) = ring_of(1, 16);
    assert_eq!(ring.primary_blocks_of(&ids[0]), ring.block_count());
    for message in message_ids(50) {
        assert_eq!(
            ring.primary(message_position(&message)),
            Some(ids[0].as_str())
        );
        assert_eq!(
            ring.owners(message_position(&message), 3),
            vec![ids[0].as_str()]
        );
    }
    let empty = Ring::new([], 16);
    assert!(empty.is_empty());
    assert_eq!(empty.block_of(7), None);
    assert!(empty.owners(7, 3).is_empty());
    assert_eq!(empty.primary_blocks_of("x"), 0);
}
