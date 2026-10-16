//! The popups of the Tokens and Profiles tabs: the choice of a profile, the forms and the secret of a new token.

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};
use ratatui::Frame;

use crate::tui::api::Severity;
use crate::tui::editor::{Editor, Kind, AREA_ROWS};
use crate::tui::model::{Model, Secret};
use crate::tui::screens::tokens::MASK_AFTER_MS;
use crate::tui::view::widgets::{centered, popup};

/// The choice of a profile (at the start, or from `P`).
pub fn draw_profiles(f: &mut Frame, area: Rect, m: &Model, selected: usize) {
    let tx = m.tx();
    let th = m.theme;
    let file = &m.profiles.file;
    let height = u16::try_from(file.profiles.len())
        .unwrap_or(u16::MAX)
        .saturating_add(5)
        .min(area.height.saturating_sub(2));
    let rect = centered(area, 70, height);
    let key = if m.awaiting_profile {
        "profiles.pick-title"
    } else {
        "profiles.switch-title"
    };
    let inner = popup(f, rect, &tx.t(key), th.accent());
    let [list, hint] = Layout::vertical([Constraint::Min(1), Constraint::Length(2)]).areas(inner);
    let capacity = usize::from(list.height).max(1);
    let skip = selected.saturating_sub(capacity - 1);
    let lines: Vec<Line> = file
        .profiles
        .iter()
        .enumerate()
        .skip(skip)
        .take(capacity)
        .map(|(i, (name, profile))| {
            let default = if file.default.as_deref() == Some(name.as_str()) {
                "●"
            } else {
                "○"
            };
            let style = if i == selected {
                th.selected()
            } else {
                Style::default()
            };
            let url = profile.get("url").unwrap_or(crate::profile::DEFAULT_URL);
            Line::from(Span::styled(
                format!(
                    "{}{default} {name:<20} {url}",
                    if i == selected { th.selector() } else { "  " }
                ),
                style,
            ))
        })
        .collect();
    f.render_widget(Paragraph::new(lines), list);
    let hint_key = if m.awaiting_profile {
        "profiles.pick-hint"
    } else {
        "profiles.switch-hint"
    };
    f.render_widget(
        Paragraph::new(tx.t(hint_key))
            .style(th.dim())
            .wrap(Wrap { trim: false }),
        hint,
    );
}

/// A form: the fields, what is wrong, the result of a test and the keys.
pub fn draw_editor(f: &mut Frame, area: Rect, m: &Model, editor: &Editor) {
    let tx = m.tx();
    let th = m.theme;
    let areas = editor
        .fields
        .iter()
        .filter(|f| f.kind == Kind::Area)
        .count();
    let rows = u16::try_from(editor.fields.len() + areas * AREA_ROWS).unwrap_or(u16::MAX);
    let height = rows.saturating_add(9).min(area.height.saturating_sub(2));
    let rect = centered(area, 76, height);
    let inner = popup(f, rect, &tx.t(editor.purpose.title()), th.accent());
    let [body, help, status, hint] = Layout::vertical([
        Constraint::Min(1),
        Constraint::Length(2),
        Constraint::Length(2),
        Constraint::Length(1),
    ])
    .areas(inner);
    f.render_widget(
        Paragraph::new(tx.t(if m.awaiting_profile {
            "editor.help-first-profile"
        } else {
            editor.purpose.help()
        }))
        .style(th.dim())
        .wrap(Wrap { trim: false }),
        help,
    );
    let label_width = editor
        .fields
        .iter()
        .map(|field| tx.t(field.label).chars().count())
        .max()
        .unwrap_or(8)
        .min(22);
    let mut lines: Vec<Line> = Vec::new();
    for (i, field) in editor.fields.iter().enumerate() {
        let focused = i == editor.focus;
        let label = format!("{:<label_width$} ", tx.t(field.label));
        let marker = if focused { th.selector() } else { "  " };
        let mut spans = vec![Span::styled(
            format!("{marker}{label}"),
            if focused { th.key() } else { th.dim() },
        )];
        let value_style = if field.locked.is_some() {
            th.dim()
        } else {
            Style::default()
        };
        let mut below: Vec<Line> = Vec::new();
        match field.kind {
            Kind::Area => {
                below = area_lines(field, focused, usize::from(body.width), th);
                spans.push(Span::styled(
                    tx.tw(
                        "editor.area-lines",
                        &[("n", &field.value.split('\n').count())],
                    ),
                    th.dim(),
                ));
            }
            Kind::Check => {
                let mark = if field.checked { "[x]" } else { "[ ]" };
                spans.push(Span::styled(mark, value_style));
            }
            Kind::Text | Kind::Secret => {
                let shown: String = if field.kind == Kind::Secret {
                    "•".repeat(field.value.chars().count())
                } else {
                    field.value.clone()
                };
                spans.push(Span::styled(shown, value_style));
                if focused && field.locked.is_none() {
                    spans.push(Span::styled("▏", th.accent()));
                }
            }
        }
        if let Some(reason) = field.locked {
            spans.push(Span::styled(format!("  ({})", tx.t(reason)), th.dim()));
        }
        lines.push(Line::from(spans));
        lines.extend(below);
    }
    f.render_widget(Paragraph::new(lines), body);
    let mut notes: Vec<Line> = Vec::new();
    if editor.busy {
        notes.push(Line::from(Span::styled(tx.t("editor.saving"), th.accent())));
    } else if let Some(error) = &editor.error {
        notes.push(Line::from(Span::styled(
            format!("{} {error}", th.icon(Severity::Critical)),
            th.critical(),
        )));
    } else if let Some((failed, text)) = &editor.note {
        let severity = if *failed {
            Severity::Warning
        } else {
            Severity::Ok
        };
        notes.push(Line::from(Span::styled(
            format!("{} {text}", th.icon(severity)),
            th.severity(severity),
        )));
    }
    f.render_widget(Paragraph::new(notes).wrap(Wrap { trim: false }), status);
    let hint_key = if editor.purpose.is_profile() {
        "editor.hint-profile"
    } else if editor.purpose.is_message() {
        "editor.hint-publish"
    } else {
        "editor.hint"
    };
    f.render_widget(Paragraph::new(tx.t(hint_key)).style(th.dim()), hint);
}

/// The lines of an area: a few at a time, numbered, with the cursor. The window follows the cursor.
fn area_lines(
    field: &crate::tui::editor::Field,
    focused: bool,
    width: usize,
    th: crate::tui::theme::Theme,
) -> Vec<Line<'static>> {
    let (cursor_line, cursor_col) = field.line_col();
    let top = (cursor_line + 1).saturating_sub(AREA_ROWS);
    let gutter = 5;
    let room = width.saturating_sub(gutter + 2).max(1);
    let all: Vec<&str> = field.value.split('\n').collect();
    (top..top + AREA_ROWS)
        .map(|n| {
            let number = if n < all.len() {
                format!("{:>3} │", n + 1)
            } else {
                "    │".to_owned()
            };
            let mut spans = vec![Span::styled(format!("  {number}"), th.dim())];
            let text: Vec<char> = all.get(n).map_or(Vec::new(), |l| l.chars().collect());
            let here = focused && n == cursor_line;
            // A long line scrolls sideways to keep the cursor in view.
            let start = if here {
                (cursor_col + 1).saturating_sub(room)
            } else {
                0
            };
            let shown: Vec<char> = text.iter().skip(start).take(room).copied().collect();
            if here {
                let at = cursor_col - start;
                let before: String = shown.iter().take(at).collect();
                let after: String = shown.iter().skip(at).collect();
                spans.push(Span::raw(before));
                spans.push(Span::styled("▏", th.accent()));
                spans.push(Span::raw(after));
            } else {
                spans.push(Span::raw(shown.into_iter().collect::<String>()));
            }
            Line::from(spans)
        })
        .collect()
}

/// Whether the secret on the screen is hidden now: after a while, or when the terminal lost the focus.
pub fn is_masked(m: &Model, secret: &Secret) -> bool {
    !m.focused || m.now.saturating_sub(secret.shown_at) > MASK_AFTER_MS
}

/// The secret of a new token: shown once, masked after a while or when the terminal loses the focus.
pub fn draw_secret(f: &mut Frame, area: Rect, m: &Model, secret: &Secret) {
    let tx = m.tx();
    let th = m.theme;
    let masked = is_masked(m, secret);
    let shown = if masked {
        // The id part of `tb_<id>.<secret>` is not secret; the rest becomes dots.
        match secret.token.split_once('.') {
            Some((id, rest)) => format!("{id}.{}", "•".repeat(rest.chars().count().min(32))),
            None => "•".repeat(24),
        }
    } else {
        secret.token.clone()
    };
    let mut lines = vec![
        Line::from(Span::styled(
            tx.tw("secret.created", &[("name", &secret.name)]),
            th.title(),
        )),
        Line::default(),
        Line::from(Span::styled(tx.t("secret.once"), th.warning())),
        Line::default(),
        Line::from(Span::styled(shown, th.accent())),
        Line::default(),
    ];
    if masked {
        lines.push(Line::from(Span::styled(tx.t("secret.masked"), th.dim())));
    }
    if let Some((_, old)) = &secret.replaces {
        let mark = if secret.revoke_old { "[x]" } else { "[ ]" };
        lines.push(Line::from(vec![
            Span::styled(format!("{mark} "), th.key()),
            Span::raw(tx.tw("secret.revoke-old", &[("name", old)])),
        ]));
    }
    if secret.warned && !(secret.copied || secret.saved) {
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            format!("{} {}", th.icon(Severity::Warning), tx.t("secret.not-kept")),
            th.warning(),
        )));
    }
    let status = match (secret.copied, secret.saved) {
        (true, true) => tx.t("secret.copied-saved"),
        (true, false) => tx.t("secret.copied"),
        (false, true) => tx.t("secret.saved"),
        (false, false) => String::new(),
    };
    if !status.is_empty() {
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(status, th.ok())));
    }
    let height = u16::try_from(lines.len())
        .unwrap_or(u16::MAX)
        .saturating_add(4);
    let rect = centered(area, 76, height.min(area.height.saturating_sub(2)));
    let inner = popup(f, rect, &tx.t("secret.title"), th.warning());
    let [body, hint] = Layout::vertical([Constraint::Min(1), Constraint::Length(2)]).areas(inner);
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), body);
    let hint_key = if secret.replaces.is_some() {
        "secret.hint-clone"
    } else {
        "secret.hint"
    };
    f.render_widget(
        Paragraph::new(tx.t(hint_key))
            .style(th.dim())
            .wrap(Wrap { trim: false }),
        hint,
    );
}
