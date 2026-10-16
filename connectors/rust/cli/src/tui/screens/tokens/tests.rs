use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::*;
use crate::tui::editor::Purpose;
use crate::tui::i18n::Lang;
use crate::tui::model::{Modal, Tab};
use crate::tui::msg::Msg;
use crate::tui::testing::*;

const NOW: i64 = 1_792_065_600_000;

fn page(next: Option<&str>) -> String {
    let cursor = next.map_or(String::new(), |c| format!(r#","nextCursor":"{c}""#));
    format!(
        r#"{{"items":[{{"id":"0191-a","name":"orders-app","permissions":["publish","consume"],"topicPrefixes":["orders."],"createdAt":"2026-10-01T10:00:00Z","expiresAt":null}},{{"id":"0191-b","name":"billing","permissions":["admin"],"topicPrefixes":[],"createdAt":"2026-10-10T10:00:00Z","expiresAt":"2026-10-18T00:00:00Z"}}]{cursor}}}"#
    )
}

fn second_page() -> &'static str {
    r#"{"items":[{"id":"0191-c","name":"old-batch","permissions":["consume"],"topicPrefixes":[],"createdAt":"2026-09-01T10:00:00Z","expiresAt":"2026-10-01T00:00:00Z"}]}"#
}

/// A root session on the Tokens tab with its two pages read (the clock is at `NOW`).
fn on_tokens(lang: Lang) -> Sim {
    let mut sim = Sim::root_with_status(lang);
    sim.press('4');
    assert_eq!(sim.m.tab, Tab::Tokens);
    let effects = sim.tick(1_000);
    let call = effects
        .iter()
        .find(|e| {
            matches!(
                e,
                Effect::Call {
                    call: ApiCall::Tokens(None),
                    ..
                }
            )
        })
        .expect("the tab asks for the tokens")
        .clone();
    let next = sim.answer(&call, ok(&page(Some("c2"))));
    sim.answer(&next[0], ok(second_page()));
    sim
}

fn ctrl_s() -> Msg {
    Msg::Key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL))
}

fn type_text(sim: &mut Sim, text: &str) {
    for c in text.chars() {
        sim.press(c);
    }
}

fn the_call(effects: &[Effect]) -> ApiCall {
    effects
        .iter()
        .find_map(|e| match e {
            Effect::Call { call, .. } => Some(call.clone()),
            _ => None,
        })
        .expect("a call")
}

/// TUI: TUI-TOK-02
#[test]
fn an_expiry_is_a_time_from_now_a_date_an_instant_or_nothing() {
    let day = 86_400_000;
    assert_eq!(parse_expiry("", NOW), Ok(None));
    assert_eq!(parse_expiry("none", NOW), Ok(None));
    assert_eq!(parse_expiry("Never", NOW), Ok(None));
    assert_eq!(parse_expiry("30d", NOW), Ok(Some(rfc3339(NOW + 30 * day))));
    assert_eq!(
        parse_expiry("12h", NOW),
        Ok(Some(rfc3339(NOW + 12 * 3_600_000)))
    );
    assert_eq!(
        parse_expiry("2027-01-31", NOW),
        Ok(Some("2027-01-31T00:00:00Z".to_owned()))
    );
    assert_eq!(
        parse_expiry("2027-01-31T10:30:00Z", NOW),
        Ok(Some("2027-01-31T10:30:00Z".to_owned()))
    );
    for bad in ["soon", "0d", "-3d", "2027-13-01", "10x"] {
        assert_eq!(parse_expiry(bad, NOW), Err(BadExpiry), "{bad}");
    }
    assert_eq!(rfc3339(NOW), "2026-10-15T12:00:00Z");
}

/// TUI: TUI-TOK-01
#[test]
fn the_tokens_are_read_page_by_page_until_the_server_has_no_more() {
    let sim = on_tokens(Lang::EnUs);
    let names: Vec<_> = visible(&sim.m).into_iter().map(|t| t.name).collect();
    assert_eq!(names, vec!["billing", "orders-app", "old-batch"]);
    assert!(sim.m.tokens.list.error.is_none());
    assert!(sim.m.tokens.reading.is_empty());
}

/// TUI: TUI-TOK-01
#[test]
fn the_list_is_filtered_by_name_and_sorted_and_the_selection_follows_the_token() {
    let mut sim = on_tokens(Lang::EnUs);
    sim.press('j');
    assert_eq!(selected(&sim.m).unwrap().name, "orders-app");
    sim.press('s');
    assert_eq!(sim.m.tokens.sort, TokenSort::OldestFirst);
    assert_eq!(selected(&sim.m).unwrap().name, "orders-app");
    sim.press('s');
    let names: Vec<_> = visible(&sim.m).into_iter().map(|t| t.name).collect();
    assert_eq!(names, vec!["billing", "old-batch", "orders-app"]);
    sim.press('/');
    type_text(&mut sim, "ORD");
    sim.code(KeyCode::Enter);
    let names: Vec<_> = visible(&sim.m).into_iter().map(|t| t.name).collect();
    assert_eq!(names, vec!["orders-app"]);
    sim.press('G');
    assert_eq!(selected(&sim.m).unwrap().name, "orders-app");
}

/// TUI: TUI-TOK-06
#[test]
fn only_root_and_admin_tokens_manage_tokens_and_a_server_without_authentication_has_none() {
    let mut sim = Sim::new(Lang::EnUs);
    let effects = sim.tick(0);
    sim.answer(&effects[0], ok(me_plain()));
    sim.press('4');
    assert_eq!(availability(&sim.m), Availability::NotAdmin);
    assert!(!sim.poll(5_000).contains(&ApiCall::Tokens(None)));
    assert_eq!(gate(&sim.m, Action::TokenNew), Some("gate.not-admin"));

    let mut open = Sim::new(Lang::EnUs);
    let effects = open.tick(0);
    open.answer(
        &effects[0],
        ok(r#"{"authEnabled":false,"root":false,"name":"open","permissions":["publish","consume","create","admin"]}"#),
    );
    open.press('4');
    assert_eq!(availability(&open.m), Availability::NoAuth);
    assert!(!open.poll(5_000).contains(&ApiCall::Tokens(None)));
    assert_eq!(gate(&open.m, Action::TokenNew), Some("gate.tokens-no-auth"));
}

/// TUI: TUI-TOK-02
#[test]
fn a_new_token_is_sent_as_the_form_was_filled_and_its_secret_is_shown_once() {
    let mut sim = on_tokens(Lang::EnUs);
    sim.press('n');
    assert!(matches!(sim.m.modal, Some(Modal::Editor(_))));
    // Without a name or a permission the form says what is missing and stays open.
    assert!(sim.send(ctrl_s()).is_empty());
    let Some(Modal::Editor(editor)) = &sim.m.modal else {
        panic!("the form closed");
    };
    assert!(editor.error.is_some());
    type_text(&mut sim, "reports");
    assert!(sim.send(ctrl_s()).is_empty(), "no permission yet");
    // The focus went to the permissions: publish and consume are ticked; prefixes and expiry typed.
    sim.press(' ');
    sim.code(KeyCode::Tab);
    sim.press(' ');
    for _ in 0..4 {
        sim.code(KeyCode::Tab);
    }
    type_text(&mut sim, "reports. audit.");
    sim.code(KeyCode::Tab);
    type_text(&mut sim, "2027-01-31");
    let effects = sim.send(ctrl_s());
    let call = the_call(&effects);
    let ApiCall::TokenCreate(body) = &call else {
        panic!("{call:?}");
    };
    assert_eq!(
        body,
        r#"{"name":"reports","permissions":["publish","consume"],"topicPrefixes":["reports.","audit."],"expiresAt":"2027-01-31T00:00:00Z"}"#
    );
    // While the call runs the form waits.
    let Some(Modal::Editor(editor)) = &sim.m.modal else {
        panic!("the form closed too soon");
    };
    assert!(editor.busy);

    sim.answer(
        &effects[0],
        ok(r#"{"id":"0191-z","name":"reports","permissions":["publish","consume"],"topicPrefixes":["reports.","audit."],"createdAt":"2026-10-15T12:00:00Z","expiresAt":"2027-01-31T00:00:00Z","token":"tb_0191-z.s3cret"}"#),
    );
    let Some(Modal::Secret(secret)) = &sim.m.modal else {
        panic!("{:?}", sim.m.modal);
    };
    assert_eq!(secret.token, "tb_0191-z.s3cret");
    assert_eq!(sim.m.tokens.selected.as_deref(), Some("0191-z"));
    // The secret is nowhere else: not in the notice.
    assert!(sim
        .m
        .notice
        .as_ref()
        .is_none_or(|n| !n.text.contains("s3cret")));

    // Closing without copying or saving asks again; copying lets it close.
    sim.code(KeyCode::Esc);
    let Some(Modal::Secret(secret)) = &sim.m.modal else {
        panic!("closed without asking");
    };
    assert!(secret.warned && !secret.copied);
    let effects = sim.press('c');
    assert_eq!(effects, vec![Effect::Copy("tb_0191-z.s3cret".to_owned())]);
    sim.code(KeyCode::Esc);
    assert!(sim.m.modal.is_none());
    assert!(sim
        .m
        .notice
        .as_ref()
        .is_none_or(|n| !n.text.contains("s3cret")));
}

/// TUI: TUI-TOK-03
#[test]
fn the_secret_can_be_saved_to_a_file_and_that_counts_as_kept() {
    let mut sim = on_tokens(Lang::EnUs);
    sim.m.modal = Some(Modal::Secret(Secret {
        id: "0191-z".into(),
        name: "reports".into(),
        token: "tb_0191-z.s3cret".into(),
        shown_at: 0,
        copied: false,
        saved: false,
        warned: false,
        replaces: None,
        revoke_old: false,
    }));
    let effects = sim.press('s');
    assert_eq!(
        effects,
        vec![Effect::WriteSecret {
            path: "taba-token-0191-z.txt".to_owned(),
            contents: "tb_0191-z.s3cret\n".to_owned(),
        }]
    );
    sim.send(Msg::FileWritten {
        path: "taba-token-0191-z.txt".into(),
        result: Ok(()),
    });
    sim.code(KeyCode::Esc);
    assert!(sim.m.modal.is_none(), "a saved secret closes at once");
    assert_eq!(secret_file("../x/y"), "taba-token-xy.txt");
}

/// TUI: TUI-TOK-02
#[test]
fn the_reason_the_server_refuses_a_token_stays_in_the_form() {
    let mut sim = on_tokens(Lang::EnUs);
    sim.press('n');
    type_text(&mut sim, "x");
    sim.code(KeyCode::Tab);
    sim.press(' ');
    let effects = sim.send(ctrl_s());
    sim.answer(
        &effects[0],
        fail(
            400,
            "validation.invalid-prefix",
            r#"{"code":"validation.invalid-prefix"}"#,
        ),
    );
    let Some(Modal::Editor(editor)) = &sim.m.modal else {
        panic!("{:?}", sim.m.modal);
    };
    assert!(!editor.busy);
    assert!(
        editor
            .error
            .as_deref()
            .unwrap()
            .contains("validation.invalid-prefix"),
        "{:?}",
        editor.error
    );
    assert!(sim.m.busy.is_none());
}

/// TUI: TUI-TOK-04
#[test]
fn editing_changes_only_the_name_and_the_expiry_and_sends_only_what_changed() {
    let mut sim = on_tokens(Lang::EnUs);
    // billing is the first (newest) and expires on 2026-10-18.
    sim.press('e');
    let Some(Modal::Editor(editor)) = &sim.m.modal else {
        panic!("{:?}", sim.m.modal);
    };
    assert!(matches!(editor.purpose, Purpose::TokenEdit { .. }));
    assert!(
        editor.field("admin").unwrap().locked.is_some(),
        "permissions are fixed"
    );
    assert!(editor.field("prefixes").unwrap().locked.is_some());
    // Nothing changed: nothing is sent.
    assert!(sim.send(ctrl_s()).is_empty());
    // Remove the deadline: the field is cleared.
    sim.code(KeyCode::Tab);
    for _ in 0..30 {
        sim.code(KeyCode::Backspace);
    }
    let effects = sim.send(ctrl_s());
    assert_eq!(
        the_call(&effects),
        ApiCall::TokenUpdate("0191-b".into(), r#"{"expiresAt":null}"#.into())
    );
    sim.answer(&effects[0], ok(&page(None)));
    assert!(sim.m.modal.is_none());
    assert_eq!(sim.m.notice.as_ref().unwrap().text, "Token updated");
    assert!(sim.m.tokens.list.next_at == 0, "the list is read again");
}

/// TUI: TUI-TOK-05
#[test]
fn a_clone_starts_filled_and_offers_to_revoke_the_old_token_but_not_by_default() {
    let mut sim = on_tokens(Lang::EnUs);
    sim.press('j');
    sim.press('c');
    let Some(Modal::Editor(editor)) = &sim.m.modal else {
        panic!("{:?}", sim.m.modal);
    };
    assert_eq!(editor.text("name"), "orders-app-copy");
    assert!(editor.checked("publish") && editor.checked("consume") && !editor.checked("admin"));
    assert_eq!(editor.text("prefixes"), "orders.");
    let effects = sim.send(ctrl_s());
    assert!(matches!(the_call(&effects), ApiCall::TokenCreate(_)));
    sim.answer(
        &effects[0],
        ok(r#"{"id":"0191-n","name":"orders-app-copy","permissions":["publish","consume"],"topicPrefixes":["orders."],"createdAt":"2026-10-15T12:00:00Z","expiresAt":null,"token":"tb_0191-n.abc"}"#),
    );
    let Some(Modal::Secret(secret)) = &sim.m.modal else {
        panic!("{:?}", sim.m.modal);
    };
    assert_eq!(
        secret.replaces,
        Some(("0191-a".to_owned(), "orders-app".to_owned()))
    );
    assert!(!secret.revoke_old, "the default is not to revoke");
    // Keep the new one, leave the old one alone.
    sim.press('c');
    sim.code(KeyCode::Esc);
    assert!(sim.m.modal.is_none());
    assert!(!sim.log.iter().any(|e| matches!(
        e,
        Effect::Call {
            call: ApiCall::TokenRevoke(_),
            ..
        }
    )));

    // Ticked: closing revokes the old token.
    sim.m.modal = Some(Modal::Secret(Secret {
        id: "0191-n".into(),
        name: "orders-app-copy".into(),
        token: "tb_0191-n.abc".into(),
        shown_at: 0,
        copied: true,
        saved: false,
        warned: false,
        replaces: Some(("0191-a".into(), "orders-app".into())),
        revoke_old: false,
    }));
    sim.press('r');
    let effects = sim.code(KeyCode::Esc);
    assert_eq!(the_call(&effects), ApiCall::TokenRevoke("0191-a".into()));
}

/// TUI: TUI-TOK-05
#[test]
fn revoking_asks_first_and_the_default_answer_is_cancel() {
    let mut sim = on_tokens(Lang::EnUs);
    sim.press('x');
    assert!(matches!(
        sim.m.modal,
        Some(Modal::Confirm(Confirm {
            pending: Pending::RevokeToken(..),
            ..
        }))
    ));
    assert!(
        sim.code(KeyCode::Enter).is_empty(),
        "Enter on Cancel does nothing"
    );
    assert!(sim.m.modal.is_none());
    sim.press('x');
    sim.code(KeyCode::Tab);
    let effects = sim.code(KeyCode::Enter);
    assert_eq!(the_call(&effects), ApiCall::TokenRevoke("0191-b".into()));
    sim.answer(
        &effects[0],
        Ok(taba_client::Reply {
            status: 204,
            headers: Vec::new(),
            body: Vec::new(),
            request_id: None,
            recovered: false,
        }),
    );
    assert_eq!(
        sim.m.notice.as_ref().unwrap().text,
        "Token revoked on every node"
    );
}

/// TUI: TUI-FRM-10
#[test]
fn an_action_that_cannot_run_says_why_instead_of_doing_nothing() {
    let mut sim = Sim::root_with_status(Lang::EnUs);
    sim.press('4');
    // The tokens have not arrived: there is nothing to edit.
    sim.press('e');
    assert!(sim.m.modal.is_none());
    assert_eq!(sim.m.notice.as_ref().unwrap().text, "Select a token first.");
}

/// TUI: TUI-TOK-06
#[test]
fn a_token_with_admin_manages_tokens_but_only_the_root_changes_or_creates_admin_ones() {
    let mut sim = on_tokens(Lang::EnUs);
    // The root session: the `billing` token has `admin`, and the form offers the permission.
    sim.m.tokens.selected = Some("0191-b".into());
    assert_eq!(gate(&sim.m, Action::TokenRevoke), None);

    let me = sim.m.me.data.as_mut().unwrap();
    me.root = false;
    me.permissions = vec!["admin".to_owned()];
    assert_eq!(availability(&sim.m), Availability::Ready);
    assert_eq!(gate(&sim.m, Action::TokenNew), None);
    assert_eq!(
        gate(&sim.m, Action::TokenRevoke),
        Some("gate.root-for-admin")
    );
    assert_eq!(gate(&sim.m, Action::TokenEdit), Some("gate.root-for-admin"));
    sim.m.tokens.selected = Some("0191-a".into());
    assert_eq!(gate(&sim.m, Action::TokenRevoke), None);

    open_create(&mut sim.m, None);
    let Some(Modal::Editor(editor)) = &sim.m.modal else {
        panic!("the form did not open");
    };
    assert!(editor.field("admin").unwrap().locked.is_some());
    assert!(editor.field("manage").unwrap().locked.is_none());
}

/// TUI: TUI-FRM-05
#[test]
fn altgr_characters_work_as_keys_and_in_text_fields() {
    // Windows reports AltGr (`/` and `?` on an ABNT2 keyboard) as Ctrl+Alt together.
    let altgr = |c: char| {
        Msg::Key(KeyEvent::new(
            KeyCode::Char(c),
            KeyModifiers::CONTROL | KeyModifiers::ALT,
        ))
    };
    let mut sim = on_tokens(Lang::EnUs);
    sim.send(altgr('/'));
    assert!(matches!(sim.m.modal, Some(Modal::Input(_))));
    sim.send(altgr('@'));
    let Some(Modal::Input(input)) = &sim.m.modal else {
        panic!("the filter box should be open");
    };
    assert_eq!(input.value, "@");
    sim.code(KeyCode::Esc);

    sim.send(altgr('?'));
    assert!(matches!(sim.m.modal, Some(Modal::Help { .. })));

    // A real Ctrl shortcut is still a shortcut, not text.
    let mut sim = on_tokens(Lang::EnUs);
    sim.send(Msg::Key(KeyEvent::new(
        KeyCode::Char('/'),
        KeyModifiers::CONTROL,
    )));
    assert!(sim.m.modal.is_none());
}
