//! As métricas do processo (doc 11, "Processo e host"). Memória, threads, arquivos abertos e CPU
//! vêm de `/proc` e só existem no Linux; nos outros sistemas ficam de fora (só `process.uptime`
//! existe sempre). Sem `unsafe` e sem dependência nova.

use std::sync::Mutex;
use std::time::Instant;

/// Um retrato do processo. O que o sistema não informa é `None`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Sample {
    pub memory_bytes: Option<u64>,
    pub threads: Option<u64>,
    pub open_files: Option<u64>,
    /// CPU gasta pelo processo desde o início, em segundos.
    pub cpu_seconds: Option<f64>,
}

/// A memória residente e as threads, do texto de `/proc/self/status`.
pub fn parse_status(text: &str) -> (Option<u64>, Option<u64>) {
    let field = |name: &str| {
        text.lines()
            .find_map(|line| line.strip_prefix(name)?.strip_prefix(':'))
            .and_then(|rest| rest.split_whitespace().next()?.parse::<u64>().ok())
    };
    (field("VmRSS").map(|kb| kb * 1024), field("Threads"))
}

/// O tempo de CPU (usuário mais sistema), em segundos, do texto de `/proc/self/stat`. O nome do
/// comando vem entre parênteses e pode ter espaços, então os campos contam a partir do último `)`.
pub fn parse_stat_cpu(text: &str, ticks_per_second: f64) -> Option<f64> {
    let after = &text[text.rfind(')')? + 1..];
    let fields: Vec<&str> = after.split_whitespace().collect();
    // O primeiro campo depois do `)` é o 3º do arquivo (o estado): `utime` e `stime` são o 14º e o 15º.
    let user: f64 = fields.get(11)?.parse().ok()?;
    let system: f64 = fields.get(12)?.parse().ok()?;
    Some((user + system) / ticks_per_second)
}

#[cfg(target_os = "linux")]
pub fn sample() -> Sample {
    let (memory_bytes, threads) = std::fs::read_to_string("/proc/self/status")
        .map(|text| parse_status(&text))
        .unwrap_or_default();
    Sample {
        memory_bytes,
        threads,
        open_files: std::fs::read_dir("/proc/self/fd")
            .ok()
            .map(|entries| entries.count() as u64),
        // O `sysconf(_SC_CLK_TCK)` é 100 em praticamente todo Linux.
        cpu_seconds: std::fs::read_to_string("/proc/self/stat")
            .ok()
            .and_then(|text| parse_stat_cpu(&text, 100.0)),
    }
}

#[cfg(not(target_os = "linux"))]
pub fn sample() -> Sample {
    Sample::default()
}

/// Guarda a leitura anterior da CPU para a utilização ser a de entre duas leituras.
#[derive(Default)]
pub struct CpuMeter {
    last: Mutex<Option<(Instant, f64)>>,
}

impl CpuMeter {
    /// A fração do total de núcleos que o processo usou desde a leitura anterior; `None` na
    /// primeira leitura.
    pub fn utilization(&self, cpu_seconds: f64, now: Instant, cores: f64) -> Option<f64> {
        let mut last = self.last.lock().unwrap_or_else(|e| e.into_inner());
        let previous = last.replace((now, cpu_seconds))?;
        let wall = now.duration_since(previous.0).as_secs_f64();
        (wall > 0.0).then(|| ((cpu_seconds - previous.1) / (wall * cores.max(1.0))).clamp(0.0, 1.0))
    }
}

#[cfg(test)]
mod tests;
