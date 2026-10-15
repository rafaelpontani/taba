//! The circular hash ring and the message blocks (doc 05, section 7.1). Pure logic: given only
//! the set of node IDs and the number of virtual nodes, any node arrives at the same primary and
//! the same replicas for the same position (UC-ANE-002).
//!
//! A **position** is a `u64` (the first 8 bytes of the key's SHA-256). Each physical node occupies
//! `ring.virtual-nodes` positions on the ring. A **block** is the arc that ends at a virtual-node
//! position: position `p` belongs to the first point with value `>= p`, wrapping around the end of the
//! ring. The block identifier (`BlockId`) is that point's position, in 16 hexadecimal
//! digits, and is the `{id}` of the `/cluster/blocks/{id}/...` routes (doc 06c).

use std::fmt;
use std::str::FromStr;

use sha2::{Digest, Sha256};

/// The position of a key on the ring.
pub fn position(key: &[u8]) -> u64 {
    let digest = Sha256::digest(key);
    let mut head = [0u8; 8];
    head.copy_from_slice(&digest[..8]);
    u64::from_be_bytes(head)
}

/// The position of a message: the hash of the ID (GUID v7). The hash undoes the time order that
/// the GUID's high bits carry, so messages spread over the whole ring
/// (UC-ANE-005).
pub fn message_position(message_id: &str) -> u64 {
    position(message_id.as_bytes())
}

/// The position of a FIFO queue group: `hash(topic id + ":" + groupId)`. All the group's messages
/// fall in the same block (UC-FIF-023, UC-ANE-009).
pub fn fifo_position(topic_id: &str, group_id: &str) -> u64 {
    position(format!("{topic_id}:{group_id}").as_bytes())
}

/// The position of a topic: the hash of its `id`. Whoever occupies it is the managing node of a volatile queue
/// (doc 15, section 14.5).
pub fn topic_position(topic_id: &str) -> u64 {
    position(topic_id.as_bytes())
}

/// The position of a shared definition (`kind` is `topic` or `token`): the hash of
/// `kind:key`, where the key is the topic **name** (unique in the cluster) or the token `id`. Whoever
/// occupies it is the definition's owner, the source of truth about it.
pub fn definition_position(kind: &str, key: &str) -> u64 {
    position(format!("{kind}:{key}").as_bytes())
}

/// The identifier of a block: the position of the virtual node that closes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BlockId(pub u64);

impl fmt::Display for BlockId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:016x}", self.0)
    }
}

impl FromStr for BlockId {
    type Err = ();

    /// Only the form `Display` writes: 16 lowercase hexadecimal digits.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let valid = s.len() == 16
            && s.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
        if !valid {
            return Err(());
        }
        u64::from_str_radix(s, 16).map(BlockId).map_err(|_| ())
    }
}

#[derive(Debug, Clone, Copy)]
struct Point {
    position: u64,
    /// Index into `Ring.nodes`.
    node: usize,
}

/// The ring of a set of nodes.
#[derive(Debug, Clone, Default)]
pub struct Ring {
    /// The node IDs, in order; the order does not depend on the input's.
    nodes: Vec<String>,
    /// The virtual nodes, sorted by position.
    points: Vec<Point>,
    virtual_nodes: u32,
}

impl Ring {
    /// Builds the ring. The order of the nodes and repetitions do not change the result.
    pub fn new<'a>(node_ids: impl IntoIterator<Item = &'a str>, virtual_nodes: u32) -> Self {
        let mut nodes: Vec<String> = node_ids.into_iter().map(str::to_owned).collect();
        nodes.sort();
        nodes.dedup();
        let virtual_nodes = virtual_nodes.max(1);
        let mut points: Vec<Point> = Vec::with_capacity(nodes.len() * virtual_nodes as usize);
        for (index, id) in nodes.iter().enumerate() {
            for v in 0..virtual_nodes {
                points.push(Point {
                    position: position(format!("{id}#{v}").as_bytes()),
                    node: index,
                });
            }
        }
        // Two equal positions would be the same block; the one of the node with the lowest ID stays (`nodes` is already
        // sorted, and the sort is stable by index).
        points.sort_by_key(|p| (p.position, p.node));
        points.dedup_by_key(|p| p.position);
        Self {
            nodes,
            points,
            virtual_nodes,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// How many physical nodes.
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub fn virtual_nodes(&self) -> u32 {
        self.virtual_nodes
    }

    /// The node IDs, in order.
    pub fn nodes(&self) -> impl Iterator<Item = &str> {
        self.nodes.iter().map(String::as_str)
    }

    /// The ring's blocks, in position order.
    pub fn blocks(&self) -> impl Iterator<Item = BlockId> + '_ {
        self.points.iter().map(|p| BlockId(p.position))
    }

    pub fn block_count(&self) -> usize {
        self.points.len()
    }

    /// The index of the first point with value `>= position`, wrapping around the end of the ring.
    fn start_index(&self, position: u64) -> Option<usize> {
        if self.points.is_empty() {
            return None;
        }
        let index = self.points.partition_point(|p| p.position < position);
        Some(if index == self.points.len() { 0 } else { index })
    }

    /// The block the position belongs to.
    pub fn block_of(&self, position: u64) -> Option<BlockId> {
        self.start_index(position)
            .map(|i| BlockId(self.points[i].position))
    }

    /// The nodes responsible for the position, in order (primary, secondary, tertiary...): **distinct**
    /// physical nodes, skipping the virtual nodes of an already chosen node (UC-ANE-003). There are at
    /// most `count`, and fewer if the ring has fewer nodes.
    pub fn owners(&self, position: u64, count: usize) -> Vec<&str> {
        let Some(start) = self.start_index(position) else {
            return Vec::new();
        };
        self.owners_from(start, count)
    }

    /// The nodes responsible for a block. `None` if the block does not exist on this ring.
    pub fn block_owners(&self, block: BlockId, count: usize) -> Option<Vec<&str>> {
        let start = self
            .points
            .binary_search_by_key(&block.0, |p| p.position)
            .ok()?;
        Some(self.owners_from(start, count))
    }

    fn owners_from(&self, start: usize, count: usize) -> Vec<&str> {
        let wanted = count.min(self.nodes.len());
        let mut chosen: Vec<usize> = Vec::with_capacity(wanted);
        for step in 0..self.points.len() {
            if chosen.len() == wanted {
                break;
            }
            let node = self.points[(start + step) % self.points.len()].node;
            if !chosen.contains(&node) {
                chosen.push(node);
            }
        }
        chosen.into_iter().map(|i| self.nodes[i].as_str()).collect()
    }

    /// The position's primary: the owner of delivering its messages (doc 13, section 12.7).
    pub fn primary(&self, position: u64) -> Option<&str> {
        self.owners(position, 1).into_iter().next()
    }

    /// The blocks the node is responsible for in some role (one of the first `count` responsible
    /// nodes): what it needs to sync when joining (doc 05, section 7.3).
    pub fn blocks_of(&self, node_id: &str, count: usize) -> Vec<BlockId> {
        (0..self.points.len())
            .filter(|start| self.owners_from(*start, count).contains(&node_id))
            .map(|start| BlockId(self.points[start].position))
            .collect()
    }

    /// The positions the block covers, as inclusive ranges `(start, end)`: the arc between the previous
    /// point (exclusive) and the block's point (inclusive). The ring's first block wraps around and
    /// gets two ranges. `None` if the block does not exist on this ring.
    pub fn block_ranges(&self, block: BlockId) -> Option<Vec<(u64, u64)>> {
        let index = self
            .points
            .binary_search_by_key(&block.0, |p| p.position)
            .ok()?;
        let end = self.points[index].position;
        if self.points.len() == 1 {
            return Some(vec![(0, u64::MAX)]);
        }
        if index == 0 {
            let last = self.points[self.points.len() - 1].position;
            let mut ranges = vec![(0, end)];
            if last < u64::MAX {
                ranges.push((last + 1, u64::MAX));
            }
            return Some(ranges);
        }
        Some(vec![(self.points[index - 1].position + 1, end)])
    }

    /// How many blocks the node has as primary.
    pub fn primary_blocks_of(&self, node_id: &str) -> usize {
        let Ok(index) = self.nodes.binary_search_by(|n| n.as_str().cmp(node_id)) else {
            return 0;
        };
        self.points.iter().filter(|p| p.node == index).count()
    }
}

#[cfg(test)]
mod tests;
