use super::*;

fn parse_strs(args: &[&str]) -> Result<Args, String> {
    parse(args.iter().map(|s| (*s).to_owned()))
}

#[test]
fn config_path_accepts_both_forms() {
    assert_eq!(
        parse_strs(&["--config", "a.yml"]).unwrap().config,
        Some(PathBuf::from("a.yml"))
    );
    assert_eq!(
        parse_strs(&["--config=b.yml"]).unwrap().config,
        Some(PathBuf::from("b.yml"))
    );
    assert_eq!(parse_strs(&[]).unwrap(), Args::default());
}

#[test]
fn rejects_missing_path_and_unknown_arguments() {
    assert!(parse_strs(&["--config"]).is_err());
    assert!(parse_strs(&["--nada"]).unwrap_err().contains("--nada"));
}

#[test]
fn help_and_version_flags() {
    assert!(parse_strs(&["--help"]).unwrap().help);
    assert!(parse_strs(&["-V"]).unwrap().version);
}

/// UC: UC-SEG-019
#[test]
fn init_needs_the_out_folder_and_defaults_to_five_years() {
    assert_eq!(
        parse_strs(&["init", "--out", "./secrets"]).unwrap().command,
        Command::Init {
            out: PathBuf::from("./secrets"),
            days: 1825
        }
    );
    assert_eq!(
        parse_strs(&["init", "--out=/s", "--days", "30"])
            .unwrap()
            .command,
        Command::Init {
            out: PathBuf::from("/s"),
            days: 30
        }
    );
    assert!(parse_strs(&["init"]).unwrap_err().contains("--out"));
    assert!(parse_strs(&["init", "--out", "s", "--days", "0"]).is_err());
    assert!(parse_strs(&["init", "--out", "s", "--days", "x"]).is_err());
    assert!(
        parse_strs(&["init", "--out", "s", "--starts-in", "1d"])
            .unwrap_err()
            .contains("--starts-in")
    );
    assert!(
        parse_strs(&["init", "--out", "s", "--out", "t"])
            .unwrap_err()
            .contains("repetido")
    );
    assert!(parse_strs(&["init", "--out"]).is_err());
}

/// UC: UC-SEG-020
#[test]
fn cert_generate_takes_days_out_and_an_optional_start_delay() {
    assert_eq!(
        parse_strs(&["cert", "generate", "--days", "1825", "--out", "tls"])
            .unwrap()
            .command,
        Command::CertGenerate {
            out: PathBuf::from("tls"),
            days: 1825,
            starts_in: None
        }
    );
    assert_eq!(
        parse_strs(&[
            "cert",
            "generate",
            "--days",
            "1825",
            "--starts-in",
            "14d",
            "--out",
            "tls"
        ])
        .unwrap()
        .command,
        Command::CertGenerate {
            out: PathBuf::from("tls"),
            days: 1825,
            starts_in: Some(Duration::from_secs(14 * 86_400))
        }
    );
    assert!(
        parse_strs(&["cert", "generate", "--out", "tls"])
            .unwrap_err()
            .contains("--days")
    );
    assert!(
        parse_strs(&["cert", "generate", "--days", "9"])
            .unwrap_err()
            .contains("--out")
    );
    assert!(
        parse_strs(&[
            "cert",
            "generate",
            "--days",
            "9",
            "--out",
            "t",
            "--starts-in",
            "14"
        ])
        .unwrap_err()
        .contains("--starts-in")
    );
    assert!(parse_strs(&["cert"]).is_err());
    assert!(parse_strs(&["cert", "outro"]).is_err());
}

/// UC: UC-SEG-021
#[test]
fn token_generate_takes_nothing_else() {
    assert_eq!(
        parse_strs(&["token", "generate"]).unwrap().command,
        Command::TokenGenerate
    );
    assert!(parse_strs(&["token"]).is_err());
    assert!(parse_strs(&["token", "generate", "extra"]).is_err());
}

#[test]
fn running_the_node_is_the_default_command() {
    assert_eq!(
        parse_strs(&["--config", "a.yml"]).unwrap().command,
        Command::Run
    );
}
