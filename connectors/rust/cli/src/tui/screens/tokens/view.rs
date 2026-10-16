//! The drawing of the Tokens tab: the table of tokens and the detail of the selected one.

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Cell, Paragraph, Row as TableRow, Table, Wrap};
use ratatui::Frame;

use crate::tui::api::{Severity, Token};
use crate::tui::i18n;
use crate::tui::model::Model;
use crate::tui::screens::cluster::view::error_text;
use crate::tui::screens::tokens::{self, availability, selected_index, visible, Availability};
use crate::tui::view::widgets::short;

pub fn draw(f: &mut Frame, area: Rect, m: &Model) {
    let tx = m.tx();
    let th = m.theme;
    match availability(m) {
        Availability::Unknown => {
            message(f, area, tx.t("state.loading"), th.dim());
            return;
        }
        Availability::NoAuth => {
            message(f, area, tx.t("tokens.no-auth"), th.warning());
            return;
        }
        Availability::NotAdmin => {
            let who =
                m.me.data
                    .as_ref()
                    .and_then(|me| me.name.clone())
                    .unwrap_or_else(|| "?".to_owned());
            let text = format!(
                "{}\n\n{}",
                tx.t("denied.root"),
                tx.tw("tokens.using", &[("name", &who), ("profile", &m.profile)])
            );
            message(f, area, text, th.warning());
            return;
        }
        Availability::Ready => {}
    }
    let Some(list) = &m.tokens.list.data else {
        let text = match &m.tokens.list.error {
            Some(fail) if fail.status == 404 => tx.t("tokens.no-auth"),
            Some(fail) => error_text(m, fail),
            None => tx.t("state.loading"),
        };
        message(f, area, text, th.dim());
        return;
    };
    let tokens = visible(m);
    let [head, table, detail] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(5),
        Constraint::Length(9),
    ])
    .areas(area);
    let mut spans = vec![
        Span::styled(tx.tn("tokens.count", list.len() as u64), th.title()),
        Span::styled(
            format!(
                "  {}",
                tx.tw("tokens.sorted", &[("by", &tx.t(m.tokens.sort.key()))])
            ),
            th.dim(),
        ),
    ];
    if !m.tokens.filter.is_empty() {
        spans.push(Span::styled(
            format!(
                "  {}",
                tx.tw(
                    "tokens.filtered",
                    &[("text", &m.tokens.filter), ("shown", &tokens.len())]
                )
            ),
            th.accent(),
        ));
    }
    if m.tokens.list.is_stale() {
        spans.push(Span::styled(
            format!("  {} {}", th.icon(Severity::Warning), tx.t("state.stale")),
            th.warning(),
        ));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), head);
    draw_table(f, table, m, &tokens);
    draw_detail(f, detail, m, &tokens);
}

fn message(f: &mut Frame, area: Rect, text: String, style: Style) {
    f.render_widget(
        Paragraph::new(text).style(style).wrap(Wrap { trim: false }),
        area,
    );
}

/// How soon a token expires: past, within a week, or later (or never).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Expiry {
    Never,
    Later,
    Soon,
    Expired,
}

pub fn expiry_of(m: &Model, token: &Token) -> Expiry {
    let Some(at) = token.expires_at.as_deref().and_then(i18n::parse_instant) else {
        return Expiry::Never;
    };
    let left = at - m.wall_ms;
    if left <= 0 {
        Expiry::Expired
    } else if left <= tokens::SOON_DAYS * 86_400_000 {
        Expiry::Soon
    } else {
        Expiry::Later
    }
}

fn expiry_text(m: &Model, token: &Token) -> String {
    match token.expires_at.as_deref().and_then(i18n::parse_instant) {
        Some(at) => i18n::date_time(m.lang, at),
        None => m.tx().t("tokens.never"),
    }
}

fn draw_table(f: &mut Frame, area: Rect, m: &Model, tokens: &[Token]) {
    let tx = m.tx();
    let th = m.theme;
    let selected = selected_index(m, tokens);
    // The table shows a window of the rows that always holds the selected one.
    let capacity = usize::from(area.height.saturating_sub(3)).max(1);
    let start = selected.map_or(0, |s| s.saturating_sub(capacity - 1));
    // The id has its place in the detail; the table shows it only where there is room.
    let wide = area.width >= 100;
    let mut headers = vec![tx.t("col.name")];
    let mut widths = vec![Constraint::Length(if wide { 22 } else { 18 })];
    if wide {
        headers.push(tx.t("col.id"));
        widths.push(Constraint::Length(14));
    }
    headers.extend([
        tx.t("col.permissions"),
        tx.t("col.prefixes"),
        tx.t("col.expires"),
    ]);
    widths.extend([
        Constraint::Length(22),
        Constraint::Fill(1),
        Constraint::Length(if wide { 20 } else { 11 }),
    ]);
    let rows: Vec<TableRow> = tokens
        .iter()
        .enumerate()
        .skip(start)
        .take(capacity)
        .map(|(i, token)| {
            let marker = if selected == Some(i) {
                th.selector()
            } else {
                "  "
            };
            let expiry = expiry_of(m, token);
            let expires = if wide {
                expiry_text(m, token)
            } else {
                expiry_text(m, token).chars().take(10).collect()
            };
            let expires_style = match expiry {
                Expiry::Expired => th.critical(),
                Expiry::Soon => th.warning(),
                _ => Style::default(),
            };
            let mut cells = vec![Cell::from(format!("{marker}{}", token.name))];
            if wide {
                cells.push(Cell::from(short(&token.id)));
            }
            cells.push(Cell::from(token.permissions.join(",")));
            cells.push(Cell::from(if token.prefixes.is_empty() {
                "—".to_owned()
            } else {
                token.prefixes.join(" ")
            }));
            cells.push(Cell::from(expires).style(expires_style));
            let row = TableRow::new(cells);
            if selected == Some(i) {
                row.style(th.selected())
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

fn draw_detail(f: &mut Frame, area: Rect, m: &Model, tokens: &[Token]) {
    let tx = m.tx();
    let th = m.theme;
    let block = Block::bordered().title(format!(" {} ", tx.t("tokens.detail")));
    let Some(token) = selected_index(m, tokens).map(|i| &tokens[i]) else {
        f.render_widget(
            Paragraph::new(tx.t("tokens.none"))
                .style(th.dim())
                .block(block),
            area,
        );
        return;
    };
    let width = [
        "col.name",
        "col.id",
        "col.permissions",
        "col.prefixes",
        "tokens.created",
        "col.expires",
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
    let created = token
        .created_at
        .as_deref()
        .and_then(i18n::parse_instant)
        .map_or_else(|| "—".to_owned(), |at| i18n::date_time(m.lang, at));
    let expires_style = match expiry_of(m, token) {
        Expiry::Expired => th.critical(),
        Expiry::Soon => th.warning(),
        _ => Style::default(),
    };
    let mut expires = expiry_text(m, token);
    match expiry_of(m, token) {
        Expiry::Expired => expires = format!("{expires} ({})", tx.t("tokens.expired")),
        Expiry::Soon => expires = format!("{expires} ({})", tx.t("tokens.soon")),
        _ => {}
    }
    let lines = vec![
        line(&tx.t("col.name"), token.name.clone(), th.title()),
        line(&tx.t("col.id"), token.id.clone(), Style::default()),
        line(
            &tx.t("col.permissions"),
            token.permissions.join(", "),
            Style::default(),
        ),
        line(
            &tx.t("col.prefixes"),
            if token.prefixes.is_empty() {
                tx.t("tokens.all-topics")
            } else {
                token.prefixes.join(", ")
            },
            Style::default(),
        ),
        line(&tx.t("tokens.created"), created, Style::default()),
        line(&tx.t("col.expires"), expires, expires_style),
        Line::default(),
        Line::from(Span::styled(tx.t("tokens.immutable"), th.dim())),
    ];
    f.render_widget(
        Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false }),
        area,
    );
}
