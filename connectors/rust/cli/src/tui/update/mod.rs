//! `update(model, msg) -> effects` (doc 34, section 4): the whole behavior of the interface as a function of one
//! input at a time. It never blocks and never touches the terminal or the network; the calls it wants made
//! come out as `Effect`s.

use ratatui::crossterm::event::{KeyCode, KeyEvent};
use taba_client::{Error, Reply};

use crate::tui::api::{self, ApiCall, Severity};
use crate::tui::effect::Effect;
use crate::tui::i18n::{self, LANGS};
use crate::tui::keymap::{self, Action, Scope};
use crate::tui::model::{
    short_id, ClusterView, Confirm, Fail, Goal, InputPurpose, Modal, Model, Pending, Report, Tab,
    TopicStats, TopicTab, TABS,
};
use crate::tui::msg::Msg;
use crate::tui::prom::Metrics;
use crate::tui::screens::{cluster, profiles, tokens, topics};

/// How often each thing is read.
const ME_EVERY_MS: u64 = 300_000;
const STATUS_EVERY_MS: u64 = 2_000;
const NODES_EVERY_MS: u64 = 5_000;
const NODES_BACKGROUND_MS: u64 = 10_000;
const METRICS_EVERY_MS: u64 = 5_000;
const BACKUP_EVERY_MS: u64 = 10_000;
const DRAIN_EVERY_MS: u64 = 2_000;
const TOKENS_EVERY_MS: u64 = 10_000;
const TOPICS_EVERY_MS: u64 = 5_000;

pub fn update(m: &mut Model, msg: Msg) -> Vec<Effect> {
    match msg {
        Msg::Key(event) => on_key(m, event),
        Msg::Scroll(lines) => on_scroll(m, lines),
        Msg::Resize(width, height) => {
            m.size = (width, height);
            Vec::new()
        }
        Msg::Focus(focused) => {
            m.focused = focused;
            // Coming back, the screen catches up at once.
            if focused {
                refresh(m);
            }
            Vec::new()
        }
        Msg::Tick { now, wall_ms } => on_tick(m, now, wall_ms),
        Msg::Action(action) => dispatch(m, action),
        Msg::Reply {
            id,
            generation,
            call,
            result,
        } => on_reply(m, id, generation, call, result),
        Msg::FileWritten { path, result } => {
            let tx = m.tx();
            match result {
                Ok(()) => {
                    tokens::secret_written(m, &path);
                    let text = tx.tw("notice.file-written", &[("path", &path)]);
                    m.notify(Severity::Ok, text);
                }
                Err(e) => {
                    let text = tx.tw("notice.file-failed", &[("path", &path), ("error", &e)]);
                    m.notify(Severity::Critical, text);
                }
            }
            Vec::new()
        }
        Msg::Paste(text) => {
            match &mut m.modal {
                Some(Modal::Editor(editor)) => editor.insert(&text),
                Some(Modal::Input(input)) => {
                    input.value.extend(text.chars().filter(|c| !c.is_control()));
                }
                _ => {}
            }
            Vec::new()
        }
        Msg::ProfilesSaved {
            file,
            select,
            result,
        } => profiles::saved(m, file, select, result),
        Msg::ProfileChecked(result) => {
            profiles::checked(m, &result);
            Vec::new()
        }
        Msg::Connected {
            profile,
            url,
            result,
        } => on_connected(m, profile, url, result),
        Msg::LanguageSaved(result) => {
            if let Err(e) = result {
                let text = m.tx().tw("notice.language-not-saved", &[("error", &e)]);
                m.notify(Severity::Warning, text);
            }
            Vec::new()
        }
    }
}

/// The scope whose keys apply on the screen being shown.
pub fn scope_of(m: &Model) -> Scope {
    match (m.tab, m.cluster.view) {
        (Tab::Cluster, ClusterView::Monitor) => Scope::Monitor,
        (Tab::Cluster, _) => Scope::Cluster,
        (Tab::Tokens, _) => Scope::Tokens,
        (Tab::Topics, _) => match &m.topics.monitor {
            Some(mon) if mon.tab == TopicTab::Messages => Scope::Messages,
            Some(_) => Scope::Monitor,
            None => Scope::Topics,
        },
        (Tab::Profiles, _) => Scope::Profiles,
    }
}

fn on_key(m: &mut Model, event: KeyEvent) -> Vec<Effect> {
    // `crossterm` on Windows reports the release of a key too: only the press counts.
    if event.kind == ratatui::crossterm::event::KeyEventKind::Release {
        return Vec::new();
    }
    if m.modal.is_some() {
        return on_modal_key(m, event);
    }
    match keymap::resolve(scope_of(m), &event) {
        Some(action) => dispatch(m, action),
        None => Vec::new(),
    }
}

fn on_scroll(m: &mut Model, lines: i16) -> Vec<Effect> {
    if let Some(Modal::Help { scroll }) = &mut m.modal {
        *scroll = scroll.saturating_add_signed(lines);
        return Vec::new();
    }
    if m.modal.is_some() {
        return Vec::new();
    }
    let action = if lines < 0 { Action::Up } else { Action::Down };
    let mut effects = Vec::new();
    for _ in 0..lines.unsigned_abs().min(10) {
        effects.extend(dispatch(m, action));
    }
    effects
}

/// Runs an action of the keymap, from a key or from the command palette.
pub fn dispatch(m: &mut Model, action: Action) -> Vec<Effect> {
    match action {
        Action::Quit => vec![Effect::Quit],
        Action::GoTab(n) => {
            go_to(m, usize::from(n));
            Vec::new()
        }
        Action::TabNext => {
            go_to(m, (m.tab.index() + 1) % TABS.len());
            Vec::new()
        }
        Action::TabPrev => {
            go_to(m, (m.tab.index() + TABS.len() - 1) % TABS.len());
            Vec::new()
        }
        Action::Help => {
            m.modal = Some(Modal::Help { scroll: 0 });
            Vec::new()
        }
        Action::Palette => {
            m.modal = Some(Modal::Palette {
                query: String::new(),
                selected: 0,
            });
            Vec::new()
        }
        Action::Language => {
            let selected = LANGS.iter().position(|l| *l == m.lang).unwrap_or(0);
            m.modal = Some(Modal::Language { selected });
            Vec::new()
        }
        Action::Refresh => {
            refresh(m);
            Vec::new()
        }
        Action::Pause => {
            m.paused = !m.paused;
            Vec::new()
        }
        Action::Submit => Vec::new(),
        Action::SwitchProfile => {
            open_profiles(m);
            Vec::new()
        }
        other => match m.tab {
            Tab::Cluster => cluster::on_action(m, other),
            Tab::Tokens => tokens::on_action(m, other),
            Tab::Topics => topics::on_action(m, other),
            Tab::Profiles => profiles::on_action(m, other),
        },
    }
}

fn go_to(m: &mut Model, index: usize) {
    if let Some(tab) = TABS.get(index) {
        m.tab = *tab;
    }
}

fn refresh(m: &mut Model) {
    m.me.refresh_now();
    m.status.refresh_now();
    m.cluster.nodes.refresh_now();
    m.cluster.metrics.refresh_now();
    m.cluster.backup.refresh_now();
    m.cluster.drain.refresh_now();
    m.tokens.list.refresh_now();
    m.topics.list.refresh_now();
    m.topics.stats.refresh_now();
    if let Some(mon) = &mut m.topics.monitor {
        mon.feed.refresh_now();
    }
    for preview in &mut m.topics.previews {
        preview.feed.refresh_now();
    }
}

/// Opens the choice of a profile, with the default (or the first) one under the cursor.
fn open_profiles(m: &mut Model) {
    if m.profiles.file.profiles.is_empty() {
        let text = m.tx().t("gate.no-profile");
        m.notify(Severity::Warning, text);
        return;
    }
    m.modal = Some(Modal::Profiles {
        selected: profiles::preferred_index(&m.profiles.file),
    });
}

/// The connection with a profile was made (or could not be).
fn on_connected(
    m: &mut Model,
    profile: String,
    url: String,
    result: Result<(), String>,
) -> Vec<Effect> {
    let tx = m.tx();
    match result {
        Ok(()) => {
            m.reconnect(profile.clone(), url.clone());
            if matches!(m.modal, Some(Modal::Profiles { .. })) {
                m.modal = None;
            }
            let text = tx.tw("notice.connected", &[("profile", &profile), ("url", &url)]);
            m.notify(Severity::Ok, text);
        }
        Err(e) => {
            let text = tx.tw(
                "notice.connect-failed",
                &[("profile", &profile), ("error", &e)],
            );
            m.notify(Severity::Critical, text);
        }
    }
    Vec::new()
}

// ---------- popups ----------

/// The commands of the palette: every action that has a help text, plus a jump to each tab.
pub fn palette_entries(lang: i18n::Lang) -> Vec<(String, Action)> {
    let mut entries = Vec::new();
    for (i, tab) in TABS.iter().enumerate() {
        let name = i18n::tr(lang, tab.name_key());
        entries.push((
            i18n::tr_with(lang, "palette.go-to", &[("tab", &name)]),
            Action::GoTab(i as u8),
        ));
    }
    for binding in keymap::BINDINGS {
        let Some(help) = binding.help else { continue };
        if matches!(
            binding.action,
            Action::GoTab(_) | Action::Enter | Action::Esc | Action::Submit
        ) {
            continue;
        }
        entries.push((i18n::tr(lang, help), binding.action));
    }
    entries
}

/// A subsequence match, ignoring case: `drn` finds `Drain the node`.
pub fn fuzzy(query: &str, text: &str) -> bool {
    let mut chars = text.chars().flat_map(char::to_lowercase);
    query
        .chars()
        .flat_map(char::to_lowercase)
        .filter(|c| !c.is_whitespace())
        .all(|q| chars.any(|c| c == q))
}

/// The palette entries that match a query, in either the current language or English (TUI-HLP-02). Those that
/// contain the text come before those that only have its letters in order.
pub fn palette_matches(lang: i18n::Lang, query: &str) -> Vec<(String, Action)> {
    let english = palette_entries(i18n::Lang::EnUs);
    let wanted = query.trim().to_lowercase();
    let contains = |text: &str| text.to_lowercase().contains(&wanted);
    let mut found: Vec<(bool, (String, Action))> = palette_entries(lang)
        .into_iter()
        .zip(english)
        .filter(|((own, _), (en, _))| fuzzy(query, own) || fuzzy(query, en))
        .map(|((own, action), (en, _))| (!(contains(&own) || contains(&en)), (own, action)))
        .collect();
    // A stable sort keeps the table's order inside each group.
    found.sort_by_key(|(worse, _)| *worse);
    found.into_iter().map(|(_, entry)| entry).collect()
}

fn on_modal_key(m: &mut Model, event: KeyEvent) -> Vec<Effect> {
    let Some(modal) = m.modal.clone() else {
        return Vec::new();
    };
    let ctrl = crate::tui::keymap::ctrl_of(&event);
    if ctrl && matches!(event.code, KeyCode::Char('c')) {
        return vec![Effect::Quit];
    }
    match modal {
        Modal::Help { scroll } => {
            let scroll = match event.code {
                KeyCode::Esc | KeyCode::Char('q' | 'h' | '?') | KeyCode::F(1) => {
                    m.modal = None;
                    return Vec::new();
                }
                KeyCode::Up | KeyCode::Char('k') => scroll.saturating_sub(1),
                KeyCode::Down | KeyCode::Char('j') => scroll.saturating_add(1),
                KeyCode::PageUp => scroll.saturating_sub(10),
                KeyCode::PageDown => scroll.saturating_add(10),
                KeyCode::Home => 0,
                KeyCode::End => u16::MAX,
                _ => scroll,
            };
            m.modal = Some(Modal::Help { scroll });
            Vec::new()
        }
        Modal::Palette {
            mut query,
            selected,
        } => palette_key(m, event, &mut query, selected),
        Modal::Language { selected } => match event.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                m.modal = None;
                Vec::new()
            }
            KeyCode::Up | KeyCode::Char('k') => {
                m.modal = Some(Modal::Language {
                    selected: cluster::moved(selected, -1, LANGS.len()),
                });
                Vec::new()
            }
            KeyCode::Down | KeyCode::Char('j') => {
                m.modal = Some(Modal::Language {
                    selected: cluster::moved(selected, 1, LANGS.len()),
                });
                Vec::new()
            }
            KeyCode::Enter => {
                m.modal = None;
                let lang = LANGS[selected.min(LANGS.len() - 1)];
                m.lang = lang;
                vec![Effect::SaveLanguage(lang)]
            }
            _ => Vec::new(),
        },
        Modal::Menu(menu) => crate::tui::menu::key(m, event, menu),
        Modal::Message => topics::messages::detail_key(m, event),
        Modal::Confirm(confirm) => confirm_key(m, event, confirm),
        Modal::Profiles { selected } => profiles_key(m, event, selected),
        Modal::Editor(mut editor) => {
            let outcome = editor.handle(&event);
            if editor.purpose.is_profile() {
                profiles::editor_key(m, editor, outcome)
            } else if editor.purpose.is_message() {
                topics::messages::editor_key(m, editor, outcome)
            } else if editor.purpose.is_topic() {
                topics::editor_key(m, editor, outcome)
            } else {
                tokens::editor_key(m, editor, outcome)
            }
        }
        Modal::Secret(secret) => tokens::secret_key(m, secret, event.code),
        Modal::Report(_) | Modal::Condition(_) | Modal::TopicSummary => {
            if matches!(
                event.code,
                KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q')
            ) {
                m.modal = None;
            }
            Vec::new()
        }
        Modal::Input(mut input) => match event.code {
            KeyCode::Esc => {
                m.modal = None;
                Vec::new()
            }
            KeyCode::Enter => {
                m.modal = None;
                let value = input.value.trim().to_owned();
                match &input.purpose {
                    InputPurpose::ExportCsv { node, series } => {
                        if value.is_empty() {
                            return Vec::new();
                        }
                        let contents = cluster::export_csv(m, node, *series);
                        vec![Effect::WriteFile {
                            path: value,
                            contents,
                        }]
                    }
                    InputPurpose::TokenFilter => {
                        m.tokens.filter = value;
                        Vec::new()
                    }
                    InputPurpose::TopicFilter => {
                        m.topics.filter = value;
                        Vec::new()
                    }
                    InputPurpose::ExportTopicCsv => {
                        if value.is_empty() {
                            return Vec::new();
                        }
                        let contents = topics::monitor::export_csv(m);
                        vec![Effect::WriteFile {
                            path: value,
                            contents,
                        }]
                    }
                }
            }
            KeyCode::Backspace => {
                input.value.pop();
                m.modal = Some(Modal::Input(input));
                Vec::new()
            }
            KeyCode::Char(c) if !ctrl => {
                input.value.push(c);
                m.modal = Some(Modal::Input(input));
                Vec::new()
            }
            _ => Vec::new(),
        },
    }
}

/// The choice of a profile: the arrows move, `Enter` connects. At the start `Esc` takes the default one; later it only
/// closes the popup.
fn profiles_key(m: &mut Model, event: KeyEvent, selected: usize) -> Vec<Effect> {
    let count = m.profiles.file.profiles.len();
    let set = |m: &mut Model, selected: usize| m.modal = Some(Modal::Profiles { selected });
    match event.code {
        KeyCode::Up | KeyCode::Char('k') => set(m, cluster::moved(selected, -1, count)),
        KeyCode::Down | KeyCode::Char('j') => set(m, cluster::moved(selected, 1, count)),
        KeyCode::Home => set(m, 0),
        KeyCode::End => set(m, count.saturating_sub(1)),
        KeyCode::Enter => {
            if let Some((name, _)) = m.profiles.file.profiles.get(selected) {
                return vec![Effect::Connect(name.clone())];
            }
        }
        KeyCode::Esc => {
            if !m.awaiting_profile {
                m.modal = None;
            } else if let Some((name, _)) = m
                .profiles
                .file
                .profiles
                .get(profiles::preferred_index(&m.profiles.file))
            {
                return vec![Effect::Connect(name.clone())];
            }
        }
        _ => {}
    }
    Vec::new()
}

fn palette_key(m: &mut Model, event: KeyEvent, query: &mut String, selected: usize) -> Vec<Effect> {
    let count = palette_matches(m.lang, query).len();
    match event.code {
        KeyCode::Esc => {
            m.modal = None;
        }
        KeyCode::Up => {
            m.modal = Some(Modal::Palette {
                query: query.clone(),
                selected: cluster::moved(selected, -1, count),
            });
        }
        KeyCode::Down => {
            m.modal = Some(Modal::Palette {
                query: query.clone(),
                selected: cluster::moved(selected, 1, count),
            });
        }
        KeyCode::Backspace => {
            query.pop();
            m.modal = Some(Modal::Palette {
                query: query.clone(),
                selected: 0,
            });
        }
        KeyCode::Char(c) if !crate::tui::keymap::ctrl_of(&event) => {
            query.push(c);
            m.modal = Some(Modal::Palette {
                query: query.clone(),
                selected: 0,
            });
        }
        KeyCode::Enter => {
            let chosen = palette_matches(m.lang, query)
                .get(selected)
                .map(|(_, a)| *a);
            m.modal = None;
            if let Some(action) = chosen {
                if let Some(binding) = keymap::BINDINGS.iter().find(|b| b.action == action) {
                    match binding.scope {
                        Scope::Cluster | Scope::Monitor => m.tab = Tab::Cluster,
                        Scope::Tokens => m.tab = Tab::Tokens,
                        Scope::Topics | Scope::Messages => m.tab = Tab::Topics,
                        Scope::Profiles => m.tab = Tab::Profiles,
                        Scope::Global => {}
                    }
                }
                return dispatch(m, action);
            }
        }
        _ => {}
    }
    Vec::new()
}

fn confirm_key(m: &mut Model, event: KeyEvent, mut confirm: Confirm) -> Vec<Effect> {
    match event.code {
        KeyCode::Esc => {
            m.modal = None;
            return Vec::new();
        }
        KeyCode::Enter => {
            let allowed = match &confirm.typed {
                Some(typed) => typed.input == typed.expected,
                None => confirm.ok_focused,
            };
            if allowed {
                m.modal = None;
                return match (confirm.pending.call(), &confirm.pending) {
                    (Some(call), _) => run(m, call),
                    (None, Pending::RemoveProfile(name)) => profiles::remove(m, name),
                    (None, _) => Vec::new(),
                };
            }
            if confirm.typed.is_none() {
                m.modal = None;
            }
            return Vec::new();
        }
        KeyCode::Left | KeyCode::Right | KeyCode::Tab | KeyCode::BackTab
            if confirm.typed.is_none() =>
        {
            confirm.ok_focused = !confirm.ok_focused;
        }
        KeyCode::Backspace => {
            if let Some(typed) = &mut confirm.typed {
                typed.input.pop();
            }
        }
        KeyCode::Char(c) if !crate::tui::keymap::ctrl_of(&event) => {
            if let Some(typed) = &mut confirm.typed {
                typed.input.push(c);
            } else if matches!(c, 'h' | 'l') {
                confirm.ok_focused = !confirm.ok_focused;
            }
        }
        _ => {}
    }
    m.modal = Some(Modal::Confirm(confirm));
    Vec::new()
}

/// Sends an action's call; the screen is `busy` until the reply comes.
pub fn run(m: &mut Model, call: ApiCall) -> Vec<Effect> {
    let id = m.id();
    m.busy = Some(call.clone());
    vec![Effect::Call {
        id,
        generation: m.generation,
        call,
    }]
}

// ---------- the clock: what to read, and when ----------

fn on_tick(m: &mut Model, now: u64, wall_ms: i64) -> Vec<Effect> {
    m.now = now;
    m.wall_ms = wall_ms;
    if m.notice.as_ref().is_some_and(|n| now >= n.until) {
        m.notice = None;
    }
    let mut effects = Vec::new();
    // Nothing is read until the person has chosen a profile.
    if m.awaiting_profile {
        return effects;
    }
    let background = !m.paused && m.focused && m.modal.is_none();
    if background || m.me.data.is_none() {
        poll_me(m, &mut effects);
    }
    if background && m.me.data.is_some() {
        poll_overview(m, &mut effects);
        poll_node(m, &mut effects);
        poll_tokens(m, &mut effects);
        poll_topics(m, &mut effects);
        poll_topic_monitor(m, &mut effects);
        poll_topic_preview(m, &mut effects);
    }
    poll_drain(m, &mut effects);
    effects
}

fn issue(m: &mut Model, call: ApiCall) -> (u64, Effect) {
    let id = m.id();
    let effect = Effect::Call {
        id,
        generation: m.generation,
        call,
    };
    (id, effect)
}

fn poll_me(m: &mut Model, effects: &mut Vec<Effect>) {
    if m.me.due(m.now) {
        let (id, effect) = issue(m, ApiCall::Me);
        m.me.in_flight = Some(id);
        effects.push(effect);
    }
}

/// The status of the cluster (root) or the list of nodes (any token): it feeds the status line and the
/// history. On the Cluster tab it is read often; on the others, only as often as the history samples.
fn poll_overview(m: &mut Model, effects: &mut Vec<Effect>) {
    let on_cluster = m.tab == Tab::Cluster;
    if m.uses_status() && m.status.due(m.now) {
        let (id, effect) = issue(m, ApiCall::ClusterStatus);
        m.status.in_flight = Some(id);
        effects.push(effect);
    }
    // The list of nodes also feeds the table of a root token: it remembers the nodes that left by drain.
    if (!m.uses_status() || on_cluster) && m.cluster.nodes.due(m.now) {
        let (id, effect) = issue(m, ApiCall::Nodes);
        m.cluster.nodes.in_flight = Some(id);
        effects.push(effect);
    }
}

/// The tokens, read page by page while their tab is open.
fn poll_tokens(m: &mut Model, effects: &mut Vec<Effect>) {
    if m.tab != Tab::Tokens || tokens::availability(m) != tokens::Availability::Ready {
        return;
    }
    if m.tokens.list.due(m.now) {
        m.tokens.reading.clear();
        m.tokens.pages = 1;
        let (id, effect) = issue(m, ApiCall::Tokens(None));
        m.tokens.list.in_flight = Some(id);
        effects.push(effect);
    }
}

/// The topics and their numbers, read page by page while their tab is open. The two reads go on side by side:
/// the table appears with the definitions and the numbers fill in when they arrive.
fn poll_topics(m: &mut Model, effects: &mut Vec<Effect>) {
    if m.tab != Tab::Topics {
        return;
    }
    if m.topics.list.due(m.now) {
        m.topics.reading.clear();
        m.topics.pages = 1;
        let (id, effect) = issue(m, ApiCall::Topics(None));
        m.topics.list.in_flight = Some(id);
        effects.push(effect);
    }
    if m.topics.stats.due(m.now) {
        m.topics.stats_reading = TopicStats::default();
        m.topics.stats_pages = 1;
        let (id, effect) = issue(m, ApiCall::TopicStats(None));
        m.topics.stats.in_flight = Some(id);
        effects.push(effect);
    }
}

/// One reading of the queue whose charts are open, every sampling interval, whatever tab is shown.
fn poll_topic_monitor(m: &mut Model, effects: &mut Vec<Effect>) {
    let now = m.now;
    let Some(mon) = &m.topics.monitor else {
        return;
    };
    if mon.distributor || !mon.feed.due(now) {
        return;
    }
    let name = mon.name.clone();
    let (id, effect) = issue(m, ApiCall::TopicDetail(name));
    if let Some(mon) = &mut m.topics.monitor {
        mon.feed.in_flight = Some(id);
    }
    effects.push(effect);
}

/// One reading of the selected topic for the strip of charts under the list, while the list is what is shown.
fn poll_topic_preview(m: &mut Model, effects: &mut Vec<Effect>) {
    if m.tab != Tab::Topics || m.topics.monitor.is_some() {
        return;
    }
    topics::preview::prepare(m);
    let now = m.now;
    let Some(name) = topics::preview::current(m)
        .filter(|p| p.feed.due(now))
        .map(|p| p.name.clone())
    else {
        return;
    };
    let (id, effect) = issue(m, ApiCall::TopicDetail(name.clone()));
    if let Some(preview) = m.topics.previews.iter_mut().find(|p| p.name == name) {
        preview.feed.in_flight = Some(id);
    }
    effects.push(effect);
}

fn poll_node(m: &mut Model, effects: &mut Vec<Effect>) {
    if m.tab != Tab::Cluster || m.cluster.view != ClusterView::Detail || !m.is_admin() {
        return;
    }
    let Some(row) = cluster::selected_row(m) else {
        return;
    };
    if !row.reachable {
        return;
    }
    if m.cluster.metrics.due(m.now) {
        let (id, effect) = issue(m, ApiCall::NodeMetrics(row.id.clone()));
        m.cluster.metrics.in_flight = Some(id);
        effects.push(effect);
    }
    if m.cluster.backup.due(m.now) {
        let (id, effect) = issue(m, ApiCall::NodeBackup(row.id));
        m.cluster.backup.in_flight = Some(id);
        effects.push(effect);
    }
}

fn poll_drain(m: &mut Model, effects: &mut Vec<Effect>) {
    let Some(watch) = m.cluster.watch.clone() else {
        return;
    };
    if m.cluster.drain.due(m.now) {
        let (id, effect) = issue(m, ApiCall::DrainStatus(watch.node));
        m.cluster.drain.in_flight = Some(id);
        effects.push(effect);
    }
}

fn status_every(m: &Model) -> u64 {
    if m.tab == Tab::Cluster {
        STATUS_EVERY_MS
    } else {
        m.history.interval_ms()
    }
}

fn nodes_every(m: &Model) -> u64 {
    if m.tab == Tab::Cluster {
        NODES_EVERY_MS
    } else {
        NODES_BACKGROUND_MS
    }
}

// ---------- the replies ----------

fn invalid(what: &str, detail: String) -> Fail {
    Fail {
        status: 200,
        code: "client.invalid-response".to_owned(),
        message: format!("{what}: {detail}"),
        request_id: None,
        retry_after: None,
        required: None,
    }
}

fn on_reply(
    m: &mut Model,
    id: u64,
    generation: u64,
    call: ApiCall,
    result: Result<Reply, Error>,
) -> Vec<Effect> {
    if generation != m.generation {
        return Vec::new();
    }
    let now = m.now;
    let outcome: Result<String, Fail> = result.map(|r| r.text()).map_err(|e| Fail::from(&e));
    if call.is_action() {
        return on_action_reply(m, call, outcome);
    }
    match call {
        ApiCall::Me => {
            if m.me.in_flight != Some(id) {
                return Vec::new();
            }
            match outcome.and_then(|t| api::parse_me(&t).map_err(|e| invalid("auth/me", e))) {
                Ok(me) => m.me.ok(now, me, ME_EVERY_MS),
                Err(fail) => m.me.fail(now, fail, 2_000),
            }
        }
        ApiCall::ClusterStatus => {
            if m.status.in_flight != Some(id) {
                return Vec::new();
            }
            let every = status_every(m);
            match outcome.and_then(|t| {
                api::parse_cluster_status(&t).map_err(|e| invalid("cluster/status", e))
            }) {
                Ok(status) => {
                    m.history.sample(now, &status);
                    m.status.ok(now, status, every);
                    m.status_denied = None;
                }
                Err(fail) if fail.is_denied() => {
                    // Not the root token: fall back to the list of nodes (TUI-CLU-07).
                    m.status.in_flight = None;
                    m.status_denied = Some(fail);
                }
                Err(fail) => m.status.fail(now, fail, every),
            }
        }
        ApiCall::Nodes => {
            if m.cluster.nodes.in_flight != Some(id) {
                return Vec::new();
            }
            let every = nodes_every(m);
            match outcome.and_then(|t| api::parse_nodes(&t).map_err(|e| invalid("nodes", e))) {
                Ok(nodes) => m.cluster.nodes.ok(now, nodes, every),
                Err(fail) => m.cluster.nodes.fail(now, fail, every),
            }
        }
        ApiCall::NodeMetrics(_) => {
            if m.cluster.metrics.in_flight != Some(id) {
                return Vec::new();
            }
            match outcome {
                Ok(text) => m
                    .cluster
                    .metrics
                    .ok(now, Metrics::parse(&text), METRICS_EVERY_MS),
                Err(fail) => m.cluster.metrics.fail(now, fail, METRICS_EVERY_MS),
            }
        }
        ApiCall::NodeBackup(_) => {
            if m.cluster.backup.in_flight != Some(id) {
                return Vec::new();
            }
            match outcome {
                Ok(text) => match api::parse_backup(&text) {
                    Ok(backup) => m.cluster.backup.ok(now, Some(backup), BACKUP_EVERY_MS),
                    Err(e) => m
                        .cluster
                        .backup
                        .fail(now, invalid("backup", e), BACKUP_EVERY_MS),
                },
                // No S3 backup configured is not an error: the section is simply left out.
                Err(fail) if fail.code == "backup.not-configured" => {
                    m.cluster.backup.ok(now, None, BACKUP_EVERY_MS);
                }
                Err(fail) => m.cluster.backup.fail(now, fail, BACKUP_EVERY_MS),
            }
        }
        ApiCall::Tokens(_) => return on_tokens_reply(m, id, outcome),
        ApiCall::Topics(_) => return on_topics_reply(m, id, outcome),
        ApiCall::TopicStats(_) => return on_topic_stats_reply(m, id, outcome),
        ApiCall::TopicDetail(name) => return on_topic_detail_reply(m, id, &name, outcome),
        ApiCall::DrainStatus(_) => {
            if m.cluster.drain.in_flight != Some(id) {
                return Vec::new();
            }
            match outcome.and_then(|t| api::parse_drain(&t).map_err(|e| invalid("drain", e))) {
                Ok(drain) => {
                    let state = drain.state.clone();
                    m.cluster.drain.ok(now, drain, DRAIN_EVERY_MS);
                    finish_watch(m, &state);
                }
                Err(fail) => m.cluster.drain.fail(now, fail, DRAIN_EVERY_MS),
            }
        }
        _ => {}
    }
    Vec::new()
}

/// A page of tokens: the next one is asked for until the server has no more.
fn on_tokens_reply(m: &mut Model, id: u64, outcome: Result<String, Fail>) -> Vec<Effect> {
    if m.tokens.list.in_flight != Some(id) {
        return Vec::new();
    }
    let now = m.now;
    match outcome.and_then(|t| api::parse_tokens(&t).map_err(|e| invalid("tokens", e))) {
        Ok(page) => {
            m.tokens.reading.extend(page.items);
            match page.next_cursor {
                Some(cursor) if m.tokens.pages < tokens::MAX_PAGES => {
                    m.tokens.pages += 1;
                    let (id, effect) = issue(m, ApiCall::Tokens(Some(cursor)));
                    m.tokens.list.in_flight = Some(id);
                    return vec![effect];
                }
                _ => {
                    let all = std::mem::take(&mut m.tokens.reading);
                    m.tokens.list.ok(now, all, TOKENS_EVERY_MS);
                }
            }
        }
        Err(fail) => {
            m.tokens.reading.clear();
            m.tokens.list.fail(now, fail, TOKENS_EVERY_MS);
        }
    }
    Vec::new()
}

/// A page of topics: the next one is asked for until the server has no more.
fn on_topics_reply(m: &mut Model, id: u64, outcome: Result<String, Fail>) -> Vec<Effect> {
    if m.topics.list.in_flight != Some(id) {
        return Vec::new();
    }
    let now = m.now;
    match outcome.and_then(|t| api::parse_topics(&t).map_err(|e| invalid("topics", e))) {
        Ok(page) => {
            m.topics.reading.extend(page.items);
            match page.next_cursor {
                Some(cursor) if m.topics.pages < topics::MAX_PAGES => {
                    m.topics.pages += 1;
                    let (id, effect) = issue(m, ApiCall::Topics(Some(cursor)));
                    m.topics.list.in_flight = Some(id);
                    return vec![effect];
                }
                _ => {
                    let all = std::mem::take(&mut m.topics.reading);
                    m.topics.list.ok(now, all, TOPICS_EVERY_MS);
                }
            }
        }
        Err(fail) => {
            m.topics.reading.clear();
            m.topics.list.fail(now, fail, TOPICS_EVERY_MS);
        }
    }
    Vec::new()
}

/// A page of the numbers of the topics. A failure only leaves the table without numbers (and says so).
fn on_topic_stats_reply(m: &mut Model, id: u64, outcome: Result<String, Fail>) -> Vec<Effect> {
    if m.topics.stats.in_flight != Some(id) {
        return Vec::new();
    }
    let now = m.now;
    match outcome.and_then(|t| api::parse_topic_stats(&t).map_err(|e| invalid("stats/topics", e))) {
        Ok(page) => {
            m.topics.stats_reading.by_name.extend(page.items);
            m.topics.stats_reading.partial |= page.partial;
            for missing in page.missing {
                if !m.topics.stats_reading.missing.contains(&missing) {
                    m.topics.stats_reading.missing.push(missing);
                }
            }
            match page.next_cursor {
                Some(cursor) if m.topics.stats_pages < topics::MAX_PAGES => {
                    m.topics.stats_pages += 1;
                    let (id, effect) = issue(m, ApiCall::TopicStats(Some(cursor)));
                    m.topics.stats.in_flight = Some(id);
                    return vec![effect];
                }
                _ => {
                    let all = std::mem::take(&mut m.topics.stats_reading);
                    m.topics.stats.ok(now, all, TOPICS_EVERY_MS);
                }
            }
        }
        Err(fail) => {
            m.topics.stats_reading = TopicStats::default();
            m.topics.stats.fail(now, fail, TOPICS_EVERY_MS);
        }
    }
    Vec::new()
}

/// A reading for the charts of a queue: it becomes one point per line.
fn on_topic_detail_reply(
    m: &mut Model,
    id: u64,
    name: &str,
    outcome: Result<String, Fail>,
) -> Vec<Effect> {
    let (now, every, cap) = (m.now, m.history.interval_ms(), m.history.capacity());
    let open = m
        .topics
        .monitor
        .as_ref()
        .is_some_and(|mon| mon.name == name && mon.feed.in_flight == Some(id));
    let target = if open {
        m.topics.monitor.as_mut()
    } else {
        m.topics
            .previews
            .iter_mut()
            .find(|p| p.name == name && p.feed.in_flight == Some(id))
    };
    let Some(mon) = target else {
        return Vec::new();
    };
    match outcome.and_then(|t| api::parse_topic_sample(&t).map_err(|e| invalid("stats/topics", e)))
    {
        Ok(sample) => {
            topics::monitor::record(mon, cap, now, &sample);
            mon.feed.ok(now, (), every);
        }
        Err(fail) => mon.feed.fail(now, fail, every),
    }
    Vec::new()
}

/// The drain reached what was being waited for: say so and refresh the cluster.
fn finish_watch(m: &mut Model, state: &str) {
    let Some(watch) = m.cluster.watch.clone() else {
        return;
    };
    let reached = match watch.goal {
        Goal::Drained => state == "drained",
        Goal::Active => state == "active",
    };
    if !reached {
        return;
    }
    let key = match watch.goal {
        Goal::Drained => "notice.drained",
        Goal::Active => "notice.returned",
    };
    let short = short_id(&watch.node);
    let text = m.tx().tw(key, &[("node", &short)]);
    m.notify(Severity::Ok, text);
    m.cluster.watch = None;
    m.status.refresh_now();
    m.cluster.nodes.refresh_now();
}

fn on_action_reply(m: &mut Model, call: ApiCall, outcome: Result<String, Fail>) -> Vec<Effect> {
    if m.busy.as_ref() == Some(&call) {
        m.busy = None;
    }
    let text = match outcome {
        Ok(text) => text,
        Err(fail) => {
            // A form that was refused keeps the reason in itself, and stays open.
            if matches!(
                call,
                ApiCall::TokenCreate(_)
                    | ApiCall::TokenUpdate(..)
                    | ApiCall::TopicCreate(_)
                    | ApiCall::TopicUpdate(..)
                    | ApiCall::Publish(_)
                    | ApiCall::Receive(..)
                    | ApiCall::Visibility(..)
            ) && tokens::refused(m, &fail)
            {
                return Vec::new();
            }
            // An acknowledgement that is refused is a line, not a popup: several go together.
            if let ApiCall::Ack(_, id, _) | ApiCall::Visibility(_, id, _, _) = &call {
                topics::messages::refused(m, id, &fail.message);
                return Vec::new();
            }
            m.modal = Some(Modal::Report(Report::Failure { call, fail }));
            return Vec::new();
        }
    };
    let tx = m.tx();
    m.status.refresh_now();
    m.cluster.nodes.refresh_now();
    match &call {
        ApiCall::DrainStart(node) | ApiCall::DrainCancel(node) => {
            let (key, goal) = if matches!(call, ApiCall::DrainStart(_)) {
                ("notice.drain-started", Goal::Drained)
            } else {
                ("notice.return-started", Goal::Active)
            };
            let short = short_id(node);
            m.notify(Severity::Ok, tx.tw(key, &[("node", &short)]));
            cluster::watch(m, node, goal);
        }
        ApiCall::FlushNode(_) | ApiCall::FlushCluster => match api::parse_flush(&text) {
            Ok(report) => m.modal = Some(Modal::Report(Report::Flush(report))),
            Err(e) => {
                m.notify(
                    Severity::Warning,
                    tx.tw("notice.bad-reply", &[("error", &e)]),
                );
            }
        },
        ApiCall::StopCluster => match api::parse_stop(&text) {
            Ok(report) => m.modal = Some(Modal::Report(Report::Stop(report))),
            Err(e) => {
                m.notify(
                    Severity::Warning,
                    tx.tw("notice.bad-reply", &[("error", &e)]),
                );
            }
        },
        ApiCall::TokenCreate(_) | ApiCall::TokenUpdate(..) => {
            return tokens::saved(m, &call, &text)
        }
        ApiCall::TokenRevoke(_) => tokens::revoked(m),
        ApiCall::TopicCreate(_) | ApiCall::TopicUpdate(..) => {
            return topics::saved(m, &call, &text)
        }
        ApiCall::TopicDelete(name) => topics::deleted(m, name),
        ApiCall::Publish(_) => topics::messages::published(m, &text),
        ApiCall::Receive(topic, hold, _) => topics::messages::received(m, topic, *hold, &text),
        ApiCall::Ack(_, id, _) => topics::messages::acked(m, id),
        ApiCall::Visibility(_, id, _, secs) => topics::messages::visibility_changed(m, id, *secs),
        ApiCall::Prune(_) => topics::messages::pruned(m, &text),
        _ => {}
    }
    Vec::new()
}

#[cfg(test)]
mod tests;
