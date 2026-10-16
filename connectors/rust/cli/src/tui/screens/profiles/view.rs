//! The drawing of the Profiles tab: the profiles of the file, which one is in use and which is the default.

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Cell, Paragraph, Row as TableRow, Table, Wrap};
use ratatui::Frame;

use crate::profile::{Profile, DEFAULT_URL};
use crate::tui::model::Model;
use crate::tui::screens::profiles::selected_index;

/// How a profile holds its token, for the table: never the token itself.
pub fn token_kind(m: &Model, profile: &Profile) -> String {
    let tx = m.tx();
    if profile.get("token-file").is_some() {
        tx.t("profiles.token-file")
    } else if profile.get("token").is_some() {
        tx.t("profiles.token-inline")
    } else {
        tx.t("profiles.token-none")
    }
}

pub fn draw(f: &mut Frame, area: Rect, m: &Model) {
    let tx = m.tx();
    let th = m.theme;
    let file = &m.profiles.file;
    if file.profiles.is_empty() {
        f.render_widget(
            Paragraph::new(vec![
                Line::from(Span::styled(tx.t("profiles.none"), th.warning())),
                Line::default(),
                Line::from(tx.tw("profiles.builtin", &[("url", &DEFAULT_URL)])),
                Line::from(tx.t("profiles.create-first")),
            ])
            .block(Block::bordered())
            .wrap(Wrap { trim: false }),
            area,
        );
        return;
    }
    let [table, detail] =
        Layout::vertical([Constraint::Min(5), Constraint::Length(10)]).areas(area);
    let selected = selected_index(m);
    let capacity = usize::from(table.height.saturating_sub(3)).max(1);
    let start = selected.map_or(0, |s| s.saturating_sub(capacity - 1));
    let rows: Vec<TableRow> = file
        .profiles
        .iter()
        .enumerate()
        .skip(start)
        .take(capacity)
        .map(|(i, (name, profile))| {
            let marker = if selected == Some(i) {
                th.selector()
            } else {
                "  "
            };
            let default = if file.default.as_deref() == Some(name.as_str()) {
                "●"
            } else {
                "○"
            };
            let in_use = if *name == m.profile {
                tx.t("profiles.in-use")
            } else {
                String::new()
            };
            let row = TableRow::new([
                Cell::from(format!("{marker}{name}")),
                Cell::from(profile.get("url").unwrap_or(DEFAULT_URL).to_owned()),
                Cell::from(token_kind(m, profile)),
                Cell::from(default),
                Cell::from(in_use).style(th.ok()),
            ]);
            if selected == Some(i) {
                row.style(th.selected())
            } else {
                row
            }
        })
        .collect();
    let widths = [
        Constraint::Length(16),
        Constraint::Fill(1),
        Constraint::Length(12),
        Constraint::Length(8),
        Constraint::Length(9),
    ];
    f.render_widget(
        Table::new(rows, widths)
            .header(
                TableRow::new([
                    tx.t("col.name"),
                    tx.t("col.url"),
                    tx.t("col.token"),
                    tx.t("col.default"),
                    String::new(),
                ])
                .style(th.title()),
            )
            .block(Block::bordered().border_style(th.accent())),
        table,
    );
    draw_detail(f, detail, m);
}

fn draw_detail(f: &mut Frame, area: Rect, m: &Model) {
    let tx = m.tx();
    let th = m.theme;
    let block = Block::bordered().title(format!(" {} ", tx.t("profiles.detail")));
    let Some(i) = selected_index(m) else {
        f.render_widget(block, area);
        return;
    };
    let (name, profile) = &m.profiles.file.profiles[i];
    let mut lines = vec![Line::from(Span::styled(name.clone(), th.title()))];
    for (field, value) in profile.entries() {
        // A token kept in the file is never drawn.
        let shown = if field == "token" {
            "••••••••".to_owned()
        } else {
            value.to_owned()
        };
        lines.push(Line::from(vec![
            Span::styled(format!("{field:<16}"), th.dim()),
            Span::styled(shown, Style::default()),
        ]));
    }
    lines.push(Line::default());
    lines.push(Line::from(Span::styled(
        tx.t("profiles.session-note"),
        th.dim(),
    )));
    f.render_widget(
        Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false }),
        area,
    );
}
