//! `view(model) -> frame` (doc 34, section 4): draws the whole interface from the model and changes nothing. The
//! frame is a status line, the tabs, the screen of the tab, a footer, and at most one popup over them (TUI-FRM-*).

use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph, Tabs, Wrap};
use ratatui::Frame;

use crate::tui::api::Severity;
use crate::tui::i18n;
use crate::tui::keymap::{self, Action};
use crate::tui::model::{ClusterView, Model, Tab, TopicTab, TABS};
use crate::tui::screens;
use crate::tui::screens::cluster as cluster_screen;

pub mod modal;
pub mod widgets;

/// The smallest terminal the interface is laid out for (TUI-FRM-06).
pub const MIN_SIZE: (u16, u16) = (80, 24);

pub fn view(f: &mut Frame, m: &Model) {
    let area = f.area();
    if area.width < MIN_SIZE.0 || area.height < MIN_SIZE.1 {
        draw_too_small(f, area, m);
        return;
    }
    let [status, tabs, body, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(5),
        Constraint::Length(1),
    ])
    .areas(area);
    draw_status(f, status, m);
    draw_tabs(f, tabs, m);
    match m.tab {
        Tab::Cluster => cluster_screen::view::draw(f, body, m),
        Tab::Tokens => screens::tokens::view::draw(f, body, m),
        Tab::Topics if m.topics.monitor.is_some() => {
            screens::topics::monitor::view::draw(f, body, m)
        }
        Tab::Topics => screens::topics::view::draw(f, body, m),
        Tab::Profiles => screens::profiles::view::draw(f, body, m),
    }
    draw_footer(f, footer, m);
    modal::draw(f, area, m);
}

fn draw_too_small(f: &mut Frame, area: Rect, m: &Model) {
    let tx = m.tx();
    let text = tx.tw(
        "frame.too-small",
        &[
            ("width", &area.width),
            ("height", &area.height),
            ("min_width", &MIN_SIZE.0),
            ("min_height", &MIN_SIZE.1),
        ],
    );
    f.render_widget(
        Paragraph::new(text)
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true })
            .style(m.theme.warning()),
        area,
    );
}

/// Profile, address, the worst condition (a word and a symbol) and the clock (TUI-FRM-01).
fn draw_status(f: &mut Frame, area: Rect, m: &Model) {
    let tx = m.tx();
    let th = m.theme;
    let (severity, word) = m.health();
    let label = match word.as_str() {
        "ok" | "offline" | "degraded" | "unknown" => tx.t(&format!("health.{word}")),
        other => tx.t(&format!("condition.{other}")),
    };
    let icon = if word == "offline" {
        th.offline()
    } else {
        th.icon(severity)
    };
    let mut spans = vec![
        Span::styled(format!(" {} ", m.profile), th.title()),
        Span::styled(m.url.clone(), th.dim()),
        Span::raw("  "),
        Span::styled(format!("{icon} {label}"), th.severity(severity)),
    ];
    let partial = m.status.data.as_ref().is_some_and(|s| s.partial)
        || m.cluster.nodes.data.as_ref().is_some_and(|n| n.partial);
    if partial {
        spans.push(Span::styled(
            format!("  {} {}", th.icon(Severity::Warning), tx.t("seal.short")),
            th.warning(),
        ));
    }
    let clock = format!("{} UTC ", i18n::clock(m.wall_ms));
    let used: usize = spans.iter().map(Span::width).sum();
    let pad = usize::from(area.width)
        .saturating_sub(used + clock.chars().count())
        .max(1);
    spans.push(Span::raw(" ".repeat(pad)));
    spans.push(Span::styled(clock, th.dim()));
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn draw_tabs(f: &mut Frame, area: Rect, m: &Model) {
    let tx = m.tx();
    let titles: Vec<String> = TABS
        .iter()
        .enumerate()
        .map(|(i, tab)| format!("{} {}", i + 1, tx.t(tab.name_key())))
        .collect();
    f.render_widget(
        Tabs::new(titles)
            .select(m.tab.index())
            .highlight_style(m.theme.selected())
            .divider("│")
            .block(Block::default()),
        area,
    );
}

/// The short label of an action in the footer.
fn hint_key(action: Action) -> &'static str {
    match action {
        Action::Detail => "hint.detail",
        Action::Enter => "hint.actions",
        Action::TopicCharts => "hint.charts",
        Action::Drain => "hint.drain",
        Action::Return => "hint.return",
        Action::FlushNode => "hint.flush",
        Action::FlushAll => "hint.flush-all",
        Action::StopCluster => "hint.stop",
        Action::Monitor => "hint.monitor",
        Action::Help => "hint.help",
        Action::Palette => "hint.palette",
        Action::Quit => "hint.quit",
        Action::Esc => "hint.back",
        Action::Left | Action::Right | Action::Up | Action::Down => "hint.chart",
        Action::ChartFull => "hint.full",
        Action::WindowWider | Action::WindowNarrower => "hint.window",
        Action::PauseChart => "hint.freeze",
        Action::Scale => "hint.scale",
        Action::Export => "hint.export",
        Action::Language => "hint.language",
        Action::Refresh => "hint.refresh",
        Action::Pause => "hint.pause",
        Action::Filter => "hint.filter",
        Action::Sort => "hint.sort",
        Action::SwitchProfile => "hint.profile",
        Action::TokenNew | Action::TopicNew | Action::ProfileNew => "hint.new",
        Action::TokenEdit | Action::TopicEdit | Action::ProfileEdit => "hint.edit",
        Action::TokenClone => "hint.clone",
        Action::TokenRevoke => "hint.revoke",
        Action::TopicDelete => "hint.delete",
        Action::TopicDead => "hint.dead",
        Action::TopicMessages => "hint.messages",
        Action::MsgPublish => "hint.publish",
        Action::MsgReceive => "hint.receive",
        Action::MsgAck => "hint.ack",
        Action::MsgAckAll => "hint.ack-all",
        Action::MsgRelease => "hint.release",
        Action::MsgPrune => "hint.prune",
        Action::ProfileUse => "hint.use",
        Action::ProfileDefault => "hint.default",
        Action::ProfileRemove => "hint.remove",
        Action::ProfileTest => "hint.test",
        _ => "hint.more",
    }
}

/// The first key of an action, the one the footer shows.
fn hint_key_label(action: Action) -> String {
    keymap::binding_of(action)
        .map(|b| b.label.split(' ').next().unwrap_or(b.label).to_owned())
        .unwrap_or_default()
}

fn hint_actions(m: &Model) -> &'static [Action] {
    match (m.tab, m.cluster.view) {
        (Tab::Cluster, ClusterView::List) => &[
            Action::Enter,
            Action::Drain,
            Action::Return,
            Action::FlushNode,
            Action::FlushAll,
            Action::StopCluster,
            Action::Monitor,
            Action::Help,
            Action::Quit,
        ],
        (Tab::Cluster, ClusterView::Detail) => &[
            Action::Esc,
            Action::Drain,
            Action::Return,
            Action::FlushNode,
            Action::Monitor,
            Action::Help,
            Action::Quit,
        ],
        (Tab::Cluster, ClusterView::Monitor) => &[
            Action::Esc,
            Action::Up,
            Action::ChartFull,
            Action::WindowWider,
            Action::PauseChart,
            Action::Scale,
            Action::Export,
            Action::Help,
        ],
        (Tab::Tokens, _) => &[
            Action::Enter,
            Action::TokenNew,
            Action::TokenEdit,
            Action::TokenClone,
            Action::TokenRevoke,
            Action::Filter,
            Action::Sort,
            Action::Help,
            Action::Quit,
        ],
        (Tab::Topics, _)
            if m.topics
                .monitor
                .as_ref()
                .is_some_and(|mon| mon.tab == TopicTab::Messages) =>
        {
            &[
                Action::Esc,
                Action::Enter,
                Action::MsgPublish,
                Action::MsgReceive,
                Action::MsgAck,
                Action::MsgAckAll,
                Action::MsgRelease,
                Action::MsgPrune,
                Action::Help,
            ]
        }
        (Tab::Topics, _) if m.topics.monitor.is_some() => &[
            Action::Esc,
            Action::Up,
            Action::ChartFull,
            Action::WindowWider,
            Action::PauseChart,
            Action::Scale,
            Action::Export,
            Action::Help,
            Action::Quit,
        ],
        (Tab::Topics, _) => &[
            Action::Enter,
            Action::TopicCharts,
            Action::TopicMessages,
            Action::TopicNew,
            Action::TopicEdit,
            Action::TopicDelete,
            Action::TopicDead,
            Action::Filter,
            Action::Sort,
            Action::Help,
            Action::Quit,
        ],
        (Tab::Profiles, _) => &[
            Action::Enter,
            Action::ProfileUse,
            Action::ProfileNew,
            Action::ProfileEdit,
            Action::ProfileDefault,
            Action::ProfileRemove,
            Action::ProfileTest,
            Action::Help,
            Action::Quit,
        ],
    }
}

/// `true` when the footer should show this action as disabled right now.
fn disabled(m: &Model, action: Action) -> bool {
    if m.tab == Tab::Tokens {
        return matches!(
            action,
            Action::TokenNew | Action::TokenEdit | Action::TokenClone | Action::TokenRevoke
        ) && screens::tokens::gate(m, action).is_some();
    }
    if m.tab == Tab::Topics {
        if m.topics.monitor.is_some() {
            return matches!(
                action,
                Action::MsgPublish
                    | Action::MsgReceive
                    | Action::MsgAck
                    | Action::MsgAckAll
                    | Action::MsgRelease
                    | Action::MsgPrune
            ) && screens::topics::messages::gate(m, action).is_some();
        }
        return matches!(
            action,
            Action::TopicNew | Action::TopicEdit | Action::TopicDelete
        ) && screens::topics::gate(m, action).is_some();
    }
    m.tab == Tab::Cluster
        && matches!(
            action,
            Action::Drain
                | Action::Return
                | Action::FlushNode
                | Action::FlushAll
                | Action::StopCluster
                | Action::Monitor
                | Action::Detail
        )
        && cluster_screen::gate(m, action).is_some()
}

/// The state of the refresh, for the right of the footer (TUI-FRM-03).
fn refresh_state(m: &Model) -> (String, Style) {
    let tx = m.tx();
    let th = m.theme;
    let feeds_error = if m.uses_status() {
        m.status
            .error
            .as_ref()
            .map(|_| (m.status.fetched_at, m.status.retry_in(m.now)))
    } else {
        m.cluster
            .nodes
            .error
            .as_ref()
            .map(|_| (m.cluster.nodes.fetched_at, m.cluster.nodes.retry_in(m.now)))
    };
    if m.paused {
        return (format!("⏸ {}", tx.t("footer.paused")), th.warning());
    }
    if let Some((fetched, retry)) = feeds_error {
        let since = fetched.map_or_else(
            || "—".to_owned(),
            |at| i18n::clock(m.wall_ms - i64::try_from(m.now.saturating_sub(at)).unwrap_or(0)),
        );
        let retry = retry.unwrap_or(0);
        return (
            tx.tw("footer.stale", &[("since", &since), ("retry", &retry)]),
            th.warning(),
        );
    }
    if m.busy.is_some() {
        return (
            format!("{} {}", th.refresh(), tx.t("footer.working")),
            th.accent(),
        );
    }
    let every = if m.tab == Tab::Cluster {
        2
    } else {
        m.history.interval_ms() / 1000
    };
    (format!("{} {every}s", th.refresh()), th.dim())
}

fn draw_footer(f: &mut Frame, area: Rect, m: &Model) {
    let tx = m.tx();
    let th = m.theme;
    let (right_text, right_style) = refresh_state(m);
    let right = format!("{right_text} · L {} ", m.lang.code());
    let right_width = right.chars().count();
    let left_width = usize::from(area.width).saturating_sub(right_width + 1);
    let mut spans: Vec<Span> = Vec::new();
    if let Some(notice) = &m.notice {
        spans.push(Span::styled(
            format!(" {} {}", th.icon(notice.severity), notice.text),
            th.severity(notice.severity),
        ));
    } else {
        // Help and quit always have room: the other hints fill what is left.
        let mut all = hint_actions(m).to_vec();
        // The language key is on every screen, right before the help.
        if !all.contains(&Action::Language) {
            let at = all
                .iter()
                .position(|a| *a == Action::Help)
                .unwrap_or(all.len());
            all.insert(at, Action::Language);
        }
        let is_tail = |a: &&Action| matches!(a, Action::Language | Action::Help | Action::Quit);
        let tail: Vec<Action> = all.iter().filter(is_tail).copied().collect();
        let width_of = |action: Action| {
            let key = hint_key_label(action);
            key.chars().count() + tx.t(hint_key(action)).chars().count() + 3
        };
        let reserved: usize = tail.iter().map(|a| width_of(*a)).sum();
        let mut used = 0;
        let mut shown = Vec::new();
        for action in all.iter().filter(|a| !is_tail(a)) {
            let width = width_of(*action);
            if used + width + reserved > left_width {
                break;
            }
            used += width;
            shown.push(*action);
        }
        shown.extend(tail);
        for action in shown {
            let off = disabled(m, action);
            spans.push(Span::styled(
                format!(" {}", hint_key_label(action)),
                if off { th.dim() } else { th.key() },
            ));
            spans.push(Span::styled(
                format!(" {} ", tx.t(hint_key(action))),
                if off { th.dim() } else { Style::default() },
            ));
        }
    }
    let used: usize = spans.iter().map(Span::width).sum();
    let pad = usize::from(area.width).saturating_sub(used + right_width);
    spans.push(Span::raw(" ".repeat(pad)));
    spans.push(Span::styled(right, right_style));
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

#[cfg(test)]
mod tests;
