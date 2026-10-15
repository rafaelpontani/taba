use super::*;

fn plain() -> Style {
    Style { color: false }
}

fn type_text(form: &mut Form, text: &str) {
    for c in text.chars() {
        form.handle(Key::Char(c));
    }
}

fn text_of(frame: &Frame) -> String {
    frame.lines.join("\n")
}

/// UC: UC-CLI-030
#[test]
fn arrows_and_tab_move_between_fields_and_buttons_wrapping_around() {
    let mut f = Form::new(None, false, vec![]);
    assert_eq!(f.focus, NAME);
    f.handle(Key::Down);
    f.handle(Key::Tab);
    assert_eq!(f.focus, 2);
    f.handle(Key::Up);
    f.handle(Key::BackTab);
    f.handle(Key::BackTab);
    assert_eq!(f.focus, CANCEL);
    f.handle(Key::Down);
    assert_eq!(f.focus, NAME);
}

/// UC: UC-CLI-030
#[test]
fn text_fields_are_edited_in_place_with_the_cursor_keys() {
    let mut f = Form::new(None, false, vec![]);
    type_text(&mut f, "producer");
    f.handle(Key::Home);
    f.handle(Key::Delete);
    f.handle(Key::Char('P'));
    f.handle(Key::End);
    f.handle(Key::Backspace);
    f.handle(Key::Left);
    f.handle(Key::Char('-'));
    assert_eq!(f.name(), "Produc-e");
    f.handle(Key::Ctrl('u'));
    assert_eq!(f.name(), "");
    assert!(f.dirty());
}

/// UC: UC-CLI-030
#[test]
fn editing_handles_multibyte_characters() {
    let mut f = Form::new(None, false, vec![]);
    f.handle(Key::Down);
    type_text(&mut f, "çã");
    f.handle(Key::Left);
    f.handle(Key::Backspace);
    assert_eq!(f.texts[1].value, "ã");
}

/// UC: UC-CLI-030
#[test]
fn output_is_a_choice_and_the_default_is_a_box() {
    let mut f = Form::new(None, false, vec![]);
    f.focus = OUTPUT;
    assert_eq!(f.value(OUTPUT), "");
    f.handle(Key::Right);
    assert_eq!(f.value(OUTPUT), "json");
    f.handle(Key::Right);
    f.handle(Key::Right);
    assert_eq!(f.value(OUTPUT), "raw");
    f.handle(Key::Right);
    assert_eq!(f.value(OUTPUT), "");
    f.handle(Key::Left);
    assert_eq!(f.value(OUTPUT), "raw");
    f.focus = DEFAULT;
    f.handle(Key::Char(' '));
    assert!(f.is_default);
    f.handle(Key::Char(' '));
    assert!(!f.is_default);
}

/// UC: UC-CLI-030
#[test]
fn empty_fields_show_the_value_that_will_apply_and_the_ops_url_follows_the_url() {
    let mut f = Form::new(None, false, vec![]);
    let shown = text_of(&f.render(plain(), 80, 30));
    assert!(
        shown.contains("url") && shown.contains("(http://127.0.0.1:8080)"),
        "{shown}"
    );
    assert_eq!(
        shown.matches("(http://127.0.0.1:8080)").count(),
        2,
        "{shown}"
    );
    assert!(
        shown.contains("(system roots)")
            && shown.contains("(5)")
            && shown.contains("(30)")
            && shown.contains("(3)")
    );
    f.handle(Key::Down);
    type_text(&mut f, "https://taba.example.com");
    let shown = text_of(&f.render(plain(), 80, 30));
    assert_eq!(
        shown.matches("(https://taba.example.com)").count(),
        1,
        "{shown}"
    );
}

/// UC: UC-CLI-030
#[test]
fn an_empty_field_is_left_out_of_the_profile_like_unset() {
    let mut existing = Profile::default();
    existing.set("url", "http://a:1").unwrap();
    existing.set("timeout", "60").unwrap();
    let mut f = Form::new(Some(("x", &existing)), false, vec!["x".into()]);
    f.focus = 1;
    f.handle(Key::Ctrl('u'));
    let p = f.profile();
    assert_eq!(p.get("url"), None);
    assert_eq!(p.get("timeout"), Some("60"));
}

/// UC: UC-CLI-031
#[test]
fn invalid_values_get_a_message_under_the_field() {
    let mut f = Form::new(None, false, vec!["local".into()]);
    type_text(&mut f, "local");
    f.handle(Key::Down);
    type_text(&mut f, "taba:8080");
    f.focus = 8;
    type_text(&mut f, "0");
    let errors = f.errors();
    assert!(errors[NAME].as_ref().unwrap().contains("already exists"));
    assert!(errors[1].as_ref().unwrap().contains("http://"));
    assert!(errors[8].as_ref().unwrap().contains("1 to 3600"));
    assert!(!f.is_valid());
    let shown = text_of(&f.render(plain(), 80, 40));
    assert!(
        shown.contains("a profile `local` already exists"),
        "{shown}"
    );
    assert!(f.errors()[3].is_none());
}

/// UC: UC-CLI-031
#[test]
fn the_name_is_required_and_checked_but_an_edited_profile_may_keep_its_own_name() {
    let f = Form::new(None, false, vec![]);
    assert!(f.errors()[NAME].as_ref().unwrap().contains("required"));
    let mut f = Form::new(None, false, vec![]);
    type_text(&mut f, "with space");
    assert!(f.errors()[NAME].is_some());
    let own = Form::new(
        Some(("local", &Profile::default())),
        false,
        vec!["local".into(), "stg".into()],
    );
    assert!(own.errors()[NAME].is_none());
    assert!(own.is_valid());
}

/// UC: UC-CLI-030
#[test]
fn ctrl_s_and_the_buttons_ask_to_save_test_and_cancel() {
    let mut f = Form::new(None, false, vec![]);
    assert_eq!(f.handle(Key::Ctrl('s')), FormEvent::Save);
    f.focus = SAVE;
    assert_eq!(f.handle(Key::Enter), FormEvent::Save);
    f.handle(Key::Right);
    assert_eq!(f.handle(Key::Enter), FormEvent::Test);
    f.handle(Key::Right);
    assert_eq!(f.handle(Key::Enter), FormEvent::Cancel);
}

/// UC: UC-CLI-033
#[test]
fn escape_cancels_at_once_when_nothing_changed_and_asks_when_it_did() {
    let mut f = Form::new(None, false, vec![]);
    assert_eq!(f.handle(Key::Esc), FormEvent::Cancel);
    let mut f = Form::new(None, false, vec![]);
    type_text(&mut f, "x");
    assert_eq!(f.handle(Key::Esc), FormEvent::None);
    assert!(f.status.as_ref().unwrap().1.contains("unsaved"));
    // Any other key goes back to the form.
    f.handle(Key::Char('y'));
    assert_eq!(f.name(), "xy");
    assert!(f.status.is_none());
    f.handle(Key::Esc);
    assert_eq!(f.handle(Key::Esc), FormEvent::Cancel);
}

/// UC: UC-CLI-033
#[test]
fn ctrl_c_quits_from_anywhere() {
    let mut f = Form::new(None, false, vec![]);
    type_text(&mut f, "x");
    assert_eq!(f.handle(Key::Ctrl('c')), FormEvent::Quit);
}

/// UC: UC-CLI-034
#[test]
fn the_token_is_shown_in_plain_text() {
    let mut p = Profile::default();
    p.set("token", "tb_segredo_visivel").unwrap();
    p.set("token-file", "/sec/tok").unwrap();
    let f = Form::new(Some(("x", &p)), false, vec![]);
    let shown = text_of(&f.render(plain(), 80, 40));
    assert!(
        shown.contains("tb_segredo_visivel") && shown.contains("/sec/tok"),
        "{shown}"
    );
}

/// UC: UC-CLI-029
#[test]
fn a_short_terminal_scrolls_to_keep_the_focused_line_visible() {
    let mut f = Form::new(None, false, vec![]);
    f.focus = CANCEL;
    let frame = f.render(plain(), 80, 12);
    assert_eq!(frame.lines.len(), 12);
    let shown = text_of(&frame);
    assert!(shown.contains("[>Cancel<]"), "{shown}");
    assert!(shown.contains("Esc cancel"));
}

/// UC: UC-CLI-030
#[test]
fn the_cursor_sits_after_the_typed_text_of_the_focused_field() {
    let mut f = Form::new(None, false, vec![]);
    type_text(&mut f, "abc");
    let frame = f.render(plain(), 80, 40);
    assert_eq!(frame.cursor, Some((2, 3 + LABEL_WIDTH + 3)));
    f.focus = DEFAULT;
    assert_eq!(f.render(plain(), 80, 40).cursor, None);
}
