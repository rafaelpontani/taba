//! The one table of keys (doc 34, section 5): action ⇄ keys. The program reads its keys from here, and the help
//! (`?`), the footer and the command palette (`:`) are generated from the same table, so they never disagree.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Where a binding applies. A screen's scope is looked up before `Global`, so it may shadow a global key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Global,
    Cluster,
    Monitor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Quit,
    GoTab(u8),
    TabNext,
    TabPrev,
    PaneNext,
    PanePrev,
    Up,
    Down,
    PageUp,
    PageDown,
    First,
    Last,
    Left,
    Right,
    Enter,
    Esc,
    Filter,
    Sort,
    SortReverse,
    Refresh,
    Pause,
    Palette,
    Language,
    Help,
    Submit,
    Detail,
    Drain,
    Return,
    FlushNode,
    FlushAll,
    StopCluster,
    Monitor,
    WindowWider,
    WindowNarrower,
    PauseChart,
    Scale,
    Export,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Code(KeyCode),
    Ctrl(char),
    Alt(KeyCode),
}

impl Key {
    fn matches(self, event: &KeyEvent) -> bool {
        let ctrl = event.modifiers.contains(KeyModifiers::CONTROL);
        let alt = event.modifiers.contains(KeyModifiers::ALT);
        match self {
            Key::Char(c) => matches!(event.code, KeyCode::Char(e) if e == c) && !ctrl && !alt,
            Key::Code(code) => event.code == code && !ctrl && !alt,
            Key::Ctrl(c) => {
                ctrl && matches!(event.code, KeyCode::Char(e) if e.eq_ignore_ascii_case(&c))
            }
            Key::Alt(code) => alt && event.code == code,
        }
    }
}

pub struct Binding {
    pub action: Action,
    pub keys: &'static [Key],
    pub scope: Scope,
    /// The keys as the help shows them (the same in every language).
    pub label: &'static str,
    /// The catalog key of the action's description; `None` keeps the binding out of the help and the palette.
    pub help: Option<&'static str>,
}

const fn bind(
    action: Action,
    scope: Scope,
    keys: &'static [Key],
    label: &'static str,
    help: Option<&'static str>,
) -> Binding {
    Binding {
        action,
        keys,
        scope,
        label,
        help,
    }
}

use Key::{Alt, Char, Code, Ctrl};
use Scope::{Cluster, Global, Monitor};

pub const BINDINGS: &[Binding] = &[
    bind(
        Action::GoTab(0),
        Global,
        &[Char('1')],
        "1…5",
        Some("keys.tab"),
    ),
    bind(Action::GoTab(1), Global, &[Char('2')], "2", None),
    bind(Action::GoTab(2), Global, &[Char('3')], "3", None),
    bind(Action::GoTab(3), Global, &[Char('4')], "4", None),
    bind(Action::GoTab(4), Global, &[Char('5')], "5", None),
    bind(
        Action::TabNext,
        Global,
        &[Char(']'), Alt(KeyCode::Right)],
        "] · Alt+→",
        Some("keys.tab-next"),
    ),
    bind(
        Action::TabPrev,
        Global,
        &[Char('['), Alt(KeyCode::Left)],
        "[ · Alt+←",
        Some("keys.tab-prev"),
    ),
    bind(
        Action::PaneNext,
        Global,
        &[Code(KeyCode::Tab)],
        "Tab",
        Some("keys.pane-next"),
    ),
    bind(
        Action::PanePrev,
        Global,
        &[Code(KeyCode::BackTab)],
        "Shift+Tab",
        Some("keys.pane-prev"),
    ),
    bind(
        Action::Up,
        Global,
        &[Code(KeyCode::Up), Char('k')],
        "↑ k",
        Some("keys.up"),
    ),
    bind(
        Action::Down,
        Global,
        &[Code(KeyCode::Down), Char('j')],
        "↓ j",
        Some("keys.down"),
    ),
    bind(
        Action::PageUp,
        Global,
        &[Code(KeyCode::PageUp), Ctrl('u')],
        "PgUp Ctrl+u",
        Some("keys.page-up"),
    ),
    bind(
        Action::PageDown,
        Global,
        &[Code(KeyCode::PageDown), Ctrl('d')],
        "PgDn Ctrl+d",
        Some("keys.page-down"),
    ),
    bind(
        Action::First,
        Global,
        &[Code(KeyCode::Home), Char('g')],
        "Home g",
        Some("keys.first"),
    ),
    bind(
        Action::Last,
        Global,
        &[Code(KeyCode::End), Char('G')],
        "End G",
        Some("keys.last"),
    ),
    bind(
        Action::Left,
        Global,
        &[Code(KeyCode::Left), Char('h')],
        "← h",
        Some("keys.left"),
    ),
    bind(
        Action::Right,
        Global,
        &[Code(KeyCode::Right), Char('l')],
        "→ l",
        Some("keys.right"),
    ),
    bind(
        Action::Enter,
        Global,
        &[Code(KeyCode::Enter)],
        "Enter",
        Some("keys.enter"),
    ),
    bind(
        Action::Esc,
        Global,
        &[Code(KeyCode::Esc)],
        "Esc",
        Some("keys.esc"),
    ),
    bind(
        Action::Filter,
        Global,
        &[Char('/')],
        "/",
        Some("keys.filter"),
    ),
    bind(Action::Sort, Global, &[Char('s')], "s", Some("keys.sort")),
    bind(
        Action::SortReverse,
        Global,
        &[Char('S')],
        "S",
        Some("keys.sort-reverse"),
    ),
    bind(
        Action::Refresh,
        Global,
        &[Char('r')],
        "r",
        Some("keys.refresh"),
    ),
    bind(Action::Pause, Global, &[Char('p')], "p", Some("keys.pause")),
    bind(
        Action::Palette,
        Global,
        &[Char(':')],
        ":",
        Some("keys.palette"),
    ),
    bind(
        Action::Language,
        Global,
        &[Char('L')],
        "L",
        Some("keys.language"),
    ),
    bind(
        Action::Help,
        Global,
        &[Char('?'), Code(KeyCode::F(1))],
        "? F1",
        Some("keys.help"),
    ),
    bind(
        Action::Quit,
        Global,
        &[Char('q'), Ctrl('c')],
        "q Ctrl+c",
        Some("keys.quit"),
    ),
    bind(
        Action::Submit,
        Global,
        &[Ctrl('s')],
        "Ctrl+s",
        Some("keys.submit"),
    ),
    bind(
        Action::Detail,
        Cluster,
        &[Code(KeyCode::Enter)],
        "Enter",
        Some("keys.cluster.detail"),
    ),
    bind(
        Action::Drain,
        Cluster,
        &[Char('d')],
        "d",
        Some("keys.cluster.drain"),
    ),
    bind(
        Action::Return,
        Cluster,
        &[Char('u')],
        "u",
        Some("keys.cluster.return"),
    ),
    bind(
        Action::FlushNode,
        Cluster,
        &[Char('b')],
        "b",
        Some("keys.cluster.flush-node"),
    ),
    bind(
        Action::FlushAll,
        Cluster,
        &[Char('B')],
        "B",
        Some("keys.cluster.flush-all"),
    ),
    bind(
        Action::StopCluster,
        Cluster,
        &[Char('S')],
        "S",
        Some("keys.cluster.stop"),
    ),
    bind(
        Action::Monitor,
        Cluster,
        &[Char('m')],
        "m",
        Some("keys.cluster.monitor"),
    ),
    bind(
        Action::WindowWider,
        Monitor,
        &[Char('+'), Char('=')],
        "+",
        Some("keys.monitor.wider"),
    ),
    bind(
        Action::WindowNarrower,
        Monitor,
        &[Char('-'), Char('_')],
        "-",
        Some("keys.monitor.narrower"),
    ),
    bind(
        Action::PauseChart,
        Monitor,
        &[Char(' ')],
        "Space",
        Some("keys.monitor.pause"),
    ),
    bind(
        Action::Scale,
        Monitor,
        &[Char('z')],
        "z",
        Some("keys.monitor.scale"),
    ),
    bind(
        Action::Export,
        Monitor,
        &[Char('e')],
        "e",
        Some("keys.monitor.export"),
    ),
];

/// The action of a key in a screen: its own scope first, then the global one.
pub fn resolve(scope: Scope, event: &KeyEvent) -> Option<Action> {
    let find = |wanted: Scope| {
        BINDINGS
            .iter()
            .filter(|b| b.scope == wanted)
            .find(|b| b.keys.iter().any(|k| k.matches(event)))
            .map(|b| b.action)
    };
    find(scope).or_else(|| {
        if scope == Scope::Global {
            None
        } else {
            find(Scope::Global)
        }
    })
}

/// What a scope's help lists: the scope's own bindings, then the global ones that are not shadowed.
pub fn help_for(scope: Scope) -> Vec<&'static Binding> {
    let own = BINDINGS
        .iter()
        .filter(|b| b.scope == scope && b.help.is_some());
    let shadowed = |g: &Binding| {
        BINDINGS.iter().any(|b| {
            b.scope == scope
                && b.scope != Scope::Global
                && b.keys.iter().any(|k| g.keys.contains(k))
        })
    };
    let global = BINDINGS.iter().filter(|b| {
        b.scope == Scope::Global && b.help.is_some() && !(scope != Scope::Global && shadowed(b))
    });
    own.chain(global).collect()
}

/// The first binding of an action, for the footer.
pub fn binding_of(action: Action) -> Option<&'static Binding> {
    BINDINGS.iter().find(|b| b.action == action)
}

#[cfg(test)]
mod tests;
