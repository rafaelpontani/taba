use super::*;

fn a(args: &[&str]) -> Vec<String> {
    args.iter().map(|s| s.to_string()).collect()
}

const OPTS: [Opt; 3] = [val("name"), val("tag"), flag("all")];

/// UC: UC-CLI-001
#[test]
fn reads_values_flags_and_positionals_in_any_order() {
    let p = parse(
        &a(&["pedidos", "--name", "x", "--all", "-o", "table", "extra"]),
        &OPTS,
    )
    .unwrap();
    assert_eq!(p.positional, ["pedidos", "extra"]);
    assert_eq!(p.value("name"), Some("x"));
    assert!(p.flag("all"));
    assert_eq!(p.value("output"), Some("table"));
}

/// UC: UC-CLI-001
#[test]
fn accepts_equals_attached_short_values_and_a_dash_value() {
    let p = parse(&a(&["--name=a=b", "-otable", "--tag", "-x", "-q"]), &OPTS).unwrap();
    assert_eq!(p.value("name"), Some("a=b"));
    assert_eq!(p.value("output"), Some("table"));
    assert_eq!(p.value("tag"), Some("-x"));
    assert!(p.flag("quiet"));
}

/// UC: UC-CLI-001
#[test]
fn repeated_options_keep_every_value_and_the_last_wins_for_single_reads() {
    let p = parse(&a(&["--tag", "a=1", "--tag", "b=2"]), &OPTS).unwrap();
    assert_eq!(p.values("tag"), ["a=1", "b=2"]);
    assert_eq!(p.value("tag"), Some("b=2"));
    assert!(p.values("name").is_empty());
}

/// UC: UC-CLI-001
#[test]
fn double_dash_ends_options_and_a_lone_dash_is_positional() {
    let p = parse(&a(&["--all", "--", "--name", "-"]), &OPTS).unwrap();
    assert_eq!(p.positional, ["--name", "-"]);
    assert!(p.flag("all"));
    assert_eq!(parse(&a(&["-"]), &OPTS).unwrap().positional, ["-"]);
}

/// UC: UC-CLI-005
#[test]
fn rejects_unknown_options_missing_values_and_values_on_flags() {
    assert!(parse(&a(&["--nada"]), &OPTS)
        .unwrap_err()
        .contains("--nada"));
    assert!(parse(&a(&["-z"]), &OPTS).unwrap_err().contains("-z"));
    assert!(parse(&a(&["--name"]), &OPTS)
        .unwrap_err()
        .contains("precisa de um valor"));
    assert!(parse(&a(&["--all=1"]), &OPTS)
        .unwrap_err()
        .contains("não recebe valor"));
    assert!(parse(&a(&["-qx"]), &OPTS).is_err());
}

/// UC: UC-CLI-005
#[test]
fn numbers_are_checked_without_calling_the_server() {
    let p = parse(&a(&["--name", "12", "--tag", "x"]), &OPTS).unwrap();
    assert_eq!(p.number("name"), Ok(Some(12)));
    assert_eq!(p.number("all"), Ok(None));
    assert!(p.number("tag").unwrap_err().contains("--tag"));
}
