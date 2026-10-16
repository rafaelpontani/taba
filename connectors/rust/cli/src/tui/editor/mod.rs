//! A small form of text fields and checkboxes (doc 34, sections 6.5 and 6.7): creating and editing a token, a
//! profile. It is plain state: the keys change it and say what the person asked for (`Outcome`); the screens decide
//! what that means, and the popup draws it.

use ratatui::crossterm::event::{KeyCode, KeyEvent};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Text,
    /// A text that is shown as dots (a token kept in a profile).
    Secret,
    Check,
    /// Several lines, with a cursor that moves (the body of a message).
    Area,
}

/// How many lines an area shows at once.
pub const AREA_ROWS: usize = 7;

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
    /// Where the cursor is, in characters from the start; only an area uses it.
    pub cursor: usize,
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
            cursor: 0,
        }
    }

    /// A text of several lines, with the cursor at its end.
    pub fn area(id: &'static str, label: &'static str, value: &str) -> Field {
        Field {
            kind: Kind::Area,
            cursor: value.chars().count(),
            ..Field::text(id, label, value)
        }
    }

    /// The cursor as a line and a column (both from 0, in characters).
    pub fn line_col(&self) -> (usize, usize) {
        let before: String = self.value.chars().take(self.cursor).collect();
        let line = before.matches('\n').count();
        let col = before.rsplit('\n').next().map_or(0, |l| l.chars().count());
        (line, col)
    }

    fn lines(&self) -> Vec<&str> {
        self.value.split('\n').collect()
    }

    /// Puts the cursor on a line and a column, clipped to what exists.
    fn set_line_col(&mut self, line: usize, col: usize) {
        let lines = self.lines();
        let line = line.min(lines.len() - 1);
        let before: usize = lines.iter().take(line).map(|l| l.chars().count() + 1).sum();
        self.cursor = before + col.min(lines[line].chars().count());
    }

    fn byte_at(&self, chars: usize) -> usize {
        self.value
            .char_indices()
            .nth(chars)
            .map_or(self.value.len(), |(at, _)| at)
    }

    fn insert_at_cursor(&mut self, text: &str) {
        let at = self.byte_at(self.cursor);
        self.value.insert_str(at, text);
        self.cursor += text.chars().count();
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
    TopicCreate,
    /// The topic, and the text each editable field started with (only what changed is sent).
    TopicEdit {
        name: String,
        was: Vec<(&'static str, String)>,
    },
    ProfileNew,
    ProfileEdit {
        original: String,
    },
    /// A message to publish to the topic.
    MessagePublish {
        topic: String,
    },
    /// Reading messages of a topic: how many, and how long they stay invisible to the other consumers.
    MessageReceive {
        topic: String,
    },
    /// A held message: for how long it stays invisible from now on.
    MessageVisibility {
        topic: String,
        id: String,
        receipt: String,
    },
}

impl Purpose {
    /// The catalog key of the popup's title.
    pub fn title(&self) -> &'static str {
        match self {
            Purpose::TokenCreate { clone_of: None } => "editor.token-create",
            Purpose::TokenCreate { .. } => "editor.token-clone",
            Purpose::TokenEdit { .. } => "editor.token-edit",
            Purpose::TopicCreate => "editor.topic-create",
            Purpose::TopicEdit { .. } => "editor.topic-edit",
            Purpose::ProfileNew => "editor.profile-new",
            Purpose::ProfileEdit { .. } => "editor.profile-edit",
            Purpose::MessagePublish { .. } => "editor.message-publish",
            Purpose::MessageReceive { .. } => "editor.message-receive",
            Purpose::MessageVisibility { .. } => "editor.message-visibility",
        }
    }

    /// The catalog key of the line that explains the fields.
    pub fn help(&self) -> &'static str {
        if self.is_profile() {
            "editor.help-profile"
        } else if matches!(self, Purpose::MessageReceive { .. }) {
            "editor.help-receive"
        } else if matches!(self, Purpose::MessageVisibility { .. }) {
            "editor.help-visibility"
        } else if self.is_message() {
            "editor.help-publish"
        } else if self.is_topic() {
            "editor.help-topic"
        } else {
            "editor.help-token"
        }
    }

    pub fn is_topic(&self) -> bool {
        matches!(self, Purpose::TopicCreate | Purpose::TopicEdit { .. })
    }

    pub fn is_message(&self) -> bool {
        matches!(
            self,
            Purpose::MessagePublish { .. }
                | Purpose::MessageReceive { .. }
                | Purpose::MessageVisibility { .. }
        )
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

    /// Adds typed or pasted text to the focused field. A line field drops the line breaks; an area keeps them.
    pub fn insert(&mut self, text: &str) {
        if let Some(field) = self.fields.get_mut(self.focus) {
            if field.locked.is_some() || field.kind == Kind::Check {
                return;
            }
            if field.kind == Kind::Area {
                let text = text.replace("\r\n", "\n").replace('\r', "\n");
                let clean: String = text
                    .chars()
                    .map(|c| if c == '\t' { ' ' } else { c })
                    .filter(|c| *c == '\n' || !c.is_control())
                    .collect();
                field.insert_at_cursor(&clean);
            } else {
                field.value.extend(text.chars().filter(|c| !c.is_control()));
            }
            self.error = None;
        }
    }

    /// The keys of the area that has the focus; `None` when the key is not for it.
    fn area_key(&mut self, event: &KeyEvent) -> Option<()> {
        let ctrl = crate::tui::keymap::ctrl_of(event);
        let at = self.focus;
        let field = self.fields.get_mut(at)?;
        if field.kind != Kind::Area || field.locked.is_some() {
            return None;
        }
        let (line, col) = field.line_col();
        let last = field.lines().len() - 1;
        match event.code {
            KeyCode::Enter => field.insert_at_cursor("\n"),
            KeyCode::Left => field.cursor = field.cursor.saturating_sub(1),
            KeyCode::Right => field.cursor = (field.cursor + 1).min(field.value.chars().count()),
            KeyCode::Home => field.set_line_col(line, 0),
            KeyCode::End => field.set_line_col(line, usize::MAX),
            KeyCode::Up if line > 0 => field.set_line_col(line - 1, col),
            KeyCode::Down if line < last => field.set_line_col(line + 1, col),
            // At the first or the last line the arrows leave the area, as in the other fields.
            KeyCode::Up | KeyCode::Down => return None,
            KeyCode::Backspace if field.cursor > 0 => {
                let to = field.byte_at(field.cursor);
                let from = field.byte_at(field.cursor - 1);
                field.value.replace_range(from..to, "");
                field.cursor -= 1;
            }
            KeyCode::Delete if field.cursor < field.value.chars().count() => {
                let from = field.byte_at(field.cursor);
                let to = field.byte_at(field.cursor + 1);
                field.value.replace_range(from..to, "");
            }
            KeyCode::Backspace | KeyCode::Delete => {}
            KeyCode::Char('u') if ctrl => {
                field.value.clear();
                field.cursor = 0;
            }
            KeyCode::Char(c) if !ctrl => {
                let mut buffer = [0u8; 4];
                field.insert_at_cursor(c.encode_utf8(&mut buffer));
            }
            _ => return None,
        }
        self.error = None;
        Some(())
    }

    pub fn handle(&mut self, event: &KeyEvent) -> Outcome {
        if self.busy {
            return Outcome::None;
        }
        let ctrl = crate::tui::keymap::ctrl_of(event);
        let sends = matches!(event.code, KeyCode::Char('s' | 'S')) && ctrl;
        if !sends && event.code != KeyCode::Esc && self.area_key(event).is_some() {
            return Outcome::None;
        }
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
