use super::*;

fn fields(pairs: &[(&str, &str)]) -> Profile {
    let mut p = Profile::default();
    for (k, v) in pairs {
        p.set(k, v).unwrap();
    }
    p
}

/// UC: UC-CLI-027
#[test]
fn session_lines_for_a_saved_profile_and_for_loose_fields() {
    let none = Profile::default();
    assert_eq!(
        session_lines(Shell::Sh, Some("homologacao"), &none, false).unwrap(),
        ["export TABA_CLI_PROFILE='homologacao'"]
    );
    let loose = fields(&[("url", "http://10.0.0.5:8080"), ("token-file", "./tok")]);
    assert_eq!(
        session_lines(Shell::Sh, None, &loose, false).unwrap(),
        [
            "export TABA_CLI_URL='http://10.0.0.5:8080'",
            "export TABA_CLI_TOKEN_FILE='./tok'"
        ]
    );
    assert_eq!(
        session_lines(Shell::Ps, Some("a"), &loose, false).unwrap(),
        [
            "$env:TABA_CLI_PROFILE = 'a'",
            "$env:TABA_CLI_URL = 'http://10.0.0.5:8080'",
            "$env:TABA_CLI_TOKEN_FILE = './tok'"
        ]
    );
    assert_eq!(
        session_lines(Shell::Cmd, None, &loose, false).unwrap(),
        [
            "set \"TABA_CLI_URL=http://10.0.0.5:8080\"",
            "set \"TABA_CLI_TOKEN_FILE=./tok\""
        ]
    );
}

/// UC: UC-CLI-027
#[test]
fn clearing_unsets_the_profile_and_every_field_variable() {
    let lines = session_lines(Shell::Sh, None, &Profile::default(), true).unwrap();
    assert_eq!(lines.len(), 1 + FIELDS.len());
    assert_eq!(lines[0], "unset TABA_CLI_PROFILE");
    assert!(lines.contains(&"unset TABA_CLI_MAX_ATTEMPTS".to_owned()));
    let ps = session_lines(Shell::Ps, None, &Profile::default(), true).unwrap();
    assert_eq!(
        ps[0],
        "Remove-Item Env:TABA_CLI_PROFILE -ErrorAction SilentlyContinue"
    );
    assert_eq!(
        session_lines(Shell::Cmd, None, &Profile::default(), true).unwrap()[1],
        "set \"TABA_CLI_URL=\""
    );
}

/// UC: UC-CLI-027
#[test]
fn values_are_escaped_for_the_chosen_shell() {
    assert_eq!(
        Shell::Sh.set_line("V", "it's $HOME `x`").unwrap(),
        "export V='it'\\''s $HOME `x`'"
    );
    assert_eq!(
        Shell::Ps.set_line("V", "it's $env:X").unwrap(),
        "$env:V = 'it''s $env:X'"
    );
    assert_eq!(
        Shell::Cmd.set_line("V", "C:\\a b").unwrap(),
        "set \"V=C:\\a b\""
    );
    assert!(Shell::Cmd.set_line("V", "com \" aspas").is_err());
}

/// UC: UC-CLI-027
#[test]
fn shell_names_are_checked() {
    assert_eq!(Shell::parse("sh"), Ok(Shell::Sh));
    assert_eq!(Shell::parse("powershell"), Ok(Shell::Ps));
    assert_eq!(Shell::parse("cmd"), Ok(Shell::Cmd));
    assert!(Shell::parse("fish").is_err());
}
