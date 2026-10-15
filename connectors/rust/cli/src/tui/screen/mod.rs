//! The profiles screen: the list and, over it, the form. Also a pure state machine;
//! saving the file and testing the connection are left to whoever drives it (`Services`).

use super::form::{window, Form, FormEvent};
use super::keys::Key;
use super::{Frame, Style};
use crate::profile::{ConfigFile, Profile};

pub const MIN_WIDTH: usize = 60;
pub const MIN_HEIGHT: usize = 16;

/// What the screen needs from the outside world.
pub trait Services {
    /// Writes the whole file.
    fn save(&mut self, file: &ConfigFile) -> Result<(), String>;
    /// Tests the connection with a profile that may not be saved yet; returns the result as text.
    fn check(&mut self, profile: &Profile) -> Result<String, String>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    Continue,
    Quit,
}

enum Mode {
    List,
    /// Waiting for the `y` or `N` of "Remove?".
    ConfirmRemove(String),
    Form(Box<Form>),
}

pub struct App {
    file: ConfigFile,
    path: String,
    session: Option<String>,
    selected: usize,
    mode: Mode,
    status: Option<(bool, String)>,
}

impl App {
    /// `session` is the profile `TABA_CLI_PROFILE` chose in this terminal.
    pub fn new(file: ConfigFile, path: String, session: Option<String>) -> App {
        App {
            file,
            path,
            session,
            selected: 0,
            mode: Mode::List,
            status: None,
        }
    }

    pub fn file(&self) -> &ConfigFile {
        &self.file
    }

    fn selected_name(&self) -> Option<String> {
        self.file
            .profiles
            .get(self.selected)
            .map(|(n, _)| n.clone())
    }

    pub fn handle(&mut self, key: Key, services: &mut dyn Services) -> Flow {
        if key == Key::Ctrl('c') {
            return Flow::Quit;
        }
        match std::mem::replace(&mut self.mode, Mode::List) {
            Mode::List => self.handle_list(key, services),
            Mode::ConfirmRemove(name) => {
                if matches!(key, Key::Char('y' | 'Y')) {
                    self.remove(&name, services);
                } else {
                    self.status = Some((false, "Nothing was removed.".to_owned()));
                }
                Flow::Continue
            }
            Mode::Form(mut form) => {
                let event = form.handle(key);
                self.mode = Mode::Form(form);
                self.handle_form_event(event, services)
            }
        }
    }

    fn handle_list(&mut self, key: Key, services: &mut dyn Services) -> Flow {
        let count = self.file.profiles.len();
        match key {
            Key::Up | Key::Char('k') => self.selected = self.selected.saturating_sub(1),
            Key::Down | Key::Char('j') => {
                self.selected = (self.selected + 1).min(count.saturating_sub(1))
            }
            Key::Home => self.selected = 0,
            Key::End => self.selected = count.saturating_sub(1),
            Key::Enter => {
                if let Some((name, profile)) = self.file.profiles.get(self.selected) {
                    let is_default = self.file.default.as_deref() == Some(name.as_str());
                    let taken = self.file.names().iter().map(|n| n.to_string()).collect();
                    self.mode = Mode::Form(Box::new(Form::new(
                        Some((name, profile)),
                        is_default,
                        taken,
                    )));
                    self.status = None;
                }
            }
            Key::Char('n' | 'N') => {
                let taken = self.file.names().iter().map(|n| n.to_string()).collect();
                let first = self.file.profiles.is_empty();
                self.mode = Mode::Form(Box::new(Form::new(None, first, taken)));
                self.status = None;
            }
            Key::Char('d' | 'D') => {
                if let Some(name) = self.selected_name() {
                    let mut next = self.file.clone();
                    if next.set_default(&name).is_ok() {
                        self.persist(next, format!("`{name}` is the default profile."), services);
                    }
                }
            }
            Key::Delete | Key::Char('r' | 'R') => {
                if let Some(name) = self.selected_name() {
                    self.mode = Mode::ConfirmRemove(name);
                    self.status = None;
                }
            }
            Key::Char('q' | 'Q') | Key::Esc | Key::Ctrl('d') => return Flow::Quit,
            _ => {}
        }
        Flow::Continue
    }

    /// Saves `next`; if it works it becomes the screen's file, if not the previous one stays as it was.
    fn persist(
        &mut self,
        next: ConfigFile,
        ok_message: String,
        services: &mut dyn Services,
    ) -> bool {
        match services.save(&next) {
            Ok(()) => {
                self.file = next;
                self.selected = self
                    .selected
                    .min(self.file.profiles.len().saturating_sub(1));
                self.status = Some((false, ok_message));
                true
            }
            Err(e) => {
                self.status = Some((true, e));
                false
            }
        }
    }

    fn remove(&mut self, name: &str, services: &mut dyn Services) {
        let mut next = self.file.clone();
        if next.remove(name).is_ok() {
            self.persist(next, format!("Profile `{name}` removed."), services);
        }
    }

    fn handle_form_event(&mut self, event: FormEvent, services: &mut dyn Services) -> Flow {
        match event {
            FormEvent::None => Flow::Continue,
            FormEvent::Quit => Flow::Quit,
            FormEvent::Cancel => {
                self.mode = Mode::List;
                Flow::Continue
            }
            FormEvent::Test => {
                if let Mode::Form(form) = &mut self.mode {
                    form.status = Some(if form.is_valid() {
                        match services.check(&form.profile()) {
                            Ok(message) => (false, message),
                            Err(message) => (true, message),
                        }
                    } else {
                        (true, "Fix the fields in red before testing.".to_owned())
                    });
                }
                Flow::Continue
            }
            FormEvent::Save => {
                let Mode::Form(form) = &mut self.mode else {
                    return Flow::Continue;
                };
                if !form.is_valid() {
                    form.status = Some((true, "Fix the fields in red before saving.".to_owned()));
                    return Flow::Continue;
                }
                let name = form.name().to_owned();
                let (original, profile, is_default) =
                    (form.original.clone(), form.profile(), form.is_default);
                let mut next = self.file.clone();
                let applied = match &original {
                    None => next.add(&name, profile),
                    Some(old) => rename_and_replace(&mut next, old, &name, profile),
                };
                if let Err(e) = applied {
                    if let Mode::Form(form) = &mut self.mode {
                        form.status = Some((true, e));
                    }
                    return Flow::Continue;
                }
                if is_default {
                    next.default = Some(name.clone());
                } else if next.default.as_deref() == Some(name.as_str()) {
                    next.default = None;
                }
                match services.save(&next) {
                    Ok(()) => {
                        self.selected = next
                            .profiles
                            .iter()
                            .position(|(n, _)| *n == name)
                            .unwrap_or(0);
                        self.file = next;
                        self.mode = Mode::List;
                        self.status = Some((false, format!("Profile `{name}` saved.")));
                    }
                    Err(e) => {
                        if let Mode::Form(form) = &mut self.mode {
                            form.status = Some((true, e));
                        }
                    }
                }
                Flow::Continue
            }
        }
    }

    pub fn render(&self, style: Style, width: usize, height: usize) -> Frame {
        if width < MIN_WIDTH || height < MIN_HEIGHT {
            return Frame {
                lines: vec![
                    style.paint("1;31", "Terminal too small."),
                    format!("I need at least {MIN_WIDTH} columns by {MIN_HEIGHT} lines (now: {width}x{height})."),
                    "Enlarge the window. Ctrl+C quits.".to_owned(),
                ],
                cursor: None,
            };
        }
        match &self.mode {
            Mode::Form(form) => form.render(style, width, height),
            _ => self.render_list(style, width, height),
        }
    }

    fn render_list(&self, style: Style, width: usize, height: usize) -> Frame {
        let mut lines = vec![
            format!(
                " {}   {}",
                style.paint("1", "taba-cli profiles"),
                style.paint("2", &self.path)
            ),
            String::new(),
        ];
        if self.file.profiles.is_empty() {
            lines.push(
                "   There are no profiles. Without a profile the built-in default applies (http://127.0.0.1:8080)."
                    .to_owned(),
            );
            lines.push("   Press n to create the first one.".to_owned());
        } else {
            let name_w = self
                .file
                .profiles
                .iter()
                .map(|(n, _)| n.chars().count() + session_suffix(&self.session, n).chars().count())
                .max()
                .unwrap_or(4)
                .max(4);
            let url_w = (width.saturating_sub(name_w + 36)).clamp(12, 40);
            lines.push(style.paint(
                "2",
                &format!(
                    "   {:<2}{:<name_w$}  {:<url_w$}  {:<12}  default",
                    "", "name", "url", "token-file"
                ),
            ));
            for (n, (name, profile)) in self.file.profiles.iter().enumerate() {
                let mark = if self.file.default.as_deref() == Some(name.as_str()) {
                    "●"
                } else {
                    "○"
                };
                let label = format!("{name}{}", session_suffix(&self.session, name));
                let row = format!(
                    "{:<name_w$}  {:<url_w$}  {:<12}  {mark}",
                    label,
                    cut(profile.get("url").unwrap_or("—"), url_w),
                    cut(profile.get("token-file").unwrap_or("—"), 12),
                );
                if n == self.selected {
                    lines.push(format!(
                        " {}{}",
                        style.paint("36", "▶ "),
                        style.paint("1", &row)
                    ));
                } else {
                    lines.push(format!("   {row}"));
                }
            }
        }
        lines.push(String::new());
        if let Mode::ConfirmRemove(name) = &self.mode {
            lines.push(format!(
                "   {}",
                style.paint("33", &format!("Remove \"{name}\"? [y/N]"))
            ));
        } else if let Some((is_error, message)) = &self.status {
            lines.push(format!(
                "   {}",
                style.paint(if *is_error { "31" } else { "32" }, message)
            ));
        }
        let footer = " ↑↓ choose   Enter open   n new   d default   Del remove   q quit";
        window(lines, 0, None, footer, style, width, height)
    }
}

fn session_suffix(session: &Option<String>, name: &str) -> &'static str {
    if session.as_deref() == Some(name) {
        " (session)"
    } else {
        ""
    }
}

fn cut(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_owned()
    } else {
        format!(
            "{}…",
            text.chars().take(max.saturating_sub(1)).collect::<String>()
        )
    }
}

/// Replaces the profile `old` with `profile` under the name `name`, in the same place in the list, carrying along the
/// default if it was that one.
fn rename_and_replace(
    file: &mut ConfigFile,
    old: &str,
    name: &str,
    profile: Profile,
) -> Result<(), String> {
    let at = file
        .profiles
        .iter()
        .position(|(n, _)| n == old)
        .ok_or_else(|| format!("the profile `{old}` no longer exists"))?;
    if name != old && file.get(name).is_some() {
        return Err(format!("a profile `{name}` already exists"));
    }
    file.profiles[at] = (name.to_owned(), profile);
    if file.default.as_deref() == Some(old) {
        file.default = Some(name.to_owned());
    }
    Ok(())
}

#[cfg(test)]
mod tests;
