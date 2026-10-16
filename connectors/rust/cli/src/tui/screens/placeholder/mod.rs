//! The tabs whose screen is not built yet (doc 34, section 11): they open, say what is coming and where the
//! same thing can be done today, so the tabs navigate from the first phase.

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph, Wrap};
use ratatui::Frame;

use crate::tui::model::{Model, Tab};

pub fn draw(f: &mut Frame, area: Rect, m: &Model, tab: Tab) {
    let tx = m.tx();
    let th = m.theme;
    let key = match tab {
        Tab::Profiles => "placeholder.profiles",
        Tab::Topics => "placeholder.topics",
        Tab::Tokens => "placeholder.tokens",
        Tab::Messages => "placeholder.messages",
        Tab::Cluster => "placeholder.topics",
    };
    let lines = vec![
        Line::from(Span::styled(tx.t(tab.name_key()), th.title())),
        Line::default(),
        Line::from(tx.t(key)),
        Line::default(),
        Line::from(Span::styled(tx.t("placeholder.hint"), th.dim())),
    ];
    f.render_widget(
        Paragraph::new(lines)
            .block(Block::bordered())
            .wrap(Wrap { trim: false }),
        area,
    );
}
