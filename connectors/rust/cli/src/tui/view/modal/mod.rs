//! The popups (doc 34, TUI-FRM-07): help, command palette, language, confirmation, result of an action, what a
//! condition means, and the path of an export. Only one is open at a time.

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};
use ratatui::Frame;

use crate::tui::api::{FlushReport, Severity, StopReport};
use crate::tui::i18n::{self, LANGS};
use crate::tui::keymap;
use crate::tui::model::{Confirm, Fail, Input, InputPurpose, Modal, Model, Pending, Report};
use crate::tui::update::{palette_matches, scope_of};
use crate::tui::view::widgets::{centered, denied_text, popup, scrolling, short};

mod forms;

pub fn draw(f: &mut Frame, area: Rect, m: &Model) {
    let Some(modal) = &m.modal else { return };
    match modal {
        Modal::Menu(menu) => draw_menu(f, area, m, menu),
        Modal::TopicSummary => draw_topic_summary(f, area, m),
        Modal::Help { scroll } => draw_help(f, area, m, *scroll),
        Modal::Palette { query, selected } => draw_palette(f, area, m, query, *selected),
        Modal::Language { selected } => draw_language(f, area, m, *selected),
        Modal::Confirm(confirm) => draw_confirm(f, area, m, confirm),
        Modal::Report(report) => draw_report(f, area, m, report),
        Modal::Condition(name) => draw_condition(f, area, m, name),
        Modal::Input(input) => draw_input(f, area, m, input),
        Modal::Profiles { selected } => forms::draw_profiles(f, area, m, *selected),
        Modal::Editor(editor) => forms::draw_editor(f, area, m, editor),
        Modal::Secret(secret) => forms::draw_secret(f, area, m, secret),
    }
}

/// The lines of the help: the keys of the screen being shown, then the global ones (TUI-HLP-01).
pub fn help_lines(m: &Model) -> Vec<Line<'static>> {
    let tx = m.tx();
    let th = m.theme;
    let scope = scope_of(m);
    let mut lines = Vec::new();
    for binding in keymap::help_for(scope) {
        let Some(help) = binding.help else { continue };
        lines.push(Line::from(vec![
            Span::styled(format!("{:<16}", binding.label), th.key()),
            Span::raw(tx.t(help)),
        ]));
    }
    lines
}

fn draw_help(f: &mut Frame, area: Rect, m: &Model, scroll: u16) {
    let tx = m.tx();
    let rect = centered(area, 72, area.height.saturating_sub(4));
    let inner = popup(f, rect, &tx.t("help.title"), m.theme.accent());
    let [body, hint] = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(inner);
    scrolling(f, body, help_lines(m), scroll);
    f.render_widget(
        Paragraph::new(tx.t("help.close")).style(m.theme.dim()),
        hint,
    );
}

fn draw_palette(f: &mut Frame, area: Rect, m: &Model, query: &str, selected: usize) {
    let tx = m.tx();
    let th = m.theme;
    let rect = centered(area, 60, area.height.saturating_sub(6).min(18));
    let inner = popup(f, rect, &tx.t("palette.title"), th.accent());
    let [input, list] = Layout::vertical([Constraint::Length(2), Constraint::Min(1)]).areas(inner);
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(": ", th.key()),
            Span::raw(query.to_owned()),
            Span::styled("▏", th.accent()),
        ])),
        input,
    );
    let matches = palette_matches(m.lang, query);
    if matches.is_empty() {
        f.render_widget(Paragraph::new(tx.t("palette.empty")).style(th.dim()), list);
        return;
    }
    let height = usize::from(list.height);
    let skip = selected.saturating_sub(height.saturating_sub(1));
    let lines: Vec<Line> = matches
        .iter()
        .enumerate()
        .skip(skip)
        .take(height)
        .map(|(i, (label, action))| {
            let key = keymap::binding_of(*action).map_or("", |b| b.label);
            let style = if i == selected {
                th.selected()
            } else {
                Style::default()
            };
            Line::from(vec![
                Span::styled(
                    format!(
                        "{}{label}",
                        if i == selected { th.selector() } else { "  " }
                    ),
                    style,
                ),
                Span::styled(format!("  {key}"), th.dim()),
            ])
        })
        .collect();
    f.render_widget(Paragraph::new(lines), list);
}

/// The summary of the selected topic, as a dialog.
fn draw_topic_summary(f: &mut Frame, area: Rect, m: &Model) {
    let tx = m.tx();
    let th = m.theme;
    let Some(topic) = crate::tui::screens::topics::selected(m) else {
        return;
    };
    let lines = crate::tui::screens::topics::view::summary_lines(m, &topic);
    let height = u16::try_from(lines.len())
        .unwrap_or(u16::MAX)
        .saturating_add(4);
    let rect = centered(area, 90, height);
    let inner = popup(
        f,
        rect,
        &tx.tw("topic.dialog-summary", &[("name", &topic.name)]),
        th.accent(),
    );
    let [body, hint] = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(inner);
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), body);
    f.render_widget(Paragraph::new(tx.t("summary.hint")).style(th.dim()), hint);
}

/// The actions of the selected item, each with its key; one that cannot run now is dim and says why.
fn draw_menu(f: &mut Frame, area: Rect, m: &Model, menu: &crate::tui::model::Menu) {
    let tx = m.tx();
    let th = m.theme;
    let height = u16::try_from(menu.items.len())
        .unwrap_or(u16::MAX)
        .saturating_add(4);
    let rect = centered(area, 76, height.saturating_add(0));
    let inner = popup(f, rect, &menu.title, th.accent());
    let [list, hint] = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(inner);
    let lines: Vec<Line> = menu
        .items
        .iter()
        .enumerate()
        .map(|(i, action)| {
            let reason = crate::tui::menu::gate(m, *action);
            let key = crate::tui::menu::shortcut(m.tab, *action).unwrap_or("");
            let style = match (i == menu.selected, reason.is_some()) {
                (true, _) => th.selected(),
                (false, true) => th.dim(),
                (false, false) => Style::default(),
            };
            let mut spans = vec![Span::styled(
                format!(
                    "{}{:<30}",
                    if i == menu.selected {
                        th.selector()
                    } else {
                        "  "
                    },
                    tx.t(crate::tui::menu::label_key(*action))
                ),
                style,
            )];
            spans.push(Span::styled(format!(" {key:<6}"), th.key()));
            Line::from(spans)
        })
        .collect();
    f.render_widget(Paragraph::new(lines), list);
    // The line below says why the entry under the cursor cannot run, or how to use the menu.
    let why = menu
        .items
        .get(menu.selected)
        .and_then(|action| crate::tui::menu::gate(m, *action));
    let note = match why {
        Some(reason) => Paragraph::new(format!("{} {}", th.icon(Severity::Warning), tx.t(reason)))
            .style(th.warning()),
        None => Paragraph::new(tx.t("menu.hint")).style(th.dim()),
    };
    f.render_widget(note, hint);
}

fn draw_language(f: &mut Frame, area: Rect, m: &Model, selected: usize) {
    let tx = m.tx();
    let th = m.theme;
    let rect = centered(area, 36, 7);
    let inner = popup(f, rect, &tx.t("language.title"), th.accent());
    let mut lines: Vec<Line> = LANGS
        .iter()
        .enumerate()
        .map(|(i, lang)| {
            let style = if i == selected {
                th.selected()
            } else {
                Style::default()
            };
            Line::from(Span::styled(
                format!(
                    "{}{} ({})",
                    if i == selected { th.selector() } else { "  " },
                    lang.name(),
                    lang.code()
                ),
                style,
            ))
        })
        .collect();
    lines.push(Line::default());
    lines.push(Line::from(Span::styled(tx.t("language.hint"), th.dim())));
    f.render_widget(Paragraph::new(lines), inner);
}

fn pending_keys(pending: &Pending) -> (&'static str, &'static str) {
    match pending {
        Pending::Drain(_) => ("confirm.drain.title", "confirm.drain.body"),
        Pending::Return(_) => ("confirm.return.title", "confirm.return.body"),
        Pending::FlushNode(_) => ("confirm.flush-node.title", "confirm.flush-node.body"),
        Pending::FlushAll => ("confirm.flush-all.title", "confirm.flush-all.body"),
        Pending::Stop => ("confirm.stop.title", "confirm.stop.body"),
        Pending::RevokeToken(..) => ("confirm.revoke.title", "confirm.revoke.body"),
        Pending::RemoveProfile(_) => (
            "confirm.remove-profile.title",
            "confirm.remove-profile.body",
        ),
        Pending::DeleteTopic(_) => ("confirm.delete-topic.title", "confirm.delete-topic.body"),
        Pending::Prune(_) => ("confirm.prune.title", "confirm.prune.body"),
    }
}

fn draw_confirm(f: &mut Frame, area: Rect, m: &Model, confirm: &Confirm) {
    let tx = m.tx();
    let th = m.theme;
    let (title, body) = pending_keys(&confirm.pending);
    let node = match &confirm.pending {
        Pending::Drain(id) | Pending::Return(id) | Pending::FlushNode(id) => short(id),
        Pending::RevokeToken(_, name)
        | Pending::RemoveProfile(name)
        | Pending::DeleteTopic(name)
        | Pending::Prune(name) => name.clone(),
        _ => String::new(),
    };
    let rect = centered(area, 64, 14);
    let inner = popup(f, rect, &tx.t(title), th.warning());
    let [text, input, buttons] = Layout::vertical([
        Constraint::Min(3),
        Constraint::Length(3),
        Constraint::Length(1),
    ])
    .areas(inner);
    f.render_widget(
        Paragraph::new(tx.tw(body, &[("node", &node), ("name", &node)])).wrap(Wrap { trim: false }),
        text,
    );
    if let Some(typed) = &confirm.typed {
        let ok = typed.input == typed.expected;
        let prompt = tx.tw("confirm.type", &[("name", &typed.expected)]);
        f.render_widget(
            Paragraph::new(vec![
                Line::from(Span::styled(prompt, th.dim())),
                Line::from(vec![
                    Span::styled("> ", th.key()),
                    Span::styled(
                        typed.input.clone(),
                        if ok { th.ok() } else { Style::default() },
                    ),
                    Span::styled("▏", th.accent()),
                ]),
            ]),
            input,
        );
    }
    let button = |label: String, focused: bool, style: Style| {
        let text = format!(" [ {label} ] ");
        if focused {
            Span::styled(text, th.selected().patch(style))
        } else {
            Span::raw(text)
        }
    };
    let typed_ready = confirm
        .typed
        .as_ref()
        .is_some_and(|t| t.input == t.expected);
    let confirm_focus = confirm.ok_focused || typed_ready;
    f.render_widget(
        Paragraph::new(Line::from(vec![
            button(tx.t("confirm.cancel"), !confirm_focus, Style::default()),
            button(tx.t("confirm.ok"), confirm_focus, th.critical()),
        ]))
        .alignment(ratatui::layout::Alignment::Center),
        buttons,
    );
}

fn flush_lines(m: &Model, report: &FlushReport) -> Vec<Line<'static>> {
    let tx = m.tx();
    let th = m.theme;
    let lang = m.lang;
    let mut lines = vec![if report.complete {
        Line::from(Span::styled(
            format!(
                "{} {}",
                th.icon(Severity::Ok),
                tx.t("report.flush.complete")
            ),
            th.ok(),
        ))
    } else {
        Line::from(Span::styled(
            format!(
                "{} {}",
                th.icon(Severity::Warning),
                tx.t("report.flush.incomplete")
            ),
            th.warning(),
        ))
    }];
    for node in &report.nodes {
        let name = node.node_id.as_deref().map_or("—".to_owned(), short);
        let (severity, text) = if node.complete {
            (Severity::Ok, tx.t("report.flush.node-complete"))
        } else if let Some(error) = &node.error {
            (
                Severity::Critical,
                tx.tw("report.flush.node-error", &[("error", error)]),
            )
        } else if node.timed_out {
            (Severity::Warning, tx.t("report.flush.node-timeout"))
        } else {
            (
                Severity::Warning,
                tx.tw(
                    "report.flush.node-pending",
                    &[
                        ("outbox", &i18n::group(lang, node.outbox_pending)),
                        ("memory", &i18n::group(lang, node.memory_remaining)),
                    ],
                ),
            )
        };
        lines.push(Line::from(Span::styled(
            format!("{} {name}  {text}", th.icon(severity)),
            th.severity(severity),
        )));
    }
    lines.push(Line::default());
    lines.push(Line::from(Span::styled(
        tx.t("report.flush.repeat"),
        th.dim(),
    )));
    lines
}

fn stop_lines(m: &Model, report: &StopReport) -> Vec<Line<'static>> {
    let tx = m.tx();
    let th = m.theme;
    let mut lines = vec![Line::from(Span::styled(
        tx.tn("report.stop.stopped", report.stopped.len() as u64),
        th.title(),
    ))];
    for (node, drained) in &report.nodes {
        let severity = if *drained {
            Severity::Ok
        } else {
            Severity::Warning
        };
        let state = tx.t(if *drained {
            "report.stop.drained"
        } else {
            "report.stop.not-drained"
        });
        lines.push(Line::from(Span::styled(
            format!("{} {}  {state}", th.icon(severity), short(node)),
            th.severity(severity),
        )));
    }
    lines.push(Line::from(if report.stop_list {
        Span::styled(tx.t("report.stop.list"), th.ok())
    } else {
        Span::styled(tx.t("report.stop.no-list"), th.warning())
    }));
    for warning in &report.warnings {
        lines.push(Line::from(Span::styled(
            format!("{} {warning}", th.icon(Severity::Warning)),
            th.warning(),
        )));
    }
    lines.push(Line::default());
    lines.push(Line::from(Span::styled(
        tx.t("report.stop.processes"),
        th.dim(),
    )));
    lines
}

fn failure_lines(m: &Model, fail: &Fail) -> Vec<Line<'static>> {
    let tx = m.tx();
    let th = m.theme;
    let mut lines = vec![Line::from(Span::styled(
        format!(
            "{} {}: {}",
            th.icon(Severity::Critical),
            fail.code,
            fail.message
        ),
        th.critical(),
    ))];
    if fail.is_denied() {
        lines.push(Line::default());
        lines.push(Line::from(denied_text(m, fail)));
    } else if fail.is_offline() {
        lines.push(Line::default());
        lines.push(Line::from(tx.t("report.failure.offline")));
    } else if fail.code == "backup.not-configured" {
        lines.push(Line::default());
        lines.push(Line::from(tx.t("report.failure.no-backup")));
    } else if fail.status == 404 && fail.code.contains("route") {
        lines.push(Line::default());
        lines.push(Line::from(tx.t("report.failure.no-ops-port")));
    } else if fail.status == 502 || fail.status == 504 {
        lines.push(Line::default());
        lines.push(Line::from(tx.t("report.failure.node-hint")));
    }
    if let Some(id) = &fail.request_id {
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            tx.tw("report.failure.request-id", &[("id", id)]),
            th.dim(),
        )));
    }
    lines
}

fn draw_report(f: &mut Frame, area: Rect, m: &Model, report: &Report) {
    let tx = m.tx();
    let th = m.theme;
    let (title_key, lines, style) = match report {
        Report::Flush(r) => ("report.flush.title", flush_lines(m, r), th.accent()),
        Report::Stop(r) => ("report.stop.title", stop_lines(m, r), th.accent()),
        Report::Failure { fail, .. } => (
            "report.failure.title",
            failure_lines(m, fail),
            th.critical(),
        ),
    };
    let height = u16::try_from(lines.len())
        .unwrap_or(u16::MAX)
        .saturating_add(4);
    let rect = centered(area, 72, height.min(area.height.saturating_sub(2)));
    let inner = popup(f, rect, &tx.t(title_key), style);
    let [body, hint] = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(inner);
    scrolling(f, body, lines, 0);
    f.render_widget(Paragraph::new(tx.t("popup.close")).style(th.dim()), hint);
}

fn draw_condition(f: &mut Frame, area: Rect, m: &Model, name: &str) {
    let tx = m.tx();
    let th = m.theme;
    let rect = centered(area, 70, 14);
    let inner = popup(f, rect, &tx.t(&format!("condition.{name}")), th.warning());
    let [body, hint] = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(inner);
    let lines = vec![
        Line::from(Span::styled(tx.t("condition.means"), th.title())),
        Line::from(tx.t(&format!("condition.{name}.meaning"))),
        Line::default(),
        Line::from(Span::styled(tx.t("condition.todo"), th.title())),
        Line::from(tx.t(&format!("condition.{name}.action"))),
    ];
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), body);
    f.render_widget(Paragraph::new(tx.t("popup.close")).style(th.dim()), hint);
}

fn draw_input(f: &mut Frame, area: Rect, m: &Model, input: &Input) {
    let tx = m.tx();
    let th = m.theme;
    let (title, prompt, hint) = match input.purpose {
        InputPurpose::ExportTopicCsv => ("export.title", "export.prompt", "export.hint"),
        InputPurpose::ExportCsv { .. } => ("export.title", "export.prompt", "export.hint"),
        InputPurpose::TokenFilter => ("filter.title", "filter.prompt", "filter.hint"),
        InputPurpose::TopicFilter => ("topic-filter.title", "topic-filter.prompt", "filter.hint"),
    };
    let rect = centered(area, 64, 7);
    let inner = popup(f, rect, &tx.t(title), th.accent());
    f.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(tx.t(prompt), th.dim())),
            Line::from(vec![
                Span::styled("> ", th.key()),
                Span::raw(input.value.clone()),
                Span::styled("▏", th.accent()),
            ]),
            Line::default(),
            Line::from(Span::styled(tx.t(hint), th.dim())),
        ]),
        inner,
    );
}

#[cfg(test)]
mod tests;
