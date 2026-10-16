//! The Topics tab (doc 34, section 6.4): the topics of the server with their numbers, creating, editing and
//! deleting them. This file is the state and the keys; `view.rs` draws it. The checks of the form are the cheap
//! ones (a name, a number, a `key=value`); everything else is the server's to refuse, and its reason goes in the
//! form (TUI-TPF-02).

use taba_client::{Nullable, TopicRequest, TopicUpdate};

use crate::tui::api::{ApiCall, Severity, Topic, TopicNumbers};
use crate::tui::editor::{Editor, Field, Outcome, Purpose};
use crate::tui::effect::Effect;
use crate::tui::keymap::Action;
use crate::tui::model::{
    Confirm, Input, InputPurpose, Modal, Model, Pending, TopicSort, TopicTab, Typed,
};
use crate::tui::screens::cluster::moved;
use crate::tui::update::run;

pub mod messages;
pub mod monitor;
pub mod preview;
pub mod view;

/// How many pages of topics one read follows before it gives up (200 topics each).
pub const MAX_PAGES: u32 = 25;

/// The topics shown: those whose name has the filter in it, in the chosen order.
pub fn visible(m: &Model) -> Vec<Topic> {
    let wanted = m.topics.filter.to_lowercase();
    let mut topics: Vec<Topic> = m
        .topics
        .list
        .data
        .iter()
        .flatten()
        .filter(|t| m.topics.show_dead || !t.dead_letter)
        .filter(|t| wanted.is_empty() || t.name.to_lowercase().contains(&wanted))
        .cloned()
        .collect();
    match m.topics.sort {
        TopicSort::Name => topics.sort_by_key(|t| t.name.to_lowercase()),
        TopicSort::Newest => topics.sort_by(|a, b| b.created_at.cmp(&a.created_at)),
        TopicSort::Oldest => topics.sort_by(|a, b| a.created_at.cmp(&b.created_at)),
        TopicSort::Depth => topics.sort_by(|a, b| {
            let depth = |t: &Topic| numbers(m, &t.name).and_then(|n| n.depth).unwrap_or(0);
            depth(b)
                .cmp(&depth(a))
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        }),
    }
    topics
}

/// The numbers of a topic, when the statistics have been read and have it.
pub fn numbers<'a>(m: &'a Model, name: &str) -> Option<&'a TopicNumbers> {
    m.topics.stats.data.as_ref()?.by_name.get(name)
}

/// The index of the selected topic in `topics`; the first one if the selection is gone.
pub fn selected_index(m: &Model, topics: &[Topic]) -> Option<usize> {
    if topics.is_empty() {
        return None;
    }
    let found = m
        .topics
        .selected
        .as_ref()
        .and_then(|name| topics.iter().position(|t| &t.name == name));
    Some(found.unwrap_or(0))
}

pub fn selected(m: &Model) -> Option<Topic> {
    let topics = visible(m);
    selected_index(m, &topics).map(|i| topics[i].clone())
}

/// Why an action cannot run now, as a catalog key; `None` means it can (TUI-FRM-10).
pub fn gate(m: &Model, action: Action) -> Option<&'static str> {
    let Some(me) = &m.me.data else {
        return Some("gate.me-unknown");
    };
    if m.busy.is_some() {
        return Some("gate.busy");
    }
    match action {
        Action::TopicNew if !me.can("create") => Some("gate.need-create"),
        Action::TopicEdit | Action::TopicDelete => {
            let Some(topic) = selected(m) else {
                return Some("gate.no-topic");
            };
            if topic.dead_letter {
                Some("gate.topic-read-only")
            } else if !me.can("manage") {
                Some("gate.need-manage")
            } else {
                None
            }
        }
        _ => None,
    }
}

fn refuse(m: &mut Model, key: &str) -> Vec<Effect> {
    let text = m.tx().t(key);
    m.notify(Severity::Warning, text);
    Vec::new()
}

pub fn on_action(m: &mut Model, action: Action) -> Vec<Effect> {
    // With the charts of a queue open, the keys are theirs.
    if m.topics.monitor.is_some() {
        return monitor::on_action(m, action);
    }
    match action {
        Action::Enter => crate::tui::menu::open(m),
        Action::TopicSummary => match selected(m) {
            Some(_) => {
                m.modal = Some(Modal::TopicSummary);
                Vec::new()
            }
            None => refuse(m, "gate.no-topic"),
        },
        Action::TopicCharts => monitor::open(m, TopicTab::Charts),
        Action::TopicMessages => monitor::open(m, TopicTab::Messages),
        // From the menu of the list: open the messages of the topic and then do the thing in them.
        Action::MsgPublish | Action::MsgReceive | Action::MsgPrune => {
            let mut effects = monitor::open(m, TopicTab::Messages);
            if m.topics.monitor.is_some() {
                effects.extend(messages::on_action(m, action));
            }
            effects
        }
        Action::Up => step(m, -1),
        Action::Down => step(m, 1),
        Action::PageUp => step(m, -10),
        Action::PageDown => step(m, 10),
        Action::First => step(m, i32::MIN / 2),
        Action::Last => step(m, i32::MAX / 2),
        Action::Sort => {
            m.topics.sort = m.topics.sort.next();
            Vec::new()
        }
        Action::TopicDead => {
            m.topics.show_dead = !m.topics.show_dead;
            Vec::new()
        }
        Action::Filter => {
            m.modal = Some(Modal::Input(Input {
                purpose: InputPurpose::TopicFilter,
                value: m.topics.filter.clone(),
            }));
            Vec::new()
        }
        Action::TopicNew | Action::TopicEdit | Action::TopicDelete => {
            if let Some(key) = gate(m, action) {
                return refuse(m, key);
            }
            match action {
                Action::TopicNew => open_create(m),
                Action::TopicEdit => open_edit(m),
                _ => open_delete(m),
            }
            Vec::new()
        }
        _ => Vec::new(),
    }
}

fn step(m: &mut Model, delta: i32) -> Vec<Effect> {
    let topics = visible(m);
    if let Some(current) = selected_index(m, &topics) {
        let next = moved(current, delta, topics.len());
        m.topics.selected = Some(topics[next].name.clone());
    }
    Vec::new()
}

/// The text of a number field: nothing when the topic has none of its own.
fn number_text(value: Option<u64>) -> String {
    value.map(|n| n.to_string()).unwrap_or_default()
}

fn pairs_text(tags: &[(String, String)]) -> String {
    tags.iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn open_create(m: &mut Model) {
    let fields = vec![
        Field::text("name", "editor.field.name", ""),
        Field::check("distributor", "editor.field.distributor", false),
        Field::check("fifo", "editor.field.fifo", false),
        Field::check("volatile", "editor.field.volatile", false),
        Field::text("idle", "editor.field.idle", ""),
        Field::text("max-bytes", "editor.field.max-bytes", ""),
        Field::text("ttl", "editor.field.ttl", ""),
        Field::text("max-deliveries", "editor.field.max-deliveries", ""),
        Field::text("tags", "editor.field.tags", ""),
        Field::text("prefixes", "editor.field.topic-prefixes", ""),
    ];
    m.modal = Some(Modal::Editor(Editor::new(Purpose::TopicCreate, fields)));
}

fn open_edit(m: &mut Model) {
    let Some(topic) = selected(m) else { return };
    let distributor = topic.is_distributor();
    let fixed = "editor.locked.topic";
    let queue_only = |field: Field| {
        if distributor {
            field.locked("editor.locked.not-queue")
        } else {
            field
        }
    };
    let was: Vec<(&'static str, String)> = vec![
        ("idle", number_text(topic.idle_timeout)),
        ("max-bytes", number_text(topic.max_message_bytes)),
        ("ttl", number_text(topic.ttl)),
        ("max-deliveries", number_text(topic.max_deliveries)),
        ("tags", pairs_text(&topic.tags)),
        ("prefixes", topic.prefixes.join(" ")),
    ];
    let value = |id: &str| {
        was.iter()
            .find(|(f, _)| *f == id)
            .map(|(_, v)| v.clone())
            .unwrap_or_default()
    };
    let idle = if distributor {
        Field::text("idle", "editor.field.idle", &value("idle")).locked("editor.locked.not-queue")
    } else if topic.volatile {
        Field::text("idle", "editor.field.idle", &value("idle"))
    } else {
        Field::text("idle", "editor.field.idle", &value("idle"))
            .locked("editor.locked.not-volatile")
    };
    let prefixes = {
        let field = Field::text(
            "prefixes",
            "editor.field.topic-prefixes",
            &value("prefixes"),
        );
        if distributor {
            field
        } else {
            field.locked("editor.locked.not-distributor")
        }
    };
    let fields = vec![
        Field::text("name", "editor.field.name", &topic.name).locked(fixed),
        Field::check("distributor", "editor.field.distributor", distributor).locked(fixed),
        Field::check("fifo", "editor.field.fifo", topic.fifo).locked(fixed),
        Field::check("volatile", "editor.field.volatile", topic.volatile).locked(fixed),
        idle,
        queue_only(Field::text(
            "max-bytes",
            "editor.field.max-bytes",
            &value("max-bytes"),
        )),
        queue_only(Field::text("ttl", "editor.field.ttl", &value("ttl"))),
        queue_only(Field::text(
            "max-deliveries",
            "editor.field.max-deliveries",
            &value("max-deliveries"),
        )),
        Field::text("tags", "editor.field.tags", &value("tags")),
        prefixes,
    ];
    m.modal = Some(Modal::Editor(Editor::new(
        Purpose::TopicEdit {
            name: topic.name,
            was,
        },
        fields,
    )));
}

fn open_delete(m: &mut Model) {
    if let Some(topic) = selected(m) {
        m.modal = Some(Modal::Confirm(Confirm {
            typed: Some(Typed {
                expected: topic.name.clone(),
                input: String::new(),
            }),
            pending: Pending::DeleteTopic(topic.name),
            ok_focused: false,
        }));
    }
}

/// A problem with a field of the form: the field and the catalog key of what to tell.
type Problem = (&'static str, &'static str);

/// A topic name is made of letters, digits, `_` and `-` (doc 09).
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
}

/// `Ok(None)` for an empty field, the number otherwise.
fn parse_number(text: &str) -> Result<Option<u64>, ()> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    text.parse().map(Some).map_err(|_| ())
}

fn words(text: &str) -> impl Iterator<Item = &str> {
    text.split([',', ' ']).filter(|w| !w.is_empty())
}

/// `key=value` pairs separated by spaces or commas.
fn parse_tags(text: &str) -> Result<Vec<(String, String)>, ()> {
    words(text)
        .map(|word| match word.split_once('=') {
            Some((k, v)) if !k.is_empty() => Ok((k.to_owned(), v.to_owned())),
            _ => Err(()),
        })
        .collect()
}

fn parse_prefixes(text: &str) -> Vec<String> {
    words(text).map(str::to_owned).collect()
}

/// `default` is the same as nothing: the value of the node.
fn normal(text: &str) -> &str {
    let text = text.trim();
    if text.eq_ignore_ascii_case("default") {
        ""
    } else {
        text
    }
}

fn build_create(editor: &Editor) -> Result<ApiCall, Problem> {
    let name = editor.text("name");
    if name.is_empty() {
        return Err(("name", "editor.error.name"));
    }
    if !valid_name(name) {
        return Err(("name", "editor.error.topic-name"));
    }
    let number =
        |id: &'static str| parse_number(editor.text(id)).map_err(|()| (id, "editor.error.number"));
    let tags = parse_tags(editor.text("tags")).map_err(|()| ("tags", "editor.error.tags"))?;
    let prefixes = parse_prefixes(editor.text("prefixes"));
    let (idle, max_bytes, ttl, deliveries) = (
        number("idle")?,
        number("max-bytes")?,
        number("ttl")?,
        number("max-deliveries")?,
    );
    let distributor = editor.checked("distributor");
    let (fifo, volatile) = (editor.checked("fifo"), editor.checked("volatile"));
    let mut request = TopicRequest {
        name: name.to_owned(),
        topic_type: if distributor { "distributor" } else { "queue" }.to_owned(),
        tags,
        prefixes: prefixes.clone(),
        ..TopicRequest::default()
    };
    if distributor {
        if prefixes.is_empty() {
            return Err(("prefixes", "editor.error.prefixes-needed"));
        }
        if fifo
            || volatile
            || idle.is_some()
            || max_bytes.is_some()
            || ttl.is_some()
            || deliveries.is_some()
        {
            return Err(("distributor", "editor.error.distributor-fields"));
        }
    } else {
        if !prefixes.is_empty() {
            return Err(("prefixes", "editor.error.queue-prefixes"));
        }
        if idle.is_some() && !volatile {
            return Err(("idle", "editor.error.idle-needs-volatile"));
        }
        request.volatile = volatile.then_some(true);
        request.fifo = fifo.then_some(true);
        request.idle_timeout = idle;
        request.max_message_bytes = max_bytes;
        request.ttl = ttl;
        request.max_deliveries = deliveries;
    }
    Ok(ApiCall::TopicCreate(request.to_json()))
}

fn build_update(
    editor: &Editor,
    name: &str,
    was: &[(&'static str, String)],
) -> Result<ApiCall, Problem> {
    let before = |id: &str| {
        was.iter()
            .find(|(f, _)| *f == id)
            .map_or("", |(_, v)| v.as_str())
    };
    let changed = |id: &str| normal(editor.text(id)) != normal(before(id));
    let limit = |id: &'static str| -> Result<Nullable<u64>, Problem> {
        if !changed(id) {
            return Ok(Nullable::Keep);
        }
        match parse_number(normal(editor.text(id))) {
            Ok(Some(n)) => Ok(Nullable::Set(n)),
            Ok(None) => Ok(Nullable::Default),
            Err(()) => Err((id, "editor.error.number")),
        }
    };
    let mut update = TopicUpdate::default();
    if changed("tags") {
        update.tags =
            Some(parse_tags(editor.text("tags")).map_err(|()| ("tags", "editor.error.tags"))?);
    }
    if changed("prefixes") {
        let prefixes = parse_prefixes(editor.text("prefixes"));
        if prefixes.is_empty() {
            return Err(("prefixes", "editor.error.prefixes-needed"));
        }
        update.prefixes = Some(prefixes);
    }
    if changed("idle") {
        match parse_number(editor.text("idle")) {
            Ok(Some(n)) => update.idle_timeout = Some(n),
            _ => return Err(("idle", "editor.error.number")),
        }
    }
    update.max_message_bytes = limit("max-bytes")?;
    update.ttl = limit("ttl")?;
    update.max_deliveries = limit("max-deliveries")?;
    let body = update.to_json();
    if body == "{}" {
        return Err(("tags", "editor.error.nothing"));
    }
    Ok(ApiCall::TopicUpdate(name.to_owned(), body))
}

/// The form was filled in and the person asked to save it.
pub fn submit(m: &mut Model, mut editor: Editor) -> Vec<Effect> {
    let built = match &editor.purpose {
        Purpose::TopicCreate => build_create(&editor),
        Purpose::TopicEdit { name, was } => build_update(&editor, name, was),
        _ => return Vec::new(),
    };
    match built {
        Err((field, key)) => {
            editor.error = Some(m.tx().t(key));
            editor.focus_on(field);
            m.modal = Some(Modal::Editor(editor));
            Vec::new()
        }
        Ok(call) => {
            editor.busy = true;
            editor.error = None;
            m.modal = Some(Modal::Editor(editor));
            run(m, call)
        }
    }
}

/// The keys of the form of a topic.
pub fn editor_key(m: &mut Model, editor: Editor, outcome: Outcome) -> Vec<Effect> {
    match outcome {
        Outcome::Cancel => {
            m.modal = None;
            Vec::new()
        }
        Outcome::Submit => submit(m, editor),
        Outcome::Test | Outcome::None => {
            m.modal = Some(Modal::Editor(editor));
            Vec::new()
        }
    }
}

/// The server answered the form: the topic was created or changed.
pub fn saved(m: &mut Model, call: &ApiCall, text: &str) -> Vec<Effect> {
    let tx = m.tx();
    if matches!(m.modal, Some(Modal::Editor(_))) {
        m.modal = None;
    }
    m.topics.list.refresh_now();
    m.topics.stats.refresh_now();
    let created = matches!(call, ApiCall::TopicCreate(_));
    let key = if created {
        "notice.topic-created"
    } else {
        "notice.topic-updated"
    };
    match crate::tui::api::parse_topic(text) {
        Ok(topic) => {
            let notice = tx.tw(key, &[("name", &topic.name)]);
            if created {
                m.topics.selected = Some(topic.name);
            }
            m.notify(Severity::Ok, notice);
        }
        Err(e) => {
            m.notify(
                Severity::Warning,
                tx.tw("notice.bad-reply", &[("error", &e)]),
            );
        }
    }
    Vec::new()
}

/// A topic was deleted: it leaves the table at once (with its dead-letter queue), before the next read.
pub fn deleted(m: &mut Model, name: &str) {
    let text = m.tx().tw("notice.topic-deleted", &[("name", &name)]);
    m.notify(Severity::Ok, text);
    if let Some(list) = &mut m.topics.list.data {
        list.retain(|t| t.name != name && t.source.as_deref() != Some(name));
    }
    m.topics.list.refresh_now();
    m.topics.stats.refresh_now();
}

#[cfg(test)]
mod tests;
