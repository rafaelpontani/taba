use ratatui::crossterm::event::KeyCode;

use super::*;
use crate::tui::api::ApiCall;
use crate::tui::effect::Effect;
use crate::tui::i18n::Lang;
use crate::tui::model::{ClusterView, Modal, Pane, TopicTab};
use crate::tui::testing::*;

fn menu_of(sim: &Sim) -> Menu {
    match &sim.m.modal {
        Some(Modal::Menu(menu)) => menu.clone(),
        other => panic!("no menu: {other:?}"),
    }
}

/// TUI: TUI-FRM-07
#[test]
fn enter_on_a_topic_lists_what_can_be_done_to_it_and_to_its_messages() {
    let mut sim = Sim::on_topics(Lang::EnUs);
    sim.press('j');
    sim.code(KeyCode::Enter);
    let menu = menu_of(&sim);
    assert_eq!(menu.title, "Topic orders");
    assert_eq!(menu.items, TOPIC.to_vec());
    // The topic and its messages are told apart by the words.
    let tx = sim.m.tx();
    assert_eq!(tx.t(label_key(Action::TopicEdit)), "Edit the topic");
    assert_eq!(tx.t(label_key(Action::MsgPublish)), "Publish a message");
    assert_eq!(shortcut(Tab::Topics, Action::TopicCharts), Some("g"));
    // The keys inside the messages are not offered on the list, where `p` pauses.
    assert_eq!(shortcut(Tab::Topics, Action::MsgPublish), None);
    sim.code(KeyCode::Esc);
    assert!(sim.m.modal.is_none());
}

/// TUI: TUI-MSG-01
#[test]
fn the_charts_and_the_messages_open_from_the_menu_as_dialogs_and_esc_returns_to_the_list() {
    let mut sim = Sim::on_topics(Lang::EnUs);
    sim.press('j');
    sim.code(KeyCode::Enter);
    sim.code(KeyCode::Down);
    sim.code(KeyCode::Enter);
    let open = sim.m.topics.monitor.clone().expect("the charts are open");
    assert_eq!((open.name.as_str(), open.tab), ("orders", TopicTab::Charts));
    assert!(sim.m.modal.is_none());
    sim.code(KeyCode::Esc);
    assert!(sim.m.topics.monitor.is_none());
    // The shortcuts open them straight from the list; the messages are not an entry of the menu.
    assert!(!TOPIC.contains(&Action::TopicMessages));
    sim.press('g');
    assert_eq!(sim.m.topics.monitor.as_ref().unwrap().tab, TopicTab::Charts);
    sim.code(KeyCode::Esc);
    sim.press('x');
    assert_eq!(
        sim.m.topics.monitor.as_ref().unwrap().tab,
        TopicTab::Messages
    );
}

/// TUI: TUI-MSG-02
#[test]
fn publishing_from_the_menu_opens_the_messages_and_the_form() {
    let mut sim = Sim::on_topics(Lang::EnUs);
    sim.press('j');
    sim.code(KeyCode::Enter);
    for _ in 0..2 {
        sim.code(KeyCode::Down);
    }
    assert_eq!(
        menu_of(&sim).items[menu_of(&sim).selected],
        Action::MsgPublish
    );
    sim.code(KeyCode::Enter);
    assert!(sim.m.topics.monitor.is_some());
    assert!(
        matches!(sim.m.modal, Some(Modal::Editor(_))),
        "{:?}",
        sim.m.modal
    );
}

/// TUI: TUI-FRM-10
#[test]
fn an_entry_that_cannot_run_says_why_and_the_menu_closes() {
    let mut sim = Sim::on_topics(Lang::EnUs);
    // `fanout` is the first of the list, a distributor: no charts.
    sim.code(KeyCode::Enter);
    assert_eq!(menu_of(&sim).title, "Topic fanout");
    assert_eq!(
        gate(&sim.m, Action::TopicCharts),
        Some("gate.topic-no-charts")
    );
    assert_eq!(
        gate(&sim.m, Action::MsgReceive),
        Some("gate.topic-no-receive")
    );
    sim.code(KeyCode::Down);
    sim.code(KeyCode::Enter);
    assert!(sim.m.modal.is_none());
    assert!(sim.m.topics.monitor.is_none());
    assert!(sim.m.notice.is_some());
}

/// TUI: TUI-CLU-04
#[test]
fn on_the_nodes_the_menu_opens_with_the_detail_and_the_conditions_still_explain_themselves() {
    let mut sim = Sim::root_with_status(Lang::EnUs);
    // On the nodes pane Enter opens the menu (the conditions pane explains the condition: see the update tests).
    sim.m.cluster.pane = Pane::Nodes;
    sim.code(KeyCode::Enter);
    let menu = menu_of(&sim);
    assert_eq!(menu.items[0], Action::Detail);
    sim.code(KeyCode::Enter);
    assert_eq!(sim.m.cluster.view, ClusterView::Detail);
}

/// TUI: TUI-TOP-02
#[test]
fn the_summary_of_a_topic_is_a_dialog_of_its_own_and_the_strip_under_the_list_follows_the_selection(
) {
    let mut sim = Sim::on_topics(Lang::EnUs);
    sim.press('j');
    sim.press('i');
    assert_eq!(sim.m.modal, Some(Modal::TopicSummary));
    sim.code(KeyCode::Esc);
    assert!(sim.m.modal.is_none());
    // The list asks for the numbers of the selected queue, and of no other.
    let asked: Vec<_> = sim
        .tick(3_000)
        .into_iter()
        .filter_map(|e| match e {
            Effect::Call {
                call: ApiCall::TopicDetail(name),
                ..
            } => Some(name),
            _ => None,
        })
        .collect();
    assert_eq!(asked, vec!["orders".to_owned()]);
    assert_eq!(sim.m.topics.previews.len(), 1);
    // A distributor has no queue to chart: nothing is asked for it.
    sim.code(KeyCode::Up);
    let asked = sim.tick(9_000);
    assert!(!asked.iter().any(|e| matches!(
        e,
        Effect::Call {
            call: ApiCall::TopicDetail(_),
            ..
        }
    )));
}
