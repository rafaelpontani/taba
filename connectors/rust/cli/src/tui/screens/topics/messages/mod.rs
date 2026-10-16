//! The Messages view of a topic (doc 34, TUI-MSG-*): it is one of the two views of an open topic, next to the
//! charts, so the topic is never asked for again. Publishing, reading (on request), acknowledging, giving back and
//! pruning. The API does not list a queue: the rows are the messages this session received (D-TUI-6).

use crate::tui::api::{self, ApiCall, PublishSpec, Severity, RECEIVE_HOLD_SECS};
use crate::tui::editor::{Editor, Field, Outcome, Purpose};
use crate::tui::effect::Effect;
use crate::tui::keymap::Action;
use crate::tui::model::{
    Confirm, Modal, Model, Pending, Received, TopicMonitor, Typed, MAX_RECEIVED,
};
use crate::tui::screens::cluster::moved;
use crate::tui::update::run;

use super::refuse;

pub mod view;

/// How much of a body a row keeps (the size shown is always the real one).
const KEPT_BODY: usize = 4096;

fn monitor(m: &Model) -> Option<&TopicMonitor> {
    m.topics.monitor.as_ref()
}

/// A message is held while its time of invisibility has not passed: only then does its receipt still work.
pub fn is_held(row: &Received, now: u64) -> bool {
    !row.acked && row.held_until.is_some_and(|until| until > now)
}

/// Why an action of the view cannot run now, as a catalog key; `None` means it can (TUI-FRM-10, TUI-MSG-08).
pub fn gate(m: &Model, action: Action) -> Option<&'static str> {
    let Some(me) = &m.me.data else {
        return Some("gate.me-unknown");
    };
    let mon = monitor(m)?;
    if m.busy.is_some() {
        return Some("gate.busy");
    }
    match action {
        Action::MsgPublish if mon.dead_letter => Some("gate.topic-read-only"),
        Action::MsgPublish if !me.can("publish") => Some("gate.need-publish"),
        Action::MsgReceive | Action::MsgAck | Action::MsgAckAll | Action::MsgRelease
            if mon.distributor =>
        {
            Some("gate.topic-no-receive")
        }
        Action::MsgReceive | Action::MsgAck | Action::MsgAckAll | Action::MsgRelease
            if !me.can("consume") =>
        {
            Some("gate.need-consume")
        }
        Action::MsgAck | Action::MsgRelease if selected_held(m).is_none() => {
            Some("gate.no-held-message")
        }
        Action::MsgAckAll if !mon.messages.rows.iter().any(|r| is_held(r, m.now)) => {
            Some("gate.no-held-message")
        }
        Action::MsgPrune if mon.distributor => Some("gate.topic-no-receive"),
        Action::MsgPrune if !me.can("manage") => Some("gate.need-manage"),
        _ => None,
    }
}

/// The selected row, when it is held.
fn selected_held(m: &Model) -> Option<&Received> {
    let mon = monitor(m)?;
    mon.messages
        .rows
        .get(mon.messages.selected)
        .filter(|row| is_held(row, m.now))
}

pub fn on_action(m: &mut Model, action: Action) -> Vec<Effect> {
    match action {
        Action::Up => step(m, -1),
        Action::Down => step(m, 1),
        Action::PageUp => step(m, -10),
        Action::PageDown => step(m, 10),
        Action::First => step(m, i32::MIN / 2),
        Action::Last => step(m, i32::MAX / 2),
        Action::MsgPublish
        | Action::MsgReceive
        | Action::MsgAck
        | Action::MsgAckAll
        | Action::MsgRelease
        | Action::MsgPrune => {
            if let Some(key) = gate(m, action) {
                return refuse(m, key);
            }
            carry_out(m, action)
        }
        _ => Vec::new(),
    }
}

fn carry_out(m: &mut Model, action: Action) -> Vec<Effect> {
    let Some(mon) = monitor(m) else {
        return Vec::new();
    };
    let topic = mon.name.clone();
    match action {
        Action::MsgPublish => {
            open_publish(m, topic);
            Vec::new()
        }
        Action::MsgReceive => {
            open_receive(m, topic);
            Vec::new()
        }
        Action::MsgPrune => {
            let typed = Typed {
                expected: topic.clone(),
                input: String::new(),
            };
            confirm(m, Pending::Prune(topic), Some(typed));
            Vec::new()
        }
        Action::MsgAck => match selected_held(m).map(|r| (r.id.clone(), r.receipt.clone())) {
            Some((id, receipt)) => run(m, ApiCall::Ack(topic, id, receipt)),
            None => Vec::new(),
        },
        Action::MsgRelease => match selected_held(m).map(|r| (r.id.clone(), r.receipt.clone())) {
            Some((id, receipt)) => run(m, ApiCall::Release(topic, id, receipt)),
            None => Vec::new(),
        },
        Action::MsgAckAll => ack_all(m, &topic),
        _ => Vec::new(),
    }
}

fn confirm(m: &mut Model, pending: Pending, typed: Option<Typed>) {
    m.modal = Some(Modal::Confirm(Confirm {
        pending,
        typed,
        ok_focused: false,
    }));
}

/// One acknowledgement for each message still held, sent together; each reply marks its own row.
fn ack_all(m: &mut Model, topic: &str) -> Vec<Effect> {
    let now = m.now;
    let calls: Vec<ApiCall> = monitor(m)
        .map(|mon| {
            mon.messages
                .rows
                .iter()
                .filter(|row| is_held(row, now))
                .map(|row| ApiCall::Ack(topic.to_owned(), row.id.clone(), row.receipt.clone()))
                .collect()
        })
        .unwrap_or_default();
    calls
        .into_iter()
        .map(|call| Effect::Call {
            id: m.id(),
            generation: m.generation,
            call,
        })
        .collect()
}

fn step(m: &mut Model, delta: i32) -> Vec<Effect> {
    if let Some(mon) = &mut m.topics.monitor {
        let rows = mon.messages.rows.len();
        if rows > 0 {
            mon.messages.selected = moved(mon.messages.selected, delta, rows);
        }
    }
    Vec::new()
}

// ---------- publishing ----------

fn open_publish(m: &mut Model, topic: String) {
    let fields = vec![
        Field::area("body", "editor.field.body", ""),
        Field::text(
            "content-type",
            "editor.field.content-type",
            "application/json",
        ),
        Field::text("group", "editor.field.group", ""),
        Field::text("dedup", "editor.field.dedup", ""),
    ];
    m.modal = Some(Modal::Editor(Editor::new(
        Purpose::MessagePublish { topic },
        fields,
    )));
}

/// Reading asks first how long the messages stay invisible: the form is the confirmation (D-TUI-6).
fn open_receive(m: &mut Model, topic: String) {
    let fields = vec![Field::text(
        "hold",
        "editor.field.hold",
        &RECEIVE_HOLD_SECS.to_string(),
    )];
    m.modal = Some(Modal::Editor(Editor::new(
        Purpose::MessageReceive { topic },
        fields,
    )));
}

/// The seconds of the form, or the field and the catalog key of what is wrong.
fn build_receive(editor: &Editor, topic: &str) -> Result<ApiCall, (&'static str, &'static str)> {
    match editor.text("hold").parse::<u32>() {
        Ok(secs) if secs > 0 => Ok(ApiCall::Receive(topic.to_owned(), secs)),
        _ => Err(("hold", "editor.error.hold")),
    }
}

fn build_publish(editor: &Editor, topic: &str) -> Result<ApiCall, (&'static str, &'static str)> {
    let body = editor.field("body").map_or("", |f| f.value.as_str());
    if body.trim().is_empty() {
        return Err(("body", "editor.error.body"));
    }
    let content_type = editor.text("content-type");
    if content_type.is_empty() {
        return Err(("content-type", "editor.error.content-type"));
    }
    let optional = |id: &str| {
        Some(editor.text(id))
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
    };
    Ok(ApiCall::Publish(PublishSpec {
        topic: topic.to_owned(),
        body: body.to_owned(),
        content_type: content_type.to_owned(),
        group_id: optional("group"),
        deduplication_id: optional("dedup"),
    }))
}

/// The keys of the publish form.
pub fn editor_key(m: &mut Model, mut editor: Editor, outcome: Outcome) -> Vec<Effect> {
    match outcome {
        Outcome::Cancel => {
            m.modal = None;
            Vec::new()
        }
        Outcome::Submit => {
            let built = match editor.purpose.clone() {
                Purpose::MessagePublish { topic } => build_publish(&editor, &topic),
                Purpose::MessageReceive { topic } => build_receive(&editor, &topic),
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
        Outcome::Test | Outcome::None => {
            m.modal = Some(Modal::Editor(editor));
            Vec::new()
        }
    }
}

fn say(m: &mut Model, severity: Severity, text: String) {
    if let Some(mon) = &mut m.topics.monitor {
        mon.messages.last = Some((severity, text.clone()));
    }
}

/// The server took the message.
pub fn published(m: &mut Model, text: &str) {
    if matches!(m.modal, Some(Modal::Editor(_))) {
        m.modal = None;
    }
    let tx = m.tx();
    match api::parse_published(text) {
        Ok(done) => {
            let (severity, line) = if done.duplicate {
                (Severity::Warning, tx.t("messages.published-duplicate"))
            } else if let Some(matched) = done.matched {
                (
                    Severity::Ok,
                    tx.tw("messages.published-matched", &[("n", &matched)]),
                )
            } else {
                let id = done.id.unwrap_or_default();
                (Severity::Ok, tx.tw("messages.published", &[("id", &id)]))
            };
            say(m, severity, line);
        }
        Err(e) => {
            let line = tx.tw("notice.bad-reply", &[("error", &e)]);
            say(m, Severity::Warning, line);
        }
    }
    if let Some(mon) = &mut m.topics.monitor {
        mon.feed.refresh_now();
    }
}

// ---------- receiving ----------

/// A reading came back: the messages go on top of the rows, held for `hold` seconds.
pub fn received(m: &mut Model, topic: &str, hold: u32, text: &str) {
    if matches!(m.modal, Some(Modal::Editor(_))) {
        m.modal = None;
    }
    let tx = m.tx();
    let now = m.now;
    let messages = match api::parse_received(text) {
        Ok(messages) => messages,
        Err(e) => {
            let line = tx.tw("notice.bad-reply", &[("error", &e)]);
            return say(m, Severity::Warning, line);
        }
    };
    let Some(mon) = &mut m.topics.monitor else {
        return;
    };
    if mon.name != topic {
        return;
    }
    let count = messages.len();
    let fresh: Vec<Received> = messages
        .into_iter()
        .map(|message| Received {
            size: message.body.len(),
            body: String::from_utf8_lossy(&message.body)
                .chars()
                .take(KEPT_BODY)
                .collect(),
            id: message.id,
            group: message.group_id,
            deliveries: message.delivery_count,
            content_type: message.content_type,
            receipt: message.receipt_token,
            at: now,
            held_until: Some(now + u64::from(hold) * 1000),
            acked: false,
        })
        .collect();
    // New arrivals do not move the selection (TUI-MSG-04): the selected message keeps its place.
    if !mon.messages.rows.is_empty() {
        mon.messages.selected += count;
    }
    let mut rows = fresh;
    rows.append(&mut mon.messages.rows);
    rows.truncate(MAX_RECEIVED);
    mon.messages.selected = mon.messages.selected.min(rows.len().saturating_sub(1));
    mon.messages.rows = rows;
    mon.feed.refresh_now();
    let line = if count == 0 {
        tx.t("messages.nothing")
    } else {
        tx.tw("messages.received", &[("n", &count), ("secs", &hold)])
    };
    say(m, Severity::Ok, line);
}

/// A message was acknowledged: it is gone from the queue.
pub fn acked(m: &mut Model, id: &str) {
    if let Some(mon) = &mut m.topics.monitor {
        if let Some(row) = mon.messages.rows.iter_mut().find(|r| r.id == id) {
            row.acked = true;
            row.held_until = None;
        }
        mon.feed.refresh_now();
    }
    let line = m.tx().tw("messages.acked", &[("id", &short(id))]);
    say(m, Severity::Ok, line);
}

/// A message was given back to the queue: it is visible again.
pub fn released(m: &mut Model, id: &str) {
    if let Some(mon) = &mut m.topics.monitor {
        if let Some(row) = mon.messages.rows.iter_mut().find(|r| r.id == id) {
            row.held_until = None;
        }
        mon.feed.refresh_now();
    }
    let line = m.tx().tw("messages.released", &[("id", &short(id))]);
    say(m, Severity::Ok, line);
}

/// An acknowledgement or a release was refused (usually the hold ran out): a line, not a popup, since several may fail
/// together.
pub fn refused(m: &mut Model, id: &str, message: &str) {
    if let Some(mon) = &mut m.topics.monitor {
        // The receipt no longer works, whatever the reason.
        if let Some(row) = mon.messages.rows.iter_mut().find(|r| r.id == id) {
            row.held_until = None;
        }
    }
    let line = m.tx().tw(
        "messages.refused",
        &[("id", &short(id)), ("error", &message)],
    );
    say(m, Severity::Warning, line);
}

/// The queue was pruned: what was read is gone with it.
pub fn pruned(m: &mut Model, text: &str) {
    let tx = m.tx();
    match api::parse_pruned(text) {
        Ok(removed) => {
            if let Some(mon) = &mut m.topics.monitor {
                for row in &mut mon.messages.rows {
                    row.acked = true;
                    row.held_until = None;
                }
                mon.feed.refresh_now();
            }
            let shown = crate::tui::i18n::group(m.lang, removed);
            let line = tx.tw("messages.pruned", &[("n", &shown)]);
            say(m, Severity::Ok, line);
        }
        Err(e) => {
            let line = tx.tw("notice.bad-reply", &[("error", &e)]);
            say(m, Severity::Warning, line);
        }
    }
    m.topics.stats.refresh_now();
}

/// The start of a message id, for lines of text.
pub fn short(id: &str) -> String {
    crate::tui::model::short_id(id)
}

#[cfg(test)]
mod tests;
