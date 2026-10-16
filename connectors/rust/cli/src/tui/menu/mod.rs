//! The menu of an item (`Enter` on a row of a list): the actions that can be done to the selected topic, token,
//! profile or node, each named after what it acts on (`Edit the topic`, `Publish a message`), so a topic is never
//! mistaken for a message. The keys of the screens keep working; the menu only lists them.

use ratatui::crossterm::event::{KeyCode, KeyEvent};

use crate::tui::api::Severity;
use crate::tui::effect::Effect;
use crate::tui::keymap::{self, Action, Scope};
use crate::tui::model::{short_id, Menu, Modal, Model, Pane, Tab};
use crate::tui::screens::cluster as cluster_screen;
use crate::tui::screens::{profiles, tokens, topics};
use crate::tui::update::dispatch;

#[cfg(test)]
mod tests;

const TOPIC: &[Action] = &[
    Action::TopicSummary,
    Action::TopicCharts,
    Action::MsgPublish,
    Action::MsgReceive,
    Action::MsgPrune,
    Action::TopicEdit,
    Action::TopicDelete,
];
const TOKEN: &[Action] = &[Action::TokenEdit, Action::TokenClone, Action::TokenRevoke];
const PROFILE: &[Action] = &[
    Action::ProfileUse,
    Action::ProfileEdit,
    Action::ProfileDefault,
    Action::ProfileTest,
    Action::ProfileRemove,
];
const NODE: &[Action] = &[
    Action::Detail,
    Action::Monitor,
    Action::Drain,
    Action::Return,
    Action::FlushNode,
    Action::FlushAll,
    Action::StopCluster,
];

/// The catalog key of the words that name an action in the menu.
pub fn label_key(action: Action) -> &'static str {
    match action {
        Action::TopicSummary => "menu.topic-summary",
        Action::TopicCharts => "menu.topic-charts",
        Action::TopicMessages => "menu.topic-messages",
        Action::MsgPublish => "menu.msg-publish",
        Action::MsgReceive => "menu.msg-receive",
        Action::MsgPrune => "menu.msg-prune",
        Action::TopicEdit => "menu.topic-edit",
        Action::TopicDelete => "menu.topic-delete",
        Action::TokenEdit => "menu.token-edit",
        Action::TokenClone => "menu.token-clone",
        Action::TokenRevoke => "menu.token-revoke",
        Action::ProfileUse => "menu.profile-use",
        Action::ProfileEdit => "menu.profile-edit",
        Action::ProfileDefault => "menu.profile-default",
        Action::ProfileTest => "menu.profile-test",
        Action::ProfileRemove => "menu.profile-remove",
        Action::Detail => "menu.node-detail",
        Action::Monitor => "menu.node-monitor",
        Action::Drain => "menu.node-drain",
        Action::Return => "menu.node-return",
        Action::FlushNode => "menu.node-flush",
        Action::FlushAll => "menu.node-flush-all",
        Action::StopCluster => "menu.node-stop",
        _ => "hint.more",
    }
}

fn scope_of_tab(tab: Tab) -> Scope {
    match tab {
        Tab::Cluster => Scope::Cluster,
        Tab::Topics => Scope::Topics,
        Tab::Tokens => Scope::Tokens,
        Tab::Profiles => Scope::Profiles,
    }
}

/// The keys of an action on the list the menu was opened from; none when the key belongs to another view (inside
/// the messages `p` publishes, but on the list it pauses).
pub fn shortcut(tab: Tab, action: Action) -> Option<&'static str> {
    let scope = scope_of_tab(tab);
    keymap::BINDINGS
        .iter()
        .find(|b| b.action == action && b.scope == scope)
        .map(|b| b.label)
}

/// `Enter` on a list: opens the menu of the selected item, or says why there is none.
pub fn open(m: &mut Model) -> Vec<Effect> {
    let tx = m.tx();
    let (title, items): (String, &'static [Action]) = match m.tab {
        Tab::Topics => match topics::selected(m) {
            Some(t) => (tx.tw("menu.title-topic", &[("name", &t.name)]), TOPIC),
            None => return refuse(m, "gate.no-topic"),
        },
        Tab::Tokens => match tokens::selected(m) {
            Some(t) => (tx.tw("menu.title-token", &[("name", &t.name)]), TOKEN),
            None => return refuse(m, "gate.no-token"),
        },
        Tab::Profiles => match profiles::selected(m) {
            Some((name, _)) => (tx.tw("menu.title-profile", &[("name", &name)]), PROFILE),
            None => return refuse(m, "gate.no-profile"),
        },
        Tab::Cluster => match cluster_screen::selected_row(m) {
            Some(row) => (
                tx.tw("menu.title-node", &[("name", &short_id(&row.id))]),
                NODE,
            ),
            None => return refuse(m, "gate.no-node"),
        },
    };
    m.modal = Some(Modal::Menu(Menu {
        title,
        items: items.to_vec(),
        selected: 0,
    }));
    Vec::new()
}

fn refuse(m: &mut Model, key: &str) -> Vec<Effect> {
    let text = m.tx().t(key);
    m.notify(Severity::Warning, text);
    Vec::new()
}

/// Why an entry cannot run now, as a catalog key; `None` means it can.
pub fn gate(m: &Model, action: Action) -> Option<&'static str> {
    match m.tab {
        Tab::Topics => topic_gate(m, action),
        Tab::Tokens => tokens::gate(m, action),
        Tab::Profiles => None,
        Tab::Cluster => cluster_screen::gate(m, action).map(|g| g.key()),
    }
}

/// The checks of the entries of a topic. The messages ones are the same the open view makes, asked before opening it.
fn topic_gate(m: &Model, action: Action) -> Option<&'static str> {
    if matches!(action, Action::TopicEdit | Action::TopicDelete) {
        return topics::gate(m, action);
    }
    let me = m.me.data.as_ref()?;
    let topic = topics::selected(m)?;
    let distributor = topic.is_distributor();
    match action {
        Action::TopicCharts if distributor => Some("gate.topic-no-charts"),
        Action::MsgPublish if topic.dead_letter => Some("gate.topic-read-only"),
        Action::MsgPublish if !me.can("publish") => Some("gate.need-publish"),
        Action::MsgReceive | Action::MsgPrune if distributor => Some("gate.topic-no-receive"),
        Action::MsgReceive if !me.can("consume") => Some("gate.need-consume"),
        Action::MsgPrune if !me.can("manage") => Some("gate.need-manage"),
        _ => None,
    }
}

/// The keys of the open menu: the arrows choose, `Enter` runs the entry, `Esc` closes.
pub fn key(m: &mut Model, event: KeyEvent, mut menu: Menu) -> Vec<Effect> {
    let count = menu.items.len();
    match event.code {
        KeyCode::Esc | KeyCode::Char('q') => {
            m.modal = None;
            Vec::new()
        }
        KeyCode::Up | KeyCode::Char('k') => {
            menu.selected = cluster_screen::moved(menu.selected, -1, count);
            m.modal = Some(Modal::Menu(menu));
            Vec::new()
        }
        KeyCode::Down | KeyCode::Char('j') => {
            menu.selected = cluster_screen::moved(menu.selected, 1, count);
            m.modal = Some(Modal::Menu(menu));
            Vec::new()
        }
        KeyCode::Home => {
            menu.selected = 0;
            m.modal = Some(Modal::Menu(menu));
            Vec::new()
        }
        KeyCode::End => {
            menu.selected = count.saturating_sub(1);
            m.modal = Some(Modal::Menu(menu));
            Vec::new()
        }
        KeyCode::Enter => {
            let Some(action) = menu.items.get(menu.selected).copied() else {
                m.modal = None;
                return Vec::new();
            };
            m.modal = None;
            if let Some(reason) = gate(m, action) {
                return refuse(m, reason);
            }
            dispatch(m, action)
        }
        _ => {
            m.modal = Some(Modal::Menu(menu));
            Vec::new()
        }
    }
}

/// Only the nodes pane of the Cluster tab has a menu; the conditions pane explains the condition.
pub fn cluster_has_menu(m: &Model) -> bool {
    m.cluster.pane == Pane::Nodes
}
