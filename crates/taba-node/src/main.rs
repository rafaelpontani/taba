//! O `main` é a raiz de composição: lê a configuração, cria as peças e espera o sinal
//! de parada (doc 18, seção 17.3).

use std::process::ExitCode;

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

    let running = match node::start(config, Deps::default()).await {
        Ok(running) => running,
        Err(e) => {
            tracing::error!("{e}");
            eprintln!("taba: {e}");
            return ExitCode::FAILURE;
        }
    };

    wait_for_stop().await;
    tracing::info!("sinal de parada recebido; encerrando");
    // Um segundo sinal durante o drain encerra na hora, sem terminar (doc 04, seção 6.10).
    tokio::select! {
        () = running.shutdown() => ExitCode::SUCCESS,
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
