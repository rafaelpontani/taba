//! Métricas, traces e logs (doc 11). Por enquanto só o log estruturado; as métricas
//! `taba.*` e o OTLP entram com o passo 10 de `08c-proximos-passos.md`.

use tracing_subscriber::EnvFilter;

/// Liga o log em JSON no nível de `telemetry.logs.level` (`info` enquanto o padrão
/// estiver a definir). Chamar de novo é inofensivo: vale a primeira chamada.
pub fn init_logging(level: Option<&str>) {
    let filter = EnvFilter::new(level.unwrap_or("info"));
    let _ = tracing_subscriber::fmt()
        .json()
        .with_env_filter(filter)
        .with_current_span(false)
        .try_init();
}
