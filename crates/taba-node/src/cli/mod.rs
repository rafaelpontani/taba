//! A linha de comando do binário `taba`. Por enquanto só `--config`; os subcomandos
//! `init`, `cert generate` e `token generate` entram com a segurança do cluster
//! (doc 06, seção 8.0.1).

use std::path::PathBuf;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Args {
    pub config: Option<PathBuf>,
    pub help: bool,
    pub version: bool,
}

pub const USAGE: &str = "\
Uso: taba [--config <arquivo.yml>]

  --config <arquivo>   arquivo de configuração YAML (ou TABA_CONFIG_FILE)
  --version            mostra a versão
  --help               mostra esta ajuda

Os parâmetros também vêm das variáveis TABA_*; a variável vence o arquivo.";

pub fn parse<I: IntoIterator<Item = String>>(args: I) -> Result<Args, String> {
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

#[cfg(test)]
mod tests;
