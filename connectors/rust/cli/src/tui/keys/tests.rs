use super::*;

/// UC: UC-CLI-030
#[test]
fn decodes_arrows_in_csi_and_application_mode() {
    assert_eq!(
        decode(b"\x1b[A\x1b[B\x1b[C\x1b[D"),
        [Key::Up, Key::Down, Key::Right, Key::Left]
    );
    assert_eq!(decode(b"\x1bOA\x1bOB"), [Key::Up, Key::Down]);
}

/// UC: UC-CLI-030
#[test]
fn decodes_editing_keys() {
    assert_eq!(
        decode(b"\x1b[H\x1b[F\x1b[1~\x1b[4~\x1b[7~\x1b[8~"),
        [
            Key::Home,
            Key::End,
            Key::Home,
            Key::End,
            Key::Home,
            Key::End
        ]
    );
    assert_eq!(
        decode(b"\x1b[3~\x7f\x08\r\n\t\x1b[Z"),
        [
            Key::Delete,
            Key::Backspace,
            Key::Backspace,
            Key::Enter,
            Key::Enter,
            Key::Tab,
            Key::BackTab
        ]
    );
}

/// UC: UC-CLI-030, UC-CLI-033
#[test]
fn a_lone_escape_is_the_escape_key_and_control_letters_are_named() {
    assert_eq!(decode(b"\x1b"), [Key::Esc]);
    assert_eq!(
        decode(b"\x13\x03\x04\x15"),
        [
            Key::Ctrl('s'),
            Key::Ctrl('c'),
            Key::Ctrl('d'),
            Key::Ctrl('u')
        ]
    );
}

/// UC: UC-CLI-030
#[test]
fn decodes_typed_text_including_multibyte_and_pasted_runs() {
    assert_eq!(
        decode("ab é中😀".as_bytes()),
        [
            Key::Char('a'),
            Key::Char('b'),
            Key::Char(' '),
            Key::Char('é'),
            Key::Char('中'),
            Key::Char('😀')
        ]
    );
}

/// UC: UC-CLI-030
#[test]
fn unknown_sequences_are_skipped_whole() {
    // `Ctrl+↑` (`ESC[1;5A`) vale como `↑`; `PageUp` (`ESC[5~`) não tem uso e some.
    assert_eq!(
        decode(b"\x1b[5~x\x1b[1;5Ay"),
        [Key::Char('x'), Key::Up, Key::Char('y')]
    );
    assert_eq!(decode(b"\x1b[200~"), []);
    assert!(decode(b"\x1b[").is_empty());
}
