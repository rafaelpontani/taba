//! As mensagens que o nó guarda mas de que **não** é o dono da entrega (doc 13, seção 12.7): as
//! réplicas. Só o primário do bloco coloca a mensagem nas listas de prontos e de invisíveis; as
//! réplicas ficam aqui, só com o `id` e a idade, para o `ttl` valer nelas também (cada nó apaga as
//! próprias cópias, doc 13, seção 12.10) e para o nó saber o que tem quando passa a ser o dono
//! (`Messages::reown`).

use std::collections::{BTreeSet, HashMap};

#[derive(Debug, Default)]
pub struct ReplicaSet {
    /// `id` → idade (o instante de onde o `ttl` conta).
    by_id: HashMap<String, u64>,
    by_age: BTreeSet<(u64, String)>,
}

impl ReplicaSet {
    pub fn insert(&mut self, id: &str, age_ms: u64) {
        self.remove(id);
        self.by_id.insert(id.to_owned(), age_ms);
        self.by_age.insert((age_ms, id.to_owned()));
    }

    pub fn remove(&mut self, id: &str) -> bool {
        match self.by_id.remove(id) {
            Some(age) => {
                self.by_age.remove(&(age, id.to_owned()));
                true
            }
            None => false,
        }
    }

    pub fn contains(&self, id: &str) -> bool {
        self.by_id.contains_key(id)
    }

    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }

    #[cfg(test)]
    pub fn age_of(&self, id: &str) -> Option<u64> {
        self.by_id.get(id).copied()
    }

    /// Os `id`, em ordem de idade.
    pub fn ids(&self) -> Vec<String> {
        self.by_age.iter().map(|(_, id)| id.clone()).collect()
    }

    /// As que passaram do `ttl`: `idade + ttl ≤ agora`.
    pub fn expired(&self, now_ms: u64, ttl_ms: u64) -> Vec<String> {
        let Some(limit) = now_ms.checked_sub(ttl_ms) else {
            return Vec::new();
        };
        self.by_age
            .iter()
            .take_while(|(age, _)| *age <= limit)
            .map(|(_, id)| id.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests;
