//! The charts of one queue (doc 34, TUI-MON-*, TUI-MSG-11): `Enter` on a topic opens three charts stacked on a
//! common time axis. Each reading of `GET /stats/topics/{name}` becomes one point per line; the history is only
//! what was sampled since the charts were opened.
//!
//! 1. the messages that are visible and those that are invisible (delivered and waiting for their ack);
//! 2. the flow: published, delivered and acknowledged (deleted) per second;
//! 3. the age of the oldest message of the queue and of its largest groups.

use std::collections::HashMap;

use crate::tui::api::TopicSample;
use crate::tui::effect::Effect;
use crate::tui::history::{Point, Ring};
use crate::tui::i18n;
use crate::tui::keymap::Action;
use crate::tui::model::{Input, InputPurpose, Modal, Model, TopicMonitor, TopicTab, WINDOWS_MIN};
use crate::tui::screens::cluster::{allowed_windows, moved, Unit};

use super::{messages, refuse, selected};

pub mod view;

/// The most groups drawn in the chart of ages.
pub const MAX_GROUPS: usize = 3;

/// What a chart draws: a title and its lines (a key into `TopicMonitor::series` and the catalog key of the legend).
pub struct ChartDef {
    pub key: &'static str,
    pub title: &'static str,
    pub unit: Unit,
    pub lines: &'static [(&'static str, &'static str)],
    /// The largest groups are drawn too, one line each.
    pub groups: bool,
}

pub const CHARTS: &[ChartDef] = &[
    ChartDef {
        key: "messages",
        title: "topic-chart.messages",
        unit: Unit::Count,
        lines: &[
            ("visible", "topic-chart.visible"),
            ("invisible", "topic-chart.invisible"),
        ],
        groups: false,
    },
    ChartDef {
        key: "flow",
        title: "topic-chart.flow",
        unit: Unit::PerSecond,
        lines: &[
            ("published", "topic-chart.published"),
            ("consumed", "topic-chart.consumed"),
            ("acked", "topic-chart.acked"),
        ],
        groups: false,
    },
    ChartDef {
        key: "age",
        title: "topic-chart.age",
        unit: Unit::Seconds,
        lines: &[("oldest", "topic-chart.oldest")],
        groups: true,
    },
];

/// A line of a chart: its series key and its legend text.
pub struct LineDef {
    pub key: String,
    pub label: String,
}

/// The lines of a chart now: the fixed ones, then one per group being followed.
pub fn lines_of(m: &Model, mon: &TopicMonitor, def: &ChartDef) -> Vec<LineDef> {
    let tx = m.tx();
    let mut lines: Vec<LineDef> = def
        .lines
        .iter()
        .map(|(key, label)| LineDef {
            key: (*key).to_owned(),
            label: tx.t(label),
        })
        .collect();
    if def.groups {
        for group in &mon.groups {
            lines.push(LineDef {
                key: format!("g:{group}"),
                label: tx.tw("topic-chart.group", &[("group", &group)]),
            });
        }
    }
    lines
}

/// `Enter` (the charts) or `x` (the messages) on the list: opens the selected topic. A distributor has no queue, so
/// it has no charts: only its messages can be opened, to publish.
pub fn open(m: &mut Model, tab: TopicTab) -> Vec<Effect> {
    let Some(topic) = selected(m) else {
        return refuse(m, "gate.no-topic");
    };
    let distributor = topic.is_distributor();
    if distributor && tab == TopicTab::Charts {
        return refuse(m, "gate.topic-no-charts");
    }
    let mut mon = TopicMonitor::new(topic.name, tab, distributor, topic.dead_letter);
    // What the strip under the list already sampled is not thrown away.
    if let Some(seen) = m.topics.previews.iter().find(|p| p.name == mon.name) {
        mon.series = seen.series.clone();
        mon.groups = seen.groups.clone();
    }
    m.topics.monitor = Some(mon);
    Vec::new()
}

/// The window in milliseconds, never above what the history retains (TUI-MON-03).
pub fn window_ms(m: &Model, mon: &TopicMonitor) -> u64 {
    let index = mon
        .monitor
        .window
        .min(allowed_windows(m.history.retention_ms()) - 1);
    WINDOWS_MIN[index] * 60_000
}

/// The points of a line: the window, up to the instant the chart was frozen at.
pub fn line_points(m: &Model, mon: &TopicMonitor, key: &str) -> Vec<Point> {
    let at = mon.monitor.frozen.unwrap_or(m.now);
    mon.series
        .get(key)
        .map(|ring| ring.window(at, window_ms(m, mon)))
        .unwrap_or_default()
}

fn put(series: &mut HashMap<String, Ring>, cap: usize, key: &str, point: Point) {
    series
        .entry(key.to_owned())
        .or_insert_with(|| Ring::new(cap))
        .push(point);
}

/// Keeps a reading: one point per line. A queue with no messages has an oldest age of zero; a group that is not
/// in the reading leaves a gap.
pub fn record(mon: &mut TopicMonitor, cap: usize, now: u64, sample: &TopicSample) {
    for (group, _) in &sample.groups {
        if mon.groups.len() < MAX_GROUPS && !mon.groups.contains(group) {
            mon.groups.push(group.clone());
        }
    }
    let at = |value: f64| Point {
        at: now,
        value,
        partial: sample.partial,
    };
    let series = &mut mon.series;
    put(series, cap, "visible", at(sample.visible as f64));
    put(series, cap, "invisible", at(sample.invisible as f64));
    put(series, cap, "published", at(sample.publish_rate));
    put(series, cap, "consumed", at(sample.consume_rate));
    put(series, cap, "acked", at(sample.ack_rate));
    put(series, cap, "oldest", at(sample.oldest_age.unwrap_or(0.0)));
    for (group, age) in &sample.groups {
        if mon.groups.contains(group) {
            put(&mut mon.series, cap, &format!("g:{group}"), at(*age));
        }
    }
}

/// The keys of an open topic dialog: `Esc` closes it, the rest are those of the view being shown.
pub fn on_action(m: &mut Model, action: Action) -> Vec<Effect> {
    let Some(shown) = m.topics.monitor.as_ref().map(|mon| mon.tab) else {
        return Vec::new();
    };
    if action == Action::Esc {
        // On the full screen Esc goes back to the list of charts first.
        match &mut m.topics.monitor {
            Some(mon) if shown == TopicTab::Charts && mon.monitor.full => mon.monitor.full = false,
            _ => m.topics.monitor = None,
        }
        return Vec::new();
    }
    if shown == TopicTab::Messages {
        return messages::on_action(m, action);
    }
    charts_action(m, action)
}

/// The keys of the charts of a queue.
fn charts_action(m: &mut Model, action: Action) -> Vec<Effect> {
    let now = m.now;
    let allowed = allowed_windows(m.history.retention_ms());
    let Some(mon) = &mut m.topics.monitor else {
        return Vec::new();
    };
    match action {
        Action::Up => mon.monitor.chart = moved(mon.monitor.chart, -1, CHARTS.len()),
        Action::Down => mon.monitor.chart = moved(mon.monitor.chart, 1, CHARTS.len()),
        Action::First => mon.monitor.chart = 0,
        Action::Last => mon.monitor.chart = CHARTS.len() - 1,
        Action::ChartFull => mon.monitor.full = !mon.monitor.full,
        Action::WindowWider => mon.monitor.window = (mon.monitor.window + 1).min(allowed - 1),
        Action::WindowNarrower => mon.monitor.window = mon.monitor.window.saturating_sub(1),
        Action::PauseChart => {
            mon.monitor.frozen = match mon.monitor.frozen {
                Some(_) => None,
                None => Some(now),
            }
        }
        Action::Scale => mon.monitor.fixed_scale = !mon.monitor.fixed_scale,
        // One file with every line of the three charts, so the chart is not asked for.
        Action::Export => {
            let value = format!("taba-{}-charts.csv", crate::tui::model::file_tag(&mon.name));
            m.modal = Some(Modal::Input(Input {
                purpose: InputPurpose::ExportTopicCsv,
                value,
            }));
        }
        _ => {}
    }
    Vec::new()
}

/// The visible lines of the three charts as CSV (TUI-MON-07): the time in UTC and one column per line.
pub fn export_csv(m: &Model) -> String {
    let Some(mon) = &m.topics.monitor else {
        return String::new();
    };
    let lines: Vec<(LineDef, Vec<Point>)> = CHARTS
        .iter()
        .flat_map(|def| lines_of(m, mon, def))
        .map(|line| {
            let points = line_points(m, mon, &line.key);
            (line, points)
        })
        .collect();
    let mut out = String::from("time");
    for (line, _) in &lines {
        out.push(',');
        out.push_str(&line.label.replace(',', " "));
    }
    out.push('\n');
    // Every reading is taken at one instant for all the lines, so the instants of any line are the rows.
    let mut instants: Vec<u64> = lines
        .iter()
        .flat_map(|(_, points)| points.iter().map(|p| p.at))
        .collect();
    instants.sort_unstable();
    instants.dedup();
    let offset = m.wall_ms - i64::try_from(m.now).unwrap_or(0);
    for at in instants {
        let c = i18n::civil_from_ms(offset + i64::try_from(at).unwrap_or(0));
        out.push_str(&format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
            c.year, c.month, c.day, c.hour, c.minute, c.second
        ));
        for (_, points) in &lines {
            out.push(',');
            if let Some(p) = points.iter().find(|p| p.at == at) {
                out.push_str(&p.value.to_string());
            }
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests;
