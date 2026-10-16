//! The drawing of the Cluster tab: the list of conditions and nodes, the detail of a node and its charts.

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::symbols::Marker;
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Axis, Block, Cell, Chart, Dataset, GraphType, Paragraph, Row as TableRow, Sparkline, Table,
    Wrap,
};
use ratatui::Frame;

use crate::tui::api::{NodeStatus, Severity};
use crate::tui::history::{self, Series};
use crate::tui::i18n::{self, Lang};
use crate::tui::model::{ClusterView, Fail, Model, Pane};
use crate::tui::screens::cluster::{
    self, line_points, selected_index, selected_row, status_of, window_ms, Row, Unit, MON_CHARTS,
};
use crate::tui::view::widgets::{self, partial_seal, scrolling, short};

/// The bars of a sparkline when the terminal cannot be trusted with block characters.
const ASCII_BARS: ratatui::symbols::bar::Set = ratatui::symbols::bar::Set {
    full: "#",
    seven_eighths: "#",
    three_quarters: "=",
    five_eighths: "=",
    half: "-",
    three_eighths: "-",
    one_quarter: ".",
    one_eighth: ".",
    empty: " ",
};

pub fn draw(f: &mut Frame, area: Rect, m: &Model) {
    match m.cluster.view {
        ClusterView::List => draw_list(f, area, m),
        ClusterView::Detail => draw_detail(f, area, m),
        ClusterView::Monitor => draw_monitor(f, area, m),
    }
}

/// The text for a read that failed and has nothing to show.
pub fn error_text(m: &Model, fail: &Fail) -> String {
    let tx = m.tx();
    if fail.is_offline() {
        tx.tw(
            "error.offline",
            &[("url", &m.url), ("message", &fail.message)],
        )
    } else if fail.is_denied() {
        widgets::denied_text(m, fail)
    } else if fail.code == "cluster.node-draining" {
        tx.t("error.draining")
    } else {
        tx.tw(
            "error.api",
            &[("code", &fail.code), ("message", &fail.message)],
        )
    }
}

fn draw_message(f: &mut Frame, area: Rect, text: String, style: Style) {
    f.render_widget(
        Paragraph::new(text).style(style).wrap(Wrap { trim: false }),
        area,
    );
}

fn draw_list(f: &mut Frame, area: Rect, m: &Model) {
    let tx = m.tx();
    let th = m.theme;
    if m.me.data.is_none() {
        match &m.me.error {
            Some(fail) => draw_message(f, area, error_text(m, fail), th.critical()),
            None => draw_message(f, area, tx.t("state.loading"), th.dim()),
        }
        return;
    }
    let rows = cluster::rows(m);
    if rows.is_empty() {
        let error = m.status.error.as_ref().or(m.cluster.nodes.error.as_ref());
        match error {
            Some(fail) => draw_message(f, area, error_text(m, fail), th.critical()),
            None => draw_message(f, area, tx.t("state.loading"), th.dim()),
        }
        return;
    }
    if !m.is_root() || m.status.data.is_none() {
        draw_limited(f, area, m, &rows);
        return;
    }
    let wide = area.width >= 120;
    let (main, side) = if wide {
        let [main, side] =
            Layout::horizontal([Constraint::Fill(1), Constraint::Length(40)]).areas(area);
        (main, Some(side))
    } else {
        (area, None)
    };
    let conditions = m.status.data.as_ref().map_or(0, |s| s.conditions.len());
    let partial = m.status.data.as_ref().is_some_and(|s| s.partial);
    let cond_height = u16::try_from(conditions.clamp(1, 3)).unwrap_or(3) + 2;
    let stopped = m.status.data.as_ref().is_some_and(|s| s.stopped);
    let summary_height = 1 + u16::from(partial) + u16::from(stopped);
    let chart_height = if side.is_none() { 4 } else { 0 };
    let [cond_area, summary_area, table_area, chart_area] = Layout::vertical([
        Constraint::Length(cond_height),
        Constraint::Length(summary_height),
        Constraint::Min(4),
        Constraint::Length(chart_height),
    ])
    .areas(main);
    draw_conditions(f, cond_area, m);
    draw_summary(f, summary_area, m);
    draw_nodes(f, table_area, m, &rows);
    match side {
        Some(side) => draw_sparks(f, side, m, true),
        None => draw_sparks(f, chart_area, m, false),
    }
}

/// A token that is not the root token: only the list of nodes, and why the rest is missing (TUI-CLU-07).
fn draw_limited(f: &mut Frame, area: Rect, m: &Model, rows: &[Row]) {
    let th = m.theme;
    let tx = m.tx();
    let reason = match &m.status_denied {
        Some(fail) => widgets::denied_text(m, fail),
        None => tx.t("denied.root"),
    };
    let [note, table] = Layout::vertical([Constraint::Length(3), Constraint::Min(3)]).areas(area);
    f.render_widget(
        Paragraph::new(format!("{} {reason}", th.icon(Severity::Warning)))
            .style(th.warning())
            .wrap(Wrap { trim: false }),
        note,
    );
    let header = TableRow::new([
        tx.t("col.node"),
        tx.t("col.state"),
        tx.t("col.drain"),
        tx.t("col.version"),
    ])
    .style(th.title());
    let selected = selected_index(m, rows);
    let body: Vec<TableRow> = rows
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let version = m
                .cluster
                .nodes
                .data
                .as_ref()
                .and_then(|n| n.items.iter().find(|x| x.node_id == r.id))
                .map(|x| x.version.clone())
                .unwrap_or_default();
            let marker = if selected == Some(i) {
                th.selector()
            } else {
                "  "
            };
            let id = if r.is_self {
                format!("{marker}{} *", short(&r.id))
            } else {
                format!("{marker}{}", short(&r.id))
            };
            let row = TableRow::new([id, state_word(m, r), r.drain_state.clone(), version]);
            if selected == Some(i) {
                row.style(th.selected())
            } else {
                row
            }
        })
        .collect();
    let widths = [
        Constraint::Length(14),
        Constraint::Length(14),
        Constraint::Length(10),
        Constraint::Fill(1),
    ];
    f.render_widget(
        Table::new(body, widths).header(header).block(
            Block::bordered().title(format!(" {} ", tx.tn("cluster.nodes", rows.len() as u64))),
        ),
        table,
    );
}

fn state_word(m: &Model, row: &Row) -> String {
    let th = m.theme;
    if !row.reachable {
        return format!("{} {}", th.offline(), m.tx().t("state.unreachable"));
    }
    if row.state.is_empty() {
        return "—".to_owned();
    }
    row.state.clone()
}

fn draw_conditions(f: &mut Frame, area: Rect, m: &Model) {
    let tx = m.tx();
    let th = m.theme;
    let focused = m.cluster.pane == Pane::Conditions;
    let title = format!(" {} ", tx.t("cluster.conditions"));
    let border = if focused {
        th.accent()
    } else {
        Style::default()
    };
    let block = Block::bordered().title(title).border_style(border);
    let Some(status) = &m.status.data else {
        f.render_widget(block, area);
        return;
    };
    let lines: Vec<Line> = if status.conditions.is_empty() {
        vec![Line::from(Span::styled(
            format!("{} {}", th.icon(Severity::Ok), tx.t("health.ok")),
            th.ok(),
        ))]
    } else {
        status
            .conditions
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let name = tx.t(&format!("condition.{}", c.name));
                let marker = if focused && i == m.cluster.condition {
                    th.selector()
                } else {
                    "  "
                };
                let style = if focused && i == m.cluster.condition {
                    th.severity(c.severity).patch(th.selected())
                } else {
                    th.severity(c.severity)
                };
                Line::from(Span::styled(
                    format!("{marker}{} {name}", th.icon(c.severity)),
                    style,
                ))
            })
            .collect()
    };
    f.render_widget(Paragraph::new(lines).block(block), area);
}

fn draw_summary(f: &mut Frame, area: Rect, m: &Model) {
    let tx = m.tx();
    let th = m.theme;
    let Some(status) = &m.status.data else { return };
    let members: Vec<String> = status
        .members
        .iter()
        .map(|(state, n)| format!("{state} {n}"))
        .collect();
    let quorum = if status.quorum {
        format!("{} {}", th.icon(Severity::Ok), tx.t("summary.quorum"))
    } else {
        format!(
            "{} {}",
            th.icon(Severity::Critical),
            tx.t("summary.no-quorum")
        )
    };
    let quorum_style = if status.quorum {
        th.ok()
    } else {
        th.critical()
    };
    let mut spans = vec![
        Span::raw(format!(
            "{}: {} · ",
            tx.t("summary.members"),
            members.join(", ")
        )),
        Span::styled(quorum, quorum_style),
        Span::raw(format!(
            " · {} {} · {} {}",
            tx.t("summary.table"),
            status.members_version,
            tx.t("summary.syncing"),
            i18n::group(m.lang, status.syncing_blocks)
        )),
    ];
    if m.status.is_stale() {
        spans.push(Span::styled(
            format!(" · {}", tx.t("state.stale")),
            th.warning(),
        ));
    }
    let mut lines = vec![Line::from(spans)];
    if status.stopped {
        let key = if status.planned_stop {
            "summary.stopped-planned"
        } else {
            "summary.stopped"
        };
        let text = tx.tw(
            key,
            &[("at", &status.stopped_at.clone().unwrap_or_default())],
        );
        lines.push(Line::from(Span::styled(
            format!("{} {text}", th.icon(Severity::Critical)),
            th.critical(),
        )));
    }
    if status.partial {
        lines.push(partial_seal(m, th, &status.missing));
    }
    f.render_widget(Paragraph::new(lines), area);
}

fn draw_nodes(f: &mut Frame, area: Rect, m: &Model, rows: &[Row]) {
    let tx = m.tx();
    let th = m.theme;
    let lang = m.lang;
    let full = area.width >= 100;
    let focused = m.cluster.pane == Pane::Nodes;
    let mut headers = vec![
        tx.t("col.node"),
        tx.t("col.state"),
        tx.t("col.drain"),
        tx.t("col.memory"),
        tx.t("col.messages"),
    ];
    let mut widths = vec![
        Constraint::Length(14),
        Constraint::Length(10),
        Constraint::Length(9),
        Constraint::Length(14),
        Constraint::Length(15),
    ];
    if full {
        headers.push(tx.t("col.disk-free"));
        widths.push(Constraint::Length(10));
    }
    headers.extend([tx.t("col.sync"), tx.t("col.requests")]);
    widths.extend([Constraint::Length(5), Constraint::Length(5)]);
    if full {
        headers.push(tx.t("col.rejected"));
        widths.push(Constraint::Length(6));
    }
    let selected = selected_index(m, rows);
    let body: Vec<TableRow> = rows
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let marker = if selected == Some(i) {
                th.selector()
            } else {
                "  "
            };
            let mut cells = vec![Cell::from(format!("{marker}{}", short(&r.id)))];
            match status_of(m, &r.id) {
                Some(s) => {
                    let ratio = s.memory_ratio();
                    let mem_style = if ratio >= 0.95 {
                        th.critical()
                    } else if ratio >= 0.8 {
                        th.warning()
                    } else {
                        Style::default()
                    };
                    cells.push(Cell::from(r.state.clone()));
                    cells.push(Cell::from(r.drain_state.clone()));
                    let memory = if s.memory_limit == 0 {
                        i18n::bytes(lang, s.memory_used)
                    } else {
                        format!("{} {:>3}%", th.bar(ratio, 8), (ratio * 100.0).round())
                    };
                    cells.push(Cell::from(memory).style(mem_style));
                    // Memory and disk apart: a flush moves one to the other and the sum would not show it.
                    cells.push(Cell::from(format!(
                        "{}/{}",
                        i18n::group(lang, s.memory_messages),
                        i18n::group(lang, s.disk_messages)
                    )));
                    if full {
                        cells.push(Cell::from(i18n::bytes(lang, s.disk_free)));
                    }
                    cells.push(Cell::from(i18n::group(lang, s.syncing_blocks)));
                    cells.push(Cell::from(format!("{}", s.active_requests.round())));
                    if full {
                        cells.push(Cell::from(format!("{}", s.rate_limited.round())));
                    }
                }
                None if r.reachable => {
                    // Known only by `GET /nodes` (for example drained, out of the member table).
                    cells.push(Cell::from(r.state.clone()));
                    cells.push(Cell::from(r.drain_state.clone()));
                    let blanks = widths.len() - cells.len();
                    cells.extend((0..blanks).map(|_| Cell::from("—")));
                }
                None => {
                    cells.push(Cell::from(state_word(m, r)).style(th.critical()));
                    let blanks = widths.len() - cells.len();
                    cells.extend((0..blanks).map(|_| Cell::from("—")));
                }
            }
            let row = TableRow::new(cells);
            if selected == Some(i) {
                row.style(th.selected())
            } else if !r.reachable {
                row.style(th.critical())
            } else {
                row
            }
        })
        .collect();
    let border = if focused {
        th.accent()
    } else {
        Style::default()
    };
    let table = Table::new(body, widths)
        .header(TableRow::new(headers).style(th.title()))
        .block(
            Block::bordered()
                .title(format!(" {} ", tx.tn("cluster.nodes", rows.len() as u64)))
                .border_style(border),
        );
    f.render_widget(table, area);
}

/// The two small charts of the selected node (TUI-CLU-04): memory against its limit, and messages in memory.
fn draw_sparks(f: &mut Frame, area: Rect, m: &Model, stacked: bool) {
    if area.height == 0 {
        return;
    }
    let tx = m.tx();
    let th = m.theme;
    let Some(row) = selected_row(m) else { return };
    let halves = if stacked {
        Layout::vertical([
            Constraint::Length(5),
            Constraint::Length(5),
            Constraint::Min(0),
        ])
        .split(area)
    } else {
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).split(area)
    };
    let window = 300_000;
    let make = |series: Series, title: String, area: Rect, ceiling: Option<u64>, f: &mut Frame| {
        let points = m
            .history
            .series(&row.id, series)
            .map(|r| r.window(m.now, window))
            .unwrap_or_default();
        let width = usize::from(area.width.saturating_sub(2));
        let skip = points.len().saturating_sub(width);
        let data: Vec<u64> = points
            .iter()
            .skip(skip)
            .map(|p| p.value.max(0.0) as u64)
            .collect();
        let max = ceiling
            .unwrap_or_else(|| data.iter().copied().max().unwrap_or(1))
            .max(1);
        let last = points.last().map(|p| p.value);
        let shown = match (series, last) {
            (Series::MemoryUsed, Some(v)) => format!("{title} {}", i18n::bytes(m.lang, v as u64)),
            (_, Some(v)) => format!("{title} {}", i18n::group(m.lang, v as u64)),
            (_, None) => title,
        };
        f.render_widget(
            Sparkline::default()
                .data(data)
                .bar_set(if th.ascii {
                    ASCII_BARS
                } else {
                    ratatui::symbols::bar::NINE_LEVELS
                })
                .max(max)
                .style(th.accent())
                .block(Block::bordered().title(format!(" {shown} "))),
            area,
        );
    };
    let limit = status_of(m, &row.id)
        .map(|s| s.memory_limit)
        .filter(|limit| *limit > 0);
    make(
        Series::MemoryUsed,
        tx.t("chart.memory"),
        halves[0],
        limit,
        f,
    );
    make(
        Series::MemoryMessages,
        tx.t("chart.ready"),
        halves[1],
        None,
        f,
    );
}

// ---------- the detail of a node ----------

fn pair(m: &Model, label: &str, value: impl Into<String>) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{label}: "), m.theme.dim()),
        Span::raw(value.into()),
    ])
}

fn heading(m: &Model, text: String) -> Line<'static> {
    Line::from(Span::styled(
        text,
        m.theme.title().add_modifier(Modifier::UNDERLINED),
    ))
}

/// The lines of the detail screen: the node, then its metrics. The same lines size the scroll (TUI-FRM-04).
pub fn detail_lines(m: &Model) -> Vec<Line<'static>> {
    let tx = m.tx();
    let th = m.theme;
    let lang = m.lang;
    let Some(row) = selected_row(m) else {
        return vec![Line::from(tx.t("detail.no-node"))];
    };
    let mut lines = vec![Line::from(Span::styled(
        format!("{} {}", tx.t("detail.node"), row.id),
        th.title(),
    ))];
    let Some(s) = status_of(m, &row.id) else {
        lines.push(Line::from(Span::styled(
            format!("{} {}", th.offline(), tx.t("state.unreachable")),
            th.critical(),
        )));
        return lines;
    };
    lines.push(pair(
        m,
        &tx.t("col.state"),
        format!("{} · {}: {}", s.state, tx.t("col.drain"), s.drain_state),
    ));
    lines.push(pair(
        m,
        &tx.t("col.version"),
        format!("{} · {}", s.version, s.mode),
    ));
    if let Some(started) = s.started_at.as_deref().and_then(i18n::parse_instant) {
        let up = u64::try_from((m.wall_ms - started) / 1000).unwrap_or(0);
        lines.push(pair(
            m,
            &tx.t("detail.started"),
            format!(
                "{} ({})",
                i18n::date_time(lang, started),
                i18n::duration(up)
            ),
        ));
    }
    lines.push(Line::default());
    lines.push(heading(m, tx.t("detail.storage")));
    let ratio = s.memory_ratio();
    let mem_style = if ratio >= 0.95 {
        th.critical()
    } else if ratio >= 0.8 {
        th.warning()
    } else {
        th.ok()
    };
    lines.push(Line::from(vec![
        Span::styled(format!("{}: ", tx.t("col.memory")), th.dim()),
        Span::styled(
            if s.memory_limit == 0 {
                format!(
                    "{} ({})",
                    i18n::bytes(lang, s.memory_used),
                    tx.t("detail.no-limit")
                )
            } else {
                format!(
                    "{} {:>3}%  {} / {}",
                    th.bar(ratio, 20),
                    (ratio * 100.0).round(),
                    i18n::bytes(lang, s.memory_used),
                    i18n::bytes(lang, s.memory_limit)
                )
            },
            mem_style,
        ),
    ]));
    lines.push(pair(
        m,
        &tx.t("detail.messages"),
        tx.tw(
            "detail.messages-value",
            &[
                ("memory", &i18n::group(lang, s.memory_messages)),
                ("disk", &i18n::group(lang, s.disk_messages)),
            ],
        ),
    ));
    lines.push(pair(
        m,
        &tx.t("col.disk-free"),
        i18n::bytes(lang, s.disk_free),
    ));
    lines.push(Line::default());
    lines.push(heading(m, tx.t("detail.signals")));
    lines.extend(signal_lines(m, s));
    lines.push(Line::default());
    lines.push(heading(m, tx.t("detail.backup")));
    lines.extend(backup_lines(m));
    if let Some(watch) = m.cluster.watch.as_ref().filter(|w| w.node == row.id) {
        lines.push(Line::default());
        lines.push(heading(m, tx.t("detail.drain")));
        let text = match &m.cluster.drain.data {
            Some(d) => tx.tw(
                "detail.drain-progress",
                &[
                    ("state", &d.state),
                    ("pending", &i18n::group(lang, d.pending)),
                ],
            ),
            None => tx.t("state.loading"),
        };
        let _ = watch;
        lines.push(Line::from(text));
    }
    lines.push(Line::default());
    lines.push(heading(m, tx.t("detail.metrics")));
    match &m.cluster.metrics.data {
        Some(metrics) => {
            for sample in metrics.samples.iter().take(500) {
                lines.push(Line::from(vec![
                    Span::raw(sample.series()),
                    Span::styled(format!("  {}", sample.value), th.accent()),
                ]));
            }
        }
        None => lines.push(Line::from(Span::styled(
            match &m.cluster.metrics.error {
                Some(fail) => error_text(m, fail),
                None => tx.t("state.loading"),
            },
            th.dim(),
        ))),
    }
    lines
}

fn signal_lines(m: &Model, s: &NodeStatus) -> Vec<Line<'static>> {
    let tx = m.tx();
    let th = m.theme;
    let lang = m.lang;
    let flag = |bad: bool| if bad { th.warning() } else { Style::default() };
    let quorum = if s.quorum {
        Span::styled(
            format!("{} {}", th.icon(Severity::Ok), tx.t("summary.quorum")),
            th.ok(),
        )
    } else {
        Span::styled(
            format!(
                "{} {}",
                th.icon(Severity::Critical),
                tx.t("summary.no-quorum")
            ),
            th.critical(),
        )
    };
    let count = |label: &str, n: u64| {
        Line::from(vec![
            Span::styled(format!("{label}: "), th.dim()),
            Span::styled(i18n::group(lang, n), flag(n > 0)),
        ])
    };
    let rtt = match s.rtt_p95 {
        Some(v) => fmt_unit(lang, Unit::Seconds, v),
        None => "—".to_owned(),
    };
    vec![
        Line::from(vec![
            Span::styled(format!("{}: ", tx.t("detail.quorum")), th.dim()),
            quorum,
        ]),
        count(&tx.t("detail.channels"), s.channels),
        count(&tx.t("detail.syncing"), s.syncing_blocks),
        count(&tx.t("detail.below-replication"), s.below_replication),
        count(&tx.t("detail.below-quorum"), s.below_quorum),
        count(&tx.t("detail.frozen"), s.frozen_nodes),
        pair(
            m,
            &tx.t("detail.requests"),
            format!("{}", s.active_requests.round()),
        ),
        pair(
            m,
            &tx.t("detail.rejected"),
            format!("{}", s.rate_limited.round()),
        ),
        pair(m, &tx.t("detail.rtt"), rtt),
    ]
}

fn backup_lines(m: &Model) -> Vec<Line<'static>> {
    let tx = m.tx();
    let th = m.theme;
    let lang = m.lang;
    match &m.cluster.backup.data {
        Some(Some(b)) => {
            let yes_no = |v: Option<bool>| match v {
                Some(true) => tx.t("value.yes"),
                Some(false) => tx.t("value.no"),
                None => "—".to_owned(),
            };
            let last = b
                .last_success
                .as_deref()
                .and_then(i18n::parse_instant)
                .map_or_else(|| "—".to_owned(), |ms| i18n::date_time(lang, ms));
            vec![
                pair(m, &tx.t("detail.s3-reachable"), yes_no(b.reachable)),
                pair(
                    m,
                    &tx.t("detail.pending"),
                    b.pending.map_or("—".to_owned(), |n| i18n::group(lang, n)),
                ),
                pair(m, &tx.t("detail.last-success"), last),
                pair(
                    m,
                    &tx.t("detail.segments"),
                    b.segments.map_or("—".to_owned(), |n| i18n::group(lang, n)),
                ),
            ]
        }
        Some(None) => vec![Line::from(Span::styled(
            tx.t("detail.backup-off"),
            th.dim(),
        ))],
        None => vec![Line::from(Span::styled(
            match &m.cluster.backup.error {
                Some(fail) => error_text(m, fail),
                None => tx.t("state.loading"),
            },
            th.dim(),
        ))],
    }
}

fn draw_detail(f: &mut Frame, area: Rect, m: &Model) {
    scrolling(f, area, detail_lines(m), m.cluster.scroll);
}

// ---------- the charts of a node ----------

pub fn fmt_unit(lang: Lang, unit: Unit, value: f64) -> String {
    match unit {
        Unit::Bytes => i18n::bytes(lang, value.max(0.0) as u64),
        Unit::Count => i18n::group(lang, value.round().max(0.0) as u64),
        Unit::PerSecond => format!("{}/s", i18n::decimal(lang, value, 2)),
        Unit::Seconds if value < 1.0 => format!("{} ms", i18n::decimal(lang, value * 1000.0, 1)),
        Unit::Seconds => format!("{} s", i18n::decimal(lang, value, 2)),
    }
}

/// How many charts the grid of the charts view shows at once: the screen split in four.
pub const GRID: usize = 4;

/// The nodes that have something to chart, in the order of the table.
fn charted_nodes(m: &Model) -> Vec<String> {
    cluster::rows(m)
        .into_iter()
        .filter(|row| {
            MON_CHARTS.iter().any(|def| {
                def.lines
                    .first()
                    .is_some_and(|(s, _)| m.history.series(&row.id, *s).is_some())
            })
        })
        .map(|row| row.id)
        .collect()
}

/// The charts of the whole cluster: every indicator, one line per node, four charts per screen and a scroll
/// through all of them (the first visible one is `monitor.chart`).
fn draw_monitor(f: &mut Frame, area: Rect, m: &Model) {
    let tx = m.tx();
    let th = m.theme;
    let nodes = charted_nodes(m);
    let total = MON_CHARTS.len();
    let first = m.cluster.monitor.chart.min(total.saturating_sub(GRID));
    let last = (first + GRID).min(total);
    let [head_area, grid_area, legend_area, note_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(5),
        Constraint::Length(1),
        Constraint::Length(2),
    ])
    .areas(area);

    let up = if first > 0 {
        if th.ascii {
            "^"
        } else {
            "↑"
        }
    } else {
        " "
    };
    let down = if last < total {
        if th.ascii {
            "v"
        } else {
            "↓"
        }
    } else {
        " "
    };
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                format!(
                    "{} ",
                    tx.tw(
                        "chart.range",
                        &[("from", &(first + 1)), ("to", &last), ("total", &total)],
                    )
                ),
                th.title(),
            ),
            Span::styled(format!("{up}{down}"), th.accent()),
        ])),
        head_area,
    );

    let visible = &MON_CHARTS[first..last];
    let cells: Vec<Rect> = if grid_area.width >= 70 {
        let rows = Layout::vertical([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(grid_area);
        rows.iter()
            .flat_map(|row| {
                Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
                    .split(*row)
                    .to_vec()
            })
            .collect()
    } else {
        Layout::vertical([Constraint::Ratio(1, GRID as u32); GRID])
            .split(grid_area)
            .to_vec()
    };
    for (def, cell) in visible.iter().zip(cells) {
        draw_chart(f, cell, m, def, &nodes);
    }

    // The legend: one marker and one color per node (TUI-MON-02).
    let legend: Vec<Span> = nodes
        .iter()
        .enumerate()
        .map(|(i, id)| Span::styled(format!(" {} {} ", th.marker(i), short(id)), th.series(i)))
        .collect();
    f.render_widget(Paragraph::new(Line::from(legend)), legend_area);

    // What the charts honestly show (TUI-MON-05) and their state.
    let at = m.cluster.monitor.frozen.unwrap_or(m.now);
    let window = window_ms(m);
    let collected = visible
        .iter()
        .filter_map(|def| def.lines.first())
        .flat_map(|(series, _)| {
            nodes
                .iter()
                .filter_map(|id| m.history.series(id, *series).map(|r| r.collected_ms(at)))
        })
        .max()
        .unwrap_or(0);
    let mut note = if collected < window {
        tx.tw(
            "chart.collected",
            &[("time", &i18n::duration(collected / 1000))],
        )
    } else {
        tx.t("chart.history-note")
    };
    if m.cluster.monitor.frozen.is_some() {
        note.push_str(&format!(" · {}", tx.t("chart.paused")));
    }
    note.push_str(&format!(
        " · {}",
        tx.t(if m.cluster.monitor.fixed_scale {
            "chart.scale-fixed"
        } else {
            "chart.scale-auto"
        })
    ));
    let partial = visible.iter().any(|def| {
        def.lines.iter().take(1).any(|(series, _)| {
            nodes
                .iter()
                .any(|id| line_points(m, id, *series).iter().any(|p| p.partial))
        })
    });
    if partial {
        note.push_str(&format!(" · {}", tx.t("chart.partial-samples")));
    }
    f.render_widget(
        Paragraph::new(note)
            .style(th.dim())
            .wrap(Wrap { trim: false }),
        note_area,
    );
}

/// One chart of the grid: the indicator's main series, a line per node.
fn draw_chart(f: &mut Frame, area: Rect, m: &Model, def: &cluster::ChartDef, nodes: &[String]) {
    let tx = m.tx();
    let th = m.theme;
    let lang = m.lang;
    let Some((series, _)) = def.lines.first().copied() else {
        return;
    };
    let at = m.cluster.monitor.frozen.unwrap_or(m.now);
    let window_s = window_ms(m) as f64 / 1000.0;
    let lines: Vec<Vec<history::Point>> = nodes
        .iter()
        .map(|id| line_points(m, id, series))
        .collect();
    let xy: Vec<Vec<(f64, f64)>> = lines
        .iter()
        .map(|points| {
            points
                .iter()
                .map(|p| ((p.at as f64 - at as f64) / 1000.0, p.value))
                .collect()
        })
        .collect();
    let values: Vec<f64> = lines.iter().flatten().map(|p| p.value).collect();
    if values.is_empty() {
        f.render_widget(
            Block::bordered().title(format!(" {} ", tx.t(def.title))),
            area,
        );
        let inner = area.inner(ratatui::layout::Margin::new(2, 1));
        draw_message(f, inner, tx.t("chart.no-data"), th.dim());
        return;
    }

    // The cluster figure of the chart: the sum, or the worst node for a round trip.
    let lasts: Vec<f64> = lines
        .iter()
        .filter_map(|l| l.last().map(|p| p.value))
        .collect();
    let figure = if def.unit == Unit::Seconds {
        let worst = lasts.iter().copied().fold(0.0, f64::max);
        tx.tw("chart.worst", &[("value", &fmt_unit(lang, def.unit, worst))])
    } else {
        let sum: f64 = lasts.iter().sum();
        tx.tw("chart.total", &[("value", &fmt_unit(lang, def.unit, sum))])
    };

    let (low, high) = scale(m, nodes, def, &values);
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
        .block(Block::bordered().title(format!(" {} · {} ", tx.t(def.title), figure)))
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
                    fmt_unit(lang, def.unit, low),
                    fmt_unit(lang, def.unit, mid),
                    fmt_unit(lang, def.unit, high),
                ])
                .style(th.dim()),
        );
    f.render_widget(chart, area);
}

/// The vertical bounds: the window's own range with some air, or from zero to the largest value ever kept.
fn scale(m: &Model, nodes: &[String], def: &cluster::ChartDef, values: &[f64]) -> (f64, f64) {
    if m.cluster.monitor.fixed_scale {
        let high = def
            .lines
            .iter()
            .take(1)
            .flat_map(|(series, _)| nodes.iter().map(move |id| (id, *series)))
            .flat_map(|(id, series)| {
                m.history
                    .series(id, series)
                    .map(|r| r.window(m.now, m.history.retention_ms()))
                    .unwrap_or_default()
            })
            .map(|p| p.value)
            .fold(f64::NEG_INFINITY, f64::max);
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
    let high = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let pad = if (high - low).abs() < f64::EPSILON {
        high.abs().max(1.0) * 0.1
    } else {
        (high - low) * 0.1
    };
    let floor = if low >= 0.0 {
        (low - pad).max(0.0)
    } else {
        low - pad
    };
    (floor, high + pad)
}
