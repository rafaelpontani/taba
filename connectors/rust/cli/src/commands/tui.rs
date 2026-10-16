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
    use crate::tui::app::{self, Host};
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
    let options = Options {
        lang,
        theme,
        profile,
        url,
        retention_secs: retention,
        interval_secs: interval,
    };

    struct RealHost {
        config_path: std::path::PathBuf,
    }

    impl Host for RealHost {
        fn save_language(&mut self, lang: Lang) -> Result<(), String> {
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
    }

    let mut host = RealHost { config_path };
    app::run(Arc::new(client), options, !p.flag("no-mouse"), &mut host)
        .map_err(|e| CliError::File(format!("the interface failed: {e}")))
}
