use super::*;

fn env_of<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
    move |k| {
        pairs
            .iter()
            .find(|(n, _)| *n == k)
            .map(|(_, v)| v.to_string())
    }
}

fn profile(pairs: &[(&str, &str)]) -> Profile {
    let mut p = Profile::default();
    for (k, v) in pairs {
        p.set(k, v).unwrap();
    }
    p
}

/// UC: UC-CLI-023
#[test]
fn the_file_lives_in_the_user_config_folder_of_each_system() {
    let win = config_path(
        Os::Windows,
        &env_of(&[("APPDATA", "C:\\Users\\ana\\AppData\\Roaming")]),
    )
    .unwrap();
    assert_eq!(
        win,
        Path::new("C:\\Users\\ana\\AppData\\Roaming")
            .join("taba")
            .join("config.yml")
    );
    let xdg = config_path(
        Os::Linux,
        &env_of(&[("XDG_CONFIG_HOME", "/x/cfg"), ("HOME", "/home/ana")]),
    )
    .unwrap();
    assert_eq!(xdg, Path::new("/x/cfg").join("taba").join("config.yml"));
    let home = config_path(Os::Linux, &env_of(&[("HOME", "/home/ana")])).unwrap();
    assert_eq!(
        home,
        Path::new("/home/ana")
            .join(".config")
            .join("taba")
            .join("config.yml")
    );
    let mac = config_path(Os::MacOs, &env_of(&[("HOME", "/Users/ana")])).unwrap();
    assert_eq!(
        mac,
        Path::new("/Users/ana")
            .join("Library")
            .join("Application Support")
            .join("taba")
            .join("config.yml")
    );
    assert!(config_path(Os::Linux, &env_of(&[])).is_err());
}

/// UC: UC-CLI-023
#[test]
fn taba_cli_config_overrides_the_path() {
    let p = config_path(
        Os::Windows,
        &env_of(&[("TABA_CLI_CONFIG", "/tmp/c.yml"), ("APPDATA", "x")]),
    )
    .unwrap();
    assert_eq!(p, Path::new("/tmp/c.yml"));
}

/// UC: UC-CLI-024
#[test]
fn the_first_profile_becomes_the_default_and_names_are_validated() {
    let mut f = ConfigFile::default();
    f.add("local", profile(&[("url", "http://127.0.0.1:8080")]))
        .unwrap();
    assert_eq!(f.default.as_deref(), Some("local"));
    f.add("stg", Profile::default()).unwrap();
    assert_eq!(f.default.as_deref(), Some("local"));
    assert!(f
        .add("stg", Profile::default())
        .unwrap_err()
        .contains("already exists"));
    for bad in ["", "with space", "a/b", "ç", &"x".repeat(65)] {
        assert!(f.add(bad, Profile::default()).is_err(), "{bad:?}");
    }
}

/// UC: UC-CLI-024
#[test]
fn fields_are_validated_when_set() {
    let mut p = Profile::default();
    assert!(p.set("url", "taba:8080").is_err());
    assert!(p.set("url", "https://taba").is_ok());
    assert!(p.set("output", "xml").is_err());
    assert!(p.set("timeout", "0").is_err());
    assert!(p.set("timeout", "3601").is_err());
    assert!(p.set("max-attempts", "11").is_err());
    assert!(p.set("connect-timeout", "x").is_err());
    assert!(p.set("token", " ").is_err());
    assert!(p
        .set("color", "blue")
        .unwrap_err()
        .contains("is not a profile field"));
    assert_eq!(p.entries(), [("url", "https://taba")]);
    p.unset("url");
    assert!(p.entries().is_empty());
}

/// UC: UC-CLI-024
#[test]
fn a_file_with_the_old_ops_url_field_still_loads_and_drops_it_when_written_again() {
    let text = "default: stg\nprofiles:\n  stg:\n    url: https://taba.example.com\n    ops-url: https://taba-ops.example.com:9090\n    token-file: /sec/tok\n";
    let file = ConfigFile::parse(text).expect("an older file keeps loading");
    let stg = file.get("stg").unwrap();
    assert_eq!(stg.get("url"), Some("https://taba.example.com"));
    assert_eq!(stg.get("ops-url"), None);
    assert!(!file.render().contains("ops-url"));
    // The field is not offered any more: setting it by hand is refused like any other unknown one.
    assert!(Profile::default()
        .set("ops-url", "https://taba:9090")
        .unwrap_err()
        .contains("is not a profile field"));
}

/// UC: UC-CLI-024, UC-CLI-025
#[test]
fn removing_the_default_leaves_the_file_without_one() {
    let mut f = ConfigFile::default();
    f.add("a", Profile::default()).unwrap();
    f.add("b", Profile::default()).unwrap();
    f.remove("a").unwrap();
    assert_eq!(f.default, None);
    assert_eq!(f.names(), vec!["b"]);
    assert!(f.remove("a").is_err());
}

/// UC: UC-CLI-025
#[test]
fn set_default_needs_an_existing_profile() {
    let mut f = ConfigFile::default();
    assert!(f.set_default("x").is_err());
    f.add("x", Profile::default()).unwrap();
    f.default = None;
    f.set_default("x").unwrap();
    assert_eq!(f.default.as_deref(), Some("x"));
}

/// UC: UC-CLI-028
#[test]
fn the_file_round_trips_in_order() {
    let mut f = ConfigFile::default();
    f.add(
        "local",
        profile(&[
            ("url", "http://127.0.0.1:8080"),
            ("token-file", "C:\\taba\\root-token"),
        ]),
    )
    .unwrap();
    f.add(
        "stg",
        profile(&[
            ("max-attempts", "5"),
            ("output", "table"),
            ("ca-file", "/etc/ca.pem"),
        ]),
    )
    .unwrap();
    let text = f.render();
    assert!(text.starts_with("# taba-cli profiles."));
    assert_eq!(ConfigFile::parse(&text).unwrap(), f);
    assert!(text.find("local").unwrap() < text.find("stg").unwrap());
}

/// UC: UC-CLI-028
#[test]
fn invalid_content_is_refused_with_a_reason() {
    for (doc, why) in [
        ("a: 1\n", "unknown key"),
        ("default: x\n", "does not exist"),
        ("profiles:\n  a: b\n", "must be a map"),
        ("profiles:\n  a:\n    url: nope\n", "url"),
        (
            "profiles:\n  a:\n    color: blue\n",
            "is not a profile field",
        ),
        ("profiles:\n  \"a b\": {}\n", "invalid"),
        ("profiles:\n  a:\n    url:\n      x: y\n", "must be a text"),
        ("profiles: x\n", "wrong type"),
        ("a: [\n", "line 1"),
    ] {
        let e = ConfigFile::parse(doc).unwrap_err();
        assert!(e.contains(why), "{doc:?}: {e}");
    }
}

/// UC: UC-CLI-028
#[test]
fn a_missing_file_is_empty_and_saving_replaces_it_atomically() {
    let dir = std::env::temp_dir().join(format!(
        "taba-cli-test-{}-{}",
        std::process::id(),
        "profile"
    ));
    let _ = fs::remove_dir_all(&dir);
    let path = dir.join("sub").join("config.yml");
    assert_eq!(load(&path).unwrap(), ConfigFile::default());

    let mut f = ConfigFile::default();
    f.add("a", profile(&[("url", "http://h:1")])).unwrap();
    save(&path, &f).unwrap();
    assert_eq!(load(&path).unwrap(), f);
    f.add("b", Profile::default()).unwrap();
    save(&path, &f).unwrap();
    assert_eq!(load(&path).unwrap(), f);
    let leftovers: Vec<_> = fs::read_dir(path.parent().unwrap())
        .unwrap()
        .flatten()
        .map(|e| e.file_name())
        .collect();
    assert_eq!(
        leftovers.len(),
        1,
        "temporary file left over: {leftovers:?}"
    );

    fs::write(&path, "profiles:\n  a: [\n").unwrap();
    let e = load(&path).unwrap_err();
    assert!(e.contains("config.yml") && e.contains("line 2"), "{e}");
    let _ = fs::remove_dir_all(&dir);
}

/// UC: UC-CLI-028
#[cfg(unix)]
#[test]
fn the_saved_file_is_private_on_unix() {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!("taba-cli-test-{}-{}", std::process::id(), "perm"));
    let path = dir.join("config.yml");
    save(&path, &ConfigFile::default()).unwrap();
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let _ = fs::remove_dir_all(&dir);
}

/// TUI: TUI-HLP-03
#[test]
fn the_interface_settings_are_read_and_written_back_with_the_profiles() {
    let text = "default: a\nui:\n  language: pt-BR\nhistory:\n  retention: 2h\n  interval: 5s\nprofiles:\n  a:\n    url: http://x:1\n";
    let file = ConfigFile::parse(text).unwrap();
    assert_eq!(file.language.as_deref(), Some("pt-BR"));
    assert_eq!(file.history_retention.as_deref(), Some("2h"));
    assert_eq!(file.history_interval.as_deref(), Some("5s"));
    let again = ConfigFile::parse(&file.render()).unwrap();
    assert_eq!(again, file);
    // A file without them renders without them.
    let plain = ConfigFile::parse("profiles:\n  a:\n    url: http://x:1\n").unwrap();
    assert!(!plain.render().contains("ui:") && !plain.render().contains("history:"));
}

/// TUI: TUI-HLP-03
#[test]
fn a_bad_interface_setting_is_refused_with_its_name() {
    for (text, needle) in [
        ("ui:\n  theme: dark\n", "ui.theme"),
        ("ui:\n  language: \"\"\n", "ui.language"),
        ("ui: yes\n", "`ui`"),
        ("history:\n  retention: soon\n", "history.retention"),
        ("history:\n  speed: 1s\n", "history.speed"),
    ] {
        let error = ConfigFile::parse(text).unwrap_err();
        assert!(error.contains(needle), "{text} -> {error}");
    }
}
