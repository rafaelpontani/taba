//! The interactive loop (doc 34, section 4): prepares the terminal (and gives it back on exit and on panic), turns
//! keys, the clock and the workers' replies into `Msg`s, runs `update`, carries out its effects and draws.

use std::io;
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ratatui::crossterm::event::{
    self, DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste,
    EnableFocusChange, EnableMouseCapture, Event, KeyEventKind, MouseEventKind,
};
use ratatui::crossterm::execute;

use crate::profile::{ConfigFile, Profile};
use crate::tui::effect::Effect;
use crate::tui::i18n::Lang;
use crate::tui::model::{Model, Options};
use crate::tui::msg::Msg;
use crate::tui::update::update;
use crate::tui::view::view;
use crate::tui::worker::{Backend, SwitchBackend, Workers, THREADS};

/// The clock tick of the interface.
const TICK: Duration = Duration::from_millis(250);
/// How long to wait for a key before looking at the workers again.
const POLL: Duration = Duration::from_millis(40);

/// What the loop asks of the world besides the network.
pub trait Host {
    fn save_language(&mut self, lang: Lang) -> Result<(), String>;
    /// Writes a new file (never over an existing one).
    fn write_file(&mut self, path: &str, contents: &str) -> Result<(), String>;
    /// Writes a new file that only its owner can read (a token secret); never over an existing one.
    fn write_secret(&mut self, path: &str, contents: &str) -> Result<(), String>;
    /// Writes the default profile and the profiles, keeping the rest of the file as it is on disk.
    fn save_profiles(&mut self, file: &ConfigFile) -> Result<(), String>;
    /// Tries the connection of a profile that may not be saved.
    fn check_profile(&mut self, profile: &Profile) -> crate::tui::msg::CheckResult;
    /// What the calls of a profile of the file go through, and the address it ends up using.
    fn connect(&mut self, name: &str) -> Result<Connection, String>;
}

/// A connection to a server, made with a profile.
pub struct Connection {
    pub backend: Arc<dyn Backend>,
    pub url: String,
}

/// Gives the terminal back as it was, also when the loop ends by an error or a panic unwinds.
struct TerminalGuard {
    mouse: bool,
}

impl TerminalGuard {
    fn enter(mouse: bool) -> TerminalGuard {
        let mut out = io::stdout();
        if mouse {
            let _ = execute!(out, EnableMouseCapture);
        }
        let _ = execute!(out, EnableBracketedPaste, EnableFocusChange);
        TerminalGuard { mouse }
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let mut out = io::stdout();
        if self.mouse {
            let _ = execute!(out, DisableMouseCapture);
        }
        let _ = execute!(out, DisableBracketedPaste, DisableFocusChange);
    }
}

fn wall_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(0))
}

/// Carries out the effects of an `update`. Returns `false` when the app must quit.
fn carry_out(
    effects: Vec<Effect>,
    model: &mut Model,
    workers: &Workers,
    switch: &SwitchBackend,
    host: &mut dyn Host,
) -> bool {
    let mut keep_going = true;
    for effect in effects {
        match effect {
            Effect::Call {
                id,
                generation,
                call,
            } => workers.submit(id, generation, call),
            Effect::Quit => keep_going = false,
            Effect::WriteFile { path, contents } => {
                let result = host.write_file(&path, &contents);
                let more = update(model, Msg::FileWritten { path, result });
                keep_going &= carry_out(more, model, workers, switch, host);
            }
            Effect::SaveLanguage(lang) => {
                let result = host.save_language(lang);
                let more = update(model, Msg::LanguageSaved(result));
                keep_going &= carry_out(more, model, workers, switch, host);
            }
            Effect::WriteSecret { path, contents } => {
                let result = host.write_secret(&path, &contents);
                let more = update(model, Msg::FileWritten { path, result });
                keep_going &= carry_out(more, model, workers, switch, host);
            }
            Effect::Copy(text) => copy_to_clipboard(&text),
            Effect::SaveProfiles { file, select } => {
                let result = host.save_profiles(&file);
                let more = update(
                    model,
                    Msg::ProfilesSaved {
                        file,
                        select,
                        result,
                    },
                );
                keep_going &= carry_out(more, model, workers, switch, host);
            }
            Effect::CheckProfile(profile) => {
                let result = host.check_profile(&profile);
                let more = update(model, Msg::ProfileChecked(result));
                keep_going &= carry_out(more, model, workers, switch, host);
            }
            Effect::Connect(name) => {
                let (url, result) = match host.connect(&name) {
                    Ok(connection) => {
                        switch.switch(connection.backend);
                        (connection.url, Ok(()))
                    }
                    Err(e) => (String::new(), Err(e)),
                };
                let more = update(
                    model,
                    Msg::Connected {
                        profile: name,
                        url,
                        result,
                    },
                );
                keep_going &= carry_out(more, model, workers, switch, host);
            }
        }
    }
    keep_going
}

/// Puts text on the clipboard with the OSC 52 sequence; a terminal that does not support it ignores it.
fn copy_to_clipboard(text: &str) {
    use std::io::Write;
    let mut out = io::stdout();
    let _ = write!(
        out,
        "]52;c;{}",
        taba_client::base64::encode(text.as_bytes())
    );
    let _ = out.flush();
}

/// Runs the interface until the person quits.
pub fn run(
    backend: Arc<dyn Backend>,
    options: Options,
    mouse: bool,
    host: &mut dyn Host,
) -> io::Result<()> {
    let mut terminal = ratatui::init();
    let _guard = TerminalGuard::enter(mouse);
    let result = event_loop(&mut terminal, backend, options, host);
    ratatui::restore();
    result
}

fn event_loop(
    terminal: &mut ratatui::DefaultTerminal,
    backend: Arc<dyn Backend>,
    options: Options,
    host: &mut dyn Host,
) -> io::Result<()> {
    let (results, replies) = mpsc::channel::<Msg>();
    let switch = Arc::new(SwitchBackend::new(backend));
    let workers = Workers::start(Arc::clone(&switch) as Arc<dyn Backend>, results, THREADS);
    let mut model = Model::new(options);
    let size = terminal.size()?;
    model.size = (size.width, size.height);
    let started = Instant::now();
    let mut last_tick: Option<Instant> = None;
    let mut dirty = true;
    loop {
        let mut messages: Vec<Msg> = Vec::new();
        if event::poll(POLL)? {
            match event::read()? {
                Event::Key(key) if key.kind != KeyEventKind::Release => {
                    messages.push(Msg::Key(key))
                }
                Event::Resize(width, height) => messages.push(Msg::Resize(width, height)),
                Event::Paste(text) => messages.push(Msg::Paste(text)),
                Event::FocusGained => messages.push(Msg::Focus(true)),
                Event::FocusLost => messages.push(Msg::Focus(false)),
                Event::Mouse(mouse) => match mouse.kind {
                    MouseEventKind::ScrollUp => messages.push(Msg::Scroll(-3)),
                    MouseEventKind::ScrollDown => messages.push(Msg::Scroll(3)),
                    _ => {}
                },
                _ => {}
            }
        }
        while let Ok(reply) = replies.try_recv() {
            messages.push(reply);
        }
        if last_tick.is_none_or(|t| t.elapsed() >= TICK) {
            last_tick = Some(Instant::now());
            messages.push(Msg::Tick {
                now: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
                wall_ms: wall_ms(),
            });
        }
        dirty |= !messages.is_empty();
        for message in messages {
            let effects = update(&mut model, message);
            if !carry_out(effects, &mut model, &workers, &switch, host) {
                return Ok(());
            }
        }
        if dirty {
            terminal.draw(|frame| view(frame, &model))?;
            dirty = false;
        }
    }
}

#[cfg(test)]
mod tests;
