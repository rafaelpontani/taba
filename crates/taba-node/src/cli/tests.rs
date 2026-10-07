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
