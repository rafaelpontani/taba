use super::*;
use crate::tui::i18n::Lang;
use crate::tui::model::ClusterView;
use crate::tui::testing::*;

/// TUI: TUI-CLU-05
#[test]
fn the_gate_explains_why_an_action_cannot_run() {
    let plain = Sim::plain_with_nodes(Lang::EnUs);
    for action in [
        Action::Drain,
        Action::Return,
        Action::FlushNode,
        Action::FlushAll,
        Action::StopCluster,
        Action::Monitor,
        Action::Detail,
    ] {
        assert_eq!(gate(&plain.m, action), Some(Gate::NotAdmin), "{action:?}");
    }
    let root = Sim::root_with_status(Lang::EnUs);
    assert_eq!(gate(&root.m, Action::Drain), None);
    assert_eq!(gate(&root.m, Action::Return), Some(Gate::NotDraining));
    assert_eq!(gate(&root.m, Action::FlushAll), None);
    assert_eq!(gate(&root.m, Action::StopCluster), None);

    // A token with the `admin` permission operates the cluster, but stopping it is the root token's alone.
    let mut admin = Sim::root_with_status(Lang::EnUs);
    let me = admin.m.me.data.as_mut().unwrap();
    me.root = false;
    me.permissions = vec!["admin".to_owned()];
    assert_eq!(gate(&admin.m, Action::Drain), None);
    assert_eq!(gate(&admin.m, Action::FlushAll), None);
    assert_eq!(gate(&admin.m, Action::StopCluster), Some(Gate::NotRoot));

    let drained =
        status_json(false).replacen(r#""drainState":"active""#, r#""drainState":"drained""#, 1);
    let sim = Sim::with_status_json(Lang::EnUs, &drained);
    assert_eq!(gate(&sim.m, Action::Drain), Some(Gate::NotActive));
    assert_eq!(gate(&sim.m, Action::Return), None);

    let mut busy = Sim::root_with_status(Lang::EnUs);
    busy.m.busy = Some(ApiCall::FlushCluster);
    assert_eq!(gate(&busy.m, Action::FlushAll), Some(Gate::Busy));
}

/// TUI: TUI-CLU-05
#[test]
fn every_gate_reason_has_a_text_in_both_languages() {
    for lang in [Lang::EnUs, Lang::PtBr] {
        for gate in [
            Gate::NotAdmin,
            Gate::NotRoot,
            Gate::NoNode,
            Gate::Unreachable,
            Gate::NotActive,
            Gate::NotDraining,
            Gate::Busy,
        ] {
            let text = crate::tui::i18n::tr(lang, gate.key());
            assert_ne!(text, gate.key());
        }
    }
}

/// TUI: TUI-CLU-03
#[test]
fn the_rows_come_from_the_status_for_root_and_from_the_node_list_otherwise() {
    let root = Sim::root_with_status(Lang::EnUs);
    let rows = rows(&root.m);
    assert_eq!(rows.len(), 3);
    assert!(rows.iter().all(|r| r.reachable));
    let plain = Sim::plain_with_nodes(Lang::EnUs);
    let rows = super::rows(&plain.m);
    assert_eq!(rows.len(), 2);
    assert!(rows[0].is_self && !rows[1].is_self);
    let partial = Sim::with_status_json(Lang::EnUs, &status_json(true));
    assert!(!super::rows(&partial.m)[2].reachable);
}

/// TUI: TUI-CLU-03
#[test]
fn a_selection_survives_a_reordering_and_falls_back_to_the_first() {
    let mut sim = Sim::root_with_status(Lang::EnUs);
    sim.m.cluster.node = Some(NODE_B.to_owned());
    assert_eq!(selected_row(&sim.m).unwrap().id, NODE_B);
    sim.m.cluster.node = Some("gone".to_owned());
    assert_eq!(selected_row(&sim.m).unwrap().id, NODE_A);
    let empty = Sim::new(Lang::EnUs);
    assert!(selected_row(&empty.m).is_none());
}

#[test]
fn moving_a_selection_stays_inside_the_list() {
    assert_eq!(moved(0, -1, 3), 0);
    assert_eq!(moved(1, 1, 3), 2);
    assert_eq!(moved(2, 1, 3), 2);
    assert_eq!(moved(1, i32::MAX / 2, 3), 2);
    assert_eq!(moved(1, i32::MIN / 2, 3), 0);
    assert_eq!(moved(5, 1, 0), 0);
}

/// TUI: TUI-MON-03
#[test]
fn the_windows_that_fit_the_retention() {
    assert_eq!(allowed_windows(30_000), 1);
    assert_eq!(allowed_windows(60_000), 1);
    assert_eq!(allowed_windows(300_000), 2);
    assert_eq!(allowed_windows(3_600_000), 5);
    assert_eq!(allowed_windows(6 * 3_600_000), 5);
}

/// TUI: TUI-MON-01
#[test]
fn every_chart_has_a_title_and_lines_in_both_languages() {
    for def in MON_CHARTS {
        for lang in [Lang::EnUs, Lang::PtBr] {
            assert_ne!(crate::tui::i18n::tr(lang, def.title), def.title);
            for (_, name) in def.lines {
                assert_ne!(crate::tui::i18n::tr(lang, name), *name);
            }
        }
        assert!(!def.lines.is_empty() && def.lines.len() <= 2);
    }
}

/// TUI: TUI-MON-07
#[test]
fn the_csv_has_a_header_and_one_row_per_sample_with_utc_times() {
    let mut sim = Sim::root_with_status(Lang::EnUs);
    sim.press('m');
    let effects = sim.tick(5_200);
    sim.answer(&effects[0], ok(&status_json(false)));
    let csv = export_csv(&sim.m, NODE_A, 0);
    let lines: Vec<&str> = csv.lines().collect();
    assert_eq!(lines[0], "time,used,limit");
    assert_eq!(lines.len(), 3);
    // The simulated wall clock starts at 2026-10-15 12:00:00 UTC.
    assert!(
        lines[1].starts_with("2026-10-15T12:00:00Z,"),
        "{}",
        lines[1]
    );
    assert!(
        lines[2].starts_with("2026-10-15T12:00:05Z,"),
        "{}",
        lines[2]
    );
    assert_eq!(export_csv(&sim.m, "unknown-node", 0).lines().count(), 1);
}

/// TUI: TUI-MON-03
#[test]
fn a_frozen_chart_stops_at_the_instant_it_was_frozen() {
    let mut sim = Sim::root_with_status(Lang::EnUs);
    sim.press('m');
    sim.tick(1_000);
    sim.press(' ');
    let effects = sim.tick(5_200);
    sim.answer(&effects[0], ok(&status_json(false)));
    // The sample taken after the freeze is not in the frozen window.
    let frozen = line_points(&sim.m, NODE_A, Series::MemoryUsed);
    assert_eq!(frozen.len(), 1);
    sim.press(' ');
    assert_eq!(line_points(&sim.m, NODE_A, Series::MemoryUsed).len(), 2);
}

/// TUI: TUI-CLU-01
#[test]
fn enter_on_the_conditions_pane_does_not_open_the_detail() {
    let mut sim = Sim::with_status_json(Lang::EnUs, &status_json(true));
    sim.code(ratatui::crossterm::event::KeyCode::Tab);
    sim.code(ratatui::crossterm::event::KeyCode::Enter);
    assert_eq!(sim.m.cluster.view, ClusterView::List);
    assert!(matches!(sim.m.modal, Some(Modal::Condition(_))));
}

/// TUI: TUI-NOD-01
#[test]
fn the_detail_scroll_never_goes_past_the_end_of_the_text() {
    let mut sim = Sim::root_with_status(Lang::EnUs);
    sim.code(ratatui::crossterm::event::KeyCode::Enter);
    sim.code(ratatui::crossterm::event::KeyCode::End);
    let end = sim.m.cluster.scroll;
    assert!(end < u16::MAX);
    // One step up from the end moves by one line, not from a huge number.
    sim.code(ratatui::crossterm::event::KeyCode::Up);
    assert_eq!(sim.m.cluster.scroll, end - 1);
    sim.code(ratatui::crossterm::event::KeyCode::Down);
    sim.code(ratatui::crossterm::event::KeyCode::Down);
    assert_eq!(sim.m.cluster.scroll, end);
}
