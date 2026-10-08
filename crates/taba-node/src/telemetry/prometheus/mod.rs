//! O texto do Prometheus (doc 11, "Endpoint Prometheus"): o formato `text/plain; version=0.0.4`
//! e, a pedido, o OpenMetrics. Os nomes seguem a especificação de compatibilidade do OpenTelemetry
//! com o Prometheus: pontos viram `_`, a unidade vira sufixo (`s` → `_seconds`, `By` → `_bytes`,
//! `1` → `_ratio`; contagens entre chaves não geram sufixo), contadores ganham `_total` e
//! histogramas viram `_bucket`, `_sum` e `_count`. O recurso vai em `target_info`.

use std::fmt::Write;

use super::metrics::{Def, Kind, Labels, Snapshot, Value};

pub const TEXT_CONTENT_TYPE: &str = "text/plain; version=0.0.4; charset=utf-8";
pub const OPENMETRICS_CONTENT_TYPE: &str =
    "application/openmetrics-text; version=1.0.0; charset=utf-8";

/// O nome da família no Prometheus, sem o `_total` do contador.
pub fn family_name(def: &Def) -> String {
    let mut name = sanitize(def.name);
    let suffix = match def.unit {
        "s" => Some("seconds"),
        "By" => Some("bytes"),
        "1" => Some("ratio"),
        _ => None,
    };
    if let Some(suffix) = suffix
        && !name.ends_with(&format!("_{suffix}"))
    {
        name.push('_');
        name.push_str(suffix);
    }
    name
}

/// O nome do rótulo: o atributo com os pontos trocados por `_`.
pub fn label_name(attribute: &str) -> String {
    sanitize(attribute)
}

fn sanitize(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

fn escape_help(text: &str) -> String {
    text.replace('\\', "\\\\").replace('\n', "\\n")
}

fn escape_label(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

fn number(value: f64) -> String {
    if value.is_nan() {
        "NaN".to_owned()
    } else if value.is_infinite() {
        if value > 0.0 { "+Inf" } else { "-Inf" }.to_owned()
    } else if value.fract() == 0.0 && value.abs() < 1e15 {
        format!("{}", value as i64)
    } else {
        format!("{value}")
    }
}

fn label_block(labels: &Labels, extra: Option<(&str, &str)>) -> String {
    let mut parts: Vec<String> = labels
        .iter()
        .map(|(k, v)| format!("{}=\"{}\"", label_name(k), escape_label(v)))
        .collect();
    if let Some((k, v)) = extra {
        parts.push(format!("{k}=\"{}\"", escape_label(v)));
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!("{{{}}}", parts.join(","))
    }
}

/// Escreve todas as métricas. `resource` são os atributos de recurso (`service.name`, ...), que
/// viram `target_info`. Com `open_metrics`, o formato é o OpenMetrics (contador sem `_total` na
/// família e `# EOF` no fim).
pub fn render(snapshot: &[Snapshot], resource: &[(String, String)], open_metrics: bool) -> String {
    let mut out = String::new();
    for family in snapshot {
        if family.series.is_empty() {
            continue;
        }
        let base = family_name(family.def);
        let (type_name, sample_name) = match family.def.kind {
            Kind::Counter => (
                if open_metrics {
                    base.clone()
                } else {
                    format!("{base}_total")
                },
                format!("{base}_total"),
            ),
            _ => (base.clone(), base.clone()),
        };
        let prom_type = match family.def.kind {
            Kind::Counter => "counter",
            Kind::UpDownCounter | Kind::Gauge => "gauge",
            Kind::Histogram => "histogram",
        };
        let _ = writeln!(out, "# HELP {type_name} {}", escape_help(family.def.help));
        let _ = writeln!(out, "# TYPE {type_name} {prom_type}");
        for (labels, value) in &family.series {
            match value {
                Value::Number(n) => {
                    let _ = writeln!(
                        out,
                        "{sample_name}{} {}",
                        label_block(labels, None),
                        number(*n)
                    );
                }
                Value::Histogram(h) => {
                    let cumulative = h.cumulative();
                    for (bound, total) in h.bounds().iter().zip(&cumulative) {
                        let _ = writeln!(
                            out,
                            "{base}_bucket{} {total}",
                            label_block(labels, Some(("le", &number(*bound))))
                        );
                    }
                    let _ = writeln!(
                        out,
                        "{base}_bucket{} {}",
                        label_block(labels, Some(("le", "+Inf"))),
                        h.count()
                    );
                    let block = label_block(labels, None);
                    let _ = writeln!(out, "{base}_sum{block} {}", number(h.sum()));
                    let _ = writeln!(out, "{base}_count{block} {}", h.count());
                }
            }
        }
    }
    if !resource.is_empty() {
        let block: Vec<String> = resource
            .iter()
            .map(|(k, v)| format!("{}=\"{}\"", label_name(k), escape_label(v)))
            .collect();
        let (family, kind) = if open_metrics {
            ("target", "info")
        } else {
            ("target_info", "gauge")
        };
        let _ = writeln!(out, "# HELP {family} Atributos de recurso do nó.");
        let _ = writeln!(out, "# TYPE {family} {kind}");
        let _ = writeln!(out, "target_info{{{}}} 1", block.join(","));
    }
    if open_metrics {
        out.push_str("# EOF\n");
    }
    out
}

#[cfg(test)]
mod tests;
