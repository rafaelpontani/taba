//! What `update` asks the outside world to do (doc 34, section 4). `update` never does it itself: the loop runs the
//! effects and brings the outcome back as a `Msg`.

use crate::profile::{ConfigFile, Profile};
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
    /// Write a file that holds a secret, readable only by its owner, never over an existing one.
    WriteSecret {
        path: String,
        contents: String,
    },
    /// Ask the terminal to put this text on the clipboard (OSC 52).
    Copy(String),
    /// Write the default profile and the profiles to the configuration file.
    SaveProfiles {
        file: ConfigFile,
        select: Option<String>,
    },
    /// Try the connection of a profile that may not be saved.
    CheckProfile(Profile),
    /// Connect with a profile of the file: from now on the calls go there.
    Connect(String),
    Quit,
}
