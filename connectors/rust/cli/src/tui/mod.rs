//! The interactive profiles screen (doc 19b, section 18.11.2): a list and a form navigated with
//! the arrows. The logic (`screen`, `form`, `keys`) is pure and tested; `term` is the thin part that puts the
//! terminal in raw mode.

use std::io;
use std::time::Instant;

use taba_client::{Call, Config};

use crate::app::{CliError, Ctx};
use crate::profile::{ConfigFile, Profile};

pub mod form;
pub mod keys;
pub mod screen;
pub mod term;

#[cfg(feature = "tui")]
pub mod api;
#[cfg(feature = "tui")]
pub mod app;
#[cfg(feature = "tui")]
pub mod editor;
#[cfg(feature = "tui")]
pub mod effect;
#[cfg(feature = "tui")]
pub mod history;
#[cfg(feature = "tui")]
pub mod i18n;
#[cfg(feature = "tui")]
pub mod keymap;
#[cfg(feature = "tui")]
pub mod menu;
#[cfg(feature = "tui")]
pub mod model;
#[cfg(feature = "tui")]
pub mod msg;
#[cfg(feature = "tui")]
pub mod prom;
#[cfg(feature = "tui")]
pub mod screens;
#[cfg(all(feature = "tui", test))]
pub mod testing;
#[cfg(feature = "tui")]
pub mod theme;
#[cfg(feature = "tui")]
pub mod update;
#[cfg(feature = "tui")]
pub mod view;
#[cfg(feature = "tui")]
pub mod worker;

pub use keys::Key;
use screen::{App, Flow, Services};

/// What to draw: the lines (with ANSI color sequences, if `Style.color`) and where the cursor is.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Frame {
    pub lines: Vec<String>,
    /// Line and column (zero-based) of the cursor; `None` hides the cursor.
    pub cursor: Option<(usize, usize)>,
}

#[derive(Debug, Clone, Copy)]
pub struct Style {
    pub color: bool,
}

impl Style {
    /// `text` with the given SGR code (`1` bold, `2` dim, `31` red...), if there is color.
    pub fn paint(self, code: &str, text: &str) -> String {
        if self.color && !text.is_empty() {
            format!("\x1b[{code}m{text}\x1b[0m")
        } else {
            text.to_owned()
        }
    }
}

/// The terminal in raw mode. When dropped, it gives the terminal back as it was.
pub trait Terminal {
    /// Columns and lines.
    fn size(&self) -> (usize, usize);
    /// The next key; `None` when the input ended.
    fn read_key(&mut self) -> io::Result<Option<Key>>;
    fn draw(&mut self, frame: &Frame) -> io::Result<()>;
    fn color(&self) -> bool;
}

/// Runs the screen until the person quits.
pub fn run(ctx: &mut Ctx<'_>, terminal: &mut dyn Terminal) -> Result<(), CliError> {
    let file = ctx.load_config()?;
    let path = ctx.config_path()?.display().to_string();
    let session = ctx.env("TABA_CLI_PROFILE").filter(|v| !v.is_empty());
    let mut app = App::new(file, path, session);
    let style = Style {
        color: terminal.color(),
    };
    let mut services = CtxServices { ctx };
    loop {
        let (width, height) = terminal.size();
        terminal
            .draw(&app.render(style, width, height))
            .map_err(|e| CliError::File(format!("failed to draw the screen: {e}")))?;
        match terminal.read_key() {
            Ok(Some(key)) => {
                if app.handle(key, &mut services) == Flow::Quit {
                    return Ok(());
                }
            }
            Ok(None) => return Ok(()),
            Err(e) => return Err(CliError::File(format!("failed to read the keyboard: {e}"))),
        }
    }
}

struct CtxServices<'a, 'b> {
    ctx: &'a mut Ctx<'b>,
}

impl Services for CtxServices<'_, '_> {
    fn save(&mut self, file: &ConfigFile) -> Result<(), String> {
        self.ctx.save_config(file).map_err(|e| match e {
            CliError::File(m) | CliError::Usage(m) => m,
            other => format!("{other:?}"),
        })
    }

    fn check(&mut self, profile: &Profile) -> Result<String, String> {
        let config = connection_config(profile)?;
        let url = config.base_url.clone();
        let client = self.ctx.platform.client(config).map_err(|e| e.message)?;
        let started = Instant::now();
        // `GET /topics?limit=1` on the client API: checks the address, TLS and the token all at once.
        match client.call(&Call::new("GET", "/topics").query("limit", 1)) {
            Ok(_) => Ok(format!(
                "ok: {url} answered in {} ms",
                started.elapsed().as_millis()
            )),
            Err(e) if e.status == 401 || e.status == 403 => Err(format!(
                "{url} respondeu {} ({}): confira o token",
                e.status, e.code
            )),
            Err(e) if e.status > 0 => Err(format!("{url} respondeu {} ({})", e.status, e.code)),
            Err(e) => Err(format!("failed: {}", e.message)),
        }
    }
}

/// A profile's client configuration, with the built-in values where it defines nothing.
pub(crate) fn connection_config(profile: &Profile) -> Result<Config, String> {
    let number = |field: &str, default: u64| {
        profile
            .get(field)
            .and_then(|v| v.parse().ok())
            .unwrap_or(default)
    };
    let mut config = Config::new(profile.get("url").unwrap_or(crate::profile::DEFAULT_URL));
    config.connect_timeout = std::time::Duration::from_secs(number("connect-timeout", 5));
    config.request_timeout = std::time::Duration::from_secs(number("timeout", 30));
    config.max_attempts = 1;
    config.user_agent = format!("taba-cli/{}", env!("CARGO_PKG_VERSION"));
    if let Some(token) = profile.get("token") {
        config.token = Some(token.to_owned());
    }
    if let Some(path) = profile.get("token-file") {
        let token = crate::app::read_token_file(path).map_err(|e| match e {
            CliError::File(m) => m,
            other => format!("{other:?}"),
        })?;
        config.token = Some(token);
    }
    if let Some(path) = profile.get("ca-file") {
        config.ca_pem = Some(
            std::fs::read(path).map_err(|e| format!("could not read the ca-file {path}: {e}"))?,
        );
    }
    Ok(config)
}

#[cfg(test)]
mod tests;
