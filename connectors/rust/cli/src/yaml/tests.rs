use super::*;

fn text(map: &[(String, Node)], path: &[&str]) -> Option<String> {
    let mut current = map;
    for (n, key) in path.iter().enumerate() {
        let node = &current.iter().find(|(k, _)| k == key)?.1;
        match node {
            Node::Text(t) if n + 1 == path.len() => return Some(t.clone()),
            Node::Map(m) => current = m,
            Node::Text(_) => return None,
        }
    }
    None
}

/// UC: UC-CLI-028
#[test]
fn reads_the_documented_example() {
    let doc = "default: local\nprofiles:\n  local:\n    url: http://127.0.0.1:8080\n    token-file: C:\\taba\\secrets\\root-token\n  homologacao:\n    url: https://taba.hml.exemplo.com\n    ca-file: /etc/ssl/taba-hml-ca.pem   # CA própria\n    token-file: ~/.taba/hml-token\n";
    let map = parse(doc).unwrap();
    assert_eq!(text(&map, &["default"]).as_deref(), Some("local"));
    assert_eq!(
        text(&map, &["profiles", "local", "url"]).as_deref(),
        Some("http://127.0.0.1:8080")
    );
    assert_eq!(
        text(&map, &["profiles", "local", "token-file"]).as_deref(),
        Some("C:\\taba\\secrets\\root-token")
    );
    assert_eq!(
        text(&map, &["profiles", "homologacao", "ca-file"]).as_deref(),
        Some("/etc/ssl/taba-hml-ca.pem")
    );
}

/// UC: UC-CLI-028
#[test]
fn handles_comments_blank_lines_bom_and_empty_documents() {
    assert!(parse("").unwrap().is_empty());
    assert!(parse("# só comentário\n\n   \n").unwrap().is_empty());
    let map = parse("\u{feff}# topo\na: b # fim\n\nc:   d  \n").unwrap();
    assert_eq!(text(&map, &["a"]).as_deref(), Some("b"));
    assert_eq!(text(&map, &["c"]).as_deref(), Some("d"));
    let map = parse("profiles: {}\n").unwrap();
    assert_eq!(map, [("profiles".to_owned(), Node::Map(vec![]))]);
    let map = parse("profiles:\n").unwrap();
    assert_eq!(map, [("profiles".to_owned(), Node::Map(vec![]))]);
}

/// UC: UC-CLI-028
#[test]
fn reads_quoted_scalars_and_keeps_hash_inside_quotes() {
    let map = parse("a: \"x # y \\\"z\\\" \\\\ \\u00e9\\n\"\nb: 'it''s # ok'\nc: http://h:80/p#frag\n\"d e\": ~\n").unwrap();
    assert_eq!(text(&map, &["a"]).as_deref(), Some("x # y \"z\" \\ é\n"));
    assert_eq!(text(&map, &["b"]).as_deref(), Some("it's # ok"));
    assert_eq!(text(&map, &["c"]).as_deref(), Some("http://h:80/p#frag"));
    assert_eq!(text(&map, &["d e"]).as_deref(), Some(""));
}

/// UC: UC-CLI-028
#[test]
fn reports_the_line_of_invalid_yaml() {
    for (doc, line) in [
        ("a: b\n\tc: d\n", 2),
        ("a: b\n- x\n", 2),
        ("a: b\n  c: d\n", 2),
        ("a: b\na: c\n", 2),
        ("a: \"aberta\n", 1),
        ("a: [1, 2]\n", 1),
        ("sem dois pontos\n", 1),
        ("a:\n    b: 1\n  c: 2\n", 3),
        ("a: |\n  texto\n", 1),
        ("a: &x 1\n", 1),
    ] {
        let e = parse(doc).unwrap_err();
        assert_eq!(e.line, line, "{doc:?}: {e}");
    }
}

/// UC: UC-CLI-028
#[test]
fn rendering_then_parsing_gives_the_same_values_for_awkward_text() {
    let values = [
        "http://127.0.0.1:8080",
        "C:\\taba\\secrets\\root-token",
        "",
        "tem # cerquilha",
        "dois: pontos",
        "  espaços  ",
        "true",
        "-começa",
        "aspas \" e ' juntas",
        "linha\nnova",
        "çã 中",
        "termina:",
    ];
    for v in values {
        let map = vec![("k".to_owned(), Node::Text(v.to_owned()))];
        let again = parse(&render(&map)).unwrap_or_else(|e| panic!("{v:?}: {e}\n{}", render(&map)));
        assert_eq!(again, map, "{v:?}");
    }
}

/// UC: UC-CLI-028
#[test]
fn renders_nested_maps_and_empty_maps() {
    let map = vec![
        ("default".to_owned(), Node::Text("a".to_owned())),
        (
            "profiles".to_owned(),
            Node::Map(vec![
                (
                    "a".to_owned(),
                    Node::Map(vec![("url".to_owned(), Node::Text("http://h".to_owned()))]),
                ),
                ("b".to_owned(), Node::Map(vec![])),
            ]),
        ),
    ];
    let text = render(&map);
    assert_eq!(
        text,
        "default: a\nprofiles:\n  a:\n    url: http://h\n  b: {}\n"
    );
    assert_eq!(parse(&text).unwrap(), map);
}
