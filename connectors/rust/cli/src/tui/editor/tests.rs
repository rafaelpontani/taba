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

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn typed(editor: &mut Editor, text: &str) {
    for c in text.chars() {
        if c == '\n' {
            editor.handle(&key(KeyCode::Enter));
        } else {
            editor.handle(&key(KeyCode::Char(c)));
        }
    }
}

fn body_form() -> Editor {
    Editor::new(
        Purpose::MessagePublish {
            topic: "orders".to_owned(),
        },
        vec![
            Field::area("body", "editor.field.body", ""),
            Field::text("content-type", "editor.field.content-type", "text/plain"),
        ],
    )
}

/// TUI: TUI-MSG-02
#[test]
fn enter_in_an_area_breaks_the_line_and_the_arrows_move_the_cursor() {
    let mut editor = body_form();
    typed(&mut editor, "{\n  \"a\": 1\n}");
    assert_eq!(editor.field("body").unwrap().value, "{\n  \"a\": 1\n}");
    assert_eq!(editor.field("body").unwrap().line_col(), (2, 1));
    // Up goes to the line above at the same column, clipped to its end.
    editor.handle(&key(KeyCode::Up));
    assert_eq!(editor.field("body").unwrap().line_col(), (1, 1));
    editor.handle(&key(KeyCode::End));
    assert_eq!(editor.field("body").unwrap().line_col(), (1, 8));
    editor.handle(&key(KeyCode::Home));
    typed(&mut editor, ">");
    assert_eq!(editor.field("body").unwrap().value, "{\n>  \"a\": 1\n}");
    editor.handle(&key(KeyCode::Backspace));
    editor.handle(&key(KeyCode::Delete));
    assert_eq!(editor.field("body").unwrap().value, "{\n \"a\": 1\n}");
    // Backspace at the start of a line joins it to the one above.
    editor.handle(&key(KeyCode::Home));
    editor.handle(&key(KeyCode::Backspace));
    assert_eq!(editor.field("body").unwrap().value, "{ \"a\": 1\n}");
    assert_eq!(editor.focus, 0);
}

/// TUI: TUI-MSG-02
#[test]
fn the_arrows_leave_the_area_at_its_first_and_last_lines_and_tab_always_does() {
    let mut editor = body_form();
    typed(&mut editor, "one\ntwo");
    editor.handle(&key(KeyCode::Up));
    assert_eq!(editor.focus, 0);
    editor.handle(&key(KeyCode::Up));
    assert_eq!(editor.focus, 0, "the first field has nothing above it");
    editor.handle(&key(KeyCode::Down));
    editor.handle(&key(KeyCode::Down));
    assert_eq!(editor.focus, 1);
    editor.handle(&key(KeyCode::BackTab));
    assert_eq!(editor.focus, 0);
    editor.handle(&key(KeyCode::Tab));
    assert_eq!(editor.focus, 1);
    // Enter on the last field still sends; in the area it never does.
    let mut editor = body_form();
    assert_eq!(editor.handle(&key(KeyCode::Enter)), Outcome::None);
    assert_eq!(
        editor.handle(&KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL)),
        Outcome::Submit
    );
}

/// TUI: TUI-MSG-02
#[test]
fn pasted_text_keeps_its_lines_in_an_area_and_loses_them_in_a_line_field() {
    let mut editor = body_form();
    editor.insert("a\r\nb\rc\td");
    assert_eq!(editor.field("body").unwrap().value, "a\nb\nc d");
    editor.focus = 1;
    editor.insert("x\ny");
    assert_eq!(editor.field("content-type").unwrap().value, "text/plainxy");
}
