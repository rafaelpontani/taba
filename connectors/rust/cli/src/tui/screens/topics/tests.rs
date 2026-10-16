use super::*;
use crate::tui::i18n::Lang;
use crate::tui::model::Tab;
use crate::tui::msg::Msg;
use crate::tui::testing::*;

fn page(next: Option<&str>) -> String {
    let cursor = next.map_or(String::new(), |c| format!(r#","nextCursor":"{c}""#));
    format!(
        r#"{{"items":[{{"id":"01a-1","name":"orders","type":"queue","fifo":true,"volatile":false,"tags":{{"team":"shop"}},"createdAt":"2026-10-01T10:00:00Z","maxDeliveries":null,"maxMessageBytes":null,"ttl":3600,"effective":{{"maxDeliveries":10,"maxMessageBytes":5242880,"ttl":3600}}}},{{"id":"01a-2","name":"orders-dead-letter","type":"queue","deadLetter":true,"source":"orders","fifo":false,"volatile":false,"tags":{{}},"createdAt":"2026-10-01T10:00:00Z"}}]{cursor}}}"#
    )
}

fn second_page() -> &'static str {
    r#"{"items":[{"id":"01a-3","name":"fanout","type":"distributor","prefixes":["orders","billing"],"tags":{},"createdAt":"2026-10-12T10:00:00Z"}]}"#
}

fn stats() -> &'static str {
    r#"{"items":[{"name":"fanout","id":"01a-3"},{"name":"orders","id":"01a-1","depth":1500,"publishRate":12.5,"consumeRate":3.0,"oldestAge":125.0,"worstCondition":"backlog","conditions":[{"condition":"backlog","severity":"warning"}]},{"name":"orders-dead-letter","id":"01a-2","depth":0,"publishRate":0.0,"consumeRate":0.0,"worstCondition":"ok","conditions":[]}],"partial":false}"#
}

fn calls(effects: &[Effect]) -> Vec<(Effect, ApiCall)> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::Call { call, .. } => Some((e.clone(), call.clone())),
            _ => None,
        })
        .collect()
}

/// A root session on the Topics tab with both pages of topics and their numbers read.
fn on_topics(lang: Lang) -> Sim {
    let mut sim = Sim::root_with_status(lang);
    sim.press('2');
    assert_eq!(sim.m.tab, Tab::Topics);
    let asked = calls(&sim.tick(1_000));
    let (list, _) = asked
        .iter()
        .find(|(_, c)| *c == ApiCall::Topics(None))
        .expect("the tab asks for the topics")
        .clone();
    let (numbers, _) = asked
        .iter()
        .find(|(_, c)| *c == ApiCall::TopicStats(None))
        .expect("and for their numbers")
        .clone();
    let next = sim.answer(&list, ok(&page(Some("c2"))));
    assert_eq!(calls(&next)[0].1, ApiCall::Topics(Some("c2".to_owned())));
    sim.answer(&next[0], ok(second_page()));
    sim.answer(&numbers, ok(stats()));
    sim
}

fn type_text(sim: &mut Sim, text: &str) {
    for c in text.chars() {
        sim.press(c);
    }
}

fn set(sim: &mut Sim, id: &str, value: &str) {
    if let Some(Modal::Editor(editor)) = &mut sim.m.modal {
        let field = editor.fields.iter_mut().find(|f| f.id == id).expect(id);
        field.value = value.to_owned();
    }
}

fn the_call(effects: &[Effect]) -> ApiCall {
    calls(effects).into_iter().next().expect("a call").1
}

/// TUI: TUI-TOP-01, TUI-TOP-02
#[test]
fn the_tab_reads_every_page_of_topics_and_their_numbers() {
    let sim = on_topics(Lang::EnUs);
    let names: Vec<String> = visible(&sim.m).into_iter().map(|t| t.name).collect();
    // The dead-letter queues are hidden until asked for.
    assert_eq!(names, ["fanout", "orders"]);
    let mut sim = sim;
    sim.press('v');
    let names: Vec<String> = visible(&sim.m).into_iter().map(|t| t.name).collect();
    assert_eq!(names, ["fanout", "orders", "orders-dead-letter"]);
    let orders = numbers(&sim.m, "orders").expect("numbers");
    assert_eq!(orders.depth, Some(1500));
    assert_eq!(orders.worst, "backlog");
    assert_eq!(orders.severity, Severity::Warning);
    assert_eq!(orders.oldest_age, Some(125.0));
    // A distributor has no queue, so no depth.
    assert_eq!(numbers(&sim.m, "fanout").and_then(|n| n.depth), None);
}

/// TUI: TUI-TOP-03
#[test]
fn the_filter_and_the_sort_change_what_is_shown() {
    let mut sim = on_topics(Lang::EnUs);
    sim.press('/');
    type_text(&mut sim, "ORD");
    sim.code(ratatui::crossterm::event::KeyCode::Enter);
    let names: Vec<String> = visible(&sim.m).into_iter().map(|t| t.name).collect();
    assert_eq!(names, ["orders"]);
    sim.press('v');
    let names: Vec<String> = visible(&sim.m).into_iter().map(|t| t.name).collect();
    assert_eq!(names, ["orders", "orders-dead-letter"]);
    // name -> newest -> oldest -> depth
    sim.press('s');
    sim.press('s');
    sim.press('s');
    assert_eq!(sim.m.topics.sort, TopicSort::Depth);
    let names: Vec<String> = visible(&sim.m).into_iter().map(|t| t.name).collect();
    assert_eq!(names, ["orders", "orders-dead-letter"]);
}

/// TUI: TUI-TOP-04
#[test]
fn a_dead_letter_queue_is_read_only() {
    let mut sim = on_topics(Lang::EnUs);
    sim.press('v');
    sim.press('G');
    assert_eq!(
        selected(&sim.m).expect("selected").name,
        "orders-dead-letter"
    );
    for key in ['e', 'd'] {
        sim.press(key);
        assert!(sim.m.modal.is_none());
    }
    assert_eq!(
        gate(&sim.m, Action::TopicEdit),
        Some("gate.topic-read-only")
    );
    assert_eq!(
        gate(&sim.m, Action::TopicDelete),
        Some("gate.topic-read-only")
    );
}

/// TUI: TUI-TPF-01
#[test]
fn creating_a_queue_sends_only_what_was_filled_in() {
    let mut sim = on_topics(Lang::EnUs);
    sim.press('n');
    assert!(matches!(&sim.m.modal, Some(Modal::Editor(e)) if e.purpose == Purpose::TopicCreate));
    type_text(&mut sim, "invoices");
    // distributor, fifo, volatile, idle, max-bytes, ttl
    for _ in 0..5 {
        sim.code(ratatui::crossterm::event::KeyCode::Tab);
    }
    type_text(&mut sim, "2048");
    sim.code(ratatui::crossterm::event::KeyCode::Tab);
    type_text(&mut sim, "60");
    sim.code(ratatui::crossterm::event::KeyCode::Tab);
    sim.code(ratatui::crossterm::event::KeyCode::Tab);
    type_text(&mut sim, "team=shop env=prod");
    let effects = sim.send(Msg::Key(ratatui::crossterm::event::KeyEvent::new(
        ratatui::crossterm::event::KeyCode::Char('s'),
        ratatui::crossterm::event::KeyModifiers::CONTROL,
    )));
    let ApiCall::TopicCreate(body) = the_call(&effects) else {
        panic!("expected a create");
    };
    assert_eq!(
        body,
        r#"{"name":"invoices","type":"queue","tags":{"team":"shop","env":"prod"},"maxMessageBytes":2048,"ttl":60}"#
    );
}

/// TUI: TUI-TPF-02
#[test]
fn the_form_points_at_what_is_wrong_without_calling_the_server() {
    let mut sim = on_topics(Lang::EnUs);
    let submit = |sim: &mut Sim| {
        sim.send(Msg::Key(ratatui::crossterm::event::KeyEvent::new(
            ratatui::crossterm::event::KeyCode::Char('s'),
            ratatui::crossterm::event::KeyModifiers::CONTROL,
        )))
    };
    sim.press('n');
    assert!(calls(&submit(&mut sim)).is_empty());
    type_text(&mut sim, "bad name!");
    assert!(calls(&submit(&mut sim)).is_empty());
    let Some(Modal::Editor(editor)) = &sim.m.modal else {
        panic!("the form stays open");
    };
    assert_eq!(
        editor.error.as_deref(),
        Some("A name has only letters, digits, _ and -.")
    );
    assert_eq!(editor.fields[editor.focus].id, "name");
    // A distributor without a prefix.
    if let Some(Modal::Editor(editor)) = &mut sim.m.modal {
        editor.fields[0].value = "fan".to_owned();
        editor.fields[1].checked = true;
    }
    assert!(calls(&submit(&mut sim)).is_empty());
    let Some(Modal::Editor(editor)) = &sim.m.modal else {
        panic!("the form stays open");
    };
    assert_eq!(editor.fields[editor.focus].id, "prefixes");
}

/// TUI: TUI-TPF-02
#[test]
fn the_reason_of_the_server_stays_in_the_form() {
    let mut sim = on_topics(Lang::EnUs);
    sim.press('n');
    type_text(&mut sim, "orders");
    let effects = sim.send(Msg::Key(ratatui::crossterm::event::KeyEvent::new(
        ratatui::crossterm::event::KeyCode::Char('s'),
        ratatui::crossterm::event::KeyModifiers::CONTROL,
    )));
    let (effect, _) = calls(&effects).remove(0);
    sim.answer(&effect, fail(409, "topic.already-exists", "{}"));
    let Some(Modal::Editor(editor)) = &sim.m.modal else {
        panic!("the form stays open");
    };
    assert!(!editor.busy);
    assert_eq!(
        editor.error.as_deref(),
        Some("topic.already-exists: topic.already-exists happened")
    );
}

/// TUI: TUI-TPF-03
#[test]
fn editing_locks_what_never_changes_and_sends_only_the_changes() {
    let mut sim = on_topics(Lang::EnUs);
    sim.press('j');
    assert_eq!(selected(&sim.m).expect("selected").name, "orders");
    sim.press('e');
    let Some(Modal::Editor(editor)) = &sim.m.modal else {
        panic!("the form opens");
    };
    for id in [
        "name",
        "distributor",
        "fifo",
        "volatile",
        "prefixes",
        "idle",
    ] {
        assert!(
            editor.field(id).expect(id).locked.is_some(),
            "{id} is locked"
        );
    }
    assert_eq!(editor.field("ttl").expect("ttl").value, "3600");
    // Nothing changed: nothing is sent.
    let submit = |sim: &mut Sim| {
        sim.send(Msg::Key(ratatui::crossterm::event::KeyEvent::new(
            ratatui::crossterm::event::KeyCode::Char('s'),
            ratatui::crossterm::event::KeyModifiers::CONTROL,
        )))
    };
    assert!(calls(&submit(&mut sim)).is_empty());
    // The ttl goes back to the node's value, and the maximum of deliveries is set.
    set(&mut sim, "ttl", "default");
    set(&mut sim, "max-deliveries", "5");
    let effects = submit(&mut sim);
    assert_eq!(
        the_call(&effects),
        ApiCall::TopicUpdate(
            "orders".to_owned(),
            r#"{"ttl":null,"maxDeliveries":5}"#.to_owned()
        )
    );
}

/// TUI: TUI-TPF-04
#[test]
fn deleting_asks_for_the_name_typed() {
    let mut sim = on_topics(Lang::EnUs);
    sim.press('j');
    sim.press('d');
    let Some(Modal::Confirm(confirm)) = &sim.m.modal else {
        panic!("a confirmation opens");
    };
    assert_eq!(confirm.pending, Pending::DeleteTopic("orders".to_owned()));
    // Enter without the name does nothing.
    assert!(sim
        .code(ratatui::crossterm::event::KeyCode::Enter)
        .is_empty());
    assert!(sim.m.modal.is_some());
    type_text(&mut sim, "orders");
    let effects = sim.code(ratatui::crossterm::event::KeyCode::Enter);
    let (effect, call) = calls(&effects).remove(0);
    assert_eq!(call, ApiCall::TopicDelete("orders".to_owned()));
    sim.answer(&effect, ok(""));
    // It leaves the table at once, with its dead-letter queue.
    let names: Vec<String> = visible(&sim.m).into_iter().map(|t| t.name).collect();
    assert_eq!(names, ["fanout"]);
}

/// TUI: TUI-FRM-10
#[test]
fn a_token_without_the_permission_is_told_why() {
    let mut sim = Sim::new(Lang::EnUs);
    let effects = sim.tick(0);
    sim.answer(&effects[0], ok(me_plain()));
    sim.press('2');
    assert_eq!(gate(&sim.m, Action::TopicNew), Some("gate.need-create"));
    sim.press('n');
    assert!(sim.m.modal.is_none());
    assert!(sim.m.notice.is_some());
}

/// TUI: TUI-FRM-04
#[test]
fn the_screen_draws_in_both_languages() {
    for lang in [Lang::EnUs, Lang::PtBr] {
        let sim = on_topics(lang);
        for (width, height) in [(80, 24), (120, 40)] {
            let backend = ratatui::backend::TestBackend::new(width, height);
            let mut terminal = ratatui::Terminal::new(backend).expect("terminal");
            terminal
                .draw(|f| crate::tui::view::view(f, &sim.m))
                .expect("draw");
            let text: String = terminal
                .backend()
                .buffer()
                .content()
                .iter()
                .map(|c| c.symbol())
                .collect();
            assert!(text.contains("orders"), "{lang:?} {width}x{height}");
        }
    }
}
