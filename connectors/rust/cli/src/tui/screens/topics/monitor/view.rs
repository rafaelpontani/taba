//! The drawing of the charts of a queue: three line charts stacked on one time axis, each with its legend and the
//! current value of every line in its title.

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::symbols::Marker;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Axis, Block, Chart, Dataset, GraphType, Paragraph, Wrap};
use ratatui::Frame;

use crate::tui::api::Severity;
use crate::tui::history::Point;
use crate::tui::i18n::{self, Lang};
use crate::tui::model::{Model, TopicMonitor, TopicTab};
use crate::tui::screens::cluster::view::error_text;
use crate::tui::screens::cluster::Unit;
use crate::tui::screens::topics::messages;
use crate::tui::screens::topics::monitor::{
    line_points, lines_of, window_ms, ChartDef, LineDef, CHARTS,
};

/// A value as a chart shows it: a count, a rate per second or an age.
pub fn fmt(lang: Lang, unit: Unit, value: f64) -> String {
    match unit {
        Unit::Count | Unit::Bytes => i18n::group(lang, value.round().max(0.0) as u64),
        Unit::PerSecond => format!("{}/s", i18n::decimal(lang, value, 1)),
        Unit::Seconds => i18n::duration(value.max(0.0) as u64),
    }
}

/// The line above the view: where the person is (`Topics › orders · Charts`) and, when the reading is not good,
/// its state.
pub fn head_line(m: &Model, mon: &TopicMonitor) -> Line<'static> {
    let tx = m.tx();
    let th = m.theme;
    let key = if mon.tab == TopicTab::Messages {
        "topic.screen-messages"
    } else {
        "topic.screen-charts"
    };
    let mut spans = vec![Span::styled(tx.tw(key, &[("name", &mon.name)]), th.title())];
    if let Some(fail) = &mon.feed.error {
        let text = if mon.feed.data.is_some() || !mon.series.is_empty() {
            tx.t("state.stale")
        } else {
            error_text(m, fail)
        };
        spans.push(Span::styled(
            format!("  {} {text}", th.icon(Severity::Warning)),
            th.warning(),
        ));
    }
    Line::from(spans)
}

/// The charts of a queue as in the charts of the cluster: the list of the three indicators on the left, with the
/// current figure of each, the one under the cursor drawn beside it, and `Enter` to give it the whole screen.
pub fn draw(f: &mut Frame, area: Rect, m: &Model) {
    let Some(mon) = &m.topics.monitor else { return };
    if mon.tab == TopicTab::Messages {
        return messages::view::draw(f, area, m, mon);
    }
    let tx = m.tx();
    let th = m.theme;
    let [head, main, note] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(7),
        Constraint::Length(2),
    ])
    .areas(area);

    f.render_widget(Paragraph::new(head_line(m, mon)), head);

    let chart = mon.monitor.chart.min(CHARTS.len() - 1);
    if mon.monitor.full {
        draw_chart(f, main, m, mon, &CHARTS[chart]);
    } else {
        let width = if main.width >= 100 { 40 } else { 28 };
        let [list_area, chart_area] =
            Layout::horizontal([Constraint::Length(width), Constraint::Min(20)]).areas(main);
        draw_chart_list(f, list_area, m, mon, chart);
        draw_chart(f, chart_area, m, mon, &CHARTS[chart]);
    }

    // What the charts honestly show (TUI-MON-05) and their state.
    let at = mon.monitor.frozen.unwrap_or(m.now);
    let window = window_ms(m, mon);
    let collected = mon
        .series
        .values()
        .map(|ring| ring.collected_ms(at))
        .max()
        .unwrap_or(0);
    let mut text = if collected < window {
        tx.tw(
            "chart.collected",
            &[("time", &i18n::duration(collected / 1000))],
        )
    } else {
        tx.t("chart.history-note")
    };
    if mon.monitor.frozen.is_some() {
        text.push_str(&format!(" · {}", tx.t("chart.paused")));
    }
    text.push_str(&format!(
        " · {}",
        tx.t(if mon.monitor.fixed_scale {
            "chart.scale-fixed"
        } else {
            "chart.scale-auto"
        })
    ));
    let partial = mon
        .series
        .keys()
        .any(|key| line_points(m, mon, key).iter().any(|p| p.partial));
    if partial {
        text.push_str(&format!(" · {}", tx.t("chart.partial-samples")));
    }
    f.render_widget(
        Paragraph::new(text)
            .style(th.dim())
            .wrap(Wrap { trim: false }),
        note,
    );
}

/// The list of the indicators: the name and the current figure of the first line of each.
fn draw_chart_list(f: &mut Frame, area: Rect, m: &Model, mon: &TopicMonitor, selected: usize) {
    let tx = m.tx();
    let th = m.theme;
    let inner_width = usize::from(area.width.saturating_sub(2));
    let lines: Vec<Line> = CHARTS
        .iter()
        .enumerate()
        .map(|(i, def)| {
            let marker = if i == selected { th.selector() } else { "  " };
            let value = def
                .lines
                .first()
                .and_then(|(key, _)| line_points(m, mon, key).last().map(|p| p.value))
                .map_or_else(|| "—".to_owned(), |v| fmt(m.lang, def.unit, v));
            let room = inner_width.saturating_sub(value.chars().count() + 1);
            let name: String = format!("{marker}{}", tx.t(def.title))
                .chars()
                .take(room)
                .collect();
            let pad = inner_width
                .saturating_sub(name.chars().count() + value.chars().count())
                .max(1);
            let style = if i == selected {
                th.selected()
            } else {
                ratatui::style::Style::default()
            };
            Line::from(vec![
                Span::styled(format!("{name}{}", " ".repeat(pad)), style),
                Span::styled(value, if i == selected { style } else { th.dim() }),
            ])
        })
        .collect();
    f.render_widget(
        Paragraph::new(lines).block(
            Block::bordered()
                .title(format!(" {} ", tx.t("chart.list")))
                .border_style(th.accent()),
        ),
        area,
    );
}

/// The vertical bounds: the window's own range with some air, or from zero to the largest value of the window.
fn bounds(fixed: bool, values: &[f64]) -> (f64, f64) {
    let high = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    if fixed {
        return (
            0.0,
            if high.is_finite() && high > 0.0 {
                high
            } else {
                1.0
            },
        );
    }
    let low = values.iter().copied().fold(f64::INFINITY, f64::min);
    let pad = if (high - low).abs() < f64::EPSILON {
        high.abs().max(1.0) * 0.1
    } else {
        (high - low) * 0.1
    };
    ((low - pad).max(0.0), high + pad)
}

/// One chart as a narrow cell of the strip under the list: the title carries only the markers and the current values.
pub fn draw_compact_chart(
    f: &mut Frame,
    area: Rect,
    m: &Model,
    mon: &TopicMonitor,
    def: &ChartDef,
) {
    draw_chart_in(f, area, m, mon, def, true);
}

fn draw_chart(f: &mut Frame, area: Rect, m: &Model, mon: &TopicMonitor, def: &ChartDef) {
    draw_chart_in(f, area, m, mon, def, false);
}

fn draw_chart_in(
    f: &mut Frame,
    area: Rect,
    m: &Model,
    mon: &TopicMonitor,
    def: &ChartDef,
    compact: bool,
) {
    let tx = m.tx();
    let th = m.theme;
    let lang = m.lang;
    let lines: Vec<(LineDef, Vec<Point>)> = lines_of(m, mon, def)
        .into_iter()
        .map(|line| {
            let points = line_points(m, mon, &line.key);
            (line, points)
        })
        .collect();
    // The title carries the legend: a marker and a color for each line, and what it is now (TUI-MON-02).
    let mut title = vec![Span::raw(format!(" {} ", tx.t(def.title)))];
    for (i, (line, points)) in lines.iter().enumerate() {
        let now = points
            .last()
            .map_or_else(|| "—".to_owned(), |p| fmt(lang, def.unit, p.value));
        let text = if compact {
            format!(" {} {now}", th.marker(i))
        } else {
            format!(" {} {} {now} ", th.marker(i), line.label)
        };
        title.push(Span::styled(text, th.series(i)));
    }
    let block = Block::bordered().title(Line::from(title));
    let values: Vec<f64> = lines
        .iter()
        .flat_map(|(_, points)| points.iter().map(|p| p.value))
        .collect();
    if values.is_empty() {
        let inner = area.inner(ratatui::layout::Margin::new(2, 1));
        f.render_widget(block, area);
        f.render_widget(Paragraph::new(tx.t("chart.no-data")).style(th.dim()), inner);
        return;
    }
    let at = mon.monitor.frozen.unwrap_or(m.now);
    let window_s = window_ms(m, mon) as f64 / 1000.0;
    let xy: Vec<Vec<(f64, f64)>> = lines
        .iter()
        .map(|(_, points)| {
            points
                .iter()
                .map(|p| ((p.at as f64 - at as f64) / 1000.0, p.value))
                .collect()
        })
        .collect();
    let (low, high) = bounds(mon.monitor.fixed_scale, &values);
    let mid = (low + high) / 2.0;
    let datasets: Vec<Dataset> = xy
        .iter()
        .enumerate()
        .filter(|(_, points)| !points.is_empty())
        .map(|(i, points)| {
            Dataset::default()
                .marker(if th.ascii {
                    Marker::Dot
                } else {
                    Marker::Braille
                })
                .graph_type(GraphType::Line)
                .style(th.series(i))
                .data(points)
        })
        .collect();
    let chart = Chart::new(datasets)
        .legend_position(None)
        .block(block)
        .x_axis(
            Axis::default()
                .bounds([-window_s, 0.0])
                .labels(vec![
                    format!("-{}", i18n::duration(window_s as u64)),
                    tx.t("chart.now"),
                ])
                .style(th.dim()),
        )
        .y_axis(
            Axis::default()
                .bounds([low, high])
                .labels(vec![
                    fmt(lang, def.unit, low),
                    fmt(lang, def.unit, mid),
                    fmt(lang, def.unit, high),
                ])
                .style(th.dim()),
        );
    f.render_widget(chart, area);
}
