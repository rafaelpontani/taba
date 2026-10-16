use ratatui::crossterm::event::KeyCode;

use super::*;
use crate::tui::api::Me;
use crate::tui::i18n::Lang;
use crate::tui::keymap::Scope;
use crate::tui::model::TopicTab;
use crate::tui::testing::*;
use crate::tui::update::scope_of;

fn first_call(effects: &[Effect]) -> (Effect, ApiCall) {
    effects
        .iter()
        .find_map(|e| match e {
            Effect::Call { call, .. } => Some((e.clone(), call.clone())),
            _ => None,
        })
        .expect("a call")
}

fn all_calls(effects: &[Effect]) -> Vec<ApiCall> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::Call { call, .. } => Some(call.clone()),
            _ => None,
        })
        .collect()
}

fn type_text(sim: &mut Sim, text: &str) {
    for c in text.chars() {
        sim.press(c);
    }
}

fn mon(sim: &Sim) -> TopicMonitor {
    sim.m.topics.monitor.clone().expect("a topic is open")
}

fn notice(sim: &Sim) -> String {
    sim.m
        .notice
        .as_ref()
        .map(|n| n.text.clone())
        .unwrap_or_default()
}

/// Reads `n` messages through the confirmation, as a person would.
fn read(sim: &mut Sim, n: u32) {
    sim.press('r');
    let Some(Modal::Editor(editor)) = &sim.m.modal else {
        panic!("{:?}", sim.m.modal)
    };
    assert_eq!(editor.text("hold"), "30", "the time comes filled in");
    assert_eq!(editor.text("max"), "10", "the number comes filled in");
    sim.code(KeyCode::Enter);
    let effects = sim.code(KeyCode::Enter);
    let (effect, call) = first_call(&effects);
    assert_eq!(call, ApiCall::Receive("orders".to_owned(), 30, 10));
    sim.answer(&effect, ok(&messages_json(n)));
}

/// TUI: TUI-MSG-01
#[test]
fn x_opens_the_messages_of_the_topic_as_a_dialog_and_esc_closes_it() {
    let mut sim = Sim::on_topic_messages(Lang::EnUs);
    assert_eq!(mon(&sim).name, "orders");
    assert_eq!(mon(&sim).tab, TopicTab::Messages);
    assert_eq!(scope_of(&sim.m), Scope::Messages);
    // The arrows do not change the view: the charts are their own dialog, opened from the list.
    sim.code(KeyCode::Left);
    assert_eq!(mon(&sim).tab, TopicTab::Messages);
    sim.code(KeyCode::Esc);
    assert!(sim.m.topics.monitor.is_none());
    assert_eq!(scope_of(&sim.m), Scope::Topics);
}

/// TUI: TUI-MSG-06
#[test]
fn opening_the_view_never_reads_the_queue() {
    let mut sim = Sim::on_topic_messages(Lang::EnUs);
    let asked = all_calls(&sim.tick(3_000));
    assert!(
        asked.contains(&ApiCall::TopicDetail("orders".to_owned())),
        "{asked:?}"
    );
    assert!(
        !asked.iter().any(|c| matches!(c, ApiCall::Receive(..))),
        "{asked:?}"
    );
    assert!(mon(&sim).messages.rows.is_empty());
}

/// TUI: TUI-MSG-08
#[test]
fn a_distributor_only_publishes() {
    let mut sim = Sim::on_topics(Lang::EnUs);
    // `fanout` is the first of the list.
    sim.press('g');
    assert!(
        sim.m.topics.monitor.is_none(),
        "no charts for a distributor"
    );
    sim.press('x');
    assert!(mon(&sim).distributor);
    assert_eq!(mon(&sim).tab, TopicTab::Messages);
    sim.press('r');
    assert!(sim.m.modal.is_none());
    assert!(notice(&sim).contains("distributor"), "{}", notice(&sim));
    sim.press('p');
    assert!(matches!(sim.m.modal, Some(Modal::Editor(_))));
    // It is not sampled: it has no queue.
    sim.code(KeyCode::Esc);
    let asked = all_calls(&sim.tick(3_000));
    assert!(
        !asked.iter().any(|c| matches!(c, ApiCall::TopicDetail(_))),
        "{asked:?}"
    );
}

/// TUI: TUI-MSG-02
#[test]
fn publishing_checks_the_form_sends_the_message_and_says_the_result() {
    let mut sim = Sim::on_topic_messages(Lang::EnUs);
    sim.press('p');
    // An empty body is told, in the form.
    sim.send(crate::tui::msg::Msg::Key(
        ratatui::crossterm::event::KeyEvent::new(
            KeyCode::Char('s'),
            ratatui::crossterm::event::KeyModifiers::CONTROL,
        ),
    ));
    let Some(Modal::Editor(editor)) = &sim.m.modal else {
        panic!("{:?}", sim.m.modal)
    };
    assert!(editor.error.is_some());
    assert_eq!(editor.fields[editor.focus].id, "body");
    type_text(&mut sim, r#"{"order":7}"#);
    // Content type, the base64 box, then the group.
    sim.code(KeyCode::Tab);
    sim.code(KeyCode::Tab);
    sim.code(KeyCode::Tab);
    type_text(&mut sim, "customer-7");
    let effects = sim.send(crate::tui::msg::Msg::Key(
        ratatui::crossterm::event::KeyEvent::new(
            KeyCode::Char('s'),
            ratatui::crossterm::event::KeyModifiers::CONTROL,
        ),
    ));
    let (effect, call) = first_call(&effects);
    assert_eq!(
        call,
        ApiCall::Publish(PublishSpec {
            topic: "orders".into(),
            body: r#"{"order":7}"#.into(),
            content_type: "application/json".into(),
            group_id: Some("customer-7".into()),
            deduplication_id: None,
        })
    );
    // The route carries the body, the type and the group.
    let route = call.to_call();
    assert_eq!(route.method, "POST");
    assert_eq!(route.body, br#"{"order":7}"#.to_vec());
    sim.answer(&effect, ok(r#"{"id":"01a1-m9"}"#));
    assert!(sim.m.modal.is_none());
    assert!(mon(&sim)
        .messages
        .last
        .as_ref()
        .unwrap()
        .1
        .contains("01a1-m9"));
}

/// TUI: TUI-MSG-02
#[test]
fn a_refused_publication_keeps_the_form_open_with_the_reason() {
    let mut sim = Sim::on_topic_messages(Lang::EnUs);
    sim.press('p');
    type_text(&mut sim, "hello");
    let effects = sim.code(KeyCode::Tab);
    assert!(effects.is_empty());
    let effects = sim.send(crate::tui::msg::Msg::Key(
        ratatui::crossterm::event::KeyEvent::new(
            KeyCode::Char('s'),
            ratatui::crossterm::event::KeyModifiers::CONTROL,
        ),
    ));
    let (effect, _) = first_call(&effects);
    sim.answer(&effect, fail(413, "message.too-large", "{}"));
    let Some(Modal::Editor(editor)) = &sim.m.modal else {
        panic!("{:?}", sim.m.modal)
    };
    assert!(editor
        .error
        .as_deref()
        .unwrap_or("")
        .contains("message.too-large"));
    assert!(!editor.busy);
}

/// TUI: TUI-MSG-03, TUI-MSG-04
#[test]
fn reading_asks_first_and_each_reading_replaces_the_list() {
    let mut sim = Sim::on_topic_messages(Lang::EnUs);
    // Esc on the confirmation reads nothing.
    sim.press('r');
    let effects = sim.code(KeyCode::Esc);
    assert!(all_calls(&effects).is_empty());
    // Opening the form reads nothing; only sending it does.
    sim.press('r');
    assert!(matches!(sim.m.modal, Some(Modal::Editor(_))));
    sim.code(KeyCode::Esc);

    read(&mut sim, 3);
    let rows = mon(&sim).messages.rows;
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].id, "01a1-m1");
    assert_eq!(rows[0].group.as_deref(), Some("customer-1"));
    assert!(rows.iter().all(|r| r.held_until.is_some()));
    sim.press('j');
    assert_eq!(mon(&sim).messages.selected, 1);
    read(&mut sim, 2);
    // The list is cleared: only what the last reading brought, with the first one selected.
    assert_eq!(mon(&sim).messages.rows.len(), 2);
    assert_eq!(mon(&sim).messages.selected, 0);
    assert_eq!(mon(&sim).messages.rows[0].id, "01a1-m1");
}

/// TUI: TUI-MSG-03
#[test]
fn acknowledging_gives_back_and_the_expiry_of_the_hold_are_told_by_the_row() {
    let mut sim = Sim::on_topic_messages(Lang::EnUs);
    read(&mut sim, 3);
    // Ack the first.
    let effects = sim.press('a');
    let (effect, call) = first_call(&effects);
    assert_eq!(
        call,
        ApiCall::Ack("orders".into(), "01a1-m1".into(), "rt-1".into())
    );
    assert_eq!(call.to_call().method, "DELETE");
    sim.answer(&effect, ok("{}"));
    assert!(mon(&sim).messages.rows[0].acked);
    // It cannot be acknowledged again.
    sim.press('a');
    assert!(notice(&sim).contains("not held"), "{}", notice(&sim));
    // Give the second back.
    sim.press('j');
    let effects = sim.press('v');
    let (effect, call) = first_call(&effects);
    assert_eq!(
        call,
        ApiCall::Visibility("orders".into(), "01a1-m2".into(), "rt-2".into(), 0)
    );
    assert_eq!(call.to_call().method, "PUT");
    sim.answer(
        &effect,
        ok(r#"{"id":"01a1-m2","invisibleUntil":"2026-10-16T10:00:00Z"}"#),
    );
    assert!(!is_held(&mon(&sim).messages.rows[1], sim.m.now));
    // The third is acknowledged with the others at once; only what is held goes.
    let effects = sim.press('A');
    assert_eq!(
        all_calls(&effects),
        vec![ApiCall::Ack(
            "orders".into(),
            "01a1-m3".into(),
            "rt-3".into()
        )]
    );
    // After the hold, nothing can be acknowledged: the receipt is dead.
    let mut late = Sim::on_topic_messages(Lang::EnUs);
    read(&mut late, 1);
    late.tick(1_000 + 31_000);
    late.press('a');
    assert!(notice(&late).contains("not held"), "{}", notice(&late));
}

/// TUI: TUI-MSG-03
#[test]
fn a_refused_acknowledgement_is_a_line_and_the_row_stops_being_held() {
    let mut sim = Sim::on_topic_messages(Lang::EnUs);
    read(&mut sim, 1);
    let effects = sim.press('a');
    let (effect, _) = first_call(&effects);
    sim.answer(&effect, fail(409, "message.receipt-expired", "{}"));
    assert!(sim.m.modal.is_none(), "{:?}", sim.m.modal);
    assert!(
        mon(&sim)
            .messages
            .last
            .as_ref()
            .unwrap()
            .1
            .contains("not changed"),
        "{:?}",
        mon(&sim).messages.last
    );
    assert!(!is_held(&mon(&sim).messages.rows[0], sim.m.now));
}

/// TUI: TUI-MSG-07
#[test]
fn pruning_asks_for_the_name_and_reports_how_many_were_removed() {
    let mut sim = Sim::on_topic_messages(Lang::EnUs);
    read(&mut sim, 2);
    sim.press('P');
    type_text(&mut sim, "order");
    let effects = sim.code(KeyCode::Enter);
    assert!(all_calls(&effects).is_empty(), "the name is not complete");
    type_text(&mut sim, "s");
    let effects = sim.code(KeyCode::Enter);
    let (effect, call) = first_call(&effects);
    assert_eq!(call, ApiCall::Prune("orders".to_owned()));
    assert_eq!(call.to_call().method, "DELETE");
    assert_eq!(call.to_call().path, "/topics/orders/messages");
    sim.answer(&effect, ok(r#"{"removed":1500,"nodes":[]}"#));
    assert!(mon(&sim)
        .messages
        .last
        .as_ref()
        .unwrap()
        .1
        .contains("1,500"));
    assert!(mon(&sim).messages.rows.iter().all(|r| r.acked));
}

/// TUI: TUI-MSG-08
#[test]
fn what_the_token_may_not_do_is_refused_with_the_reason() {
    let mut sim = Sim::on_topic_messages(Lang::EnUs);
    sim.m.me.data = Some(Me {
        auth_enabled: true,
        permissions: vec!["publish".to_owned()],
        ..Me::default()
    });
    sim.press('r');
    assert!(sim.m.modal.is_none());
    assert!(notice(&sim).contains("consume"), "{}", notice(&sim));
    sim.press('P');
    assert!(sim.m.modal.is_none());
    assert!(notice(&sim).contains("manage"), "{}", notice(&sim));
    sim.press('p');
    assert!(matches!(sim.m.modal, Some(Modal::Editor(_))));
    sim.code(KeyCode::Esc);
    sim.m.me.data = Some(Me {
        auth_enabled: true,
        permissions: vec!["consume".to_owned()],
        ..Me::default()
    });
    sim.press('p');
    assert!(sim.m.modal.is_none());
    assert!(notice(&sim).contains("publish"), "{}", notice(&sim));
}

/// TUI: TUI-MSG-04
#[test]
fn reading_again_clears_the_list_even_when_nothing_came() {
    let mut sim = Sim::on_topic_messages(Lang::EnUs);
    read(&mut sim, 3);
    assert_eq!(mon(&sim).messages.rows.len(), 3);
    sim.press('j');
    read(&mut sim, 0);
    assert!(mon(&sim).messages.rows.is_empty());
    assert_eq!(mon(&sim).messages.selected, 0);
    assert!(
        !notice_or_line(&sim).is_empty(),
        "the line says that nothing came"
    );
}

fn notice_or_line(sim: &Sim) -> String {
    mon(sim)
        .messages
        .last
        .as_ref()
        .map(|(_, text)| text.clone())
        .unwrap_or_default()
}

/// TUI: TUI-MSG-02
#[test]
fn the_body_may_have_several_lines_and_goes_as_typed() {
    let mut sim = Sim::on_topic_messages(Lang::EnUs);
    sim.press('p');
    type_text(&mut sim, "{");
    sim.code(KeyCode::Enter);
    type_text(&mut sim, "  \"order\": 7");
    sim.code(KeyCode::Enter);
    type_text(&mut sim, "}");
    let Some(Modal::Editor(editor)) = &sim.m.modal else {
        panic!("{:?}", sim.m.modal)
    };
    assert_eq!(editor.focus, 0, "Enter in the body breaks the line");
    let effects = sim.send(crate::tui::msg::Msg::Key(
        ratatui::crossterm::event::KeyEvent::new(
            KeyCode::Char('s'),
            ratatui::crossterm::event::KeyModifiers::CONTROL,
        ),
    ));
    let (_, call) = first_call(&effects);
    let ApiCall::Publish(spec) = call else {
        panic!("{call:?}")
    };
    assert_eq!(
        String::from_utf8(spec.body).unwrap(),
        "{
  \"order\": 7
}"
    );
}

/// TUI: TUI-MSG-06
#[test]
fn the_time_of_invisibility_can_be_changed_and_must_be_a_number_above_zero() {
    let mut sim = Sim::on_topic_messages(Lang::EnUs);
    sim.press('r');
    sim.code(KeyCode::Tab);
    // The field comes with 30; Ctrl+u clears it.
    sim.send(crate::tui::msg::Msg::Key(
        ratatui::crossterm::event::KeyEvent::new(
            KeyCode::Char('u'),
            ratatui::crossterm::event::KeyModifiers::CONTROL,
        ),
    ));
    type_text(&mut sim, "0");
    assert!(sim.code(KeyCode::Enter).is_empty());
    let Some(Modal::Editor(editor)) = &sim.m.modal else {
        panic!("{:?}", sim.m.modal)
    };
    assert!(editor.error.is_some(), "a zero is refused in the form");
    sim.code(KeyCode::Backspace);
    type_text(&mut sim, "90");
    let effects = sim.code(KeyCode::Enter);
    let (effect, call) = first_call(&effects);
    assert_eq!(call, ApiCall::Receive("orders".to_owned(), 90, 10));
    assert!(call
        .to_call()
        .query
        .contains(&("visibilityTimeout".to_owned(), "90".to_owned())));
    sim.answer(&effect, ok(&messages_json(2)));
    assert!(sim.m.modal.is_none());
    // The rows are held for the time that was asked.
    let row = &mon(&sim).messages.rows[0];
    assert_eq!(row.held_until, Some(sim.m.now + 90_000));
}

/// TUI: TUI-MSG-06
#[test]
fn the_form_of_reading_asks_how_many_messages_with_ten_by_default() {
    let mut sim = Sim::on_topic_messages(Lang::EnUs);
    sim.press('r');
    let Some(Modal::Editor(editor)) = &sim.m.modal else {
        panic!("{:?}", sim.m.modal)
    };
    assert_eq!(editor.text("max"), "10");
    assert_eq!(editor.text("hold"), "30");
    // A zero is refused in the form; 3 goes to the call.
    sim.send(crate::tui::msg::Msg::Key(
        ratatui::crossterm::event::KeyEvent::new(
            KeyCode::Char('u'),
            ratatui::crossterm::event::KeyModifiers::CONTROL,
        ),
    ));
    type_text(&mut sim, "0");
    sim.code(KeyCode::Tab);
    assert!(sim.code(KeyCode::Enter).is_empty());
    let Some(Modal::Editor(editor)) = &sim.m.modal else {
        panic!("{:?}", sim.m.modal)
    };
    assert!(editor.error.is_some());
    sim.code(KeyCode::BackTab);
    sim.code(KeyCode::Backspace);
    type_text(&mut sim, "3");
    sim.code(KeyCode::Tab);
    let effects = sim.code(KeyCode::Enter);
    let (effect, call) = first_call(&effects);
    assert_eq!(call, ApiCall::Receive("orders".to_owned(), 30, 3));
    assert!(call
        .to_call()
        .query
        .contains(&("maxMessages".to_owned(), "3".to_owned())));
    sim.answer(&effect, ok(&messages_json(3)));
}

/// TUI: TUI-MSG-07
#[test]
fn enter_opens_the_message_and_it_can_change_its_time_or_be_deleted_while_it_is_held() {
    let mut sim = Sim::on_topic_messages(Lang::EnUs);
    read(&mut sim, 2);
    sim.code(KeyCode::Enter);
    assert_eq!(sim.m.modal, Some(Modal::Message));
    // v asks for the new time (30 by default) and sends it for the message that is open.
    sim.press('v');
    let Some(Modal::Editor(editor)) = &sim.m.modal else {
        panic!("{:?}", sim.m.modal)
    };
    assert_eq!(editor.text("seconds"), "30");
    let effects = sim.code(KeyCode::Enter);
    let (effect, call) = first_call(&effects);
    let ApiCall::Visibility(topic, id, _, secs) = call.clone() else {
        panic!("{call:?}")
    };
    assert_eq!(
        (topic.as_str(), id.as_str(), secs),
        ("orders", "01a1-m1", 30)
    );
    sim.answer(&effect, ok("{}"));
    assert!(sim.m.modal.is_none());
    let row = &mon(&sim).messages.rows[0];
    assert_eq!(row.held_until, Some(sim.m.now + 30_000));
    // d deletes it and closes the dialog.
    sim.code(KeyCode::Enter);
    let effects = sim.press('d');
    let (effect, call) = first_call(&effects);
    assert!(matches!(call, ApiCall::Ack(..)), "{call:?}");
    sim.answer(&effect, ok("{}"));
    assert!(sim.m.modal.is_none());
    assert!(mon(&sim).messages.rows[0].acked);
}

/// TUI: TUI-MSG-08
#[test]
fn when_the_time_runs_out_the_open_message_says_it_cannot_be_handled_any_more() {
    let mut sim = Sim::on_topic_messages(Lang::EnUs);
    read(&mut sim, 1);
    sim.code(KeyCode::Enter);
    // The hold ends while the message is open.
    sim.m.now += 31_000;
    assert!(sim.press('v').is_empty());
    assert_eq!(
        sim.m.modal,
        Some(Modal::Message),
        "the dialog stays; the key says why it cannot run"
    );
    assert!(notice(&sim).contains("not held"), "{}", notice(&sim));
    assert!(sim.press('d').is_empty());
    sim.code(KeyCode::Esc);
    assert!(sim.m.modal.is_none());
}

/// TUI: TUI-MSG-02
#[test]
fn a_body_in_base64_is_sent_as_the_bytes_it_stands_for_and_a_bad_one_is_refused() {
    let mut sim = Sim::on_topic_messages(Lang::EnUs);
    sim.press('p');
    type_text(&mut sim, "aGVsbG8gd29ybGQ=");
    // Tab to the box (content type, then the box) and tick it.
    sim.code(KeyCode::Tab);
    sim.code(KeyCode::Tab);
    sim.press(' ');
    let send = |sim: &mut Sim| {
        sim.send(crate::tui::msg::Msg::Key(
            ratatui::crossterm::event::KeyEvent::new(
                KeyCode::Char('s'),
                ratatui::crossterm::event::KeyModifiers::CONTROL,
            ),
        ))
    };
    let effects = send(&mut sim);
    let (_, call) = first_call(&effects);
    let ApiCall::Publish(spec) = call else {
        panic!("{call:?}")
    };
    assert_eq!(spec.body, b"hello world".to_vec());
    // Text that is not base64 stays in the form with the reason.
    let mut sim = Sim::on_topic_messages(Lang::EnUs);
    sim.press('p');
    type_text(&mut sim, "this is *not* base64");
    sim.code(KeyCode::Tab);
    sim.code(KeyCode::Tab);
    sim.press(' ');
    assert!(send(&mut sim).is_empty());
    let Some(Modal::Editor(editor)) = &sim.m.modal else {
        panic!("{:?}", sim.m.modal)
    };
    assert!(editor.error.is_some());
    assert_eq!(editor.fields[editor.focus].id, "body");
}

/// TUI: TUI-MSG-07
#[test]
fn f_and_b_switch_the_way_the_open_message_is_shown_and_pressing_again_goes_back() {
    let mut sim = Sim::on_topic_messages(Lang::EnUs);
    read(&mut sim, 1);
    sim.code(KeyCode::Enter);
    assert_eq!(mon(&sim).messages.view, BodyView::Sent);
    sim.press('f');
    assert_eq!(mon(&sim).messages.view, BodyView::Formatted);
    sim.press('b');
    assert_eq!(mon(&sim).messages.view, BodyView::Base64);
    sim.press('b');
    assert_eq!(mon(&sim).messages.view, BodyView::Sent);
    assert_eq!(sim.m.modal, Some(Modal::Message));
}
