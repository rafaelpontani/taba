//! Reader of the Prometheus text a node serves at `GET /nodes/{id}/metrics` (doc 34, section 7): names,
//! labels and values, plus the `# TYPE` of each family so counters can be told from gauges.

use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub struct Sample {
    pub name: String,
    pub labels: Vec<(String, String)>,
    pub value: f64,
}

impl Sample {
    pub fn label(&self, name: &str) -> Option<&str> {
        self.labels
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    /// `name{a="b",c="d"}`, the way a person reads a series.
    pub fn series(&self) -> String {
        if self.labels.is_empty() {
            return self.name.clone();
        }
        let labels: Vec<String> = self
            .labels
            .iter()
            .map(|(k, v)| format!("{k}=\"{v}\""))
            .collect();
        format!("{}{{{}}}", self.name, labels.join(","))
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Metrics {
    pub samples: Vec<Sample>,
    types: HashMap<String, String>,
}

impl Metrics {
    pub fn parse(text: &str) -> Metrics {
        let mut metrics = Metrics::default();
        for line in text.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("# TYPE ") {
                if let Some((name, kind)) = rest.split_once(' ') {
                    metrics
                        .types
                        .insert(name.to_owned(), kind.trim().to_owned());
                }
            } else if !line.is_empty() && !line.starts_with('#') {
                if let Some(sample) = parse_sample(line) {
                    metrics.samples.push(sample);
                }
            }
        }
        metrics
    }

    /// The family's type (`counter`, `gauge`, `histogram`...), if the text declared it.
    pub fn kind(&self, name: &str) -> Option<&str> {
        self.types.get(name).map(String::as_str)
    }

    /// `true` for a counter, including the `_total` series of a family declared without the suffix.
    pub fn is_counter(&self, name: &str) -> bool {
        self.kind(name) == Some("counter")
            || name
                .strip_suffix("_total")
                .is_some_and(|base| self.kind(base) == Some("counter"))
    }

    /// The sum of all the series with this name (a family split by labels).
    pub fn sum(&self, name: &str) -> Option<f64> {
        let mut found = false;
        let mut total = 0.0;
        for sample in self.samples.iter().filter(|s| s.name == name) {
            found = true;
            total += sample.value;
        }
        found.then_some(total)
    }
}

/// Events per second between two readings of a counter. A counter that went down means the node restarted:
/// there is no rate for that interval (never a negative one).
pub fn rate(previous: f64, now: f64, seconds: f64) -> Option<f64> {
    (seconds > 0.0 && now >= previous).then(|| (now - previous) / seconds)
}

fn parse_sample(line: &str) -> Option<Sample> {
    let (head, rest) = match line.find('{') {
        Some(open) => {
            let close = closing_brace(line, open)?;
            (&line[..=close], &line[close + 1..])
        }
        None => line.split_once(' ')?,
    };
    let (name, labels) = match head.find('{') {
        Some(open) => (&head[..open], parse_labels(&head[open + 1..head.len() - 1])),
        None => (head, Vec::new()),
    };
    let value = rest.split_whitespace().next()?;
    Some(Sample {
        name: name.to_owned(),
        labels,
        value: parse_value(value)?,
    })
}

fn parse_value(text: &str) -> Option<f64> {
    match text {
        "+Inf" | "Inf" => Some(f64::INFINITY),
        "-Inf" => Some(f64::NEG_INFINITY),
        "NaN" => Some(f64::NAN),
        other => other.parse().ok(),
    }
}

/// The index of the `}` that closes the labels, skipping braces inside quoted values.
fn closing_brace(line: &str, open: usize) -> Option<usize> {
    let mut quoted = false;
    let mut escaped = false;
    for (i, c) in line.char_indices().skip_while(|(i, _)| *i < open) {
        match c {
            _ if escaped => escaped = false,
            '\\' if quoted => escaped = true,
            '"' => quoted = !quoted,
            '}' if !quoted => return Some(i),
            _ => {}
        }
    }
    None
}

fn parse_labels(text: &str) -> Vec<(String, String)> {
    let mut labels = Vec::new();
    let mut rest = text;
    while let Some((name, after)) = rest.split_once("=\"") {
        let mut value = String::new();
        let mut chars = after.char_indices();
        let mut end = after.len();
        while let Some((i, c)) = chars.next() {
            match c {
                '\\' => match chars.next() {
                    Some((_, 'n')) => value.push('\n'),
                    Some((_, other)) => value.push(other),
                    None => break,
                },
                '"' => {
                    end = i + 1;
                    break;
                }
                other => value.push(other),
            }
        }
        labels.push((name.trim_start_matches(',').trim().to_owned(), value));
        rest = after.get(end..).unwrap_or("");
    }
    labels
}

#[cfg(test)]
mod tests;
