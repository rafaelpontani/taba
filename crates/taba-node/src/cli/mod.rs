//! The command line of the `taba` binary: `--config` to start the node and the secrets-folder
//! subcommands, `init`, `cert generate` and `token generate` (doc 06, section 8.0.1).

use std::path::PathBuf;
use std::time::Duration;

use crate::config::parse_duration;
use crate::secrets::DEFAULT_DAYS;

/// What the binary does.
#[derive(Debug, Default, PartialEq, Eq)]
pub enum Command {
    /// Starts the node.
    #[default]
    Run,
    /// `taba init --out <folder> [--days <n>]`: the secrets folder (certificate and root token).
    Init { out: PathBuf, days: u64 },
    /// `taba cert generate --days <n> [--starts-in <duration>] --out <folder>`.
    CertGenerate {
        out: PathBuf,
        days: u64,
        starts_in: Option<Duration>,
    },
    /// `taba token generate`: prints a random 256-bit secret.
    TokenGenerate,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Args {
    pub config: Option<PathBuf>,
    pub help: bool,
    pub version: bool,
    pub command: Command,
}

pub const USAGE: &str = "\
Usage: taba [--config <file.yml>]
     taba init --out <folder> [--days <n>]
     taba cert generate --days <n> [--starts-in <duration>] --out <folder>
     taba token generate

  --config <file>   YAML configuration file (or TABA_CONFIG_FILE)
  --version            shows the version
  --help               shows this help

Subcomandos:
  init            creates the secrets folder: the cluster's first certificate (in tls/) and the
                  root token (root-token). --days is the validity (default 1825)
  cert generate   creates only a cluster certificate pair, for a rotation.
                  --starts-in (e.g. 14d) postpones the start of the validity
  token generate  prints a random 256-bit secret (a standalone root token)

The parameters also come from the TABA_* variables; the variable beats the file.";

pub fn parse<I: IntoIterator<Item = String>>(args: I) -> Result<Args, String> {
    let args: Vec<String> = args.into_iter().collect();
    match args.first().map(String::as_str) {
        Some("init") => {
            let flags = Flags::parse(&args[1..])?;
            flags.only(&["--out", "--days"])?;
            Ok(Args {
                command: Command::Init {
                    out: flags.required_path("--out")?,
                    days: flags.days()?.unwrap_or(DEFAULT_DAYS),
                },
                ..Args::default()
            })
        }
        Some("cert") => {
            if args.get(1).map(String::as_str) != Some("generate") {
                return Err("use `taba cert generate`".into());
            }
            let flags = Flags::parse(&args[2..])?;
            flags.only(&["--out", "--days", "--starts-in"])?;
            let starts_in = match flags.value("--starts-in") {
                None => None,
                Some(text) => Some(parse_duration(text).ok_or_else(|| {
                    format!(
                        "--starts-in `{text}` is invalid: use a duration with a unit (`30m`, `14d`)"
                    )
                })?),
            };
            Ok(Args {
                command: Command::CertGenerate {
                    out: flags.required_path("--out")?,
                    days: flags.days()?.ok_or("--days is required")?,
                    starts_in,
                },
                ..Args::default()
            })
        }
        Some("token") => {
            if args.get(1).map(String::as_str) != Some("generate") || args.len() > 2 {
                return Err("use `taba token generate`".into());
            }
            Ok(Args {
                command: Command::TokenGenerate,
                ..Args::default()
            })
        }
        _ => parse_run(args),
    }
}

fn parse_run(args: Vec<String>) -> Result<Args, String> {
    let mut out = Args::default();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--config" => {
                let path = args.next().ok_or("--config needs the file path")?;
                out.config = Some(PathBuf::from(path));
            }
            "--help" | "-h" => out.help = true,
            "--version" | "-V" => out.version = true,
            other if other.starts_with("--config=") => {
                out.config = Some(PathBuf::from(&other["--config=".len()..]));
            }
            other => return Err(format!("argumento desconhecido: {other}")),
        }
    }
    Ok(out)
}

/// The `--name value` (or `--name=value`) pairs of a subcommand.
struct Flags(Vec<(String, String)>);

impl Flags {
    fn parse(args: &[String]) -> Result<Self, String> {
        let mut pairs = Vec::new();
        let mut iter = args.iter();
        while let Some(arg) = iter.next() {
            let (name, value) = match arg.split_once('=') {
                Some((name, value)) if name.starts_with("--") => {
                    (name.to_owned(), value.to_owned())
                }
                _ if arg.starts_with("--") => {
                    let value = iter.next().ok_or(format!("{arg} needs a value"))?;
                    (arg.clone(), value.clone())
                }
                _ => return Err(format!("argumento desconhecido: {arg}")),
            };
            if pairs.iter().any(|(n, _)| *n == name) {
                return Err(format!("{name} repetido"));
            }
            pairs.push((name, value));
        }
        Ok(Self(pairs))
    }

    /// Refuses the argument the subcommand does not know.
    fn only(&self, known: &[&str]) -> Result<(), String> {
        match self
            .0
            .iter()
            .find(|(name, _)| !known.contains(&name.as_str()))
        {
            Some((name, _)) => Err(format!("argumento desconhecido: {name}")),
            None => Ok(()),
        }
    }

    fn value(&self, name: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    fn required_path(&self, name: &str) -> Result<PathBuf, String> {
        self.value(name)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .ok_or_else(|| format!("{name} is required"))
    }

    fn days(&self) -> Result<Option<u64>, String> {
        match self.value("--days") {
            None => Ok(None),
            Some(text) => match text.parse::<u64>() {
                Ok(n) if n > 0 => Ok(Some(n)),
                _ => Err(format!(
                    "--days `{text}` is invalid: use a number of days greater than zero"
                )),
            },
        }
    }
}

#[cfg(test)]
mod tests;
