//! Drawing tests: each screen is drawn on a `TestBackend` at 80×24 and 120×40, in both languages, and compared with
//! the text snapshots in `tests/snapshots/`. Set `TABA_UPDATE_SNAPSHOTS=1` to write them again, and read the diff.

use std::fs;
use std::path::PathBuf;

use ratatui::backend::TestBackend;
use ratatui::crossterm::event::KeyCode;
use ratatui::Terminal;

use super::*;
use crate::tui::api::ApiCall;
use crate::tui::effect::Effect;
use crate::tui::i18n::Lang;
use crate::tui::model::short_id;
use crate::tui::msg::Msg;
use crate::tui::testing::*;

fn render(m: &Model, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|f| view(f, m)).unwrap();
    let buffer = terminal.backend().buffer().clone();
    let mut lines = Vec::new();
    for y in 0..buffer.area.height {
        let mut line = String::new();
        for x in 0..buffer.area.width {
            line.push_str(buffer[(x, y)].symbol());
        }
        lines.push(line.trim_end().to_owned());
    }
    lines.join("\n") + "\n"
}

fn snapshot(name: &str, text: &str) {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("snapshots");
    let path = dir.join(format!("{name}.txt"));
    if std::env::var_os("TABA_UPDATE_SNAPSHOTS").is_some() {
        fs::create_dir_all(&dir).unwrap();
        fs::write(&path, text).unwrap();
        return;
    }
    let expected = fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("missing snapshot {name}; run with TABA_UPDATE_SNAPSHOTS=1"));
    let normalize = |s: &str| s.replace("\r\n", "\n");
    assert_eq!(
        normalize(text),
        normalize(&expected),
        "snapshot {name} differs\n--- got ---\n{text}"
    );
}

/// A session with a few minutes of samples, so the charts have something to draw.
fn with_history(lang: Lang) -> Sim {
    let mut sim = Sim::root_with_status(lang);
    for i in 1..=24u64 {
        let now = 100 + 5_000 * i;
        let used = 805_306_368 + i * 8_000_000;
        let rejected = i * 3;
        let body = status_json(false)
            .replacen("805306368", &used.to_string(), 1)
            .replacen(
                "rateLimitRejected\":5.0",
                &format!("rateLimitRejected\":{rejected}.0"),
                1,
            );
        let effects = sim.tick(now);
        if let Some(call) = effects.iter().find(|e| matches!(e, Effect::Call { .. })) {
            sim.answer(call, ok(&body));
        }
    }
    sim.tick(125_000);
    sim
}

const SIZES: [(u16, u16); 2] = [(80, 24), (120, 40)];

/// TUI: TUI-FRM-01, TUI-FRM-02, TUI-FRM-03, TUI-CLU-01, TUI-CLU-02, TUI-CLU-03, TUI-CLU-04, TUI-CLU-06
#[test]
fn the_cluster_tab_in_both_sizes_and_languages() {
    for lang in [Lang::EnUs, Lang::PtBr] {
        let sim = with_history(lang);
        for (w, h) in SIZES {
            snapshot(
                &format!("cluster-{}-{w}x{h}", lang.code()),
                &render(&sim.m, w, h),
            );
        }
    }
}

/// TUI: TUI-FRM-08, TUI-CLU-01, TUI-CLU-02, TUI-CLU-03
#[test]
fn an_incomplete_read_shows_the_seal_the_condition_and_the_unreachable_node() {
    for lang in [Lang::EnUs, Lang::PtBr] {
        let sim = Sim::with_status_json(lang, &status_json(true));
        snapshot(
            &format!("partial-{}-80x24", lang.code()),
            &render(&sim.m, 80, 24),
        );
    }
    let sim = Sim::with_status_json(Lang::EnUs, &status_json(true));
    let screen = render(&sim.m, 120, 40);
    assert!(screen.contains("incomplete"), "{screen}");
    assert!(screen.contains("timeout"), "{screen}");
    snapshot("partial-en-US-120x40", &screen);
}

/// TUI: TUI-CLU-07, TUI-FRM-10
#[test]
fn a_plain_token_sees_the_node_list_and_why_the_rest_is_missing() {
    for lang in [Lang::EnUs, Lang::PtBr] {
        let sim = Sim::plain_with_nodes(lang);
        let screen = render(&sim.m, 80, 24);
        assert!(
            !screen.contains("Memory") && !screen.contains("Memória"),
            "{screen}"
        );
        snapshot(&format!("plain-{}-80x24", lang.code()), &screen);
    }
}

/// TUI: TUI-FRM-08
#[test]
fn before_the_first_answer_and_when_the_server_is_unreachable() {
    let mut sim = Sim::new(Lang::EnUs);
    snapshot("loading-en-US-80x24", &render(&sim.m, 80, 24));
    let effects = sim.tick(0);
    sim.answer(&effects[0], offline());
    let screen = render(&sim.m, 80, 24);
    assert!(
        screen.contains("offline") && screen.contains("taba.example"),
        "{screen}"
    );
    snapshot("offline-en-US-80x24", &screen);
    // With data on screen, the data stays and is marked old.
    let mut sim = Sim::root_with_status(Lang::EnUs);
    let effects = sim.tick(2_100);
    sim.answer(&effects[0], offline());
    let screen = render(&sim.m, 80, 24);
    assert!(screen.contains("old data"), "{screen}");
    assert!(screen.contains(&short_id(NODE_A)), "{screen}");
    snapshot("stale-en-US-80x24", &screen);
}

/// TUI: TUI-FRM-06
#[test]
fn a_terminal_below_80_by_24_shows_a_warning_instead_of_the_screen() {
    let sim = Sim::root_with_status(Lang::EnUs);
    for (w, h) in [(79, 24), (80, 23), (40, 10)] {
        let screen = render(&sim.m, w, h);
        let flat = screen.split_whitespace().collect::<Vec<_>>().join(" ");
        assert!(flat.contains("at least 80x24"), "{screen}");
        assert!(!screen.contains(&short_id(NODE_A)), "{screen}");
    }
    snapshot("too-small-en-US-79x24", &render(&sim.m, 79, 24));
    let pt = Sim::root_with_status(Lang::PtBr);
    let flat = render(&pt.m, 70, 20)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    assert!(flat.contains("pelo menos 80x24"), "{flat}");
    // Resizing back redraws at once.
    assert!(render(&sim.m, 80, 24).contains(&short_id(NODE_A)));
}

/// TUI: TUI-NOD-01, TUI-NOD-02, TUI-NOD-03
#[test]
fn the_node_detail_in_both_languages() {
    for lang in [Lang::EnUs, Lang::PtBr] {
        let mut sim = with_history(lang);
        sim.code(KeyCode::Enter);
        let effects = sim.tick(126_000);
        for effect in &effects {
            let Effect::Call { call, .. } = effect else {
                continue;
            };
            match call {
                ApiCall::NodeMetrics(_) => {
                    sim.answer(
                        effect,
                        ok("# TYPE taba_storage_memory_messages gauge\ntaba_storage_memory_messages 1200\n# TYPE taba_ratelimit_rejected_total counter\ntaba_ratelimit_rejected_total{category=\"publish\"} 5\n"),
                    );
                }
                ApiCall::NodeBackup(_) => {
                    sim.answer(
                        effect,
                        ok(r#"{"s3Reachable":true,"outboxPending":4,"lastSuccessAt":"2026-10-15T11:58:00Z","segments":12}"#),
                    );
                }
                _ => {}
            }
        }
        for (w, h) in SIZES {
            snapshot(
                &format!("detail-{}-{w}x{h}", lang.code()),
                &render(&sim.m, w, h),
            );
        }
    }
}

/// TUI: TUI-NOD-02
#[test]
fn a_node_without_backup_says_so() {
    let mut sim = Sim::root_with_status(Lang::EnUs);
    sim.code(KeyCode::Enter);
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
    assert!(render(&sim.m, 80, 40).contains("S3 backup is not configured"));
}

/// TUI: TUI-NOD-03
#[test]
fn a_drain_in_progress_shows_its_pending_messages() {
    let mut sim = Sim::root_with_status(Lang::EnUs);
    sim.code(KeyCode::Enter);
    sim.press('d');
    sim.code(KeyCode::Right);
    let effects = sim.code(KeyCode::Enter);
    sim.answer(
        &effects[0],
        ok(r#"{"drainState":"draining","pendingMessages":40}"#),
    );
    let polled = sim.tick(300);
    let status = polled
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
        &status,
        ok(r#"{"drainState":"draining","pendingMessages":12}"#),
    );
    let screen = render(&sim.m, 80, 40);
    assert!(screen.contains("12 messages still in memory"), "{screen}");
}

/// TUI: TUI-MON-01, TUI-MON-02, TUI-MON-03, TUI-MON-04, TUI-MON-05
#[test]
fn the_charts_of_a_node() {
    for lang in [Lang::EnUs, Lang::PtBr] {
        let mut sim = with_history(lang);
        sim.press('m');
        for (w, h) in SIZES {
            snapshot(
                &format!("monitor-{}-{w}x{h}", lang.code()),
                &render(&sim.m, w, h),
            );
        }
    }
    let mut sim = with_history(Lang::EnUs);
    sim.press('m');
    let screen = render(&sim.m, 100, 30);
    // Four charts at a time, each with the cluster figure.
    assert!(screen.contains("Charts 1-4 of 6"), "{screen}");
    assert!(screen.contains("total "), "{screen}");
    assert!(screen.contains("Messages in memory"), "{screen}");
    // A short history says so instead of pretending a full window (TUI-MON-05).
    assert!(screen.contains("collected"), "{screen}");
}

/// The charts view scrolls through every indicator, four at a time.
#[test]
fn the_charts_scroll_through_every_indicator() {
    let mut sim = with_history(Lang::EnUs);
    sim.press('m');
    sim.code(KeyCode::Down);
    let screen = render(&sim.m, 100, 30);
    assert!(screen.contains("Charts 3-6 of 6"), "{screen}");
    assert!(screen.contains("Channel round trip"), "{screen}");
    sim.code(KeyCode::Down);
    assert_eq!(sim.m.cluster.monitor.chart, 2);
    sim.code(KeyCode::Up);
    assert_eq!(sim.m.cluster.monitor.chart, 0);
}

/// TUI: TUI-MON-02, TUI-MON-03
#[test]
fn every_chart_draws_with_and_without_data_and_frozen() {
    let mut sim = Sim::root_with_status(Lang::EnUs);
    sim.press('m');
    for i in 0..crate::tui::screens::cluster::MON_CHARTS.len() {
        sim.m.cluster.monitor.chart = i;
        let screen = render(&sim.m, 80, 24);
        assert!(!screen.is_empty());
    }
    let mut sim = with_history(Lang::EnUs);
    sim.press('m');
    for i in 0..crate::tui::screens::cluster::MON_CHARTS.len() {
        sim.m.cluster.monitor.chart = i;
        sim.m.cluster.monitor.frozen = Some(60_000);
        sim.m.cluster.monitor.fixed_scale = i % 2 == 0;
        let screen = render(&sim.m, 80, 24);
        assert!(screen.contains("paused"), "{screen}");
    }
}

/// TUI: TUI-FRM-07, TUI-HLP-01, TUI-HLP-02, TUI-HLP-03, TUI-CLU-05, TUI-NOD-05
#[test]
fn the_popups() {
    for lang in [Lang::EnUs, Lang::PtBr] {
        let code = lang.code();
        let mut sim = Sim::root_with_status(lang);
        sim.press('?');
        snapshot(&format!("help-{code}-80x24"), &render(&sim.m, 80, 24));
        sim.code(KeyCode::Esc);
        sim.press(':');
        for c in "dr".chars() {
            sim.press(c);
        }
        snapshot(&format!("palette-{code}-80x24"), &render(&sim.m, 80, 24));
        sim.code(KeyCode::Esc);
        sim.press('L');
        snapshot(&format!("language-{code}-80x24"), &render(&sim.m, 80, 24));
        sim.code(KeyCode::Esc);
        sim.press('d');
        snapshot(
            &format!("confirm-drain-{code}-80x24"),
            &render(&sim.m, 80, 24),
        );
        sim.code(KeyCode::Esc);
        sim.press('S');
        for c in "prod".chars() {
            sim.press(c);
        }
        snapshot(
            &format!("confirm-stop-{code}-80x24"),
            &render(&sim.m, 80, 24),
        );
    }
    let mut sim = Sim::with_status_json(Lang::EnUs, &status_json(true));
    sim.code(KeyCode::Tab);
    sim.code(KeyCode::Enter);
    snapshot("condition-en-US-80x24", &render(&sim.m, 80, 24));
}

/// TUI: TUI-NOD-04, TUI-NOD-05
#[test]
fn the_reports_of_the_actions() {
    let mut sim = Sim::root_with_status(Lang::EnUs);
    sim.press('B');
    sim.code(KeyCode::Right);
    let effects = sim.code(KeyCode::Enter);
    sim.answer(
        &effects[0],
        ok(&format!(
            r#"{{"complete":false,"nodes":[{{"nodeId":"{NODE_A}","complete":true}},{{"nodeId":"{NODE_B}","complete":false,"pending":true,"outboxPending":9,"memoryRemaining":3}},{{"nodeId":"{NODE_C}","complete":false,"error":"timeout"}}]}}"#
        )),
    );
    snapshot("report-flush-en-US-80x24", &render(&sim.m, 80, 24));
    sim.code(KeyCode::Esc);

    sim.press('S');
    for c in "prod-eu".chars() {
        sim.press(c);
    }
    let effects = sim.code(KeyCode::Enter);
    sim.answer(
        &effects[0],
        ok(&format!(
            r#"{{"stopped":["{NODE_B}","{NODE_A}"],"stopList":false,"warnings":["S3 is down"],"nodes":[{{"nodeId":"{NODE_B}","drained":true}},{{"nodeId":"{NODE_A}","drained":true}}]}}"#
        )),
    );
    snapshot("report-stop-en-US-80x24", &render(&sim.m, 80, 24));
    sim.code(KeyCode::Esc);

    sim.press('b');
    sim.code(KeyCode::Right);
    let effects = sim.code(KeyCode::Enter);
    sim.answer(
        &effects[0],
        fail(
            502,
            "forward.bad-gateway",
            r#"{"message":"node did not answer"}"#,
        ),
    );
    snapshot("report-failure-en-US-80x24", &render(&sim.m, 80, 24));
}

/// TUI: TUI-MON-07
#[test]
fn the_export_popup_asks_for_the_path() {
    let mut sim = with_history(Lang::EnUs);
    sim.press('m');
    sim.press('e');
    let screen = render(&sim.m, 80, 24);
    assert!(screen.contains("taba-00000001-memory.csv"), "{screen}");
    assert!(screen.contains("never overwritten"), "{screen}");
}

/// TUI: TUI-FRM-02
#[test]
fn the_tabs_that_are_not_built_yet_say_so_and_navigate() {
    for lang in [Lang::EnUs, Lang::PtBr] {
        let mut sim = Sim::root_with_status(lang);
        for (key, name) in [
            ('2', "topics"),
            ('3', "tokens"),
            ('4', "messages"),
            ('5', "profiles"),
        ] {
            sim.press(key);
            let screen = render(&sim.m, 80, 24);
            if key == '2' {
                snapshot(&format!("{name}-{}-80x24", lang.code()), &screen);
            }
            assert!(screen.contains("taba-cli"), "{screen}");
        }
    }
}

/// TUI: TUI-FRM-03
#[test]
fn the_footer_shows_the_keys_of_the_screen_and_dims_what_cannot_run() {
    let sim = Sim::root_with_status(Lang::EnUs);
    let screen = render(&sim.m, 80, 24);
    let footer = screen.lines().last().unwrap();
    assert!(
        footer.contains("drain") && footer.contains("quit") && footer.contains("help"),
        "{footer}"
    );
    assert!(footer.contains("en-US"), "{footer}");
    assert!(footer.contains("2s"), "{footer}");

    let mut paused = Sim::root_with_status(Lang::EnUs);
    paused.press('p');
    let footer = render(&paused.m, 80, 24).lines().last().unwrap().to_owned();
    assert!(footer.contains("paused"), "{footer}");

    let mut stale = Sim::root_with_status(Lang::PtBr);
    let effects = stale.tick(2_100);
    stale.answer(&effects[0], offline());
    let footer = render(&stale.m, 80, 24).lines().last().unwrap().to_owned();
    assert!(
        footer.contains("dado velho") && footer.contains("nova tentativa"),
        "{footer}"
    );
}

/// TUI: TUI-FRM-09
#[test]
fn the_selected_row_is_a_band_not_a_reversed_cell_and_no_color_still_marks_it() {
    use ratatui::style::Modifier;
    let sim = Sim::root_with_status(Lang::EnUs);
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|f| view(f, &sim.m)).unwrap();
    let buffer = terminal.backend().buffer().clone();
    let row = (0..24)
        .find(|y| {
            (0..80)
                .map(|x| buffer[(x, *y)].symbol())
                .collect::<String>()
                .contains(&short_id(NODE_A))
        })
        .unwrap();
    let cell = &buffer[(10, row)];
    assert!(cell.modifier.contains(Modifier::BOLD));
    assert!(!cell.modifier.contains(Modifier::REVERSED));
    assert_ne!(cell.bg, ratatui::style::Color::Reset);

    let mut plain = Sim::root_with_status(Lang::EnUs);
    plain.m.theme = crate::tui::theme::Theme {
        color: false,
        ascii: true,
    };
    let screen = render(&plain.m, 80, 24);
    // ASCII symbols and a marker in the first column of the selected row.
    assert!(screen.contains("> "), "{screen}");
    assert!(
        !screen.contains('✔')
            && !screen.contains('▶')
            && !screen.contains('█')
            && !screen.contains('⟳'),
        "{screen}"
    );
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|f| view(f, &plain.m)).unwrap();
    let colored = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .any(|c| c.fg != ratatui::style::Color::Reset || c.bg != ratatui::style::Color::Reset);
    assert!(!colored, "NO_COLOR must not paint");
}

/// TUI: TUI-FRM-01
#[test]
fn the_status_line_says_the_worst_condition_in_words_and_a_symbol() {
    let sim = Sim::with_status_json(Lang::EnUs, &status_json(true));
    let first = render(&sim.m, 80, 24).lines().next().unwrap().to_owned();
    assert!(
        first.contains("prod") && first.contains("taba.example"),
        "{first}"
    );
    assert!(
        first.contains("member unreachable") && first.contains('▲'),
        "{first}"
    );
    assert!(first.contains("UTC"), "{first}");
    let healthy = Sim::root_with_status(Lang::PtBr);
    let first = render(&healthy.m, 80, 24)
        .lines()
        .next()
        .unwrap()
        .to_owned();
    assert!(first.contains("saudável") && first.contains('✔'), "{first}");
}

/// TUI: TUI-FRM-04
#[test]
fn a_long_detail_scrolls_and_shows_its_bar_only_when_it_overflows() {
    let mut sim = Sim::root_with_status(Lang::EnUs);
    sim.code(KeyCode::Enter);
    let top = render(&sim.m, 80, 24);
    assert!(
        top.contains('█') || top.contains('║') || top.contains('▲') || top.contains('↑'),
        "{top}"
    );
    sim.code(KeyCode::End);
    sim.send(Msg::Scroll(3));
    let bottom = render(&sim.m, 80, 24);
    assert_ne!(top, bottom);
    sim.code(KeyCode::Home);
    assert_eq!(render(&sim.m, 80, 24), top);
}

/// TUI: TUI-NOD-01, TUI-CLU-03
#[test]
fn a_node_without_a_memory_limit_shows_its_use_and_says_so() {
    let status = status_json(false).replace(
        "\"memoryLimitBytes\":1073741824",
        "\"memoryLimitBytes\":null",
    );
    let mut sim = Sim::with_status_json(Lang::EnUs, &status);
    let table = render(&sim.m, 80, 24);
    assert!(
        table.contains("768.0 MiB") && !table.contains(" 75%"),
        "{table}"
    );
    sim.code(KeyCode::Enter);
    let detail = render(&sim.m, 80, 40);
    assert!(detail.contains("(no limit)"), "{detail}");
}

/// TUI: TUI-NOD-04
#[test]
fn a_flush_on_a_cluster_without_s3_says_there_is_nothing_to_send() {
    let mut sim = Sim::root_with_status(Lang::EnUs);
    sim.press('B');
    sim.code(KeyCode::Right);
    let effects = sim.code(KeyCode::Enter);
    sim.answer(&effects[0], fail(422, "backup.not-configured", "{}"));
    let screen = render(&sim.m, 100, 30);
    assert!(screen.contains("backup.not-configured"), "{screen}");
    assert!(screen.contains("nothing to send"), "{screen}");
    assert!(screen.contains("request id: req-1"), "{screen}");
}

/// A cluster that every node left by drain says so, with the reason when the S3 stop list has one.
#[test]
fn a_stopped_cluster_says_so_and_why() {
    for (planned, expected) in [(true, "planned stop"), (false, "every node is in drain")] {
        let status = status_json(false).replace(
            r#""syncingBlocks":0}"#,
            &format!(
                r#""syncingBlocks":0,"servingNodes":0,"stopped":true,"plannedStop":{planned},"stoppedAt":"2026-10-15T12:00:00.000Z"}}"#
            ),
        );
        let sim = Sim::with_status_json(Lang::EnUs, &status);
        let screen = render(&sim.m, 120, 30);
        assert!(screen.contains(expected), "{screen}");
        let pt = Sim::with_status_json(Lang::PtBr, &status);
        let screen = render(&pt.m, 120, 30);
        assert!(screen.contains("Cluster parado"), "{screen}");
    }
    // A running cluster shows no such line.
    let sim = Sim::root_with_status(Lang::EnUs);
    assert!(!render(&sim.m, 120, 30).contains("Cluster stopped"));
}
