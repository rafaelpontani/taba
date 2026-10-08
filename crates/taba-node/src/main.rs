//! O `main` é a raiz de composição: lê a configuração, cria as peças e espera o sinal
//! de parada (doc 18, seção 17.3).

use std::process::ExitCode;

use taba_node::cli::Command;
use taba_node::config::Config;
use taba_node::node::{self, Deps};
use taba_node::{cli, telemetry};

#[tokio::main]
async fn main() -> ExitCode {
    let args = match cli::parse(std::env::args().skip(1)) {
        Ok(args) => args,
        Err(message) => {
            eprintln!("taba: {message}\n\n{}", cli::USAGE);
            return ExitCode::from(2);
        }
    };
    match &args.command {
        Command::Run => {}
        other => return run_secrets_command(other),
    }
    if args.help {
        println!("{}", cli::USAGE);
        return ExitCode::SUCCESS;
    }
    if args.version {
        println!("taba {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }

    let config = match Config::load(args.config.as_deref()) {
        Ok(config) => config,
        Err(e) => {
            eprintln!("taba: {e}");
            return ExitCode::from(2);
        }
    };
    telemetry::init_logging(config.telemetry.logs_level.as_deref());

    let deps = match Deps::production(&config) {
        Ok(deps) => deps,
        Err(e) => {
            eprintln!("taba: {e}");
            return ExitCode::FAILURE;
        }
    };
    let mut running = match node::start(config, deps).await {
        Ok(running) => running,
        Err(e) => {
            tracing::error!("{e}");
            eprintln!("taba: {e}");
            return ExitCode::FAILURE;
        }
    };

    // O nó também para sozinho quando o bootstrap esgota as tentativas (doc 04, seção 6.5).
    let fatal = tokio::select! {
        () = wait_for_stop() => None,
        reason = running.fatal() => Some(reason),
    };
    if let Some(reason) = &fatal {
        tracing::error!("{reason}");
        eprintln!("taba: {reason}");
    } else {
        tracing::info!("sinal de parada recebido; encerrando");
    }
    // Um segundo sinal durante o drain encerra na hora, sem terminar (doc 04, seção 6.10).
    let success = if fatal.is_some() {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    };
    tokio::select! {
        () = running.shutdown() => success,
        () = wait_for_stop() => {
            tracing::warn!("segundo sinal durante o drain: encerrando na hora");
            ExitCode::FAILURE
        }
    }
}

/// `SIGINT` (Ctrl+C) em qualquer plataforma e `SIGTERM` onde existe. O drain ao receber o
/// sinal (`drain.on-stop`) está em `RunningNode::shutdown`.
async fn wait_for_stop() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        match signal(SignalKind::terminate()) {
            Ok(mut term) => {
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => {}
                    _ = term.recv() => {}
                }
            }
            Err(_) => {
                let _ = tokio::signal::ctrl_c().await;
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

/// Os subcomandos da pasta de segredos: criam arquivos, imprimem o que fizeram e saem.
fn run_secrets_command(command: &Command) -> ExitCode {
    use taba_node::secrets;
    let now = time::OffsetDateTime::now_utc();
    let result = match command {
        Command::Init { out, days } => secrets::init(out, now, *days).map(|files| {
            println!("Pasta de segredos criada em {}", out.display());
            println!(
                "  certificado do cluster: {}",
                files.certificate.certificate.display()
            );
            println!(
                "  chave do certificado:   {}",
                files.certificate.key.display()
            );
            println!("  token raiz:             {}", files.root_token.display());
            println!();
            println!("Monte a pasta em todos os nós (secrets.dir, padrão /etc/taba/secrets):");
            println!(
                "a autenticação e o TLS do cluster ligam sozinhos. Guarde o token raiz num cofre."
            );
        }),
        Command::CertGenerate {
            out,
            days,
            starts_in,
        } => secrets::generate_cert(out, now, *days, *starts_in).map(|files| {
            println!("{}", files.certificate.display());
            println!("{}", files.key.display());
        }),
        Command::TokenGenerate => {
            println!("{}", secrets::generate_root_token());
            Ok(())
        }
        Command::Run => Ok(()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("taba: {e}");
            ExitCode::FAILURE
        }
    }
}
