//! O anel de hash circular e os blocos de mensagem (doc 05, seção 7.1). Lógica pura: dado só
//! o conjunto de IDs de nós e o número de nós virtuais, qualquer nó chega ao mesmo primário e
//! às mesmas réplicas para a mesma posição (UC-ANE-002).
//!
//! Uma **posição** é um `u64` (os 8 primeiros bytes do SHA-256 da chave). Cada nó físico ocupa
//! `ring.virtual-nodes` posições no anel. Um **bloco** é o arco que termina numa posição de nó
//! virtual: a posição `p` pertence ao primeiro ponto com valor `>= p`, dando a volta ao fim do
//! anel. O identificador do bloco (`BlockId`) é a posição desse ponto, em 16 dígitos
//! hexadecimais, e é o `{id}` das rotas `/cluster/blocks/{id}/...` (doc 06c).

use std::fmt;
use std::str::FromStr;

use sha2::{Digest, Sha256};

/// A posição de uma chave no anel.
pub fn position(key: &[u8]) -> u64 {
    let digest = Sha256::digest(key);
    let mut head = [0u8; 8];
    head.copy_from_slice(&digest[..8]);
    u64::from_be_bytes(head)
}

/// A posição de uma mensagem: o hash do ID (GUID v7). O hash desfaz a ordem do tempo que
/// os bits altos do GUID carregam, então as mensagens se espalham por todo o anel
/// (UC-ANE-005).
pub fn message_position(message_id: &str) -> u64 {
    position(message_id.as_bytes())
}

/// A posição de um grupo de fila FIFO: `hash(id do tópico + ":" + groupId)`. Todas as mensagens
/// do grupo caem no mesmo bloco (UC-FIF-023, UC-ANE-009).
pub fn fifo_position(topic_id: &str, group_id: &str) -> u64 {
    position(format!("{topic_id}:{group_id}").as_bytes())
}

/// A posição de um tópico: o hash do `id` dele. Quem a ocupa é o nó gestor de uma fila volátil
/// (doc 15, seção 14.5).
pub fn topic_position(topic_id: &str) -> u64 {
    position(topic_id.as_bytes())
}

/// O identificador de um bloco: a posição do nó virtual que o fecha.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BlockId(pub u64);

impl fmt::Display for BlockId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:016x}", self.0)
    }
}

impl FromStr for BlockId {
    type Err = ();

    /// Só a forma que o `Display` escreve: 16 dígitos hexadecimais minúsculos.
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
    /// Índice em `Ring.nodes`.
    node: usize,
}

/// O anel de um conjunto de nós.
#[derive(Debug, Clone, Default)]
pub struct Ring {
    /// Os IDs dos nós, em ordem; a ordem não depende da de entrada.
    nodes: Vec<String>,
    /// Os nós virtuais, ordenados por posição.
    points: Vec<Point>,
    virtual_nodes: u32,
}

impl Ring {
    /// Monta o anel. A ordem dos nós e as repetições não mudam o resultado.
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
        // Duas posições iguais seriam o mesmo bloco; fica a do nó de menor ID (`nodes` já é
        // ordenado, e a ordenação é estável pelo índice).
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

    /// Quantos nós físicos.
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub fn virtual_nodes(&self) -> u32 {
        self.virtual_nodes
    }

    /// Os IDs dos nós, em ordem.
    pub fn nodes(&self) -> impl Iterator<Item = &str> {
        self.nodes.iter().map(String::as_str)
    }

    /// Os blocos do anel, em ordem de posição.
    pub fn blocks(&self) -> impl Iterator<Item = BlockId> + '_ {
        self.points.iter().map(|p| BlockId(p.position))
    }

    pub fn block_count(&self) -> usize {
        self.points.len()
    }

    /// O índice do primeiro ponto com valor `>= position`, dando a volta ao fim do anel.
    fn start_index(&self, position: u64) -> Option<usize> {
        if self.points.is_empty() {
            return None;
        }
        let index = self.points.partition_point(|p| p.position < position);
        Some(if index == self.points.len() { 0 } else { index })
    }

    /// O bloco a que a posição pertence.
    pub fn block_of(&self, position: u64) -> Option<BlockId> {
        self.start_index(position)
            .map(|i| BlockId(self.points[i].position))
    }

    /// Os nós responsáveis pela posição, em ordem (primário, secundário, terciário...): nós
    /// físicos **distintos**, pulando os nós virtuais de um nó já escolhido (UC-ANE-003). Há no
    /// máximo `count`, e menos se o anel tem menos nós.
    pub fn owners(&self, position: u64, count: usize) -> Vec<&str> {
        let Some(start) = self.start_index(position) else {
            return Vec::new();
        };
        self.owners_from(start, count)
    }

    /// Os nós responsáveis por um bloco. `None` se o bloco não existe neste anel.
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

    /// O primário da posição: o dono da entrega das mensagens dela (doc 13, seção 12.7).
    pub fn primary(&self, position: u64) -> Option<&str> {
        self.owners(position, 1).into_iter().next()
    }

    /// Quantos blocos o nó tem como primário.
    pub fn primary_blocks_of(&self, node_id: &str) -> usize {
        let Ok(index) = self.nodes.binary_search_by(|n| n.as_str().cmp(node_id)) else {
            return 0;
        };
        self.points.iter().filter(|p| p.node == index).count()
    }
}

#[cfg(test)]
mod tests;
