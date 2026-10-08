//! A taxa de publicação e de leitura de um tópico (doc 02, seção 3.2; doc 11): a média móvel
//! das mensagens por segundo na janela `memory.rate.window` (5 minutos). A taxa de leitura
//! decide quais tópicos perdem a memória primeiro quando ela enche.
//!
//! Guarda um balde por segundo e descarta o que saiu da janela, então o custo é fixo por tópico.

use std::collections::VecDeque;

/// Contagens por segundo dentro de uma janela.
#[derive(Debug, Clone, Default)]
pub struct RateWindow {
    /// `(segundo, quantidade)`, em ordem de segundo.
    buckets: VecDeque<(u64, u64)>,
}

impl RateWindow {
    pub fn record(&mut self, now_ms: u64, count: u64, window_secs: u64) {
        let second = now_ms / 1000;
        self.prune(second, window_secs);
        match self.buckets.back_mut() {
            Some((s, n)) if *s == second => *n += count,
            _ => self.buckets.push_back((second, count)),
        }
    }

    fn prune(&mut self, second: u64, window_secs: u64) {
        let oldest = second.saturating_sub(window_secs.saturating_sub(1));
        while self.buckets.front().is_some_and(|(s, _)| *s < oldest) {
            self.buckets.pop_front();
        }
    }

    /// Mensagens por segundo na janela que termina em `now_ms`.
    pub fn per_second(&self, now_ms: u64, window_secs: u64) -> f64 {
        let second = now_ms / 1000;
        let oldest = second.saturating_sub(window_secs.saturating_sub(1));
        let total: u64 = self
            .buckets
            .iter()
            .filter(|(s, _)| *s >= oldest && *s <= second)
            .map(|(_, n)| *n)
            .sum();
        total as f64 / window_secs.max(1) as f64
    }
}

/// As duas taxas de um tópico.
#[derive(Debug, Clone, Default)]
pub struct Rates {
    pub published: RateWindow,
    pub consumed: RateWindow,
}

#[cfg(test)]
mod tests;
