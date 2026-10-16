use super::*;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// A catalog (`_version: 1`, then `key: "text"` lines) as key → text.
fn catalog(code: &str) -> BTreeMap<String, String> {
    let text = fs::read_to_string(root().join("locales").join(format!("{code}.yml"))).unwrap();
    let mut map = BTreeMap::new();
    for (n, line) in text.lines().enumerate() {
        let line = line.trim_end();
        if line.is_empty() || line.starts_with('#') || line.starts_with("_version") {
            continue;
        }
        let (key, value) = line
            .split_once(": ")
            .unwrap_or_else(|| panic!("{code}.yml line {}: no `key: value`", n + 1));
        let value = value
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .unwrap_or_else(|| panic!("{code}.yml line {}: the text must be quoted", n + 1));
        assert!(
            map.insert(key.to_owned(), value.to_owned()).is_none(),
            "{code}.yml repeats the key {key}"
        );
    }
    map
}

fn sources(dir: &Path, out: &mut Vec<(PathBuf, String)>) {
    for entry in fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            sources(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs")
            && path.file_name().is_some_and(|n| n != "tests.rs")
        {
            let text = fs::read_to_string(&path).unwrap();
            out.push((path, text));
        }
    }
}

fn tui_sources() -> String {
    let mut all = Vec::new();
    sources(&root().join("src").join("tui"), &mut all);
    // The catalog's own helpers and tests do not count as uses.
    all.into_iter()
        .filter(|(p, _)| !p.ends_with("i18n/mod.rs"))
        .map(|(_, t)| t)
        .collect::<Vec<_>>()
        .join("\n")
}

/// TUI: TUI-HLP-03
#[test]
fn every_catalog_has_exactly_the_keys_and_variables_of_en_us() {
    let source = catalog("en-US");
    for lang in LANGS {
        let other = catalog(lang.code());
        let missing: Vec<_> = source.keys().filter(|k| !other.contains_key(*k)).collect();
        let extra: Vec<_> = other.keys().filter(|k| !source.contains_key(*k)).collect();
        assert!(missing.is_empty(), "{} lacks {missing:?}", lang.code());
        assert!(extra.is_empty(), "{} has extra {extra:?}", lang.code());
        for (key, text) in &source {
            assert_eq!(
                variables(text),
                variables(&other[key]),
                "{} / {key}: the variables differ",
                lang.code()
            );
        }
    }
}

/// TUI: TUI-HLP-03
#[test]
fn every_plural_key_has_both_forms() {
    for lang in LANGS {
        let map = catalog(lang.code());
        for key in map.keys() {
            if let Some(base) = key.strip_suffix(".one") {
                assert!(map.contains_key(&format!("{base}.other")), "{key}");
            }
            if let Some(base) = key.strip_suffix(".other") {
                assert!(map.contains_key(&format!("{base}.one")), "{key}");
            }
        }
    }
}

/// TUI: TUI-HLP-03
#[test]
fn every_key_the_code_uses_exists_in_the_catalog() {
    let source = tui_sources();
    let known = catalog("en-US");
    for (opener, plural) in [(".t(\"", false), (".tw(\"", false), (".tn(\"", true)] {
        for part in source.split(opener).skip(1) {
            let key = part.split('"').next().unwrap();
            if key.contains('{') {
                continue;
            }
            let full = if plural {
                format!("{key}.one")
            } else {
                key.to_owned()
            };
            assert!(
                known.contains_key(&full),
                "the code uses {key}, the catalog lacks it"
            );
        }
    }
}

/// TUI: TUI-HLP-03
#[test]
fn every_key_of_the_catalog_is_used() {
    let source = tui_sources();
    for key in catalog("en-US").keys() {
        let base = key
            .strip_suffix(".one")
            .or_else(|| key.strip_suffix(".other"))
            .unwrap_or(key);
        // A key built at run time (`condition.{name}`) is used if any of its prefixes is written that way.
        let built = base
            .match_indices('.')
            .any(|(i, _)| source.contains(&format!("\"{}.{{", &base[..i])));
        let used = source.contains(&format!("\"{base}\"")) || built;
        assert!(used, "the catalog key {key} is not used by the code");
    }
}

/// TUI: TUI-HLP-03
#[test]
fn the_translation_falls_back_to_english_and_then_to_the_key() {
    assert_eq!(tr(Lang::PtBr, "help.title"), catalog("pt-BR")["help.title"]);
    assert_eq!(tr(Lang::EnUs, "no.such.key"), "no.such.key");
    assert_eq!(tr(Lang::PtBr, "no.such.key"), "no.such.key");
}

/// TUI: TUI-HLP-03
#[test]
fn variables_are_replaced() {
    let text = tr_with(Lang::EnUs, "notice.drained", &[("node", &"prod")]);
    assert!(text.contains("prod") && !text.contains("%{"), "{text}");
}

/// TUI: TUI-HLP-03
#[test]
fn plural_forms_follow_the_language() {
    assert_eq!(tr_n(Lang::EnUs, "cluster.nodes", 1), "1 node");
    assert_eq!(tr_n(Lang::EnUs, "cluster.nodes", 0), "0 nodes");
    assert_eq!(tr_n(Lang::PtBr, "cluster.nodes", 0), "0 nó");
    assert_eq!(tr_n(Lang::PtBr, "cluster.nodes", 2), "2 nós");
    assert_eq!(tr_n(Lang::EnUs, "cluster.nodes", 1500), "1,500 nodes");
}

/// TUI: TUI-HLP-03
#[test]
fn the_language_is_chosen_in_the_documented_order() {
    let env = |vars: &'static [(&'static str, &'static str)]| {
        move |name: &str| {
            vars.iter()
                .find(|(k, _)| *k == name)
                .map(|(_, v)| (*v).to_owned())
        }
    };
    let none = env(&[]);
    assert_eq!(detect(None, &none, None, None), Lang::EnUs);
    assert_eq!(detect(None, &none, None, Some("pt-BR")), Lang::PtBr);
    let lang = env(&[("LANG", "pt_BR.UTF-8")]);
    assert_eq!(detect(None, &lang, None, Some("en-US")), Lang::PtBr);
    assert_eq!(detect(None, &lang, Some("en-US"), None), Lang::EnUs);
    let var = env(&[("TABA_CLI_LANG", "en"), ("LANG", "pt_BR")]);
    assert_eq!(detect(None, &var, Some("pt-BR"), None), Lang::EnUs);
    assert_eq!(detect(Some("pt"), &var, None, None), Lang::PtBr);
    // A language we do not have is skipped, not an error.
    assert_eq!(detect(Some("fr"), &none, Some("pt-BR"), None), Lang::PtBr);
}

#[test]
fn locale_names_are_normalized() {
    for text in ["pt", "pt_BR", "pt-BR", "PT-br", "pt_BR.UTF-8", "pt_BR@euro"] {
        assert_eq!(Lang::parse(text), Some(Lang::PtBr), "{text}");
    }
    assert_eq!(Lang::parse("en_US.UTF-8"), Some(Lang::EnUs));
    assert_eq!(Lang::parse("C"), None);
    assert_eq!(Lang::parse(""), None);
}

#[test]
fn numbers_sizes_and_durations_follow_the_language() {
    assert_eq!(group(Lang::EnUs, 1_234_567), "1,234,567");
    assert_eq!(group(Lang::PtBr, 1_234_567), "1.234.567");
    assert_eq!(group(Lang::EnUs, 12), "12");
    assert_eq!(bytes(Lang::EnUs, 1536), "1.5 KiB");
    assert_eq!(bytes(Lang::PtBr, 1536), "1,5 KiB");
    assert_eq!(bytes(Lang::EnUs, 12), "12 B");
    assert_eq!(bytes(Lang::EnUs, 3 * 1024 * 1024 * 1024), "3.0 GiB");
    assert_eq!(duration(9), "9s");
    assert_eq!(duration(247), "4m 07s");
    assert_eq!(duration(3 * 3600 + 5 * 60), "3h 05m");
    assert_eq!(duration(90_000), "1d 01h");
}

#[test]
fn instants_are_read_and_written_in_utc() {
    let ms = parse_instant("2026-10-15T12:34:56.789Z").unwrap();
    assert_eq!(date_time(Lang::EnUs, ms), "2026-10-15 12:34:56");
    assert_eq!(date_time(Lang::PtBr, ms), "15/10/2026 12:34:56");
    assert_eq!(ms % 1000, 789);
    assert_eq!(parse_instant("1970-01-01T00:00:00Z"), Some(0));
    assert_eq!(
        parse_instant("2026-10-15T14:34:56+02:00"),
        parse_instant("2026-10-15T12:34:56Z")
    );
    assert_eq!(parse_instant("nonsense"), None);
    assert_eq!(clock(ms), "12:34:56");
}
