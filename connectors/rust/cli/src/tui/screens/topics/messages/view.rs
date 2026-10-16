//! The drawing of the Messages view of a topic: the counters of the queue, the messages this session received and
//! the detail of the selected one.

use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Cell, Paragraph, Row as TableRow, Table, Wrap};
use ratatui::Frame;

use crate::tui::i18n;
use crate::tui::model::{Model, Received, TopicMonitor};
use crate::tui::screens::topics::messages::is_held;
use crate::tui::screens::topics::monitor::line_points;
use crate::tui::screens::topics::monitor::view::head_line;

pub fn draw(f: &mut Frame, area: Rect, m: &Model, mon: &TopicMonitor) {
    let th = m.theme;
    let [head, counters, table, detail, last] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(5),
        Constraint::Length(8),
        Constraint::Length(1),
    ])
    .areas(area);

    f.render_widget(Paragraph::new(head_line(m, mon)), head);

    f.render_widget(
        Paragraph::new(counters_text(m, mon)).style(th.dim()),
        counters,
    );
    draw_table(f, table, m, mon);
    draw_detail(f, detail, m, mon);

    if let Some((severity, text)) = &mon.messages.last {
        f.render_widget(
            Paragraph::new(format!(" {} {text}", th.icon(*severity))).style(th.severity(*severity)),
            last,
        );
    }
}

/// The truth about the queue, from the sampler (TUI-MSG-09): the rows below are only what was received.
fn counters_text(m: &Model, mon: &TopicMonitor) -> String {
    let tx = m.tx();
    if mon.distributor {
        return tx.t("messages.distributor");
    }
    let now_of = |key: &str| line_points(m, mon, key).last().map(|p| p.value);
    let count = |value: Option<f64>| {
        value.map_or_else(
            || "—".to_owned(),
            |v| i18n::group(m.lang, v.round().max(0.0) as u64),
        )
    };
    let (visible, invisible) = (now_of("visible"), now_of("invisible"));
    let depth = visible.zip(invisible).map(|(v, i)| v + i);
    let oldest = now_of("oldest").map_or_else(|| "—".to_owned(), |s| i18n::duration(s as u64));
    tx.tw(
        "messages.counters",
        &[
            ("visible", &count(visible)),
            ("invisible", &count(invisible)),
            ("depth", &count(depth)),
            ("age", &oldest),
        ],
    )
}

fn state_text(m: &Model, row: &Received) -> (String, Style) {
    let tx = m.tx();
    let th = m.theme;
    if row.acked {
        (tx.t("messages.state.deleted"), th.dim())
    } else if is_held(row, m.now) {
        let secs = row
            .held_until
            .unwrap_or(0)
            .saturating_sub(m.now)
            .div_ceil(1000);
        (
            tx.tw("messages.state.held", &[("secs", &secs)]),
            th.warning(),
        )
    } else {
        (tx.t("messages.state.visible"), Style::default())
    }
}

/// One line of the body for a table cell: line breaks become spaces.
fn snippet(body: &str) -> String {
    body.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn draw_table(f: &mut Frame, area: Rect, m: &Model, mon: &TopicMonitor) {
    let tx = m.tx();
    let th = m.theme;
    let rows = &mon.messages.rows;
    let title = format!(" {} ", tx.tw("messages.title", &[("n", &rows.len())]));
    let block = Block::bordered().border_style(th.accent()).title(title);
    if rows.is_empty() {
        let key = if mon.distributor {
            "messages.empty-distributor"
        } else {
            "messages.empty"
        };
        f.render_widget(
            Paragraph::new(tx.t(key))
                .style(th.dim())
                .wrap(Wrap { trim: false })
                .block(block),
            area,
        );
        return;
    }
    let selected = mon.messages.selected.min(rows.len() - 1);
    let capacity = usize::from(area.height.saturating_sub(3)).max(1);
    let start = selected.saturating_sub(capacity - 1);
    let wide = area.width >= 100;
    let headers = [
        tx.t("col.time"),
        tx.t("col.id"),
        tx.t("col.group"),
        tx.t("col.deliveries"),
        tx.t("col.state"),
        tx.t("col.size"),
        tx.t("col.body"),
    ];
    let widths = if wide {
        [10, 14, 16, 5, 18, 8, 0]
    } else {
        [10, 11, 10, 4, 14, 6, 0]
    };
    let widths: Vec<Constraint> = widths
        .iter()
        .enumerate()
        .map(|(i, w)| {
            if i == 6 {
                Constraint::Fill(1)
            } else {
                Constraint::Length(*w)
            }
        })
        .collect();
    let right = |text: String| Cell::from(Line::from(text).alignment(Alignment::Right));
    let table_rows: Vec<TableRow> = rows
        .iter()
        .enumerate()
        .skip(start)
        .take(capacity)
        .map(|(i, row)| {
            let marker = if i == selected { th.selector() } else { "  " };
            let at =
                i18n::clock(m.wall_ms - i64::try_from(m.now.saturating_sub(row.at)).unwrap_or(0));
            let (state, state_style) = state_text(m, row);
            let cells = vec![
                Cell::from(format!("{marker}{at}")),
                Cell::from(crate::tui::model::short_id(&row.id)),
                Cell::from(row.group.clone().unwrap_or_else(|| "—".to_owned())),
                right(row.deliveries.to_string()),
                Cell::from(state).style(state_style),
                right(i18n::group(m.lang, row.size as u64)),
                Cell::from(snippet(&row.body)),
            ];
            let line = TableRow::new(cells);
            if i == selected {
                line.style(th.selected())
            } else if row.acked {
                line.style(th.dim())
            } else {
                line
            }
        })
        .collect();
    let table = Table::new(table_rows, widths)
        .header(TableRow::new(headers).style(th.title()))
        .block(block);
    f.render_widget(table, area);
}

fn draw_detail(f: &mut Frame, area: Rect, m: &Model, mon: &TopicMonitor) {
    let tx = m.tx();
    let th = m.theme;
    let block = Block::bordered().title(format!(" {} ", tx.t("messages.detail")));
    let rows = &mon.messages.rows;
    let Some(row) = rows.get(mon.messages.selected.min(rows.len().saturating_sub(1))) else {
        f.render_widget(
            Paragraph::new(tx.t("messages.no-selection"))
                .style(th.dim())
                .block(block),
            area,
        );
        return;
    };
    let (state, state_style) = state_text(m, row);
    let mut lines = vec![
        Line::from(vec![
            Span::styled(format!("{} ", tx.t("col.id")), th.dim()),
            Span::raw(row.id.clone()),
            Span::styled(format!("  {} ", tx.t("col.state")), th.dim()),
            Span::styled(state, state_style),
        ]),
        Line::from(Span::styled(
            tx.tw(
                "messages.facts",
                &[
                    ("type", &row.content_type),
                    ("size", &i18n::group(m.lang, row.size as u64)),
                    ("deliveries", &row.deliveries),
                ],
            ),
            th.dim(),
        )),
    ];
    lines.extend(row.body.lines().map(|line| Line::from(line.to_owned())));
    if row.body.len() < row.size {
        lines.push(Line::from(Span::styled(
            tx.tw(
                "messages.truncated",
                &[("size", &i18n::group(m.lang, row.size as u64))],
            ),
            th.dim(),
        )));
    }
    f.render_widget(
        Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false }),
        area,
    );
}
