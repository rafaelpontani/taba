//! The process metrics (doc 11, "Process and host"). Memory, threads, open files and CPU
//! come from `/proc` and only exist on Linux; on other systems they are left out (only `process.uptime`
//! always exists). No `unsafe` and no new dependency.

use std::sync::Mutex;
use std::time::Instant;

/// A snapshot of the process. What the system does not report is `None`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Sample {
    pub memory_bytes: Option<u64>,
    pub threads: Option<u64>,
    pub open_files: Option<u64>,
    /// CPU spent by the process since the start, in seconds.
    pub cpu_seconds: Option<f64>,
}

/// The resident memory and the threads, from the text of `/proc/self/status`.
pub fn parse_status(text: &str) -> (Option<u64>, Option<u64>) {
    let field = |name: &str| {
        text.lines()
            .find_map(|line| line.strip_prefix(name)?.strip_prefix(':'))
            .and_then(|rest| rest.split_whitespace().next()?.parse::<u64>().ok())
    };
    (field("VmRSS").map(|kb| kb * 1024), field("Threads"))
}

/// The CPU time (user plus system), in seconds, from the text of `/proc/self/stat`. The command
/// name comes in parentheses and may have spaces, so the fields count from the last `)`.
pub fn parse_stat_cpu(text: &str, ticks_per_second: f64) -> Option<f64> {
    let after = &text[text.rfind(')')? + 1..];
    let fields: Vec<&str> = after.split_whitespace().collect();
    // The first field after the `)` is the 3rd of the file (the state): `utime` and `stime` are the 14th and 15th.
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
        // `sysconf(_SC_CLK_TCK)` is 100 on practically every Linux.
        cpu_seconds: std::fs::read_to_string("/proc/self/stat")
            .ok()
            .and_then(|text| parse_stat_cpu(&text, 100.0)),
    }
}

#[cfg(not(target_os = "linux"))]
pub fn sample() -> Sample {
    Sample::default()
}

/// Keeps the previous CPU reading so the utilization is that between two readings.
#[derive(Default)]
pub struct CpuMeter {
    last: Mutex<Option<(Instant, f64)>>,
}

impl CpuMeter {
    /// The fraction of the total cores the process used since the previous reading; `None` on the
    /// first reading.
    pub fn utilization(&self, cpu_seconds: f64, now: Instant, cores: f64) -> Option<f64> {
        let mut last = self.last.lock().unwrap_or_else(|e| e.into_inner());
        let previous = last.replace((now, cpu_seconds))?;
        let wall = now.duration_since(previous.0).as_secs_f64();
        (wall > 0.0).then(|| ((cpu_seconds - previous.1) / (wall * cores.max(1.0))).clamp(0.0, 1.0))
    }
}

#[cfg(test)]
mod tests;
