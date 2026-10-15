//! A profile's form: text fields, the `output` choice, the "Default profile" box and the
//! buttons. A pure state machine: receives keys and draws lines, without touching the terminal.

use super::keys::Key;
use super::{Frame, Style};
use crate::profile::{self, Profile, DEFAULT_URL, FIELDS, OUTPUTS};

pub const NAME: usize = 0;
pub const OUTPUT: usize = 6;
pub const DEFAULT: usize = 10;
pub const SAVE: usize = 11;
pub const TEST: usize = 12;
pub const CANCEL: usize = 13;
const ITEMS: usize = 14;
const LABEL_WIDTH: usize = 18;

const LABELS: [&str; 11] = [
    "Name",
    "url",
    "ops-url",
    "token-file",
    "token",
    "ca-file",
    "output",
    "connect-timeout",
    "timeout",
    "max-attempts",
    "Default profile",
];

#[derive(Debug, Clone, Default)]
struct Text {
    value: String,
    /// Cursor position, in characters.
    cursor: usize,
}

impl Text {
    fn from(value: &str) -> Text {
        Text {
            value: value.to_owned(),
            cursor: value.chars().count(),
        }
    }

    fn byte_at(&self, chars: usize) -> usize {
        self.value
            .char_indices()
            .nth(chars)
            .map_or(self.value.len(), |(i, _)| i)
    }

    fn insert(&mut self, c: char) {
        let at = self.byte_at(self.cursor);
        self.value.insert(at, c);
        self.cursor += 1;
    }

    fn backspace(&mut self) -> bool {
        if self.cursor == 0 {
            return false;
        }
        let at = self.byte_at(self.cursor - 1);
        self.value.remove(at);
        self.cursor -= 1;
        true
    }

    fn delete(&mut self) -> bool {
        if self.cursor >= self.value.chars().count() {
            return false;
        }
        let at = self.byte_at(self.cursor);
        self.value.remove(at);
        true
    }
}

/// What the form asks of whoever drives it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormEvent {
    None,
    Save,
    Test,
    Cancel,
    Quit,
}

#[derive(Debug, Clone)]
pub struct Form {
    /// The name of the profile being edited; `None` on a new profile.
    pub original: Option<String>,
    texts: Vec<Text>,
    /// 0 = no value (the built-in one), then `json`, `table`, `raw`.
    output: usize,
    pub is_default: bool,
    focus: usize,
    /// The result line (connection test, save error), with `true` if it is an error.
    pub status: Option<(bool, String)>,
    dirty: bool,
    confirm_discard: bool,
    taken_names: Vec<String>,
}

impl Form {
    /// A new form (`existing` empty) or filled in with a profile. `taken_names` are the
    /// names that already exist, to refuse a repeated one.
    pub fn new(
        existing: Option<(&str, &Profile)>,
        is_default: bool,
        taken_names: Vec<String>,
    ) -> Form {
        let mut texts = vec![Text::default(); 10];
        let mut output = 0;
        if let Some((name, profile)) = existing {
            texts[NAME] = Text::from(name);
            for (n, field) in FIELDS.iter().enumerate() {
                if let Some(value) = profile.get(field) {
                    if *field == "output" {
                        output = OUTPUTS
                            .iter()
                            .position(|o| *o == value)
                            .map_or(0, |p| p + 1);
                    } else {
                        texts[n + 1] = Text::from(value);
                    }
                }
            }
        }
        Form {
            original: existing.map(|(n, _)| n.to_owned()),
            texts,
            output,
            is_default,
            focus: NAME,
            status: None,
            dirty: false,
            confirm_discard: false,
            taken_names,
        }
    }

    pub fn name(&self) -> &str {
        self.texts[NAME].value.trim()
    }

    pub fn dirty(&self) -> bool {
        self.dirty
    }

    fn value(&self, item: usize) -> String {
        if item == OUTPUT {
            return match self.output {
                0 => String::new(),
                n => OUTPUTS[n - 1].to_owned(),
            };
        }
        self.texts[item].value.trim().to_owned()
    }

    /// The profile with the fields filled in (empty ones are left out). Ignores invalid fields.
    pub fn profile(&self) -> Profile {
        let mut profile = Profile::default();
        for (n, field) in FIELDS.iter().enumerate() {
            let value = self.value(n + 1);
            if !value.is_empty() {
                let _ = profile.set(field, &value);
            }
        }
        profile
    }

    /// The error message of each item (`None` = valid).
    pub fn errors(&self) -> Vec<Option<String>> {
        let mut errors = vec![None; ITEMS];
        let name = self.name();
        errors[NAME] = if name.is_empty() {
            Some("the name is required".to_owned())
        } else if let Err(e) = profile::validate_name(name) {
            Some(e)
        } else if self.original.as_deref() != Some(name)
            && self.taken_names.iter().any(|n| n == name)
        {
            Some(format!("a profile `{name}` already exists"))
        } else {
            None
        };
        for (n, field) in FIELDS.iter().enumerate() {
            let value = self.value(n + 1);
            if !value.is_empty() {
                errors[n + 1] = profile::validate_field(field, &value).err();
            }
        }
        errors
    }

    pub fn is_valid(&self) -> bool {
        self.errors().iter().all(Option::is_none)
    }

    fn touched(&mut self) {
        self.dirty = true;
        self.status = None;
    }

    fn move_focus(&mut self, delta: isize) {
        self.focus = (self.focus as isize + delta).rem_euclid(ITEMS as isize) as usize;
    }

    pub fn handle(&mut self, key: Key) -> FormEvent {
        if self.confirm_discard {
            self.confirm_discard = false;
            self.status = None;
            if key == Key::Esc {
                return FormEvent::Cancel;
            }
        }
        match key {
            Key::Ctrl('c') => return FormEvent::Quit,
            Key::Ctrl('s') => return FormEvent::Save,
            Key::Esc => {
                if self.dirty {
                    self.confirm_discard = true;
                    self.status = Some((
                        true,
                        "There are unsaved changes. Esc again discards; any other key goes back."
                            .to_owned(),
                    ));
                    return FormEvent::None;
                }
                return FormEvent::Cancel;
            }
            Key::Up | Key::BackTab => self.move_focus(-1),
            Key::Down | Key::Tab => self.move_focus(1),
            _ => {}
        }
        match self.focus {
            SAVE | TEST | CANCEL => match key {
                Key::Enter | Key::Char(' ') => {
                    return match self.focus {
                        SAVE => FormEvent::Save,
                        TEST => FormEvent::Test,
                        _ => self.cancel_or_confirm(),
                    };
                }
                Key::Left => self.move_focus(-1),
                Key::Right => self.focus = (self.focus + 1).min(CANCEL),
                _ => {}
            },
            DEFAULT => match key {
                Key::Char(' ') | Key::Left | Key::Right => {
                    self.is_default = !self.is_default;
                    self.touched();
                }
                Key::Enter => self.move_focus(1),
                _ => {}
            },
            OUTPUT => match key {
                Key::Left => {
                    self.output = (self.output + OUTPUTS.len()) % (OUTPUTS.len() + 1);
                    self.touched();
                }
                Key::Right | Key::Char(' ') => {
                    self.output = (self.output + 1) % (OUTPUTS.len() + 1);
                    self.touched();
                }
                Key::Enter => self.move_focus(1),
                _ => {}
            },
            item => {
                let text = &mut self.texts[item];
                let changed = match key {
                    Key::Char(c) => {
                        text.insert(c);
                        true
                    }
                    Key::Backspace => text.backspace(),
                    Key::Delete => text.delete(),
                    Key::Left => {
                        text.cursor = text.cursor.saturating_sub(1);
                        false
                    }
                    Key::Right => {
                        text.cursor = (text.cursor + 1).min(text.value.chars().count());
                        false
                    }
                    Key::Home => {
                        text.cursor = 0;
                        false
                    }
                    Key::End => {
                        text.cursor = text.value.chars().count();
                        false
                    }
                    Key::Ctrl('u') => {
                        let had = !text.value.is_empty();
                        *text = Text::default();
                        had
                    }
                    Key::Enter => {
                        self.move_focus(1);
                        false
                    }
                    _ => false,
                };
                if changed {
                    self.touched();
                }
            }
        }
        FormEvent::None
    }

    fn cancel_or_confirm(&mut self) -> FormEvent {
        if self.dirty {
            self.confirm_discard = true;
            self.status = Some((
                true,
                "There are unsaved changes. Esc again discards; any other key goes back."
                    .to_owned(),
            ));
            FormEvent::None
        } else {
            FormEvent::Cancel
        }
    }

    /// The value that will apply when the field is empty (or `None` if there is none).
    fn placeholder(&self, item: usize) -> Option<String> {
        match item {
            1 => Some(DEFAULT_URL.to_owned()),
            2 => {
                let url = self.value(1);
                Some(if url.is_empty() {
                    DEFAULT_URL.to_owned()
                } else {
                    url
                })
            }
            3 | 4 => Some("none".to_owned()),
            5 => Some("system roots".to_owned()),
            6 => Some("json".to_owned()),
            7 => Some("5".to_owned()),
            8 => Some("30".to_owned()),
            9 => Some("3".to_owned()),
            _ => None,
        }
    }

    pub fn render(&self, style: Style, width: usize, height: usize) -> Frame {
        let errors = self.errors();
        let title = match &self.original {
            Some(name) => format!(" Profile  {name}"),
            None => " New profile".to_owned(),
        };
        let mut lines = vec![style.paint("1", &title), String::new()];
        let mut focus_line = 0;
        let mut cursor = None;
        for item in 0..=DEFAULT {
            let focused = self.focus == item;
            let marker = if focused {
                style.paint("36", " ▸ ")
            } else {
                "   ".to_owned()
            };
            let label = format!("{:<LABEL_WIDTH$}", LABELS[item]);
            let label = if focused {
                style.paint("1;36", &label)
            } else {
                label
            };
            let shown = match item {
                OUTPUT => {
                    let current = match self.output {
                        0 => style.paint("2", &format!("({})", OUTPUTS[0])),
                        n => OUTPUTS[n - 1].to_owned(),
                    };
                    format!("‹ {current} ›")
                }
                DEFAULT => format!("[{}]", if self.is_default { "x" } else { " " }),
                _ => {
                    let text = &self.texts[item];
                    if text.value.is_empty() {
                        match self.placeholder(item) {
                            Some(p) => style.paint("2", &format!("({p})")),
                            None => String::new(),
                        }
                    } else {
                        text.value.clone()
                    }
                }
            };
            if focused {
                focus_line = lines.len();
                if item != OUTPUT && item != DEFAULT {
                    cursor = Some((lines.len(), 3 + LABEL_WIDTH + self.texts[item].cursor));
                }
            }
            lines.push(format!("{marker}{label}{shown}"));
            if let Some(message) = &errors[item] {
                lines.push(format!(
                    "{}{}",
                    " ".repeat(3 + LABEL_WIDTH),
                    style.paint("31", message)
                ));
            }
        }
        lines.push(String::new());
        let button = |item: usize, text: &str| {
            if self.focus == item {
                if style.color {
                    style.paint("7", &format!("[ {text} ]"))
                } else {
                    format!("[>{text}<]")
                }
            } else {
                format!("[ {text} ]")
            }
        };
        if matches!(self.focus, SAVE | TEST | CANCEL) {
            focus_line = lines.len();
        }
        lines.push(format!(
            "   {}  {}  {}",
            button(SAVE, "Save"),
            button(TEST, "Test connection"),
            button(CANCEL, "Cancel")
        ));
        if let Some((is_error, message)) = &self.status {
            lines.push(String::new());
            lines.push(format!(
                "   {}",
                style.paint(if *is_error { "31" } else { "32" }, message)
            ));
        }
        let footer = " ↑↓/Tab field   ←→ option   Space toggle   Ctrl+S save   Esc cancel";
        window(lines, focus_line, cursor, footer, style, width, height)
    }
}

/// Trims the lines to fit the height, keeping the focused line in view, and adds the footer.
pub fn window(
    lines: Vec<String>,
    focus_line: usize,
    cursor: Option<(usize, usize)>,
    footer: &str,
    style: Style,
    _width: usize,
    height: usize,
) -> Frame {
    let body = height.saturating_sub(2).max(1);
    let start = if lines.len() <= body {
        0
    } else {
        focus_line.saturating_sub(body - 1).min(lines.len() - body)
    };
    let mut shown: Vec<String> = lines.into_iter().skip(start).take(body).collect();
    while shown.len() < body {
        shown.push(String::new());
    }
    shown.push(String::new());
    shown.push(style.paint("2", footer));
    Frame {
        cursor: cursor.and_then(|(line, col)| {
            line.checked_sub(start)
                .filter(|l| *l < body)
                .map(|l| (l, col))
        }),
        lines: shown,
    }
}

#[cfg(test)]
mod tests;
