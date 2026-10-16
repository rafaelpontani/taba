//! `taba-cli tui`: the full-screen interface (doc 34). The command only gathers what the interface needs
//! (language, look, profile, history, a client) and hands it over; the interface itself is in `crate::tui`.

use crate::app::{CliError, Command, Ctx};
use crate::args::{flag, val, Parsed};

pub fn commands() -> Vec<Command> {
    vec![Command {
        path: &["tui"],
        args: "",
        summary: "Opens the full-screen interface: cluster, tokens, topics and messages (needs a terminal).",
        min_pos: 0,
        max_pos: 0,
        opts: vec![
            flag("no-mouse"),
            val("history-retention"),
            val("history-interval"),
        ],
        run: tui,
    }]
}

#[cfg(not(feature = "tui"))]
fn tui(_: &mut Ctx<'_>, _: &Parsed) -> Result<(), CliError> {
    Err(CliError::Usage(
        "this build of taba-cli has no full-screen interface (it was built without the `tui` feature)"
            .to_owned(),
    ))
}

#[cfg(feature = "tui")]
fn tui(ctx: &mut Ctx<'_>, p: &Parsed) -> Result<(), CliError> {
    use std::sync::Arc;

    use crate::settings::parse_span;
    use crate::tui::app;
    use crate::tui::history::{self, DEFAULT_INTERVAL_SECS, DEFAULT_RETENTION_SECS};
    use crate::tui::i18n::{self, Lang};
    use crate::tui::model::Options;
    use crate::tui::theme::Theme;

    if !(ctx.platform.stdin_is_tty() && ctx.platform.stdout_is_tty()) {
        let text = super::help_text(&super::table(), "tui");
        return Err(CliError::Usage(format!(
            "the interface needs a terminal; use the other commands.\n{text}"
        )));
    }
    if let Some(code) = &ctx.globals.lang {
        if Lang::parse(code).is_none() {
            return Err(CliError::Usage(format!(
                "--lang: `{code}` is not a language (use en-US or pt-BR)"
            )));
        }
    }
    let file = ctx.load_config()?;
    let config_path = ctx.config_path()?;
    let seconds = |flag: &str, saved: &Option<String>, default: u64| -> Result<u64, CliError> {
        match p.value(flag).or(saved.as_deref()) {
            Some(text) => parse_span(text).map_err(|e| CliError::Usage(format!("--{flag}: {e}"))),
            None => Ok(default),
        }
    };
    let retention = seconds(
        "history-retention",
        &file.history_retention,
        DEFAULT_RETENTION_SECS,
    )?;
    let interval = seconds(
        "history-interval",
        &file.history_interval,
        DEFAULT_INTERVAL_SECS,
    )?;
    history::validate(retention, interval).map_err(CliError::Usage)?;

    let system = sys_locale::get_locale();
    let lang = i18n::detect(
        ctx.globals.lang.as_deref(),
        &|name| ctx.env(name),
        file.language.as_deref(),
        system.as_deref(),
    );
    let theme = Theme::from_env(&|name| ctx.env(name));
    let (profile, url) = {
        let settings = ctx.settings()?;
        let name = settings
            .profile
            .as_ref()
            .map_or_else(|| "(none)".to_owned(), |(name, _)| name.clone());
        (name, settings.url().to_owned())
    };
    let client = ctx.api_client()?;
    // With no profile given (no `--profile`, no `TABA_CLI_PROFILE`, no `TABA_CLI_URL`) and more than one profile in
    // the file, the interface asks which one to use.
    let given = ctx.globals.profile.is_some()
        || ctx.env("TABA_CLI_PROFILE").is_some_and(|v| !v.is_empty())
        || ctx.env("TABA_CLI_URL").is_some_and(|v| !v.is_empty());
    let options = Options {
        lang,
        theme,
        profile,
        url,
        retention_secs: retention,
        interval_secs: interval,
        choose_profile: !given && file.profiles.len() != 1,
        profiles: file,
    };

    let mouse = !p.flag("no-mouse");
    let mut host = RealHost { ctx, config_path };
    app::run(Arc::new(client), options, mouse, &mut host)
        .map_err(|e| CliError::File(format!("the interface failed: {e}")))
}

/// What the interface asks of the world: the configuration file, the files it writes and the connections.
#[cfg(feature = "tui")]
struct RealHost<'a, 'b> {
    ctx: &'a mut Ctx<'b>,
    config_path: std::path::PathBuf,
}

#[cfg(feature = "tui")]
fn message_of(e: CliError) -> String {
    match e {
        CliError::File(m) | CliError::Usage(m) => m,
        other => format!("{other:?}"),
    }
}

#[cfg(feature = "tui")]
impl crate::tui::app::Host for RealHost<'_, '_> {
    fn save_language(&mut self, lang: crate::tui::i18n::Lang) -> Result<(), String> {
        let mut file = crate::profile::load(&self.config_path)?;
        file.language = Some(lang.code().to_owned());
        crate::profile::save(&self.config_path, &file)
    }

    fn write_file(&mut self, path: &str, contents: &str) -> Result<(), String> {
        use std::io::Write;
        let mut out = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|e| e.to_string())?;
        out.write_all(contents.as_bytes())
            .map_err(|e| e.to_string())
    }

    fn write_secret(&mut self, path: &str, contents: &str) -> Result<(), String> {
        use std::io::Write;
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut out = options.open(path).map_err(|e| e.to_string())?;
        out.write_all(contents.as_bytes())
            .map_err(|e| e.to_string())
    }

    fn save_profiles(&mut self, file: &crate::profile::ConfigFile) -> Result<(), String> {
        // Only the profiles and the default are replaced: the language and the history are kept as they are on disk.
        let mut on_disk = crate::profile::load(&self.config_path)?;
        on_disk.default = file.default.clone();
        on_disk.profiles = file.profiles.clone();
        crate::profile::save(&self.config_path, &on_disk)
    }

    fn check_profile(&mut self, profile: &crate::profile::Profile) -> crate::tui::msg::CheckResult {
        use crate::tui::msg::CheckResult;
        use taba_client::Call;

        let config = match crate::tui::connection_config(profile) {
            Ok(config) => config,
            Err(e) => return CheckResult::Failed(e),
        };
        let url = config.base_url.clone();
        let client = match self.ctx.platform.client(config) {
            Ok(client) => client,
            Err(e) => return CheckResult::Failed(e.message),
        };
        let started = std::time::Instant::now();
        // `GET /topics?limit=1` on the client API: checks the address, TLS and the token all at once.
        match client.call(&Call::new("GET", "/topics").query("limit", 1)) {
            Ok(_) => CheckResult::Reachable {
                url,
                millis: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            },
            Err(e) if e.status == 401 || e.status == 403 => CheckResult::Refused {
                url,
                status: e.status,
            },
            Err(e) if e.status > 0 => CheckResult::Answered {
                url,
                status: e.status,
                code: e.code,
            },
            Err(e) => CheckResult::Failed(e.message),
        }
    }

    fn connect(&mut self, name: &str) -> Result<crate::tui::app::Connection, String> {
        let before = self.ctx.globals.profile.clone();
        self.ctx.select_profile(Some(name.to_owned()));
        let made = (|| -> Result<crate::tui::app::Connection, CliError> {
            let url = self.ctx.settings()?.url().to_owned();
            let client = self.ctx.api_client()?;
            Ok(crate::tui::app::Connection {
                backend: std::sync::Arc::new(client),
                url,
            })
        })();
        if made.is_err() {
            self.ctx.select_profile(before);
        }
        made.map_err(message_of)
    }
}
