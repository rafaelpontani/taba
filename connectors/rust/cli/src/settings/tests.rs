use super::*;

fn env_of<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
    move |k| {
        pairs
            .iter()
            .find(|(n, _)| *n == k)
            .map(|(_, v)| v.to_string())
    }
}

fn file() -> ConfigFile {
    let mut f = ConfigFile::default();
    let mut local = Profile::default();
    local.set("url", "http://10.0.0.1:8080").unwrap();
    local.set("timeout", "60").unwrap();
    local.set("token-file", "/sec/tok").unwrap();
    f.add("local", local).unwrap();
    let mut stg = Profile::default();
    stg.set("url", "https://stg:8443").unwrap();
    stg.set("token", "tb_literal").unwrap();
    f.add("stg", stg).unwrap();
    f
}

/// UC: UC-CLI-022
#[test]
fn without_file_variables_or_profile_the_built_in_defaults_apply() {
    let e = resolve(&ConfigFile::default(), None, &env_of(&[])).unwrap();
    assert_eq!(e.profile, None);
    assert_eq!(e.url(), "http://127.0.0.1:8080");
    assert_eq!(e.get("token"), None);
    assert_eq!(e.token_source(), TokenSource::None);
    assert_eq!(
        (
            e.get("output"),
            e.number("connect-timeout"),
            e.number("timeout"),
            e.number("max-attempts")
        ),
        (Some("json"), 5, 30, 3)
    );
    assert!(e.settings().all(|(_, s)| s.source == Source::Default));
}

/// UC: UC-CLI-026
#[test]
fn the_profile_is_the_flag_then_the_variable_then_the_file_default() {
    let f = file();
    let e = resolve(&f, None, &env_of(&[])).unwrap();
    assert_eq!(e.profile, Some(("local".to_owned(), Source::Default)));
    assert_eq!(e.url(), "http://10.0.0.1:8080");
    let e = resolve(&f, None, &env_of(&[("TABA_CLI_PROFILE", "stg")])).unwrap();
    assert_eq!(e.profile.as_ref().map(|p| p.0.as_str()), Some("stg"));
    assert_eq!(e.url(), "https://stg:8443");
    let e = resolve(&f, Some("local"), &env_of(&[("TABA_CLI_PROFILE", "stg")])).unwrap();
    assert_eq!(e.profile.as_ref().map(|p| p.0.as_str()), Some("local"));
}

/// UC: UC-CLI-026
#[test]
fn an_unknown_profile_is_an_error_that_lists_the_known_ones() {
    let f = file();
    let e = resolve(&f, Some("nothing"), &env_of(&[])).unwrap_err();
    assert!(
        e.contains("`nothing` does not exist") && e.contains("local, stg"),
        "{e}"
    );
    let e = resolve(&ConfigFile::default(), Some("x"), &env_of(&[])).unwrap_err();
    assert!(e.contains("there are no profiles"), "{e}");
    assert!(resolve(&f, None, &env_of(&[("TABA_CLI_PROFILE", "zzz")])).is_err());
}

/// UC: UC-CLI-026
#[test]
fn a_variable_overrides_only_the_field_it_covers() {
    let f = file();
    let e = resolve(
        &f,
        None,
        &env_of(&[
            ("TABA_CLI_URL", "http://other:1"),
            ("TABA_CLI_MAX_ATTEMPTS", "7"),
        ]),
    )
    .unwrap();
    assert_eq!(e.url(), "http://other:1");
    assert_eq!(
        e.setting("url").source,
        Source::Variable("TABA_CLI_URL".to_owned())
    );
    assert_eq!(e.number("max-attempts"), 7);
    assert_eq!(e.number("timeout"), 60);
    assert_eq!(
        e.setting("timeout").source,
        Source::Profile("local".to_owned())
    );
    assert_eq!(e.get("token-file"), Some("/sec/tok"));
    assert_eq!(e.setting("connect-timeout").source, Source::Default);
}

/// UC: UC-CLI-026
#[test]
fn an_invalid_variable_value_is_reported_with_its_name() {
    let e = resolve(&file(), None, &env_of(&[("TABA_CLI_TIMEOUT", "lots")])).unwrap_err();
    assert!(e.contains("TABA_CLI_TIMEOUT"), "{e}");
    let e = resolve(&file(), None, &env_of(&[("TABA_CLI_URL", "taba")])).unwrap_err();
    assert!(e.contains("TABA_CLI_URL"), "{e}");
}

/// UC: UC-CLI-002
#[test]
fn the_token_comes_from_the_highest_level_and_the_file_wins_a_tie() {
    let f = file();
    assert_eq!(
        resolve(&f, None, &env_of(&[])).unwrap().token_source(),
        TokenSource::File("/sec/tok")
    );
    assert_eq!(
        resolve(&f, Some("stg"), &env_of(&[]))
            .unwrap()
            .token_source(),
        TokenSource::Text("tb_literal")
    );
    let e = resolve(&f, None, &env_of(&[("TABA_CLI_TOKEN", "tb_env")])).unwrap();
    assert_eq!(e.token_source(), TokenSource::Text("tb_env"));
    let e = resolve(&f, Some("stg"), &env_of(&[("TABA_CLI_TOKEN_FILE", "/x")])).unwrap();
    assert_eq!(e.token_source(), TokenSource::File("/x"));
    let e = resolve(
        &f,
        None,
        &env_of(&[("TABA_CLI_TOKEN", "a"), ("TABA_CLI_TOKEN_FILE", "/b")]),
    )
    .unwrap();
    assert_eq!(e.token_source(), TokenSource::File("/b"));
}

#[test]
fn variable_names_follow_the_field_names() {
    assert_eq!(variable_name("token-file"), "TABA_CLI_TOKEN_FILE");
    assert_eq!(variable_name("max-attempts"), "TABA_CLI_MAX_ATTEMPTS");
    assert_eq!(variable_name("url"), "TABA_CLI_URL");
}

/// TUI: TUI-MON-05
#[test]
fn a_time_is_seconds_minutes_or_hours() {
    assert_eq!(parse_span("90"), Ok(90));
    assert_eq!(parse_span("90s"), Ok(90));
    assert_eq!(parse_span("5m"), Ok(300));
    assert_eq!(parse_span(" 1h "), Ok(3600));
    assert_eq!(parse_span("0s"), Ok(0));
    for bad in [
        "",
        "h",
        "-5s",
        "1d",
        "1.5h",
        "soon",
        "99999999999999999999h",
    ] {
        assert!(parse_span(bad).is_err(), "{bad}");
    }
}

/// UC: UC-CLI-026
#[test]
fn a_single_profile_is_used_even_when_it_is_not_the_default() {
    let mut f = ConfigFile::default();
    let mut only = Profile::default();
    only.set("url", "https://only:8443").unwrap();
    f.add("only", only).unwrap();
    f.default = None;
    let e = resolve(&f, None, &env_of(&[])).unwrap();
    assert_eq!(e.profile, Some(("only".to_owned(), Source::Default)));
    assert_eq!(e.url(), "https://only:8443");

    // With two and no default, nothing is chosen for the person.
    f.add("other", Profile::default()).unwrap();
    f.default = None;
    assert_eq!(resolve(&f, None, &env_of(&[])).unwrap().profile, None);
}
