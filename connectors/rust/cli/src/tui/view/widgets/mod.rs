//! The small pieces the screens share (doc 34, `view/widgets`): popup placement, scrolling text with its bar, the
//! selected row, the "incomplete" seal and the missing-permission notice.

use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Clear, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap,
};
use ratatui::Frame;

use crate::tui::api::{Missing, Severity};
use crate::tui::model::{short_id, Fail, Model};
use crate::tui::theme::Theme;

/// A rectangle of at most `width` × `height`, centered in `area`.
pub fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    let [_, row, _] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(height),
        Constraint::Fill(1),
    ])
    .areas(area);
    let [_, cell, _] = Layout::horizontal([
        Constraint::Fill(1),
        Constraint::Length(width),
        Constraint::Fill(1),
    ])
    .areas(row);
    cell
}

/// Clears a popup's area and draws its frame; returns the inside.
pub fn popup(f: &mut Frame, area: Rect, title: &str, style: Style) -> Rect {
    f.render_widget(Clear, area);
    let block = Block::bordered()
        .title(format!(" {title} "))
        .border_style(style);
    let inner = block.inner(area);
    f.render_widget(block, area);
    inner
}

/// Text that scrolls by `offset` lines; the bar shows only when the text is longer than the area (TUI-FRM-04).
/// Returns the offset actually used (clamped).
pub fn scrolling(f: &mut Frame, area: Rect, lines: Vec<Line<'static>>, offset: u16) -> u16 {
    let total = lines.len();
    let height = usize::from(area.height);
    let max = total.saturating_sub(height);
    let used = usize::from(offset).min(max);
    let overflow = total > height;
    let text_area = if overflow && area.width > 2 {
        Rect {
            width: area.width - 1,
            ..area
        }
    } else {
        area
    };
    f.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((u16::try_from(used).unwrap_or(u16::MAX), 0)),
        text_area,
    );
    if overflow {
        let mut state = ScrollbarState::new(max).position(used);
        f.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight),
            area,
            &mut state,
        );
    }
    u16::try_from(used).unwrap_or(u16::MAX)
}

/// The seal of an incomplete read: which nodes did not answer, and why (D-TUI-10).
pub fn partial_seal(m: &Model, theme: Theme, missing: &[Missing]) -> Line<'static> {
    let tx = m.tx();
    let names: Vec<String> = missing
        .iter()
        .map(|n| {
            let short = short_id(&n.node_id);
            if n.reason.is_empty() {
                short
            } else {
                format!("{short} ({})", n.reason)
            }
        })
        .collect();
    let text = if names.is_empty() {
        tx.t("seal.partial")
    } else {
        tx.tw("seal.partial-nodes", &[("nodes", &names.join(", "))])
    };
    Line::from(Span::styled(
        format!("{} {text}", theme.icon(Severity::Warning)),
        theme.warning(),
    ))
}

/// The explanation of a refused call (TUI-FRM-10): what is missing, in words.
pub fn denied_text(m: &Model, fail: &Fail) -> String {
    let tx = m.tx();
    match fail.required.as_deref() {
        Some("root") => tx.t("denied.root"),
        Some(permission) => tx.tw("denied.permission", &[("permission", &permission)]),
        None if fail.status == 401 => tx.t("denied.unauthorized"),
        None => tx.t("denied.generic"),
    }
}

pub fn centered_line(text: impl Into<String>, style: Style) -> Paragraph<'static> {
    Paragraph::new(Line::from(Span::styled(text.into(), style))).alignment(Alignment::Center)
}

/// A short form of a node id for tables: the first eight characters.
pub fn short(id: &str) -> String {
    short_id(id)
}

#[cfg(test)]
mod tests;
