//! What `update` asks the outside world to do (doc 34, section 4). `update` never does it itself: the loop runs the
//! effects and brings the outcome back as a `Msg`.

use crate::tui::api::ApiCall;
use crate::tui::i18n::Lang;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Make an API call on a worker; the reply returns with this `id` and `generation`.
    Call {
        id: u64,
        generation: u64,
        call: ApiCall,
    },
    /// Write a file the person chose a path for (never without asking).
    WriteFile {
        path: String,
        contents: String,
    },
    /// Keep the language in `ui.language` of the configuration file.
    SaveLanguage(Lang),
    Quit,
}
