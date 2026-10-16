//! A small form of text fields and checkboxes (doc 34, sections 6.5 and 6.7): creating and editing a token, a
//! profile. It is plain state: the keys change it and say what the person asked for (`Outcome`); the screens decide
//! what that means, and the popup draws it.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Text,
    /// A text that is shown as dots (a token kept in a profile).
    Secret,
    Check,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    /// What the code that reads the form calls it.
    pub id: &'static str,
    /// The catalog key of its label.
    pub label: &'static str,
    pub kind: Kind,
    pub value: String,
    pub checked: bool,
    /// The catalog key of the reason the field cannot be changed, when it cannot.
    pub locked: Option<&'static str>,
}

impl Field {
    pub fn text(id: &'static str, label: &'static str, value: &str) -> Field {
        Field {
            id,
            label,
            kind: Kind::Text,
            value: value.to_owned(),
            checked: false,
            locked: None,
        }
    }

    pub fn secret(id: &'static str, label: &'static str, value: &str) -> Field {
        Field {
            kind: Kind::Secret,
            ..Field::text(id, label, value)
        }
    }

    pub fn check(id: &'static str, label: &'static str, checked: bool) -> Field {
        Field {
            kind: Kind::Check,
            checked,
            ..Field::text(id, label, "")
        }
    }

    pub fn locked(mut self, reason: &'static str) -> Field {
        self.locked = Some(reason);
        self
    }
}

/// What the form is for, with what the code needs to carry it out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Purpose {
    /// `clone_of` is the id and the name of the token being cloned.
    TokenCreate {
        clone_of: Option<(String, String)>,
    },
    /// The id, and the name and expiry the form started with (only what changed is sent).
    TokenEdit {
        id: String,
        name: String,
        expires: String,
    },
    ProfileNew,
    ProfileEdit {
        original: String,
    },
}

impl Purpose {
    /// The catalog key of the popup's title.
    pub fn title(&self) -> &'static str {
        match self {
            Purpose::TokenCreate { clone_of: None } => "editor.token-create",
            Purpose::TokenCreate { .. } => "editor.token-clone",
            Purpose::TokenEdit { .. } => "editor.token-edit",
            Purpose::ProfileNew => "editor.profile-new",
            Purpose::ProfileEdit { .. } => "editor.profile-edit",
        }
    }

    /// The catalog key of the line that explains the fields.
    pub fn help(&self) -> &'static str {
        if self.is_profile() {
            "editor.help-profile"
        } else {
            "editor.help-token"
        }
    }

    pub fn is_profile(&self) -> bool {
        matches!(self, Purpose::ProfileNew | Purpose::ProfileEdit { .. })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Editor {
    pub purpose: Purpose,
    pub fields: Vec<Field>,
    pub focus: usize,
    /// Why the last attempt to save did not work.
    pub error: Option<String>,
    /// The result of a test of the connection: `true` for a failure.
    pub note: Option<(bool, String)>,
    /// A call is running: the keys wait.
    pub busy: bool,
}

/// What a key asked of the form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    None,
    Cancel,
    Submit,
    Test,
}

impl Editor {
    pub fn new(purpose: Purpose, fields: Vec<Field>) -> Editor {
        let mut editor = Editor {
            purpose,
            fields,
            focus: 0,
            error: None,
            note: None,
            busy: false,
        };
        if editor.fields.first().is_some_and(|f| f.locked.is_some()) {
            editor.focus = editor.step(0, 1);
        }
        editor
    }

    pub fn field(&self, id: &str) -> Option<&Field> {
        self.fields.iter().find(|f| f.id == id)
    }

    pub fn text(&self, id: &str) -> &str {
        self.field(id).map_or("", |f| f.value.trim())
    }

    pub fn checked(&self, id: &str) -> bool {
        self.field(id).is_some_and(|f| f.checked)
    }

    /// Puts the focus on a field by its id (to point at what is wrong).
    pub fn focus_on(&mut self, id: &str) {
        if let Some(at) = self.fields.iter().position(|f| f.id == id) {
            self.focus = at;
        }
    }

    /// The next field that can be edited from `from`, going `delta` (1 or -1) and stopping at the ends.
    fn step(&self, from: usize, delta: i32) -> usize {
        let mut at = from;
        loop {
            let next = i64::try_from(at).unwrap_or(0) + i64::from(delta);
            if next < 0 || next >= i64::try_from(self.fields.len()).unwrap_or(0) {
                return from;
            }
            at = next as usize;
            if self.fields[at].locked.is_none() {
                return at;
            }
        }
    }

    fn is_last(&self) -> bool {
        self.step(self.focus, 1) == self.focus
    }

    /// Adds typed or pasted text to the focused field, without line breaks.
    pub fn insert(&mut self, text: &str) {
        if let Some(field) = self.fields.get_mut(self.focus) {
            if field.locked.is_none() && field.kind != Kind::Check {
                field.value.extend(text.chars().filter(|c| !c.is_control()));
                self.error = None;
            }
        }
    }

    pub fn handle(&mut self, event: &KeyEvent) -> Outcome {
        if self.busy {
            return Outcome::None;
        }
        let ctrl = event.modifiers.contains(KeyModifiers::CONTROL);
        match event.code {
            KeyCode::Esc => return Outcome::Cancel,
            KeyCode::Char('s' | 'S') if ctrl => return Outcome::Submit,
            KeyCode::Char('t' | 'T') if ctrl && self.purpose.is_profile() => return Outcome::Test,
            KeyCode::Tab | KeyCode::Down => self.focus = self.step(self.focus, 1),
            KeyCode::BackTab | KeyCode::Up => self.focus = self.step(self.focus, -1),
            KeyCode::Enter => {
                if self.is_last() {
                    return Outcome::Submit;
                }
                self.focus = self.step(self.focus, 1);
            }
            KeyCode::Backspace => {
                if let Some(field) = self.fields.get_mut(self.focus) {
                    if field.locked.is_none() && field.kind != Kind::Check {
                        field.value.pop();
                        self.error = None;
                    }
                }
            }
            KeyCode::Char('u') if ctrl => {
                if let Some(field) = self.fields.get_mut(self.focus) {
                    if field.locked.is_none() && field.kind != Kind::Check {
                        field.value.clear();
                    }
                }
            }
            KeyCode::Char(' ')
                if self
                    .fields
                    .get(self.focus)
                    .is_some_and(|f| f.kind == Kind::Check && f.locked.is_none()) =>
            {
                let field = &mut self.fields[self.focus];
                field.checked = !field.checked;
                self.error = None;
            }
            KeyCode::Char(c) if !ctrl => {
                let mut buffer = [0u8; 4];
                self.insert(c.encode_utf8(&mut buffer));
            }
            _ => {}
        }
        Outcome::None
    }
}

#[cfg(test)]
mod tests;
