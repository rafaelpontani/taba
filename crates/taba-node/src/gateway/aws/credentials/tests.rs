use super::*;

/// UC: UC-GTW-001
#[test]
fn a_valid_file_gives_the_secret_and_the_token_of_each_key() {
    let entries = parse(
        r#"{"AKIA1":{"secret":"s1","token":"tb_a.b"},"AKIA2":{"secret":"s2","token":"tb_c.d"}}"#,
    )
    .expect("valid file");
    assert_eq!(entries.len(), 2);
    assert_eq!(entries["AKIA1"].secret.expose(), "s1");
    assert_eq!(entries["AKIA2"].token.expose(), "tb_c.d");
}

/// UC: UC-GTW-001
#[test]
fn an_entry_without_a_secret_or_a_token_is_refused() {
    assert!(parse(r#"{"AKIA1":{"secret":"","token":"tb_a.b"}}"#).is_err());
    assert!(parse(r#"{"AKIA1":{"secret":"s"}}"#).is_err());
    assert!(parse("[]").is_err());
    assert!(parse("not json").is_err());
}

/// UC: UC-GTW-001
#[test]
fn the_file_is_read_again_and_an_invalid_one_keeps_the_previous_keys() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("credentials.json");
    std::fs::write(&path, r#"{"K1":{"secret":"s","token":"t"}}"#).expect("write");
    let credentials = Credentials::open(&path).expect("open");
    assert!(credentials.lookup("K1").is_some());
    assert!(credentials.lookup("K2").is_none());

    std::fs::write(&path, r#"{"K2":{"secret":"s","token":"t"}}"#).expect("write");
    credentials.reload();
    assert!(credentials.lookup("K1").is_none());
    assert!(credentials.lookup("K2").is_some());

    std::fs::write(&path, "garbage").expect("write");
    credentials.reload();
    assert!(
        credentials.lookup("K2").is_some(),
        "an invalid file must not drop the keys"
    );
}

/// UC: UC-GTW-001
#[test]
fn an_unreadable_file_at_the_start_is_an_error() {
    assert!(Credentials::open(Path::new("/nonexistent/credentials.json")).is_err());
}
