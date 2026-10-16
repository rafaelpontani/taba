use super::*;
use crate::tui::testing::*;

fn failure(status: u16) -> Fail {
    Fail {
        status,
        code: "x.failed".to_owned(),
        message: "no".to_owned(),
        request_id: None,
        retry_after: None,
        required: None,
    }
}

/// TUI: TUI-FRM-08
#[test]
fn a_feed_waits_for_its_turn_and_never_asks_twice_at_once() {
    let mut feed: Feed<u32> = Feed::default();
    assert!(feed.due(0));
    feed.in_flight = Some(1);
    assert!(!feed.due(10_000));
    feed.ok(1_000, 7, 2_000);
    assert!(!feed.due(2_999));
    assert!(feed.due(3_000));
    assert_eq!(feed.data, Some(7));
    assert_eq!(feed.fetched_at, Some(1_000));
}

/// TUI: TUI-FRM-08
#[test]
fn a_failure_keeps_the_value_and_doubles_the_wait_each_time_up_to_thirty_seconds() {
    let mut feed: Feed<u32> = Feed::default();
    feed.ok(0, 1, 2_000);
    let mut waits = Vec::new();
    for _ in 0..6 {
        let at = feed.next_at;
        feed.fail(at, failure(0), 2_000);
        waits.push(feed.next_at - at);
    }
    assert_eq!(waits, vec![4_000, 8_000, 16_000, 30_000, 30_000, 30_000]);
    assert!(feed.is_stale());
    assert_eq!(feed.data, Some(1));
    feed.ok(500_000, 2, 2_000);
    assert!(!feed.is_stale());
    assert_eq!(feed.failures, 0);
}

/// TUI: TUI-FRM-08
#[test]
fn retry_after_wins_over_the_backoff() {
    let mut feed: Feed<u32> = Feed::default();
    let mut busy = failure(429);
    busy.retry_after = Some(9);
    feed.fail(1_000, busy, 2_000);
    assert_eq!(feed.next_at, 10_000);
    assert_eq!(feed.retry_in(4_000), Some(6));
    assert_eq!(feed.retry_in(11_000), Some(0));
}

#[test]
fn a_feed_without_error_has_no_retry_and_a_reset_forgets_everything() {
    let mut feed: Feed<u32> = Feed::default();
    assert_eq!(feed.retry_in(0), None);
    feed.ok(5, 1, 100);
    feed.reset();
    assert_eq!(feed, Feed::default());
}

/// TUI: TUI-FRM-10
#[test]
fn the_required_permission_is_read_from_the_body_of_the_error() {
    let error = Error {
        status: 403,
        code: "auth.forbidden".into(),
        message: "no".into(),
        request_id: Some("r1".into()),
        retry_after: None,
        details: None,
        body: br#"{"code":"auth.forbidden","required":"root"}"#.to_vec(),
    };
    let fail = Fail::from(&error);
    assert_eq!(fail.required.as_deref(), Some("root"));
    assert!(fail.is_denied() && !fail.is_offline());
    let net = Fail::from(&Error::from_message("client.network", "refused"));
    assert!(net.is_offline() && net.required.is_none());
}

/// TUI: TUI-CLU-03
#[test]
fn an_id_is_shortened_by_its_start_and_its_end() {
    assert_eq!(
        short_id("01a13640-7774-7000-8000-00000000abcd"),
        "01a1…00abcd"
    );
    assert_eq!(short_id("short"), "short");
    assert_eq!(file_tag("01a13640-7774-7000-8000-00000000abcd"), "0000abcd");
}

/// TUI: TUI-FRM-01
#[test]
fn the_health_is_the_worst_condition_then_degraded_then_unknown() {
    let sim = Sim::new(Lang::EnUs);
    assert_eq!(sim.m.health(), (Severity::Ok, "unknown".to_owned()));
    let healthy = Sim::root_with_status(Lang::EnUs);
    assert_eq!(healthy.m.health(), (Severity::Ok, "ok".to_owned()));
    let partial = Sim::with_status_json(Lang::EnUs, &status_json(true));
    assert_eq!(
        partial.m.health(),
        (Severity::Warning, "member-unreachable".to_owned())
    );
    let plain = Sim::plain_with_nodes(Lang::EnUs);
    assert_eq!(plain.m.health(), (Severity::Ok, "ok".to_owned()));
    let mut critical = Sim::with_status_json(
        Lang::EnUs,
        &status_json(true).replace(r#""severity":"warning""#, r#""severity":"critical""#),
    );
    assert_eq!(critical.m.health().0, Severity::Critical);
    critical.m.status.error = Some(Fail {
        status: 0,
        ..failure(0)
    });
    assert_eq!(critical.m.health().1, "offline");
}

/// TUI: TUI-FRM-02
#[test]
fn the_five_tabs_have_names_and_indexes() {
    for (i, tab) in TABS.iter().enumerate() {
        assert_eq!(tab.index(), i);
        assert!(tab.name_key().starts_with("tab."));
    }
    assert_eq!(TABS.len(), 5);
}

/// TUI: TUI-CLU-05
#[test]
fn a_confirmation_maps_to_the_call_it_will_make() {
    assert_eq!(
        Pending::Drain("n".into()).call(),
        ApiCall::DrainStart("n".into())
    );
    assert_eq!(
        Pending::Return("n".into()).call(),
        ApiCall::DrainCancel("n".into())
    );
    assert_eq!(
        Pending::FlushNode("n".into()).call(),
        ApiCall::FlushNode("n".into())
    );
    assert_eq!(Pending::FlushAll.call(), ApiCall::FlushCluster);
    assert_eq!(Pending::Stop.call(), ApiCall::StopCluster);
}
