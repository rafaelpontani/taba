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

/// Message IDs created in consecutive milliseconds, like those of a burst of publications.
fn message_ids(count: u64) -> Vec<String> {
    (0..count)
        .map(|i| id::new_at(1_791_288_000_000 + i))
        .collect()
}

/// The fraction of the positions of `ids` that each node has as primary.
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
    assert_eq!(position, message_position(&message), "the hash is stable");

    let block = ring.block_of(position).unwrap();
    let owners = ring.owners(position, 3);
    assert_eq!(owners.len(), 3);
    assert_eq!(
        ring.block_owners(block, 3).unwrap(),
        owners,
        "the position's block has the same owners, in the same order"
    );
    assert_eq!(ring.primary(position), Some(owners[0]));
}

/// UC: UC-ANE-002
#[test]
fn any_node_reaches_the_same_owners_from_the_ids_alone() {
    let (ids, ring) = ring_of(5, 32);
    // Another node receives the members in another order (and with a repetition).
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
    // Few nodes and many virtual nodes: the following positions almost always belong to the same node.
    let (_, ring) = ring_of(4, 64);
    for message in message_ids(300) {
        let owners = ring.owners(message_position(&message), 3);
        let distinct: HashSet<&str> = owners.iter().copied().collect();
        assert_eq!(owners.len(), 3);
        assert_eq!(distinct.len(), 3, "{owners:?}");
    }
    // Asking for more owners than nodes returns all the nodes, once each.
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
        "with 256 virtual nodes the variation ({fine:.3}) must be smaller than with 1 ({coarse:.3})"
    );
    assert!(fine < 0.10, "variation with 256 virtual nodes: {fine:.3}");
}

/// UC: UC-ANE-005
#[test]
fn the_hash_spreads_time_ordered_ids_over_the_whole_ring() {
    let (_, ring) = ring_of(3, 128);
    let ids = message_ids(6000);
    for (node, share) in primary_shares(&ring, &ids) {
        assert!(
            (0.20..0.47).contains(&share),
            "node {node} ended up with {share:.3} of the messages"
        );
    }
    // The same holds for the topic `id` (managing node of a volatile queue).
    let topics: Vec<String> = (0..3000u64)
        .map(|i| id::new_at(1_791_288_000_000 + i * 7))
        .collect();
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for topic in &topics {
        *counts
            .entry(ring.primary(topic_position(topic)).unwrap())
            .or_default() += 1;
    }
    assert_eq!(counts.len(), 3, "every node manages some queue");
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
    // The key is `<topic id>:<groupId>`.
    assert_eq!(
        fifo_position(&topic, "g1"),
        position(format!("{topic}:g1").as_bytes())
    );

    let (_, ring) = ring_of(4, 32);
    let owners = ring.owners(fifo_position(&topic, "g1"), 3);
    assert_eq!(
        owners,
        ring.owners(fifo_position(&topic, "g1"), 3),
        "the same group always has the same primary and the same replicas"
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
    assert!(!moved.is_empty(), "the new node takes over some blocks");
    assert!(moved.len() < sample.len() / 2, "but not the whole ring");
    for message in &moved {
        assert_eq!(
            with_new.primary(message_position(message)),
            Some(newcomer.as_str()),
            "what changes primary goes to the new node"
        );
    }

    // Without the node that failed, the ring goes back to exactly the previous one.
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
    assert_eq!(
        ring.block_of(first.0),
        Some(first),
        "the point closes the block"
    );
    assert_eq!(ring.block_of(first.0 + 1), Some(blocks[1]));
    assert_eq!(
        ring.block_of(last.0 + 1),
        Some(first),
        "it wraps around the ring"
    );
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

/// UC: UC-SIN-001
#[test]
fn the_ranges_of_the_blocks_cover_every_position_exactly_once() {
    let (_, ring) = ring_of(3, 8);
    let in_ranges = |ranges: &[(u64, u64)], position: u64| {
        ranges
            .iter()
            .any(|(from, to)| (*from..=*to).contains(&position))
    };
    for message in message_ids(200) {
        let position = message_position(&message);
        let owner = ring.block_of(position).unwrap();
        for block in ring.blocks() {
            let ranges = ring.block_ranges(block).unwrap();
            assert_eq!(
                in_ranges(&ranges, position),
                block == owner,
                "position {position} and block {block}"
            );
        }
    }
    // The ring's extremes also belong to a single block.
    for position in [0, u64::MAX] {
        let owner = ring.block_of(position).unwrap();
        let holders = ring
            .blocks()
            .filter(|b| in_ranges(&ring.block_ranges(*b).unwrap(), position))
            .collect::<Vec<_>>();
        assert_eq!(holders, vec![owner]);
    }
    assert!(ring.block_ranges(BlockId(1)).is_none());
}

/// UC: UC-SIN-001
#[test]
fn a_node_syncs_only_the_blocks_it_is_responsible_for() {
    let (ids, ring) = ring_of(4, 8);
    let rf = 3;
    for id in &ids {
        let mine = ring.blocks_of(id, rf);
        assert!(!mine.is_empty());
        for block in ring.blocks() {
            let owners = ring.block_owners(block, rf).unwrap();
            assert_eq!(
                mine.contains(&block),
                owners.contains(&id.as_str()),
                "o bloco {block}"
            );
        }
    }
    let single = Ring::new([ids[0].as_str()], 4);
    assert_eq!(single.blocks_of(&ids[0], 3).len(), single.block_count());
    let one_point = Ring::new([ids[0].as_str()], 1);
    assert_eq!(
        one_point.block_ranges(one_point.blocks().next().unwrap()),
        Some(vec![(0, u64::MAX)])
    );
}

/// UC: UC-FIF-024
#[test]
fn recreating_a_fifo_topic_moves_its_groups_in_the_ring() {
    let old = crate::core::id::new_at(1_791_288_000_000);
    let new = crate::core::id::new_at(1_791_288_000_001);
    let moved = (0..64)
        .filter(|n| fifo_position(&old, &format!("g{n}")) != fifo_position(&new, &format!("g{n}")))
        .count();
    assert_eq!(
        moved, 64,
        "another topic `id`, another position for every group"
    );
}

/// UC: UC-ESC-001
#[test]
fn the_id_alone_gives_the_block_and_the_primary_secondary_and_tertiary() {
    let (ids, ring) = ring_of(5, 32);
    // The coordinator generates the GUID v7 first...
    let message = id::new_at(1_791_288_000_123);
    assert!(id::is_guid_v7(&message));
    // ...and from it alone it reaches the block and the three responsible nodes, in order.
    let position = message_position(&message);
    let block = ring.block_of(position).unwrap();
    let owners = ring.block_owners(block, 3).unwrap();
    assert_eq!(owners.len(), 3);
    let (primary, secondary, tertiary) = (owners[0], owners[1], owners[2]);
    assert_eq!(ring.primary(position), Some(primary));
    assert_eq!(ring.owners(position, 3), vec![primary, secondary, tertiary]);
    assert!(
        primary != secondary && secondary != tertiary && primary != tertiary,
        "three distinct nodes"
    );
    assert!(
        owners.iter().all(|o| ids.iter().any(|n| n == o)),
        "all of them are members of the ring"
    );
}

/// UC: UC-RET-010
#[test]
fn the_dead_letter_copy_falls_in_the_same_block_and_nodes_as_the_original() {
    // The position of a message depends on its `id` alone, never on the queue it is in: the
    // original (in the queue) and the copy (in its dead-letter) have the same `id`, hence the same
    // block and the same primary, secondary and tertiary.
    let (_, ring) = ring_of(5, 32);
    for message in message_ids(200) {
        let original = message_position(&message);
        let dead_letter = message_position(&message);
        assert_eq!(original, dead_letter);
        assert_eq!(ring.block_of(original), ring.block_of(dead_letter));
        assert_eq!(ring.owners(original, 3), ring.owners(dead_letter, 3));
    }
}
