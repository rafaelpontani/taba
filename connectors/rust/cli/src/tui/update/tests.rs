use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use super::*;
use crate::tui::api::Severity;
use crate::tui::i18n::Lang;
use crate::tui::model::{ClusterView, Pane, Pending};
use crate::tui::testing::*;

fn calls(effects: &[Effect]) -> Vec<ApiCall> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::Call { call, .. } => Some(call.clone()),
            _ => None,
        })
        .collect()
}

fn call_effect(effects: &[Effect]) -> Effect {
    effects
        .iter()
        .find(|e| matches!(e, Effect::Call { .. }))
        .cloned()
        .unwrap_or_else(|| panic!("no call in {effects:?}"))
}

// ---------- what is read, and when ----------

/// TUI: TUI-FRM-01
#[test]
fn the_first_tick_asks_who_the_token_is_and_nothing_else() {
    let mut sim = Sim::new(Lang::EnUs);
    assert_eq!(calls(&sim.tick(0)), vec![ApiCall::Me]);
    // The question is still in flight: it is not asked again.
    assert!(calls(&sim.tick(250)).is_empty());
}

/// TUI: TUI-CLU-01
#[test]
fn a_root_token_reads_the_cluster_status_and_a_plain_one_the_node_list() {
    let mut sim = Sim::new(Lang::EnUs);
    let effects = sim.tick(0);
    sim.answer(&effects[0], ok(me_root()));
    // The node list comes too: it remembers the nodes that left by drain.
    assert_eq!(
        calls(&sim.tick(100)),
        vec![ApiCall::ClusterStatus, ApiCall::Nodes]
    );

    let mut plain = Sim::new(Lang::EnUs);
    let effects = plain.tick(0);
    plain.answer(&effects[0], ok(me_plain()));
    assert_eq!(calls(&plain.tick(100)), vec![ApiCall::Nodes]);
}

/// TUI: TUI-CLU-06
#[test]
fn the_status_is_read_every_two_seconds_on_the_cluster_tab() {
    let mut sim = Sim::root_with_status(Lang::EnUs);
    assert!(calls(&sim.tick(1_000)).is_empty());
    assert_eq!(calls(&sim.tick(2_100)), vec![ApiCall::ClusterStatus]);
}

/// TUI: TUI-CLU-06
#[test]
fn pause_and_an_open_popup_stop_the_reading_and_r_asks_for_it_now() {
    let mut sim = Sim::root_with_status(Lang::EnUs);
    sim.press('p');
    assert!(sim.m.paused);
    assert!(calls(&sim.tick(5_000)).is_empty());
    sim.press('p');
    assert_eq!(calls(&sim.tick(5_250)).len(), 1);

    let mut sim = Sim::root_with_status(Lang::EnUs);
    sim.press('?');
    assert!(calls(&sim.tick(5_000)).is_empty());
    sim.code(KeyCode::Esc);
    assert_eq!(calls(&sim.tick(5_250)).len(), 1);

    let mut sim = Sim::root_with_status(Lang::EnUs);
    sim.press('r');
    assert!(calls(&sim.tick(150)).contains(&ApiCall::ClusterStatus));
}

/// TUI: TUI-FRM-08
#[test]
fn a_failed_read_keeps_the_last_data_marks_it_old_and_backs_off() {
    let mut sim = Sim::root_with_status(Lang::EnUs);
    let effects = sim.tick(2_100);
    sim.answer(&effects[0], offline());
    assert!(sim.m.status.is_stale());
    assert!(sim.m.status.data.is_some());
    assert!(sim.m.is_offline());
    assert_eq!(sim.m.health().1, "offline");
    // 2 s base, doubled by the first failure: 4 s.
    assert_eq!(sim.m.status.retry_in(2_100), Some(4));
    assert!(calls(&sim.tick(5_000)).is_empty());
    let effects = sim.tick(6_200);
    assert_eq!(calls(&effects), vec![ApiCall::ClusterStatus]);
    sim.answer(&effects[0], offline());
    assert_eq!(sim.m.status.retry_in(6_200), Some(8));
    // And it recovers: the stale mark goes away.
    let effects = sim.tick(15_000);
    sim.answer(&effects[0], ok(&status_json(false)));
    assert!(!sim.m.status.is_stale());
    assert_eq!(sim.m.status.failures, 0);
}

/// TUI: TUI-FRM-08
#[test]
fn the_backoff_never_goes_above_thirty_seconds_and_retry_after_is_obeyed() {
    let mut sim = Sim::root_with_status(Lang::EnUs);
    let mut now = 2_100;
    for _ in 0..8 {
        let effects = sim.tick(now);
        sim.answer(&effects[0], offline());
        now = sim.m.status.next_at + 1;
    }
    assert!(sim.m.status.next_at - (now - 1) <= 30_000);

    let effects = sim.tick(now);
    let mut busy = fail(429, "rate.limited", "{}");
    if let Err(e) = &mut busy {
        e.retry_after = Some(17);
    }
    sim.answer(&effects[0], busy);
    assert_eq!(sim.m.status.next_at, now + 17_000);
}

/// TUI: TUI-FRM-08
#[test]
fn an_answer_that_nobody_waits_for_is_dropped() {
    let mut sim = Sim::root_with_status(Lang::EnUs);
    let effects = sim.tick(2_100);
    let Effect::Call {
        call, generation, ..
    } = &effects[0]
    else {
        panic!()
    };
    // A wrong request id.
    sim.send(Msg::Reply {
        id: 9_999,
        generation: *generation,
        call: call.clone(),
        result: offline(),
    });
    assert!(!sim.m.status.is_stale());
    // An older generation.
    sim.send(Msg::Reply {
        id: 1,
        generation: generation + 1,
        call: call.clone(),
        result: offline(),
    });
    assert!(!sim.m.status.is_stale());
    // The real one still lands.
    sim.answer(&effects[0], ok(&status_json(false)));
    assert_eq!(sim.m.status.data.as_ref().map(|s| s.nodes.len()), Some(3));
}

/// TUI: TUI-FRM-10
#[test]
fn a_forbidden_status_falls_back_to_the_node_list_and_says_what_is_missing() {
    let mut sim = Sim::new(Lang::EnUs);
    let effects = sim.tick(0);
    // `me` claims root but the status says otherwise (a stale grant): the screen must still cope.
    sim.answer(&effects[0], ok(me_root()));
    let effects = sim.tick(100);
    sim.answer(
        &effects[0],
        fail(403, "auth.forbidden", r#"{"required":"root"}"#),
    );
    assert!(sim.m.status_denied.is_some());
    assert!(!sim.m.uses_status());
    assert!(sim.m.cluster.nodes.in_flight.is_some());
    let denied = sim.m.status_denied.clone().unwrap();
    assert_eq!(denied.required.as_deref(), Some("root"));
}

/// TUI: TUI-FRM-10
#[test]
fn a_missing_permission_is_named() {
    let sim = Sim::new(Lang::EnUs);
    let mut fail = crate::tui::model::Fail {
        status: 403,
        code: "auth.forbidden".into(),
        message: String::new(),
        request_id: None,
        retry_after: None,
        required: Some("admin".into()),
    };
    assert!(crate::tui::view::widgets::denied_text(&sim.m, &fail).contains("admin"));
    fail.required = Some("root".into());
    assert!(crate::tui::view::widgets::denied_text(&sim.m, &fail).contains("root token"));
    fail.required = None;
    fail.status = 401;
    assert!(crate::tui::view::widgets::denied_text(&sim.m, &fail).contains("401"));
}

/// TUI: TUI-MON-05
#[test]
fn every_status_read_feeds_the_history_by_its_interval() {
    let mut sim = Sim::root_with_status(Lang::EnUs);
    let ring_len = |sim: &Sim| {
        sim.m
            .history
            .series(NODE_A, crate::tui::history::Series::MemoryUsed)
            .map_or(0, |r| r.len())
    };
    assert_eq!(ring_len(&sim), 1);
    let effects = sim.tick(2_100);
    sim.answer(&effects[0], ok(&status_json(false)));
    // 2 s after the first sample: not yet 5 s.
    assert_eq!(ring_len(&sim), 1);
    let effects = sim.tick(4_300);
    sim.answer(&effects[0], ok(&status_json(false)));
    let effects = sim.tick(6_500);
    sim.answer(&effects[0], ok(&status_json(false)));
    assert_eq!(ring_len(&sim), 2);
}

/// TUI: TUI-FRM-08
#[test]
fn a_partial_read_is_kept_and_its_sample_is_marked() {
    let sim = Sim::with_status_json(Lang::EnUs, &status_json(true));
    let status = sim.m.status.data.as_ref().unwrap();
    assert!(status.partial);
    assert_eq!(status.missing[0].node_id, NODE_C);
    let ring = sim
        .m
        .history
        .series(NODE_A, crate::tui::history::Series::MemoryUsed)
        .unwrap();
    assert!(ring.last().unwrap().partial);
    assert_eq!(
        sim.m.health(),
        (Severity::Warning, "member-unreachable".to_owned())
    );
}

// ---------- keys ----------

/// TUI: TUI-FRM-02
#[test]
fn the_tabs_navigate_by_number_and_brackets() {
    let mut sim = Sim::root_with_status(Lang::EnUs);
    sim.press('3');
    assert_eq!(sim.m.tab, Tab::Tokens);
    sim.press(']');
    assert_eq!(sim.m.tab, Tab::Profiles);
    sim.press(']');
    assert_eq!(sim.m.tab, Tab::Cluster);
    sim.press('[');
    assert_eq!(sim.m.tab, Tab::Profiles);
    sim.send(Msg::Key(KeyEvent::new(KeyCode::Right, KeyModifiers::ALT)));
    assert_eq!(sim.m.tab, Tab::Cluster);
}

/// TUI: TUI-FRM-02
#[test]
fn the_empty_tabs_still_keep_the_status_reading_alive() {
    let mut sim = Sim::root_with_status(Lang::EnUs);
    sim.press('4');
    // The read already planned still happens; after it, away from the Cluster tab, the status is read only as
    // often as the history samples (5 s).
    let effects = sim.tick(2_100);
    assert_eq!(calls(&effects), vec![ApiCall::ClusterStatus]);
    sim.answer(&effects[0], ok(&status_json(false)));
    assert!(calls(&sim.tick(4_200)).is_empty());
    assert_eq!(calls(&sim.tick(7_200)), vec![ApiCall::ClusterStatus]);
}

#[test]
fn quitting_and_a_key_release_are_handled() {
    let mut sim = Sim::root_with_status(Lang::EnUs);
    let mut release = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE);
    release.kind = KeyEventKind::Release;
    assert!(sim.send(Msg::Key(release)).is_empty());
    assert_eq!(sim.press('q'), vec![Effect::Quit]);
    let ctrl_c = Msg::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
    assert_eq!(sim.send(ctrl_c), vec![Effect::Quit]);
    // Also with a popup open.
    sim.press('?');
    let ctrl_c = Msg::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
    assert_eq!(sim.send(ctrl_c), vec![Effect::Quit]);
}

/// TUI: TUI-CLU-03
#[test]
fn the_selection_moves_by_node_and_stays_inside_the_table() {
    let mut sim = Sim::root_with_status(Lang::EnUs);
    let selected = |sim: &Sim| cluster::selected_row(&sim.m).unwrap().id;
    assert_eq!(selected(&sim), NODE_A);
    sim.press('j');
    assert_eq!(selected(&sim), NODE_B);
    sim.code(KeyCode::End);
    assert_eq!(selected(&sim), NODE_C);
    sim.code(KeyCode::Down);
    assert_eq!(selected(&sim), NODE_C);
    sim.code(KeyCode::Home);
    assert_eq!(selected(&sim), NODE_A);
    sim.code(KeyCode::Up);
    assert_eq!(selected(&sim), NODE_A);
    sim.send(Msg::Scroll(3));
    assert_eq!(selected(&sim), NODE_C);
}

/// TUI: TUI-CLU-01
#[test]
fn enter_on_a_condition_explains_it() {
    let mut sim = Sim::with_status_json(Lang::EnUs, &status_json(true));
    sim.code(KeyCode::Tab);
    assert_eq!(sim.m.cluster.pane, Pane::Conditions);
    sim.code(KeyCode::Enter);
    assert_eq!(
        sim.m.modal,
        Some(Modal::Condition("member-unreachable".to_owned()))
    );
    sim.code(KeyCode::Esc);
    assert!(sim.m.modal.is_none());
}

// ---------- actions ----------

fn open_confirm(sim: &Sim) -> Confirm {
    match &sim.m.modal {
        Some(Modal::Confirm(c)) => c.clone(),
        other => panic!("expected a confirmation, got {other:?}"),
    }
}

/// TUI: TUI-FRM-07
#[test]
fn a_destructive_confirmation_defaults_to_cancel() {
    let mut sim = Sim::root_with_status(Lang::EnUs);
    sim.press('d');
    let confirm = open_confirm(&sim);
    assert_eq!(confirm.pending, Pending::Drain(NODE_A.to_owned()));
    assert!(!confirm.ok_focused);
    // Enter on the default does nothing but close.
    let effects = sim.code(KeyCode::Enter);
    assert!(effects.is_empty());
    assert!(sim.m.modal.is_none());
    assert!(sim.m.busy.is_none());
}

/// TUI: TUI-CLU-05
#[test]
fn drain_runs_only_after_the_person_moves_to_confirm() {
    let mut sim = Sim::root_with_status(Lang::EnUs);
    sim.press('d');
    sim.code(KeyCode::Right);
    assert!(open_confirm(&sim).ok_focused);
    let effects = sim.code(KeyCode::Enter);
    assert_eq!(
        calls(&effects),
        vec![ApiCall::DrainStart(NODE_A.to_owned())]
    );
    assert!(sim.m.modal.is_none());
    assert!(sim.m.busy.is_some());
    // Another action meanwhile is refused with the reason.
    sim.press('b');
    assert!(sim.m.modal.is_none());
    assert_eq!(
        sim.m.notice.as_ref().unwrap().text,
        "Another action is still running."
    );
}

/// TUI: TUI-FRM-07
#[test]
fn esc_cancels_a_confirmation() {
    let mut sim = Sim::root_with_status(Lang::EnUs);
    sim.press('b');
    sim.code(KeyCode::Right);
    assert!(sim.code(KeyCode::Esc).is_empty());
    assert!(sim.m.modal.is_none() && sim.m.busy.is_none());
}

/// TUI: TUI-CLU-05
#[test]
fn an_action_that_cannot_run_says_why_and_opens_nothing() {
    let mut sim = Sim::plain_with_nodes(Lang::EnUs);
    sim.press('d');
    assert!(sim.m.modal.is_none());
    assert_eq!(
        sim.m.notice.as_ref().unwrap().text,
        "This needs the admin permission or the root token."
    );
    assert_eq!(sim.m.notice.as_ref().unwrap().severity, Severity::Warning);

    let mut sim = Sim::root_with_status(Lang::EnUs);
    sim.press('u');
    assert_eq!(
        sim.m.notice.as_ref().unwrap().text,
        "The node is not draining: there is nothing to return."
    );
    // The third node of a partial read did not answer.
    let mut sim = Sim::with_status_json(Lang::PtBr, &status_json(true));
    sim.code(KeyCode::End);
    sim.press('d');
    assert!(sim.m.modal.is_none());
    assert_eq!(
        sim.m.notice.as_ref().unwrap().text,
        "O nó selecionado não respondeu."
    );
}

/// TUI: TUI-CLU-07
#[test]
fn a_plain_token_sees_the_node_list_and_no_detail() {
    let mut sim = Sim::plain_with_nodes(Lang::EnUs);
    assert_eq!(cluster::rows(&sim.m).len(), 2);
    sim.press('i');
    assert_eq!(sim.m.cluster.view, ClusterView::List);
    assert_eq!(
        sim.m.notice.as_ref().unwrap().text,
        "This needs the admin permission or the root token."
    );
}

/// TUI: TUI-NOD-05
#[test]
fn stopping_the_cluster_needs_its_name_typed() {
    let mut sim = Sim::root_with_status(Lang::EnUs);
    sim.press('S');
    let confirm = open_confirm(&sim);
    let typed = confirm.typed.unwrap();
    assert_eq!(typed.expected, "prod-eu");
    // Enter with nothing typed does not run it, and does not close the popup.
    assert!(sim.code(KeyCode::Enter).is_empty());
    assert!(sim.m.modal.is_some());
    for c in "prod-e".chars() {
        sim.press(c);
    }
    assert!(sim.code(KeyCode::Enter).is_empty());
    assert!(sim.m.modal.is_some());
    sim.press('u');
    let effects = sim.code(KeyCode::Enter);
    assert_eq!(calls(&effects), vec![ApiCall::StopCluster]);
    assert!(sim.m.modal.is_none());
}

/// TUI: TUI-NOD-05
#[test]
fn backspace_edits_the_typed_name_and_h_l_are_text_there() {
    let mut sim = Sim::root_with_status(Lang::EnUs);
    sim.press('S');
    sim.press('x');
    sim.press('h');
    sim.code(KeyCode::Backspace);
    sim.code(KeyCode::Backspace);
    let typed = open_confirm(&sim).typed.unwrap();
    assert_eq!(typed.input, "");
}

/// TUI: TUI-NOD-03
#[test]
fn a_drain_is_watched_until_the_node_is_drained() {
    let mut sim = Sim::root_with_status(Lang::EnUs);
    sim.press('d');
    sim.code(KeyCode::Right);
    let effects = sim.code(KeyCode::Enter);
    sim.answer(
        &call_effect(&effects),
        ok(r#"{"drainState":"draining","pendingMessages":40}"#),
    );
    assert!(sim.m.busy.is_none());
    assert_eq!(sim.m.cluster.watch.as_ref().unwrap().goal, Goal::Drained);
    assert_eq!(
        sim.m.notice.as_ref().unwrap().text,
        "Drain started on 01a1…000001"
    );

    let polled = sim.tick(300);
    assert!(calls(&polled).contains(&ApiCall::DrainStatus(NODE_A.to_owned())));
    let status_call = polled
        .iter()
        .find(|e| {
            matches!(
                e,
                Effect::Call {
                    call: ApiCall::DrainStatus(_),
                    ..
                }
            )
        })
        .unwrap()
        .clone();
    sim.answer(
        &status_call,
        ok(r#"{"drainState":"draining","pendingMessages":12}"#),
    );
    assert!(sim.m.cluster.watch.is_some());
    assert_eq!(sim.m.cluster.drain.data.as_ref().unwrap().pending, 12);

    let polled = sim.tick(2_400);
    let status_call = polled
        .iter()
        .find(|e| {
            matches!(
                e,
                Effect::Call {
                    call: ApiCall::DrainStatus(_),
                    ..
                }
            )
        })
        .unwrap()
        .clone();
    sim.answer(
        &status_call,
        ok(r#"{"drainState":"drained","pendingMessages":0}"#),
    );
    assert!(sim.m.cluster.watch.is_none());
    assert_eq!(
        sim.m.notice.as_ref().unwrap().text,
        "Node 01a1…000001 is drained"
    );
}

/// TUI: TUI-NOD-03
#[test]
fn returning_a_drained_node_is_watched_until_it_is_active() {
    let drained =
        status_json(false).replacen(r#""drainState":"active""#, r#""drainState":"drained""#, 1);
    let mut sim = Sim::with_status_json(Lang::EnUs, &drained);
    sim.press('u');
    sim.code(KeyCode::Right);
    let effects = sim.code(KeyCode::Enter);
    assert_eq!(
        calls(&effects),
        vec![ApiCall::DrainCancel(NODE_A.to_owned())]
    );
    sim.answer(
        &effects[0],
        ok(r#"{"drainState":"drained","pendingMessages":0}"#),
    );
    assert_eq!(sim.m.cluster.watch.as_ref().unwrap().goal, Goal::Active);
    let polled = sim.tick(300);
    let status_call = polled
        .iter()
        .find(|e| {
            matches!(
                e,
                Effect::Call {
                    call: ApiCall::DrainStatus(_),
                    ..
                }
            )
        })
        .unwrap()
        .clone();
    sim.answer(
        &status_call,
        ok(r#"{"drainState":"active","pendingMessages":0}"#),
    );
    assert!(sim.m.cluster.watch.is_none());
    assert!(sim.m.notice.as_ref().unwrap().text.contains("active again"));
}

/// TUI: TUI-NOD-04
#[test]
fn a_flush_shows_a_report_per_node() {
    let mut sim = Sim::root_with_status(Lang::EnUs);
    sim.press('B');
    sim.code(KeyCode::Right);
    let effects = sim.code(KeyCode::Enter);
    assert_eq!(calls(&effects), vec![ApiCall::FlushCluster]);
    sim.answer(
        &effects[0],
        ok(&format!(
            r#"{{"complete":false,"nodes":[{{"nodeId":"{NODE_A}","complete":true}},{{"nodeId":"{NODE_B}","complete":false,"pending":true,"error":"timeout"}}]}}"#
        )),
    );
    let Some(Modal::Report(Report::Flush(report))) = &sim.m.modal else {
        panic!("{:?}", sim.m.modal)
    };
    assert!(!report.complete);
    assert_eq!(report.nodes.len(), 2);
}

/// TUI: TUI-NOD-05
#[test]
fn a_stop_shows_its_report() {
    let mut sim = Sim::root_with_status(Lang::EnUs);
    sim.press('S');
    for c in "prod-eu".chars() {
        sim.press(c);
    }
    let effects = sim.code(KeyCode::Enter);
    sim.answer(
        &effects[0],
        ok(&format!(
            r#"{{"stopped":["{NODE_B}","{NODE_A}"],"stopList":false,"warnings":["S3 is down"],"nodes":[{{"nodeId":"{NODE_B}","drained":true}}]}}"#
        )),
    );
    let Some(Modal::Report(Report::Stop(report))) = &sim.m.modal else {
        panic!("{:?}", sim.m.modal)
    };
    assert_eq!(report.stopped.len(), 2);
    assert_eq!(report.warnings, vec!["S3 is down"]);
}

/// TUI: TUI-NOD-05
#[test]
fn a_failed_action_shows_why_and_the_unreachable_node_stays_named() {
    let mut sim = Sim::root_with_status(Lang::EnUs);
    sim.press('b');
    sim.code(KeyCode::Right);
    let effects = sim.code(KeyCode::Enter);
    sim.answer(
        &effects[0],
        fail(
            502,
            "forward.bad-gateway",
            r#"{"message":"node 01a13640 did not answer"}"#,
        ),
    );
    assert!(sim.m.busy.is_none());
    let Some(Modal::Report(Report::Failure { fail, call })) = &sim.m.modal else {
        panic!("{:?}", sim.m.modal)
    };
    assert_eq!(fail.status, 502);
    assert_eq!(call, &ApiCall::FlushNode(NODE_A.to_owned()));
}

// ---------- popups ----------

/// TUI: TUI-HLP-02
#[test]
fn the_palette_finds_an_action_in_either_language_and_runs_it() {
    let mut sim = Sim::root_with_status(Lang::PtBr);
    sim.press(':');
    for c in "drain".chars() {
        sim.press(c);
    }
    let found = palette_matches(Lang::PtBr, "drain");
    assert!(found.iter().any(|(_, a)| *a == Action::Drain));
    // The English name also works in Portuguese.
    assert!(palette_matches(Lang::PtBr, "flush")
        .iter()
        .any(|(_, a)| *a == Action::FlushNode));
    assert!(palette_matches(Lang::PtBr, "devolver")
        .iter()
        .any(|(_, a)| *a == Action::Return));
    assert!(palette_matches(Lang::PtBr, "return")
        .iter()
        .any(|(_, a)| *a == Action::Return));
    // Enter runs the first match: here it opens the confirmation to drain.
    sim.code(KeyCode::Enter);
    assert!(
        matches!(sim.m.modal, Some(Modal::Confirm(_))),
        "{:?}",
        sim.m.modal
    );
}

/// TUI: TUI-HLP-02
#[test]
fn the_palette_goes_to_a_tab_and_switches_tab_for_a_screen_action() {
    let mut sim = Sim::root_with_status(Lang::EnUs);
    sim.press('3');
    sim.press(':');
    for c in "charts".chars() {
        sim.press(c);
    }
    sim.code(KeyCode::Enter);
    assert_eq!(sim.m.tab, Tab::Cluster);
    assert_eq!(sim.m.cluster.view, ClusterView::Monitor);

    let mut sim = Sim::root_with_status(Lang::EnUs);
    sim.press(':');
    for c in "go to tokens".chars() {
        sim.press(c);
    }
    sim.code(KeyCode::Enter);
    assert_eq!(sim.m.tab, Tab::Tokens);
}

#[test]
fn fuzzy_matching_is_a_case_insensitive_subsequence() {
    assert!(fuzzy("drn", "Drain the node"));
    assert!(fuzzy("DRAIN", "drain the node"));
    assert!(fuzzy("", "anything"));
    assert!(!fuzzy("xyz", "Drain the node"));
    assert!(!fuzzy("nrd", "drain"));
}

/// TUI: TUI-HLP-03
#[test]
fn the_language_changes_at_once_and_is_saved() {
    let mut sim = Sim::root_with_status(Lang::EnUs);
    sim.press('l');
    assert_eq!(sim.m.modal, Some(Modal::Language { selected: 0 }));
    sim.code(KeyCode::Down);
    let effects = sim.code(KeyCode::Enter);
    assert_eq!(sim.m.lang, Lang::PtBr);
    assert_eq!(effects, vec![Effect::SaveLanguage(Lang::PtBr)]);
    assert!(sim.m.modal.is_none());
    sim.press('l');
    sim.code(KeyCode::Esc);
    assert_eq!(sim.m.lang, Lang::PtBr);
}

/// TUI: TUI-HLP-03
#[test]
fn a_language_that_could_not_be_saved_is_reported_but_kept() {
    let mut sim = Sim::root_with_status(Lang::EnUs);
    sim.send(Msg::LanguageSaved(Err("read-only".to_owned())));
    assert!(sim.m.notice.as_ref().unwrap().text.contains("read-only"));
    sim.send(Msg::LanguageSaved(Ok(())));
}

/// TUI: TUI-HLP-01
#[test]
fn help_opens_scrolls_and_closes() {
    let mut sim = Sim::root_with_status(Lang::EnUs);
    sim.press('?');
    assert_eq!(sim.m.modal, Some(Modal::Help { scroll: 0 }));
    sim.code(KeyCode::Down);
    sim.send(Msg::Scroll(3));
    assert_eq!(sim.m.modal, Some(Modal::Help { scroll: 4 }));
    sim.code(KeyCode::F(1));
    assert!(sim.m.modal.is_none());
}

/// TUI: TUI-FRM-07
#[test]
fn only_one_popup_is_open_at_a_time() {
    let mut sim = Sim::root_with_status(Lang::EnUs);
    sim.press('?');
    // `d` is text for the popup, not an action: no second popup opens.
    sim.press('d');
    assert!(matches!(sim.m.modal, Some(Modal::Help { .. })));
    sim.code(KeyCode::Esc);
    sim.press('d');
    assert!(matches!(sim.m.modal, Some(Modal::Confirm(_))));
}

// ---------- the node: detail and charts ----------

/// TUI: TUI-NOD-01
#[test]
fn enter_opens_the_detail_and_reads_its_metrics_and_backup() {
    let mut sim = Sim::root_with_status(Lang::EnUs);
    sim.press('i');
    assert_eq!(sim.m.cluster.view, ClusterView::Detail);
    let asked = calls(&sim.tick(300));
    assert!(
        asked.contains(&ApiCall::NodeMetrics(NODE_A.to_owned())),
        "{asked:?}"
    );
    assert!(
        asked.contains(&ApiCall::NodeBackup(NODE_A.to_owned())),
        "{asked:?}"
    );
    sim.code(KeyCode::Esc);
    assert_eq!(sim.m.cluster.view, ClusterView::List);
}

/// TUI: TUI-NOD-02
#[test]
fn a_node_without_s3_backup_is_not_an_error() {
    let mut sim = Sim::root_with_status(Lang::EnUs);
    sim.press('i');
    let effects = sim.tick(300);
    let backup = effects
        .iter()
        .find(|e| {
            matches!(
                e,
                Effect::Call {
                    call: ApiCall::NodeBackup(_),
                    ..
                }
            )
        })
        .unwrap()
        .clone();
    sim.answer(&backup, fail(422, "backup.not-configured", "{}"));
    assert_eq!(sim.m.cluster.backup.data, Some(None));
    assert!(sim.m.cluster.backup.error.is_none());
}

/// TUI: TUI-CLU-04
#[test]
fn the_node_metrics_are_kept_as_read() {
    let mut sim = Sim::root_with_status(Lang::EnUs);
    sim.press('i');
    let effects = sim.tick(300);
    let metrics = effects
        .iter()
        .find(|e| {
            matches!(
                e,
                Effect::Call {
                    call: ApiCall::NodeMetrics(_),
                    ..
                }
            )
        })
        .unwrap()
        .clone();
    sim.answer(
        &metrics,
        ok("# TYPE taba_storage_memory_messages gauge\ntaba_storage_memory_messages 12\n"),
    );
    let data = sim.m.cluster.metrics.data.as_ref().unwrap();
    assert_eq!(data.sum("taba_storage_memory_messages"), Some(12.0));
}

/// TUI: TUI-MON-03
#[test]
fn the_chart_window_never_goes_above_the_retention() {
    let mut options = options(Lang::EnUs);
    options.retention_secs = 600;
    let mut sim = Sim::new(Lang::EnUs);
    sim.m = Model::new(options);
    let effects = sim.tick(0);
    sim.answer(&effects[0], ok(me_root()));
    let effects = sim.tick(100);
    sim.answer(&effects[0], ok(&status_json(false)));
    sim.press('m');
    assert_eq!(sim.m.cluster.view, ClusterView::Monitor);
    for _ in 0..6 {
        sim.press('+');
    }
    // 600 s keeps 1 and 5 minutes only.
    assert_eq!(cluster::window_ms(&sim.m), 5 * 60_000);
    sim.press('-');
    sim.press('-');
    sim.press('-');
    assert_eq!(cluster::window_ms(&sim.m), 60_000);
}

/// TUI: TUI-MON-03
#[test]
fn the_chart_pauses_changes_scale_and_series() {
    let mut sim = Sim::root_with_status(Lang::EnUs);
    sim.press('m');
    sim.tick(1_000);
    sim.press(' ');
    assert_eq!(sim.m.cluster.monitor.frozen, Some(1_000));
    sim.press(' ');
    assert_eq!(sim.m.cluster.monitor.frozen, None);
    sim.press('z');
    assert!(sim.m.cluster.monitor.fixed_scale);
    sim.code(KeyCode::Down);
    sim.code(KeyCode::Down);
    assert_eq!(sim.m.cluster.monitor.chart, 2);
    sim.code(KeyCode::Up);
    assert_eq!(sim.m.cluster.monitor.chart, 1);
    // Enter gives the chart the whole screen; Esc leaves it before it leaves the charts.
    sim.code(KeyCode::Enter);
    assert!(sim.m.cluster.monitor.full);
    sim.code(KeyCode::Esc);
    assert!(!sim.m.cluster.monitor.full);
    assert_eq!(sim.m.cluster.view, ClusterView::Monitor);
    sim.code(KeyCode::Esc);
    assert_eq!(sim.m.cluster.view, ClusterView::List);
}

/// TUI: TUI-MON-07
#[test]
fn exporting_asks_for_a_path_and_writes_the_visible_series_as_csv() {
    let mut sim = Sim::root_with_status(Lang::EnUs);
    // Two samples of memory, 5 s apart.
    let effects = sim.tick(5_200);
    sim.answer(&effects[0], ok(&status_json(false)));
    sim.press('m');
    sim.press('e');
    let Some(Modal::Input(input)) = &sim.m.modal else {
        panic!("{:?}", sim.m.modal)
    };
    assert_eq!(input.value, "taba-00000001-memory.csv");
    // The person edits the path.
    for _ in 0..4 {
        sim.code(KeyCode::Backspace);
    }
    for c in "x.csv".chars() {
        sim.press(c);
    }
    let effects = sim.code(KeyCode::Enter);
    let [Effect::WriteFile { path, contents }] = &effects[..] else {
        panic!("{effects:?}")
    };
    assert_eq!(path, "taba-00000001-memoryx.csv");
    let lines: Vec<&str> = contents.lines().collect();
    assert_eq!(lines[0], "time,used,limit");
    assert_eq!(lines.len(), 3, "{contents}");
    assert!(lines[1].starts_with("2026-10-15T"), "{}", lines[1]);
    assert!(lines[1].ends_with(",805306368,1073741824"), "{}", lines[1]);
}

/// TUI: TUI-MON-07
#[test]
fn nothing_is_written_when_the_export_is_cancelled_or_the_path_is_empty() {
    let mut sim = Sim::root_with_status(Lang::EnUs);
    sim.press('m');
    sim.press('e');
    assert!(sim.code(KeyCode::Esc).is_empty());
    sim.press('e');
    for _ in 0..80 {
        sim.code(KeyCode::Backspace);
    }
    assert!(sim.code(KeyCode::Enter).is_empty());
}

/// TUI: TUI-MON-07
#[test]
fn the_result_of_writing_a_file_is_shown() {
    let mut sim = Sim::root_with_status(Lang::EnUs);
    sim.send(Msg::FileWritten {
        path: "a.csv".into(),
        result: Ok(()),
    });
    assert_eq!(sim.m.notice.as_ref().unwrap().text, "Wrote a.csv");
    sim.send(Msg::FileWritten {
        path: "a.csv".into(),
        result: Err("exists".into()),
    });
    assert_eq!(sim.m.notice.as_ref().unwrap().severity, Severity::Critical);
}

#[test]
fn a_notice_goes_away_by_itself() {
    let mut sim = Sim::root_with_status(Lang::EnUs);
    sim.press('u');
    assert!(sim.m.notice.is_some());
    sim.tick(1_000);
    assert!(sim.m.notice.is_some());
    sim.tick(6_000);
    assert!(sim.m.notice.is_none());
}

#[test]
fn a_resize_is_recorded() {
    let mut sim = Sim::root_with_status(Lang::EnUs);
    sim.send(Msg::Resize(100, 30));
    assert_eq!(sim.m.size, (100, 30));
}

/// TUI: TUI-CLU-06
#[test]
fn the_reading_pauses_while_the_terminal_is_out_of_focus_and_catches_up_on_return() {
    let mut sim = Sim::root_with_status(Lang::EnUs);
    sim.send(Msg::Focus(false));
    assert!(calls(&sim.tick(5_000)).is_empty());
    sim.send(Msg::Focus(true));
    assert!(calls(&sim.tick(5_250)).contains(&ApiCall::ClusterStatus));
}

/// TUI: TUI-NOD-03
#[test]
fn a_drained_node_stays_in_the_table_so_it_can_be_returned() {
    let mut sim = Sim::root_with_status(Lang::EnUs);
    // The status no longer lists the drained node (it left the member table); `GET /nodes` still does.
    let nodes = sim
        .log
        .iter()
        .rev()
        .find(|e| {
            matches!(
                e,
                Effect::Call {
                    call: ApiCall::Nodes,
                    ..
                }
            )
        })
        .unwrap()
        .clone();
    sim.answer(
        &nodes,
        ok(&format!(
            r#"{{"partial":false,"missingNodes":[],"items":[{{"nodeId":"{NODE_A}","reachable":true,"state":"active","drainState":"active"}},{{"nodeId":"ghost","self":false,"reachable":false,"state":"drained","drainState":"drained"}}]}}"#
        )),
    );
    let rows = cluster::rows(&sim.m);
    assert_eq!(rows.len(), 4);
    let ghost = rows.iter().find(|r| r.id == "ghost").unwrap();
    assert!(ghost.reachable && ghost.drain_state == "drained");
    sim.code(KeyCode::End);
    assert_eq!(cluster::selected_row(&sim.m).unwrap().id, "ghost");
    sim.press('u');
    assert!(
        matches!(sim.m.modal, Some(Modal::Confirm(_))),
        "{:?}",
        sim.m.notice
    );
}
