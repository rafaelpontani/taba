use ratatui::crossterm::event::KeyCode;

use super::*;
use crate::tui::api::{parse_topic_sample, ApiCall};
use crate::tui::i18n::Lang;
use crate::tui::model::{Feed, Messages, Monitor};
use crate::tui::testing::*;

fn the_reading(sim: &Sim) -> TopicMonitor {
    sim.m.topics.monitor.clone().expect("the charts are open")
}

/// TUI: TUI-MSG-11
#[test]
fn g_opens_the_charts_of_a_queue_and_reads_it_every_interval() {
    let mut sim = Sim::on_topics(Lang::EnUs);
    sim.press('j');
    sim.press('g');
    assert_eq!(the_reading(&sim).name, "orders");
    let first = sim.tick(2_000);
    let call = first
        .iter()
        .find_map(|e| match e {
            Effect::Call { call, .. } => Some(call.clone()),
            _ => None,
        })
        .expect("a call");
    assert_eq!(call, ApiCall::TopicDetail("orders".to_owned()));
    sim.answer(&first[0], ok(&topic_sample_json(0)));
    // Not again before the sampling interval (5 s).
    assert!(!sim.tick(4_000).iter().any(|e| matches!(
        e,
        Effect::Call {
            call: ApiCall::TopicDetail(_),
            ..
        }
    )));
    assert!(sim.tick(7_100).iter().any(|e| matches!(
        e,
        Effect::Call {
            call: ApiCall::TopicDetail(_),
            ..
        }
    )));
}

/// TUI: TUI-MSG-11
#[test]
fn every_reading_adds_a_point_to_each_line_and_the_largest_groups_are_followed() {
    let sim = Sim::on_topic_charts(Lang::EnUs);
    let mon = the_reading(&sim);
    for key in [
        "visible",
        "invisible",
        "published",
        "consumed",
        "acked",
        "oldest",
        "g:customer-17",
        "g:customer-4",
    ] {
        assert_eq!(mon.series.get(key).map(Ring::len), Some(5), "{key}");
    }
    assert_eq!(mon.groups, ["customer-17", "customer-4"]);
    assert_eq!(mon.series["visible"].last().map(|p| p.value), Some(1800.0));
    assert_eq!(
        mon.series["g:customer-17"].last().map(|p| p.value),
        Some(200.0)
    );
}

/// TUI: TUI-MSG-11
#[test]
fn at_most_three_groups_are_followed_and_an_empty_queue_has_an_age_of_zero() {
    let mut mon = TopicMonitor {
        name: "q".into(),
        tab: TopicTab::Charts,
        distributor: false,
        dead_letter: false,
        messages: Messages::default(),
        monitor: Monitor::default(),
        feed: Feed::default(),
        series: HashMap::new(),
        groups: Vec::new(),
    };
    let sample = parse_topic_sample(
        r#"{"topGroups":[{"groupId":"a","oldestAge":1},{"groupId":"b","oldestAge":2},{"groupId":"c","oldestAge":3},{"groupId":"d","oldestAge":4}]}"#,
    )
    .expect("a sample");
    record(&mut mon, 10, 1_000, &sample);
    assert_eq!(mon.groups, ["a", "b", "c"]);
    assert!(!mon.series.contains_key("g:d"));
    // No messages: the oldest age is zero, not a gap.
    assert_eq!(mon.series["oldest"].last().map(|p| p.value), Some(0.0));
}

/// TUI: TUI-MON-03
#[test]
fn the_window_the_freeze_and_the_scale_are_kept_by_the_charts() {
    let mut sim = Sim::on_topic_charts(Lang::EnUs);
    assert_eq!(the_reading(&sim).monitor.window, 1);
    sim.press('-');
    assert_eq!(the_reading(&sim).monitor.window, 0);
    sim.press('+');
    sim.press('+');
    assert_eq!(the_reading(&sim).monitor.window, 2);
    sim.press(' ');
    assert_eq!(the_reading(&sim).monitor.frozen, Some(22_000));
    sim.press(' ');
    assert_eq!(the_reading(&sim).monitor.frozen, None);
    sim.press('z');
    assert!(the_reading(&sim).monitor.fixed_scale);
}

/// TUI: TUI-MSG-11
#[test]
fn esc_goes_back_to_the_list_and_a_distributor_has_no_charts() {
    let mut sim = Sim::on_topic_charts(Lang::EnUs);
    sim.code(KeyCode::Esc);
    assert!(sim.m.topics.monitor.is_none());
    sim.code(KeyCode::Home);
    assert_eq!(
        super::super::selected(&sim.m).expect("selected").name,
        "fanout"
    );
    sim.press('g');
    assert!(sim.m.topics.monitor.is_none());
    assert!(sim.m.notice.is_some());
}

/// TUI: TUI-MON-07
#[test]
fn the_charts_export_one_column_per_line() {
    let mut sim = Sim::on_topic_charts(Lang::EnUs);
    sim.press('e');
    assert!(matches!(
        &sim.m.modal,
        Some(Modal::Input(i)) if i.purpose == InputPurpose::ExportTopicCsv
    ));
    let csv = export_csv(&sim.m);
    let mut lines = csv.lines();
    assert_eq!(
        lines.next(),
        Some("time,visible,invisible,in,out,deleted,queue,customer-17,customer-4")
    );
    assert_eq!(lines.count(), 5);
}

/// TUI: TUI-FRM-04
#[test]
fn the_charts_draw_in_both_languages() {
    for lang in [Lang::EnUs, Lang::PtBr] {
        let sim = Sim::on_topic_charts(lang);
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
