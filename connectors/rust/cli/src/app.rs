//! The core of the program: the context of a run, the errors and exit codes, and the dispatch
//! of the command. Everything that touches the system (environment, terminal, network, processes) goes through `Platform`,
//! so the tests can swap in a fake one.

use std::io::{self, BufRead, Write};

use taba_client::{Client, Config, Error as ApiError};

use crate::args::{self, Opt, Parsed};
use crate::commands;
use crate::output::{self, Format};
use crate::profile::{self, ConfigFile, Os};
use crate::settings::{self, Effective, TokenSource};
use crate::tui::Terminal;

pub const EXIT_API: i32 = 1;
pub const EXIT_USAGE: i32 = 2;
pub const EXIT_CONNECTION: i32 = 3;
pub const EXIT_TIMEOUT: i32 = 4;
pub const EXIT_FILE: i32 = 5;
pub const EXIT_EMPTY: i32 = 6;

#[derive(Debug)]
pub enum CliError {
    /// Wrong usage: unknown command or option, missing argument, invalid value.
    Usage(String),
    /// The API answered with an error, or the client could not talk to it.
    Api(ApiError),
    /// Unreadable file (`--file`, the `token-file`, the `ca-file` or `config.yml`).
    File(String),
    /// Ends with this code without printing anything else (the message already came out).
    Exit(i32),
}

impl CliError {
    pub fn usage(message: impl Into<String>) -> CliError {
        CliError::Usage(message.into())
    }

    pub fn code(&self) -> i32 {
        match self {
            CliError::Usage(_) => EXIT_USAGE,
            CliError::File(_) => EXIT_FILE,
            CliError::Exit(code) => *code,
            CliError::Api(e) => match e.code.as_str() {
                _ if e.status > 0 => EXIT_API,
                "client.network" => EXIT_CONNECTION,
                "client.timeout" => EXIT_TIMEOUT,
                "client.invalid-config" => EXIT_USAGE,
                _ => EXIT_API,
            },
        }
    }
}

impl From<ApiError> for CliError {
    fn from(e: ApiError) -> CliError {
        CliError::Api(e)
    }
}

impl From<String> for CliError {
    fn from(message: String) -> CliError {
        CliError::Usage(message)
    }
}

/// What the program asks of the system.
pub trait Platform {
    fn env(&self, name: &str) -> Option<String>;
    fn os(&self) -> Os;
    fn stdin_is_tty(&self) -> bool;
    fn stdout_is_tty(&self) -> bool;
    /// The API client for this configuration.
    fn client(&self, config: Config) -> Result<Client, ApiError>;
    /// Runs `command` through the system's command interpreter, with `stdin` on standard input, and
    /// returns the exit code.
    fn exec(&self, command: &str, stdin: &[u8]) -> io::Result<i32>;
    /// The terminal in raw mode for the interactive screen, if there is one.
    fn terminal(&self) -> Option<Box<dyn Terminal>>;
}

/// The global options read from the command line.
#[derive(Debug, Clone, Default)]
pub struct Globals {
    pub profile: Option<String>,
    /// `--lang`: the language of the interactive interface.
    pub lang: Option<String>,
    pub request_id: Option<String>,
    pub output: Option<String>,
    pub quiet: bool,
}

/// A run: the platform, the outputs and the configuration, read only when a command needs it.
pub struct Ctx<'a> {
    pub platform: &'a dyn Platform,
    pub out: &'a mut dyn Write,
    pub err: &'a mut dyn Write,
    pub input: &'a mut dyn BufRead,
    pub globals: Globals,
    effective: Option<Effective>,
}

impl<'a> Ctx<'a> {
    pub fn new(
        platform: &'a dyn Platform,
        out: &'a mut dyn Write,
        err: &'a mut dyn Write,
        input: &'a mut dyn BufRead,
        globals: Globals,
    ) -> Ctx<'a> {
        Ctx {
            platform,
            out,
            err,
            input,
            globals,
            effective: None,
        }
    }

    pub fn env(&self, name: &str) -> Option<String> {
        self.platform.env(name)
    }

    /// The path of `config.yml`.
    pub fn config_path(&self) -> Result<std::path::PathBuf, CliError> {
        profile::config_path(self.platform.os(), &|k| self.platform.env(k)).map_err(CliError::File)
    }

    /// The profiles file (empty if it does not exist yet).
    pub fn load_config(&self) -> Result<ConfigFile, CliError> {
        profile::load(&self.config_path()?).map_err(CliError::File)
    }

    pub fn save_config(&self, file: &ConfigFile) -> Result<(), CliError> {
        profile::save(&self.config_path()?, file).map_err(CliError::File)
    }

    /// The effective configuration: variables, chosen profile and built-in values.
    pub fn settings(&mut self) -> Result<&Effective, CliError> {
        if self.effective.is_none() {
            let file = self.load_config()?;
            let platform = self.platform;
            let effective =
                settings::resolve(&file, self.globals.profile.as_deref(), &|k| platform.env(k))
                    .map_err(CliError::Usage)?;
            self.effective = Some(effective);
        }
        Ok(self.effective.as_ref().expect("it was just defined"))
    }

    /// `true` when the person chose `json` (on the command line, in the environment or in the profile), and not
    /// when it is only the built-in value: only then does the error come out as the server's `ErrorResponse`.
    pub fn json_chosen(&mut self) -> bool {
        if let Some(name) = &self.globals.output {
            return name == "json";
        }
        match self.settings() {
            Ok(s) => {
                let setting = s.setting("output");
                setting.source != crate::settings::Source::Default
                    && setting.value.as_deref() == Some("json")
            }
            Err(_) => false,
        }
    }

    pub fn format(&mut self) -> Result<Format, CliError> {
        let flag = self.globals.output.clone();
        let name = match flag {
            Some(name) => name,
            None => self.settings()?.get("output").unwrap_or("json").to_owned(),
        };
        Format::parse(&name).map_err(CliError::Usage)
    }

    fn client_config(&mut self, url: String, with_token: bool) -> Result<Config, CliError> {
        let (token, ca_file, connect, total, attempts) = {
            let s = self.settings()?;
            let token = if with_token {
                match s.token_source() {
                    TokenSource::None => None,
                    TokenSource::Text(t) => Some(t.to_owned()),
                    TokenSource::File(path) => Some(read_token_file(path)?),
                }
            } else {
                None
            };
            (
                token,
                s.get("ca-file").map(str::to_owned),
                s.number("connect-timeout"),
                s.number("timeout"),
                s.number("max-attempts"),
            )
        };
        let mut config = Config::new(url);
        config.token = token;
        config.connect_timeout = std::time::Duration::from_secs(connect.max(1));
        config.request_timeout = std::time::Duration::from_secs(total.max(1));
        config.max_attempts = attempts.max(1) as u32;
        config.user_agent = format!("taba-cli/{}", env!("CARGO_PKG_VERSION"));
        if let Some(path) = ca_file {
            config.ca_pem =
                Some(std::fs::read(&path).map_err(|e| {
                    CliError::File(format!("could not read the ca-file {path}: {e}"))
                })?);
        }
        Ok(config)
    }

    /// The client API client (with the profile's token).
    pub fn api_client(&mut self) -> Result<Client, CliError> {
        let url = self.settings()?.url().to_owned();
        let config = self.client_config(url, true)?;
        Ok(self.platform.client(config)?)
    }

    /// The operations API client: no token. `attempts` overrides the profile's `max-attempts`.
    pub fn ops_client(&mut self, attempts: Option<u32>) -> Result<Client, CliError> {
        let url = self.settings()?.ops_url();
        let mut config = self.client_config(url, false)?;
        if let Some(n) = attempts {
            config.max_attempts = n;
        }
        Ok(self.platform.client(config)?)
    }

    /// Writes a line to standard output.
    pub fn say(&mut self, text: &str) -> Result<(), CliError> {
        writeln!(self.out, "{text}").map_err(|e| {
            // Whoever reads the output closed the pipe (`| head`): stop silently, with no error.
            if e.kind() == io::ErrorKind::BrokenPipe {
                CliError::Exit(0)
            } else {
                CliError::File(format!("failed to write the output: {e}"))
            }
        })
    }

    /// Asks `[y/N]` on a terminal. Off a terminal it does not ask: the caller requires `--yes`.
    pub fn confirm(&mut self, question: &str) -> Result<bool, CliError> {
        let _ = write!(self.err, "{question} [y/N] ");
        let _ = self.err.flush();
        let mut line = String::new();
        let _ = self.input.read_line(&mut line);
        Ok(matches!(
            line.trim().to_ascii_lowercase().as_str(),
            "y" | "yes"
        ))
    }

    /// Asks for confirmation of an action that deletes: `--yes` is enough; without it, it asks on a terminal and
    /// refuses (code 2) off one.
    pub fn require_confirmation(&mut self, yes: bool, question: &str) -> Result<(), CliError> {
        if yes {
            return Ok(());
        }
        if !self.platform.stdin_is_tty() {
            return Err(CliError::Usage(format!(
                "{question}\nOff a terminal the confirmation cannot be asked for: use --yes."
            )));
        }
        if self.confirm(question)? {
            Ok(())
        } else {
            let _ = writeln!(self.err, "Cancelled.");
            Err(CliError::Exit(EXIT_USAGE))
        }
    }
}

pub(crate) fn read_token_file(path: &str) -> Result<String, CliError> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| CliError::File(format!("could not read the token-file {path}: {e}")))?;
    let token = text.lines().next().unwrap_or_default().trim();
    if token.is_empty() {
        return Err(CliError::File(format!("the token-file {path} is empty")));
    }
    Ok(token.to_owned())
}

/// Runs the program with the arguments (without the binary name) and returns the exit code.
pub fn run(
    platform: &dyn Platform,
    args: &[String],
    out: &mut dyn Write,
    err: &mut dyn Write,
    input: &mut dyn BufRead,
) -> i32 {
    let table = commands::table();
    let invocation = match commands::resolve(&table, args) {
        Ok(i) => i,
        Err(message) => return report_usage(err, &message),
    };
    let command = match invocation {
        commands::Invocation::Help(topic) => {
            let text = commands::help_text(&table, &topic);
            let _ = write!(out, "{text}");
            return 0;
        }
        commands::Invocation::Version => {
            let _ = writeln!(out, "{}", commands::version_text());
            return 0;
        }
        commands::Invocation::Run(command, rest) => (command, rest),
    };
    let (command, rest) = command;
    let parsed = match args::parse(&rest, &command.opts) {
        Ok(p) => p,
        Err(message) => {
            return report_usage(
                err,
                &format!("{message}\n{}", commands::usage_line(command)),
            )
        }
    };
    if parsed.flag("help") {
        let _ = write!(
            out,
            "{}",
            commands::help_text(&table, &command.path.join(" "))
        );
        return 0;
    }
    if parsed.flag("version") {
        let _ = writeln!(out, "{}", commands::version_text());
        return 0;
    }
    if parsed.positional.len() < command.min_pos || parsed.positional.len() > command.max_pos {
        let what = if parsed.positional.len() < command.min_pos {
            "faltam argumentos"
        } else {
            "too many arguments"
        };
        return report_usage(err, &format!("{what}\n{}", commands::usage_line(command)));
    }
    let globals = Globals {
        profile: parsed.value("profile").map(str::to_owned),
        lang: parsed.value("lang").map(str::to_owned),
        request_id: parsed.value("request-id").map(str::to_owned),
        output: parsed.value("output").map(str::to_owned),
        quiet: parsed.flag("quiet"),
    };
    if let Some(name) = &globals.output {
        if let Err(message) = Format::parse(name) {
            return report_usage(err, &message);
        }
    }
    let mut ctx = Ctx::new(platform, out, err, input, globals);
    match (command.run)(&mut ctx, &parsed) {
        Ok(()) => 0,
        Err(error) => report(&mut ctx, error),
    }
}

fn report_usage(err: &mut dyn Write, message: &str) -> i32 {
    let _ = writeln!(
        err,
        "taba-cli: {message}\nUse `taba-cli help` to see the commands."
    );
    EXIT_USAGE
}

fn report(ctx: &mut Ctx<'_>, error: CliError) -> i32 {
    let code = error.code();
    match error {
        CliError::Usage(message) => {
            let _ = writeln!(ctx.err, "taba-cli: {message}");
        }
        CliError::File(message) => {
            let _ = writeln!(ctx.err, "taba-cli: {message}");
        }
        CliError::Exit(_) => {}
        CliError::Api(e) => {
            let json = ctx.json_chosen();
            let _ = output::write_api_error(ctx.err, &e, json);
        }
    }
    code
}

/// For the commands: the options, a command name and a list of options inside a table.
pub struct Command {
    pub path: &'static [&'static str],
    /// The positional arguments, for the usage line (`<name>`).
    pub args: &'static str,
    pub summary: &'static str,
    pub min_pos: usize,
    pub max_pos: usize,
    pub opts: Vec<Opt>,
    pub run: fn(&mut Ctx<'_>, &Parsed) -> Result<(), CliError>,
}
