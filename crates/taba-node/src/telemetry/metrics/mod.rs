//! O registro das métricas do nó (doc 11): uma instrumentação só, de onde `GET /metrics` (e,
//! mais tarde, o OTLP) lê. Cada métrica é uma [`Def`] com nome, tipo e unidade no vocabulário do
//! OpenTelemetry (a tabela está em `defs`); o registro guarda as séries (um valor por combinação de
//! atributos).
//!
//! Dois jeitos de alimentar: quem está no caminho da requisição soma (`add`, `observe`), e o que o
//! `engine` já conta por conta própria é copiado na hora da leitura (`set`, `set_histogram`). Não
//! há dependência do resto do nó.

mod defs;

pub use defs::*;

use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard};

use super::histogram::Histogram;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Só cresce.
    Counter,
    /// Sobe e desce e se soma entre nós.
    UpDownCounter,
    /// Um valor que não se soma (limite, idade, estado).
    Gauge,
    Histogram,
}

/// A definição de uma métrica.
#[derive(Debug)]
pub struct Def {
    pub name: &'static str,
    pub kind: Kind,
    /// Unidade UCUM: `s`, `By`, `1` (só razões) ou uma contagem entre chaves (`{message}`).
    pub unit: &'static str,
    pub help: &'static str,
    /// Só em histograma: os limites dos baldes.
    pub buckets: &'static [f64],
}

/// Os atributos de uma série, ordenados pelo nome.
pub type Labels = Vec<(String, String)>;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Number(f64),
    Histogram(Histogram),
}

/// Uma métrica com todas as suas séries, como a leitura as vê.
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub def: &'static Def,
    pub series: Vec<(Labels, Value)>,
}

#[derive(Default)]
pub struct Registry {
    families: Mutex<BTreeMap<&'static str, Family>>,
}

struct Family {
    def: &'static Def,
    series: BTreeMap<Labels, Value>,
}

fn labels_of(labels: &[(&str, &str)]) -> Labels {
    let mut out: Labels = labels
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect();
    out.sort();
    out
}

impl Registry {
    fn families(&self) -> MutexGuard<'_, BTreeMap<&'static str, Family>> {
        self.families.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn with_series(
        &self,
        def: &'static Def,
        labels: &[(&str, &str)],
        update: impl FnOnce(&mut Value),
        init: impl FnOnce() -> Value,
    ) {
        let mut families = self.families();
        let family = families.entry(def.name).or_insert_with(|| Family {
            def,
            series: BTreeMap::new(),
        });
        update(family.series.entry(labels_of(labels)).or_insert_with(init));
    }

    /// Soma `delta` à série (contador ou contador que sobe e desce).
    pub fn add(&self, def: &'static Def, labels: &[(&str, &str)], delta: f64) {
        debug_assert!(matches!(def.kind, Kind::Counter | Kind::UpDownCounter));
        self.with_series(
            def,
            labels,
            |value| {
                if let Value::Number(n) = value {
                    *n += delta;
                }
            },
            || Value::Number(0.0),
        );
    }

    /// Põe o valor da série. Serve ao medidor e ao contador que vem de uma contagem de fora.
    pub fn set(&self, def: &'static Def, labels: &[(&str, &str)], value: f64) {
        debug_assert!(def.kind != Kind::Histogram);
        self.with_series(
            def,
            labels,
            |v| *v = Value::Number(value),
            || Value::Number(0.0),
        );
    }

    /// Acrescenta uma observação ao histograma da série.
    pub fn observe(&self, def: &'static Def, labels: &[(&str, &str)], value: f64) {
        debug_assert_eq!(def.kind, Kind::Histogram);
        self.with_series(
            def,
            labels,
            |v| {
                if let Value::Histogram(h) = v {
                    h.record(value);
                }
            },
            || Value::Histogram(Histogram::new(def.buckets)),
        );
    }

    /// Troca o histograma da série por um que já vem contado.
    pub fn set_histogram(&self, def: &'static Def, labels: &[(&str, &str)], histogram: Histogram) {
        debug_assert_eq!(def.kind, Kind::Histogram);
        self.with_series(
            def,
            labels,
            |v| *v = Value::Histogram(histogram.clone()),
            || Value::Histogram(histogram.clone()),
        );
    }

    /// Apaga todas as séries da métrica (para as que são refeitas a cada leitura).
    pub fn clear(&self, def: &'static Def) {
        self.families().remove(def.name);
    }

    /// O valor de uma série numérica (para os testes).
    pub fn get(&self, def: &'static Def, labels: &[(&str, &str)]) -> Option<f64> {
        match self
            .families()
            .get(def.name)?
            .series
            .get(&labels_of(labels))?
        {
            Value::Number(n) => Some(*n),
            Value::Histogram(_) => None,
        }
    }

    /// O histograma de uma série (para os testes).
    pub fn histogram(&self, def: &'static Def, labels: &[(&str, &str)]) -> Option<Histogram> {
        match self
            .families()
            .get(def.name)?
            .series
            .get(&labels_of(labels))?
        {
            Value::Histogram(h) => Some(h.clone()),
            Value::Number(_) => None,
        }
    }

    /// Uma cópia de tudo o que há, em ordem de nome, para quem escreve a saída.
    pub fn snapshot(&self) -> Vec<Snapshot> {
        self.families()
            .values()
            .map(|family| Snapshot {
                def: family.def,
                series: family
                    .series
                    .iter()
                    .map(|(labels, value)| (labels.clone(), value.clone()))
                    .collect(),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests;
