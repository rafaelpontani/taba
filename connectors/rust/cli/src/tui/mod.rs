//! A tela interativa dos perfis (doc 19b, seção 18.11.2): uma lista e um formulário navegados com
//! as setas. A lógica (`screen`, `form`, `keys`) é pura e testada; `term` é a parte fina que põe o
//! terminal em modo cru.

use std::io;
use std::time::Instant;

use taba_client::{Call, Config};

use crate::app::{CliError, Ctx};
use crate::profile::{ConfigFile, Profile};

pub mod form;
pub mod keys;
pub mod screen;
pub mod term;

pub use keys::Key;
use screen::{App, Flow, Services};

/// O que desenhar: as linhas (com sequências ANSI de cor, se `Style.color`) e onde fica o cursor.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Frame {
    pub lines: Vec<String>,
    /// Linha e coluna (a partir de zero) do cursor; `None` esconde o cursor.
    pub cursor: Option<(usize, usize)>,
}

#[derive(Debug, Clone, Copy)]
pub struct Style {
    pub color: bool,
}

impl Style {
    /// `text` com o código SGR dado (`1` negrito, `2` apagado, `31` vermelho...), se há cor.
    pub fn paint(self, code: &str, text: &str) -> String {
        if self.color && !text.is_empty() {
            format!("\x1b[{code}m{text}\x1b[0m")
        } else {
            text.to_owned()
        }
    }
}

/// O terminal em modo cru. Ao ser descartado, devolve o terminal como estava.
pub trait Terminal {
    /// Colunas e linhas.
    fn size(&self) -> (usize, usize);
    /// A próxima tecla; `None` quando a entrada acabou.
    fn read_key(&mut self) -> io::Result<Option<Key>>;
    fn draw(&mut self, frame: &Frame) -> io::Result<()>;
    fn color(&self) -> bool;
}

/// Roda a tela até a pessoa sair.
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
            .map_err(|e| CliError::File(format!("falha ao desenhar a tela: {e}")))?;
        match terminal.read_key() {
            Ok(Some(key)) => {
                if app.handle(key, &mut services) == Flow::Quit {
                    return Ok(());
                }
            }
            Ok(None) => return Ok(()),
            Err(e) => return Err(CliError::File(format!("falha ao ler o teclado: {e}"))),
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
        // `GET /topics?limit=1` na API de cliente: confere o endereço, o TLS e o token de uma vez.
        match client.call(&Call::new("GET", "/topics").query("limit", 1)) {
            Ok(_) => Ok(format!(
                "ok: {url} respondeu em {} ms",
                started.elapsed().as_millis()
            )),
            Err(e) if e.status == 401 || e.status == 403 => Err(format!(
                "{url} respondeu {} ({}): confira o token",
                e.status, e.code
            )),
            Err(e) if e.status > 0 => Err(format!("{url} respondeu {} ({})", e.status, e.code)),
            Err(e) => Err(format!("falhou: {}", e.message)),
        }
    }
}

/// A configuração de cliente de um perfil, com os valores embutidos onde ele não define nada.
fn connection_config(profile: &Profile) -> Result<Config, String> {
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
            std::fs::read(path).map_err(|e| format!("não consegui ler o ca-file {path}: {e}"))?,
        );
    }
    Ok(config)
}

#[cfg(test)]
mod tests;
