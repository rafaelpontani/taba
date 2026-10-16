//! Everything that can happen to the interface (doc 34, section 4): a key, a tick of the clock, a reply that came
//! back from a worker. `update` takes one at a time.

use ratatui::crossterm::event::KeyEvent;
use taba_client::{Error, Reply};

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
}
