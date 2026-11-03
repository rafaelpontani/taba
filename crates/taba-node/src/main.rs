//! `main` is the composition root: it reads the configuration, creates the pieces and waits for the
//! stop signal (doc 18, section 17.3).

use std::process::ExitCode;

use taba_node::cli::Command;
use taba_node::config::Config;
use taba_node::node::{self, Deps};
use taba_node::{cli, telemetry};

// O alocador do musl (a imagem é estática) serializa as threads num lock global e ocupava cerca de um quinto da CPU
// do nó sob carga; o mimalloc tem cache por thread.
#[global_allocator]
static ALLOCATOR: mimalloc::MiMalloc = mimalloc::MiMalloc;

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
        // TABA_GIT_SHA is injected by the pipeline build; a local build has no commit.
        match option_env!("TABA_GIT_SHA") {
            Some(sha) => println!("taba {} (commit {sha})", env!("CARGO_PKG_VERSION")),
            None => println!("taba {}", env!("CARGO_PKG_VERSION")),
        }
        return ExitCode::SUCCESS;
    }

    let config = match Config::load(args.config.as_deref()) {
        Ok(config) => config,
        Err(e) => {
            eprintln!("taba: {e}");
            return ExitCode::from(2);
        }
    };
    telemetry::init_logging(
        config.telemetry.logs_level.as_deref(),
        config.telemetry.logs_format,
    );

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

    // The node also stops by itself when the bootstrap runs out of attempts (doc 04, section 6.5).
    let fatal = tokio::select! {
        () = wait_for_stop() => None,
        reason = running.fatal() => Some(reason),
    };
    if let Some(reason) = &fatal {
        tracing::error!("{reason}");
        eprintln!("taba: {reason}");
    } else {
        tracing::info!("stop signal received; shutting down");
    }
    // A second signal during the drain ends it at once, without finishing (doc 04, section 6.10).
    let success = if fatal.is_some() {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    };
    tokio::select! {
        () = running.shutdown() => success,
        () = wait_for_stop() => {
            tracing::warn!("second signal during the drain: shutting down at once");
            ExitCode::FAILURE
        }
    }
}

/// `SIGINT` (Ctrl+C) on any platform and `SIGTERM` where it exists. The drain on receiving the
/// signal (`drain.on-stop`) is in `RunningNode::shutdown`.
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

/// The secrets-folder subcommands: they create files, print what they did and exit.
fn run_secrets_command(command: &Command) -> ExitCode {
    use taba_node::secrets;
    let now = time::OffsetDateTime::now_utc();
    let result = match command {
        Command::Init { out, days } => secrets::init(out, now, *days).map(|files| {
            println!("Secrets folder created at {}", out.display());
            println!(
                "  certificado do cluster: {}",
                files.certificate.certificate.display()
            );
            println!(
                "  certificate key:        {}",
                files.certificate.key.display()
            );
            println!("  token raiz:             {}", files.root_token.display());
            println!();
            println!("Mount the folder on every node (secrets.dir, default /etc/taba/secrets):");
            println!(
                "authentication and the cluster TLS turn on by themselves. Keep the root token in a vault."
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
        Command::Openapi { api } => {
            println!("{}", taba_node::http::openapi::document(*api));
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
