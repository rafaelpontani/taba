//! A linha de comando do binário `taba`: `--config` para subir o nó e os subcomandos da pasta
//! de segredos, `init`, `cert generate` e `token generate` (doc 06, seção 8.0.1).

use std::path::PathBuf;
use std::time::Duration;

use crate::config::parse_duration;
use crate::secrets::DEFAULT_DAYS;

/// O que o binário faz.
#[derive(Debug, Default, PartialEq, Eq)]
pub enum Command {
    /// Sobe o nó.
    #[default]
    Run,
    /// `taba init --out <pasta> [--days <n>]`: a pasta de segredos (certificado e token raiz).
    Init { out: PathBuf, days: u64 },
    /// `taba cert generate --days <n> [--starts-in <duração>] --out <pasta>`.
    CertGenerate {
        out: PathBuf,
        days: u64,
        starts_in: Option<Duration>,
    },
    /// `taba token generate`: imprime um segredo aleatório de 256 bits.
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
Uso: taba [--config <arquivo.yml>]
     taba init --out <pasta> [--days <n>]
     taba cert generate --days <n> [--starts-in <duração>] --out <pasta>
     taba token generate

  --config <arquivo>   arquivo de configuração YAML (ou TABA_CONFIG_FILE)
  --version            mostra a versão
  --help               mostra esta ajuda

Subcomandos:
  init            cria a pasta de segredos: o primeiro certificado do cluster (em tls/) e o
                  token raiz (root-token). --days é a validade (padrão 1825)
  cert generate   cria só um par de certificado do cluster, para uma rotação.
                  --starts-in (ex.: 14d) adia o início da validade
  token generate  imprime um segredo aleatório de 256 bits (um token raiz avulso)

Os parâmetros também vêm das variáveis TABA_*; a variável vence o arquivo.";

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
                        "--starts-in `{text}` inválido: use uma duração com unidade (`30m`, `14d`)"
                    )
                })?),
            };
            Ok(Args {
                command: Command::CertGenerate {
                    out: flags.required_path("--out")?,
                    days: flags.days()?.ok_or("--days é obrigatório")?,
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
                let path = args.next().ok_or("--config pede o caminho do arquivo")?;
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

/// Os pares `--nome valor` (ou `--nome=valor`) de um subcomando.
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
                    let value = iter.next().ok_or(format!("{arg} pede um valor"))?;
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

    /// Recusa o argumento que o subcomando não conhece.
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
            .ok_or_else(|| format!("{name} é obrigatório"))
    }

    fn days(&self) -> Result<Option<u64>, String> {
        match self.value("--days") {
            None => Ok(None),
            Some(text) => match text.parse::<u64>() {
                Ok(n) if n > 0 => Ok(Some(n)),
                _ => Err(format!(
                    "--days `{text}` inválido: use um número de dias maior que zero"
                )),
            },
        }
    }
}

#[cfg(test)]
mod tests;
