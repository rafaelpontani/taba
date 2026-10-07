//! O relógio do nó, atrás de um trait para os testes controlarem o tempo
//! (invisibilidade, `ttl`, freezing, votação; doc 18, seção 17.5).

use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub trait Clock: Send + Sync + 'static {
    fn now(&self) -> SystemTime;

    /// Milissegundos desde a época Unix, a base do GUID v7 e dos prazos absolutos.
    fn now_millis(&self) -> u64 {
        self.now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as u64)
    }
}

/// O relógio do sistema.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> SystemTime {
        SystemTime::now()
    }
}

/// Relógio manual: só anda quando o teste manda.
#[derive(Debug)]
pub struct ManualClock {
    now: Mutex<SystemTime>,
}

impl ManualClock {
    pub fn new(start: SystemTime) -> Self {
        Self {
            now: Mutex::new(start),
        }
    }

    /// Começa em `2026-10-06T12:00:00Z`, o instante dos exemplos da documentação.
    pub fn at_doc_epoch() -> Self {
        Self::new(UNIX_EPOCH + Duration::from_secs(1_791_288_000))
    }

    pub fn advance(&self, by: Duration) {
        let mut now = self.now.lock().unwrap_or_else(|e| e.into_inner());
        *now += by;
    }
}

impl Clock for ManualClock {
    fn now(&self) -> SystemTime {
        *self.now.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[cfg(test)]
mod tests;
