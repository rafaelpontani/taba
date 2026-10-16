//! Everything that can happen to the interface (doc 34, section 4): a key, a tick of the clock, a reply that came
//! back from a worker. `update` takes one at a time.

use ratatui::crossterm::event::KeyEvent;
use taba_client::{Error, Reply};

use crate::profile::ConfigFile;
use crate::tui::api::ApiCall;
use crate::tui::keymap::Action;

#[derive(Debug)]
pub enum Msg {
    Key(KeyEvent),
    /// The wheel: lines to scroll (negative is up).
    Scroll(i16),
    Resize(u16, u16),
    /// The terminal gained or lost the focus: the reading pauses while it is away.
    Focus(bool),
    /// The clock: milliseconds since the app opened, and the wall clock (UTC, ms since the epoch).
    Tick {
        now: u64,
        wall_ms: i64,
    },
    /// An action chosen in the command palette.
    Action(Action),
    /// A worker's answer, marked with the request and the generation it was asked in.
    Reply {
        id: u64,
        generation: u64,
        call: ApiCall,
        result: Result<Reply, Error>,
    },
    /// The result of writing a file the person asked for.
    FileWritten {
        path: String,
        result: Result<(), String>,
    },
    /// The result of saving the language.
    LanguageSaved(Result<(), String>),
    /// Text pasted into the terminal.
    Paste(String),
    /// The result of writing the profiles file; `select` is the profile to leave selected.
    ProfilesSaved {
        file: ConfigFile,
        select: Option<String>,
        result: Result<(), String>,
    },
    /// The result of testing a connection with the values of a form.
    ProfileChecked(CheckResult),
    /// The result of connecting with a profile.
    Connected {
        profile: String,
        url: String,
        result: Result<(), String>,
    },
}

/// How testing the connection of a profile went (the screens say it in the person's language).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckResult {
    /// The server answered: its address and how long it took.
    Reachable { url: String, millis: u64 },
    /// The server refused the token (`401` or `403`).
    Refused { url: String, status: u16 },
    /// The server answered something else.
    Answered {
        url: String,
        status: u16,
        code: String,
    },
    /// Nothing could be tried or nobody answered (a token file that cannot be read, no connection).
    Failed(String),
}
