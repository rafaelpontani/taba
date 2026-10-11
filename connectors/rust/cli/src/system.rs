//! A plataforma de verdade: ambiente, terminal, rede e processos do sistema.

use std::io::{self, IsTerminal, Write};
use std::process::{Command, Stdio};

use taba_client::{Client, Config, Error as ApiError};

use crate::app::Platform;
use crate::profile::Os;
use crate::tui::{term::RawTerminal, Terminal};

pub struct RealPlatform;

impl Platform for RealPlatform {
    fn env(&self, name: &str) -> Option<String> {
        std::env::var(name).ok()
    }

    fn os(&self) -> Os {
        Os::current()
    }

    fn stdin_is_tty(&self) -> bool {
        io::stdin().is_terminal()
    }

    fn stdout_is_tty(&self) -> bool {
        io::stdout().is_terminal()
    }

    fn client(&self, config: Config) -> Result<Client, ApiError> {
        Client::new(config)
    }

    fn exec(&self, command: &str, stdin: &[u8]) -> io::Result<i32> {
        let mut shell = if cfg!(windows) {
            let mut c = Command::new("cmd");
            c.args(["/C", command]);
            c
        } else {
            let mut c = Command::new("sh");
            c.args(["-c", command]);
            c
        };
        let mut child = shell.stdin(Stdio::piped()).spawn()?;
        if let Some(mut pipe) = child.stdin.take() {
            // O comando pode sair sem ler tudo; um pipe quebrado aqui não é erro.
            let _ = pipe.write_all(stdin);
        }
        Ok(child.wait()?.code().unwrap_or(-1))
    }

    fn terminal(&self) -> Option<Box<dyn Terminal>> {
        RawTerminal::open()
            .ok()
            .map(|t| Box::new(t) as Box<dyn Terminal>)
    }
}
