//! The drawing of the Topics tab: the table of topics with their numbers and the detail of the selected one.

use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Cell, Paragraph, Row as TableRow, Table, Wrap};
use ratatui::Frame;

use crate::tui::api::{Severity, Topic};
use crate::tui::i18n;
use crate::tui::model::Model;
use crate::tui::screens::cluster::view::error_text;
use crate::tui::screens::topics::monitor::view::draw_compact_chart;
use crate::tui::screens::topics::monitor::CHARTS;
use crate::tui::screens::topics::{numbers, selected_index, visible};
use crate::tui::view::widgets::partial_seal;

pub fn draw(f: &mut Frame, area: Rect, m: &Model) {
    let tx = m.tx();
    let th = m.theme;
    let Some(list) = &m.topics.list.data else {
        let (text, style) = match &m.topics.list.error {
            Some(fail) => (error_text(m, fail), th.warning()),
            None => (tx.t("state.loading"), th.dim()),
        };
        f.render_widget(
            Paragraph::new(text).style(style).wrap(Wrap { trim: false }),
            area,
        );
        return;
    };
    let topics = visible(m);
    let [head, table, detail] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(5),
        Constraint::Length(10),
    ])
    .areas(area);
    let hidden = if m.topics.show_dead {
        0
    } else {
        list.iter().filter(|t| t.dead_letter).count()
    };
    let mut spans = vec![
        Span::styled(
            tx.tn("topics.count", (list.len() - hidden) as u64),
            th.title(),
        ),
        Span::styled(
            format!(
                "  {}",
                tx.tw("tokens.sorted", &[("by", &tx.t(m.topics.sort.key()))])
            ),
            th.dim(),
        ),
    ];
    if !m.topics.filter.is_empty() {
        spans.push(Span::styled(
            format!(
                "  {}",
                tx.tw(
                    "tokens.filtered",
                    &[("text", &m.topics.filter), ("shown", &topics.len())]
                )
            ),
            th.accent(),
        ));
    }
    if hidden > 0 {
        spans.push(Span::styled(
            format!("  {}", tx.tw("topics.dead-hidden", &[("n", &hidden)])),
            th.dim(),
        ));
    }
    if m.topics.list.is_stale() {
        spans.push(Span::styled(
            format!("  {} {}", th.icon(Severity::Warning), tx.t("state.stale")),
            th.warning(),
        ));
    }
    if m.topics.stats.error.is_some() && m.topics.stats.data.is_none() {
        spans.push(Span::styled(
            format!(
                "  {} {}",
                th.icon(Severity::Warning),
                tx.t("topics.no-numbers")
            ),
            th.warning(),
        ));
    } else if let Some(stats) = m.topics.stats.data.as_ref().filter(|s| s.partial) {
        spans.push(Span::raw("  "));
        let seal = partial_seal(m, th, &stats.missing);
        spans.extend(seal.spans);
    }
    f.render_widget(Paragraph::new(Line::from(spans)), head);
    draw_table(f, table, m, &topics);
    draw_strip(f, detail, m, &topics);
}

/// Under the table: the three charts of the selected queue side by side. A topic without a queue (a distributor),
/// or none at all, shows its summary instead.
fn draw_strip(f: &mut Frame, area: Rect, m: &Model, topics: &[Topic]) {
    let Some(mon) = crate::tui::screens::topics::preview::current(m) else {
        return draw_detail(f, area, m, topics);
    };
    let cells =
        Layout::horizontal([Constraint::Ratio(1, CHARTS.len() as u32); CHARTS.len()]).split(area);
    for (def, cell) in CHARTS.iter().zip(cells.iter()) {
        draw_compact_chart(f, *cell, m, mon, def);
    }
}

/// `queue`, `queue fifo`, `queue vol`, `distributor` or `dead-letter`: the kind and what marks it.
fn kind_text(m: &Model, topic: &Topic) -> String {
    let tx = m.tx();
    if topic.dead_letter {
        return tx.t("topics.kind.dead-letter");
    }
    if topic.is_distributor() {
        return tx.t("topics.kind.distributor");
    }
    let mut text = tx.t("topics.kind.queue");
    if topic.fifo {
        text.push_str(" fifo");
    }
    if topic.volatile {
        text.push_str(" vol");
    }
    text
}

fn rate(m: &Model, value: Option<f64>) -> String {
    value.map_or_else(|| "—".to_owned(), |v| i18n::decimal(m.lang, v, 1))
}

fn age(value: Option<f64>) -> String {
    value.map_or_else(|| "—".to_owned(), |secs| i18n::duration(secs as u64))
}

fn draw_table(f: &mut Frame, area: Rect, m: &Model, topics: &[Topic]) {
    let tx = m.tx();
    let th = m.theme;
    let selected = selected_index(m, topics);
    // The table shows a window of the rows that always holds the selected one.
    let capacity = usize::from(area.height.saturating_sub(3)).max(1);
    let start = selected.map_or(0, |s| s.saturating_sub(capacity - 1));
    let wide = area.width >= 100;
    let headers = [
        tx.t("col.name"),
        tx.t("col.type"),
        tx.t("col.depth"),
        tx.t("col.publish"),
        tx.t("col.consume"),
        tx.t("col.age"),
        tx.t("col.state"),
    ];
    let widths = if wide {
        [10, 14, 9, 9, 9, 10, 18]
    } else {
        [0, 12, 8, 6, 6, 7, 12]
    };
    let widths: Vec<Constraint> = widths
        .iter()
        .enumerate()
        .map(|(i, w)| {
            if i == 0 {
                Constraint::Fill(1)
            } else {
                Constraint::Length(*w)
            }
        })
        .collect();
    let right = |text: String| Cell::from(Line::from(text).alignment(Alignment::Right));
    let rows: Vec<TableRow> = topics
        .iter()
        .enumerate()
        .skip(start)
        .take(capacity)
        .map(|(i, topic)| {
            let marker = if selected == Some(i) {
                th.selector()
            } else {
                "  "
            };
            let n = numbers(m, &topic.name);
            let (state, state_style) = match n {
                Some(n) if n.worst == "ok" => (
                    format!("{} {}", th.icon(Severity::Ok), tx.t("topics.ok")),
                    Style::default(),
                ),
                Some(n) => (
                    format!("{} {}", th.icon(n.severity), n.worst),
                    th.severity(n.severity),
                ),
                None => ("—".to_owned(), th.dim()),
            };
            let cells = vec![
                Cell::from(format!("{marker}{}", topic.name)),
                Cell::from(kind_text(m, topic)),
                right(
                    n.and_then(|n| n.depth)
                        .map_or_else(|| "—".to_owned(), |d| i18n::group(m.lang, d)),
                ),
                right(rate(m, n.and_then(|n| n.publish_rate))),
                right(rate(m, n.and_then(|n| n.consume_rate))),
                right(age(n.and_then(|n| n.oldest_age))),
                Cell::from(state).style(state_style),
            ];
            let row = TableRow::new(cells);
            if selected == Some(i) {
                row.style(th.selected())
            } else if topic.dead_letter {
                // A dead-letter queue is dim: it follows its source queue (TUI-TOP-04).
                row.style(th.dim())
            } else {
                row
            }
        })
        .collect();
    let table = Table::new(rows, widths)
        .header(TableRow::new(headers).style(th.title()))
        .block(Block::bordered().border_style(th.accent()));
    f.render_widget(table, area);
}

fn draw_detail(f: &mut Frame, area: Rect, m: &Model, topics: &[Topic]) {
    let tx = m.tx();
    let th = m.theme;
    let block = Block::bordered().title(format!(" {} ", tx.t("topics.detail")));
    let Some(topic) = selected_index(m, topics).map(|i| &topics[i]) else {
        let key = if m.topics.filter.is_empty() {
            "topics.none"
        } else {
            "topics.none-filtered"
        };
        f.render_widget(Paragraph::new(tx.t(key)).style(th.dim()).block(block), area);
        return;
    };
    let lines = summary_lines(m, topic);
    f.render_widget(
        Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false }),
        area,
    );
}

/// The summary of a topic: its name and kind, id, creation, tags and limits (or prefixes).
pub fn summary_lines(m: &Model, topic: &Topic) -> Vec<Line<'static>> {
    let tx = m.tx();
    let th = m.theme;
    let width = [
        "col.name",
        "col.id",
        "topics.created",
        "topics.tags",
        "topics.prefixes",
        "topics.limits",
    ]
    .iter()
    .map(|key| tx.t(key).chars().count())
    .max()
    .unwrap_or(14);
    let line = |label: &str, value: String, style: Style| {
        Line::from(vec![
            Span::styled(format!("{label:<width$} "), th.dim()),
            Span::styled(value, style),
        ])
    };
    let none = || "—".to_owned();
    let created = topic
        .created_at
        .as_deref()
        .and_then(i18n::parse_instant)
        .map_or_else(none, |at| i18n::date_time(m.lang, at));
    let tags = if topic.tags.is_empty() {
        none()
    } else {
        topic
            .tags
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let mut lines = vec![
        line(
            &tx.t("col.name"),
            format!("{}  ({})", topic.name, kind_text(m, topic)),
            th.title(),
        ),
        line(&tx.t("col.id"), topic.id.clone(), Style::default()),
        line(&tx.t("topics.created"), created, Style::default()),
        line(&tx.t("topics.tags"), tags, Style::default()),
    ];
    if topic.is_distributor() {
        let prefixes = if topic.prefixes.is_empty() {
            none()
        } else {
            topic.prefixes.join(", ")
        };
        lines.push(line(&tx.t("topics.prefixes"), prefixes, Style::default()));
    } else if !topic.dead_letter {
        let unset = |own: Option<u64>, shown: String| {
            if own.is_some() {
                shown
            } else {
                format!("{shown} {}", tx.t("topics.default"))
            }
        };
        let or_dash =
            |value: Option<u64>, show: &dyn Fn(u64) -> String| value.map_or_else(none, show);
        let limits = tx.tw(
            "topics.limits-value",
            &[
                (
                    "bytes",
                    &unset(
                        topic.max_message_bytes,
                        or_dash(topic.effective_max_message_bytes, &|n| {
                            i18n::bytes(m.lang, n)
                        }),
                    ),
                ),
                (
                    "ttl",
                    &unset(topic.ttl, or_dash(topic.effective_ttl, &i18n::duration)),
                ),
                (
                    "deliveries",
                    &unset(
                        topic.max_deliveries,
                        or_dash(topic.effective_max_deliveries, &|n| n.to_string()),
                    ),
                ),
            ],
        );
        lines.push(line(&tx.t("topics.limits"), limits, Style::default()));
        if let Some(idle) = topic.idle_timeout.filter(|_| topic.volatile) {
            lines.push(line(
                &tx.t("topics.idle"),
                i18n::duration(idle),
                Style::default(),
            ));
        }
    }
    lines.push(Line::default());
    lines.push(Line::from(Span::styled(
        match &topic.source {
            Some(source) if topic.dead_letter => {
                tx.tw("topics.dead-letter-note", &[("source", source)])
            }
            _ => tx.t("topics.immutable"),
        },
        th.dim(),
    )));
    lines
}
