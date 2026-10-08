//! Um histograma de baldes fixos (doc 11): o dado que o registro de métricas guarda e que o
//! `engine` usa para as latências de tópico (p50, p95 e p99) e para a espera da entrega justa.
//! Não depende de nada: só conta e soma.

/// Limites superiores, em segundos, das latências de publicação e de consumo.
pub const LATENCY_BOUNDS: &[f64] = &[
    0.0001, 0.00025, 0.0005, 0.001, 0.0025, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5,
    5.0, 10.0,
];

#[derive(Debug, Clone, PartialEq)]
pub struct Histogram {
    bounds: &'static [f64],
    /// Uma contagem por balde (não acumulada); a última é o balde `+Inf`.
    counts: Vec<u64>,
    sum: f64,
    count: u64,
}

impl Histogram {
    pub fn new(bounds: &'static [f64]) -> Self {
        Self {
            bounds,
            counts: vec![0; bounds.len() + 1],
            sum: 0.0,
            count: 0,
        }
    }

    pub fn record(&mut self, value: f64) {
        let bucket = self
            .bounds
            .iter()
            .position(|bound| value <= *bound)
            .unwrap_or(self.bounds.len());
        self.counts[bucket] += 1;
        self.sum += value;
        self.count += 1;
    }

    pub fn bounds(&self) -> &'static [f64] {
        self.bounds
    }

    pub fn count(&self) -> u64 {
        self.count
    }

    pub fn sum(&self) -> f64 {
        self.sum
    }

    /// A contagem de cada balde (não acumulada), o `+Inf` por último (o formato do OTLP).
    pub fn buckets(&self) -> &[u64] {
        &self.counts
    }

    /// As contagens acumuladas por balde, o `+Inf` por último (o formato do Prometheus).
    pub fn cumulative(&self) -> Vec<u64> {
        let mut total = 0;
        self.counts
            .iter()
            .map(|n| {
                total += n;
                total
            })
            .collect()
    }

    /// O limite do balde onde cai o percentil `p` (de 0 a 1): uma aproximação por cima, no
    /// tamanho do balde. `None` sem nenhuma observação. Quem passou do último limite recebe o
    /// último limite.
    pub fn percentile(&self, p: f64) -> Option<f64> {
        if self.count == 0 {
            return None;
        }
        let rank = ((p * self.count as f64).ceil() as u64).clamp(1, self.count);
        let mut total = 0;
        for (index, n) in self.counts.iter().enumerate() {
            total += n;
            if total >= rank {
                return Some(self.bounds[index.min(self.bounds.len() - 1)]);
            }
        }
        None
    }

    /// Soma as observações de `other`, que tem de ter os mesmos limites.
    pub fn merge(&mut self, other: &Histogram) {
        debug_assert_eq!(self.bounds, other.bounds);
        for (mine, theirs) in self.counts.iter_mut().zip(&other.counts) {
            *mine += theirs;
        }
        self.sum += other.sum;
        self.count += other.count;
    }
}

#[cfg(test)]
mod tests;
