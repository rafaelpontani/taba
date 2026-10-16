use super::*;

fn press(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

/// TUI: TUI-FRM-05
#[test]
fn no_key_is_bound_twice_in_the_same_scope() {
    for scope in [
        Scope::Global,
        Scope::Cluster,
        Scope::Monitor,
        Scope::Tokens,
        Scope::Profiles,
    ] {
        let mut seen: Vec<(Key, Action)> = Vec::new();
        for b in BINDINGS.iter().filter(|b| b.scope == scope) {
            for key in b.keys {
                if let Some((_, other)) = seen.iter().find(|(k, _)| k == key) {
                    panic!(
                        "{key:?} is bound to {other:?} and {:?} in {scope:?}",
                        b.action
                    );
                }
                seen.push((*key, b.action));
            }
        }
    }
}

/// TUI: TUI-HLP-01
#[test]
fn a_screen_may_shadow_only_the_global_keys_listed_here() {
    let allowed = [
        (Key::Char('S'), Scope::Cluster),
        (Key::Code(KeyCode::Enter), Scope::Cluster),
        (Key::Code(KeyCode::Enter), Scope::Profiles),
    ];
    for b in BINDINGS.iter().filter(|b| b.scope != Scope::Global) {
        for key in b.keys {
            let shadows = BINDINGS
                .iter()
                .any(|g| g.scope == Scope::Global && g.keys.contains(key));
            assert!(
                !shadows || allowed.contains(&(*key, b.scope)),
                "{key:?} in {:?}",
                b.scope
            );
        }
    }
}

/// TUI: TUI-FRM-05
#[test]
fn arrows_and_vim_keys_do_the_same() {
    assert_eq!(
        resolve(Scope::Global, &press(KeyCode::Down)),
        Some(Action::Down)
    );
    assert_eq!(
        resolve(Scope::Global, &press(KeyCode::Char('j'))),
        Some(Action::Down)
    );
    assert_eq!(resolve(Scope::Global, &ctrl('d')), Some(Action::PageDown));
    assert_eq!(
        resolve(Scope::Global, &press(KeyCode::End)),
        Some(Action::Last)
    );
    assert_eq!(
        resolve(
            Scope::Global,
            &KeyEvent::new(KeyCode::Char('G'), KeyModifiers::SHIFT)
        ),
        Some(Action::Last)
    );
}

/// TUI: TUI-FRM-02
#[test]
fn tabs_are_reached_by_number_bracket_and_alt_arrows() {
    assert_eq!(
        resolve(Scope::Global, &press(KeyCode::Char('3'))),
        Some(Action::GoTab(2))
    );
    assert_eq!(
        resolve(Scope::Global, &press(KeyCode::Char(']'))),
        Some(Action::TabNext)
    );
    assert_eq!(
        resolve(
            Scope::Global,
            &KeyEvent::new(KeyCode::Left, KeyModifiers::ALT)
        ),
        Some(Action::TabPrev)
    );
    assert_eq!(
        resolve(Scope::Global, &press(KeyCode::Left)),
        Some(Action::Left)
    );
}

/// TUI: TUI-CLU-05
#[test]
fn a_screen_key_wins_over_the_global_one() {
    assert_eq!(
        resolve(Scope::Cluster, &press(KeyCode::Char('S'))),
        Some(Action::StopCluster)
    );
    assert_eq!(
        resolve(Scope::Global, &press(KeyCode::Char('S'))),
        Some(Action::SortReverse)
    );
    assert_eq!(
        resolve(Scope::Cluster, &press(KeyCode::Char('j'))),
        Some(Action::Down)
    );
    assert_eq!(
        resolve(Scope::Cluster, &press(KeyCode::Char('d'))),
        Some(Action::Drain)
    );
    assert_eq!(resolve(Scope::Global, &press(KeyCode::Char('d'))), None);
}

#[test]
fn a_ctrl_chord_is_not_the_plain_letter() {
    assert_eq!(
        resolve(Scope::Global, &press(KeyCode::Char('q'))),
        Some(Action::Quit)
    );
    assert_eq!(resolve(Scope::Global, &ctrl('c')), Some(Action::Quit));
    assert_eq!(resolve(Scope::Cluster, &ctrl('d')), Some(Action::PageDown));
    assert_eq!(resolve(Scope::Cluster, &ctrl('b')), None);
}

/// TUI: TUI-HLP-01
#[test]
fn the_help_of_a_screen_lists_its_keys_and_the_global_ones_it_does_not_shadow() {
    let cluster = help_for(Scope::Cluster);
    assert!(cluster.iter().any(|b| b.action == Action::Drain));
    assert!(cluster.iter().any(|b| b.action == Action::Quit));
    assert!(!cluster.iter().any(|b| b.action == Action::SortReverse));
    assert!(!help_for(Scope::Global)
        .iter()
        .any(|b| b.action == Action::Drain));
}

/// TUI: TUI-HLP-01
#[test]
fn every_help_text_is_a_catalog_key() {
    for b in BINDINGS {
        if let Some(key) = b.help {
            assert!(key.starts_with("keys."), "{key}");
        }
    }
}
