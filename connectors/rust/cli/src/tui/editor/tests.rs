use super::*;
use ratatui::crossterm::event::KeyModifiers;

fn press(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn editor() -> Editor {
    Editor::new(
        Purpose::ProfileNew,
        vec![
            Field::text("name", "editor.field.name", ""),
            Field::check("flag", "editor.field.default", false),
            Field::text("last", "editor.field.url", ""),
        ],
    )
}

/// TUI: TUI-TOK-02
#[test]
fn typing_goes_to_the_focused_field_and_tab_moves_on() {
    let mut e = editor();
    for c in "abc".chars() {
        e.handle(&press(KeyCode::Char(c)));
    }
    e.handle(&press(KeyCode::Backspace));
    assert_eq!(e.text("name"), "ab");
    e.handle(&press(KeyCode::Tab));
    assert_eq!(e.focus, 1);
    // A space on a checkbox ticks it; letters do nothing there.
    e.handle(&press(KeyCode::Char('x')));
    assert!(!e.checked("flag"));
    e.handle(&press(KeyCode::Char(' ')));
    assert!(e.checked("flag"));
    e.handle(&press(KeyCode::BackTab));
    assert_eq!(e.focus, 0);
}

/// TUI: TUI-TOK-02
#[test]
fn enter_goes_on_until_the_last_field_and_then_saves() {
    let mut e = editor();
    assert_eq!(e.handle(&press(KeyCode::Enter)), Outcome::None);
    assert_eq!(e.handle(&press(KeyCode::Enter)), Outcome::None);
    assert_eq!(e.focus, 2);
    assert_eq!(e.handle(&press(KeyCode::Enter)), Outcome::Submit);
    assert_eq!(e.handle(&press(KeyCode::Esc)), Outcome::Cancel);
    let ctrl_s = KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL);
    assert_eq!(e.handle(&ctrl_s), Outcome::Submit);
}

/// TUI: TUI-TOK-04
#[test]
fn a_locked_field_is_skipped_and_cannot_change() {
    let mut e = Editor::new(
        Purpose::ProfileNew,
        vec![
            Field::text("a", "editor.field.name", "keep").locked("editor.locked.token"),
            Field::text("b", "editor.field.url", ""),
        ],
    );
    assert_eq!(
        e.focus, 1,
        "the form opens on the first field that can change"
    );
    e.handle(&press(KeyCode::BackTab));
    assert_eq!(e.focus, 1);
    e.focus = 0;
    e.handle(&press(KeyCode::Char('z')));
    assert_eq!(e.text("a"), "keep");
}

/// TUI: TUI-HLP-04
#[test]
fn the_connection_test_is_only_for_profiles_and_a_paste_drops_line_breaks() {
    let mut e = editor();
    let ctrl_t = KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL);
    assert_eq!(e.handle(&ctrl_t), Outcome::Test);
    let mut token = Editor::new(
        Purpose::TokenCreate { clone_of: None },
        vec![Field::text("name", "editor.field.name", "")],
    );
    assert_eq!(token.handle(&ctrl_t), Outcome::None);
    token.insert("ab\ncd\r\n");
    assert_eq!(token.text("name"), "abcd");
    token.busy = true;
    assert_eq!(token.handle(&press(KeyCode::Esc)), Outcome::None);
}
