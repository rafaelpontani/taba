//! Verification of use-case coverage (doc 20, section 19.5).
//!
//! Reads the `UC-<AREA>-<NNN>` codes from the tables of `doc/20*.md` and the codes cited in the
//! tests, on the `UC:` comment line right above each test function, and fails
//! when:
//!
//! 1. a test cites a code that does not exist (or that is `(retirado)`);
//! 2. a code appears twice in the documentation.
//!
//! The third rule, "every documented case has at least one test", only fails with
//! `TABA_UC_STRICT=1`: the documentation has more than eight hundred cases and the implementation
//! is at the beginning. Without the variable the test prints the scoreboard (`cargo test -- --nocapture`).

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

const CODE_PREFIX: &str = "UC-";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn is_code(s: &str) -> bool {
    let parts: Vec<&str> = s.split('-').collect();
    parts.len() == 3
        && parts[0] == "UC"
        && parts[1].len() == 3
        && parts[1].chars().all(|c| c.is_ascii_uppercase())
        && parts[2].len() == 3
        && parts[2].chars().all(|c| c.is_ascii_digit())
}

/// The catalog of the use cases in a table (`| UC-XXX-NNN | case | source | level | tests |`): the
/// history in `doc/20*.md`, if it still exists.
fn catalogue_tables() -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(root().join("doc"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or_default();
            name.starts_with("20") && name.ends_with(".md")
        })
        .collect();
    files.sort();
    files
}

/// The cases that only exist as cards (`### UC-XXX-NNN`) in the generated catalogs `26a` and `26b`:
/// they count when the `doc/20*.md` history is no longer in the repository.
fn catalogue_cards() -> Vec<String> {
    let legacy = fs::read_dir(root().join("doc"))
        .into_iter()
        .flatten()
        .flatten()
        .any(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            name.starts_with("20") && name.ends_with(".md")
        });
    if legacy {
        return Vec::new();
    }
    let mut codes = Vec::new();
    for name in [
        "26a-casos-de-uso-implementados.md",
        "26b-casos-de-uso-planejados.md",
    ] {
        let Ok(text) = fs::read_to_string(root().join("doc/pt-BR").join(name)) else {
            continue;
        };
        for line in text.lines() {
            if let Some(code) = line.strip_prefix("### ")
                && is_code(code.trim())
            {
                codes.push(code.trim().to_owned());
            }
        }
    }
    codes
}

/// Code -> `true` if the case is `(retirado)`. Also returns the repeated codes.
fn documented_codes() -> (BTreeMap<String, bool>, Vec<String>) {
    let mut codes = BTreeMap::new();
    let mut duplicated = Vec::new();
    let files = catalogue_tables();
    for code in catalogue_cards() {
        if codes.insert(code.clone(), false).is_some() {
            duplicated.push(code);
        }
    }
    assert!(
        !files.is_empty() || !codes.is_empty(),
        "no use-case catalog found"
    );

    for file in files {
        let text = fs::read_to_string(&file).expect("read doc");
        for line in text.lines() {
            // Only table lines whose first field is the code: `| UC-MSG-003 | ... |`.
            let Some(rest) = line.strip_prefix("| ") else {
                continue;
            };
            let code = rest.split(' ').next().unwrap_or_default();
            if !is_code(code) {
                continue;
            }
            let retired = line.contains("(retirado");
            if codes.insert(code.to_owned(), retired).is_some() {
                duplicated.push(code.to_owned());
            }
        }
    }
    (codes, duplicated)
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// The source files of a connector folder (Rust, C#, Java, JavaScript and Python), at any depth, without the
/// build, output and dependency folders.
fn connector_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        if path.is_dir() {
            if ![
                "target",
                "bin",
                "obj",
                "dist",
                "build",
                ".work",
                "node_modules",
                "__pycache__",
                ".venv",
            ]
            .iter()
            .any(|skip| name == *skip)
            {
                connector_files(&path, out);
            }
        } else if path.extension().is_some_and(|e| {
            ["rs", "cs", "java", "js", "mjs", "py"]
                .iter()
                .any(|x| e == *x)
        }) {
            out.push(path);
        }
    }
}

/// The sources that may cite use cases, each with the folder the shown path is relative to:
/// the `src` and `tests` of this crate (relative to the crate), all of each connector in `connectors/<language>` and the
/// tests of `bench/` (relative to the repository root).
fn source_files() -> Vec<(PathBuf, PathBuf)> {
    let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut found = Vec::new();
    rust_files(&crate_dir.join("src"), &mut found);
    rust_files(&crate_dir.join("tests"), &mut found);
    let mut out: Vec<(PathBuf, PathBuf)> =
        found.into_iter().map(|f| (f, crate_dir.clone())).collect();
    if let Ok(entries) = fs::read_dir(root().join("connectors")) {
        for entry in entries.flatten() {
            let mut found = Vec::new();
            connector_files(&entry.path(), &mut found);
            out.extend(found.into_iter().map(|f| (f, root())));
        }
    }
    // The local benchmark (bench/): its tests are Node.js files.
    let mut found = Vec::new();
    connector_files(&root().join("bench"), &mut found);
    out.extend(found.into_iter().map(|f| (f, root())));
    out
}

/// The list of codes of a citation line: `/// UC: …` (Rust, C#, Java, JavaScript) or `# UC: …` (Python).
/// The markers are built here so this file does not cite itself.
fn citation(line: &str) -> Option<&str> {
    let slashes = format!("{} UC:", "///");
    let hash = format!("{} UC:", "#");
    let line = line.trim_start();
    line.strip_prefix(slashes.as_str())
        .or_else(|| line.strip_prefix(hash.as_str()))
}

/// Code -> files that cite it.
fn cited_codes() -> BTreeMap<String, BTreeSet<String>> {
    let files = source_files();

    let mut cited: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (file, base) in files {
        let text = fs::read_to_string(&file).expect("read source");
        for line in text.lines() {
            let Some(list) = citation(line) else {
                continue;
            };
            for code in list.split(',').map(str::trim).filter(|c| !c.is_empty()) {
                let name = file
                    .strip_prefix(&base)
                    .unwrap_or(&file)
                    .display()
                    .to_string();
                cited.entry(code.to_owned()).or_default().insert(name);
            }
        }
    }
    cited
}

#[test]
fn every_cited_code_exists_and_no_code_is_documented_twice() {
    let (documented, duplicated) = documented_codes();
    let cited = cited_codes();

    assert!(
        duplicated.is_empty(),
        "repeated codes in the documentation: {duplicated:?}"
    );

    let mut problems = Vec::new();
    for (code, files) in &cited {
        let files = files.iter().cloned().collect::<Vec<_>>().join(", ");
        if !code.starts_with(CODE_PREFIX) || !is_code(code) {
            problems.push(format!(
                "`{code}` (in {files}) does not have the format UC-<AREA>-<NNN>"
            ));
        } else {
            match documented.get(code) {
                None => problems.push(format!(
                    "`{code}` (in {files}) does not exist in the use-case catalog (doc/pt-BR/26*.md)"
                )),
                Some(true) => problems.push(format!("`{code}` (in {files}) is (retirado)")),
                Some(false) => {}
            }
        }
    }
    assert!(
        problems.is_empty(),
        "tests cite invalid use cases:\n  - {}",
        problems.join("\n  - ")
    );
}

#[test]
fn coverage_report_and_optional_strict_check() {
    let (documented, _) = documented_codes();
    let cited = cited_codes();
    let active: Vec<&String> = documented
        .iter()
        .filter(|(_, retired)| !**retired)
        .map(|(c, _)| c)
        .collect();
    let uncovered: Vec<&&String> = active
        .iter()
        .filter(|c| !cited.contains_key(c.as_str()))
        .collect();
    let covered = active.len() - uncovered.len();

    eprintln!(
        "use cases with at least one test: {covered} of {} ({} retired)",
        active.len(),
        documented.len() - active.len()
    );

    if std::env::var("TABA_UC_STRICT").is_ok_and(|v| v == "1") {
        assert!(
            uncovered.is_empty(),
            "{} use cases without a test, for example: {:?}",
            uncovered.len(),
            &uncovered[..uncovered.len().min(10)]
        );
    }
}

/// Marks the start of the generated cell of the "Testes" column, to regenerate the column without duplicating it.
const TESTS_MARK: &str = "<!--t-->";

/// Code -> the tests that cite it, as `file::function` (the file relative to the crate, or to the repository root for the connectors), in order.
/// The citation line counts for the first function that comes after it, skipping attributes, decorators and
/// other documentation lines.
fn tests_by_code() -> BTreeMap<String, BTreeSet<String>> {
    let mut files = source_files();
    files.sort();

    let mut tests: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (file, base) in files {
        let shown = file
            .strip_prefix(&base)
            .unwrap_or(&file)
            .display()
            .to_string()
            .replace('\\', "/");
        let text = fs::read_to_string(&file).expect("read source");
        let extension = file
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or_default()
            .to_owned();
        let mut pending: Vec<String> = Vec::new();
        for line in text.lines() {
            if let Some(list) = citation(line) {
                pending.extend(
                    list.split(',')
                        .map(str::trim)
                        .filter(|c| !c.is_empty())
                        .map(str::to_owned),
                );
                continue;
            }
            let line = line.trim_start();
            if pending.is_empty()
                || line.is_empty()
                || line.starts_with("#[")
                || (extension == "py" && (line.starts_with('#') || line.starts_with('@')))
                || (extension == "cs" && line.starts_with('['))
                || (extension == "java" && line.starts_with('@'))
                || line.starts_with("//")
            {
                continue;
            }
            // Rust: `fn name(`. C# and Java: `public async Task name(` (the last term before the parenthesis).
            // Python: `def name(`. JavaScript: `test('name', …)`.
            let name = match extension.as_str() {
                "cs" | "java" => line
                    .split('(')
                    .next()
                    .filter(|_| line.contains('('))
                    .and_then(|head| head.split_whitespace().last()),
                "py" => line
                    .split("def ")
                    .nth(1)
                    .and_then(|after| after.split('(').next()),
                "js" | "mjs" => line
                    .strip_prefix("test(")
                    .or_else(|| line.strip_prefix("it("))
                    .and_then(|after| after.split(['\'', '"']).nth(1)),
                _ => line
                    .split("fn ")
                    .nth(1)
                    .and_then(|after| after.split(['(', '<']).next()),
            };
            if let Some(name) = name {
                for code in pending.drain(..) {
                    tests
                        .entry(code)
                        .or_default()
                        .insert(format!("{shown}::{}", name.trim()));
                }
            } else {
                pending.clear();
            }
        }
    }
    tests
}

/// The text of `doc/20*.md` with the "Testes" column of each use-case table.
fn with_tests_column(text: &str, tests: &BTreeMap<String, BTreeSet<String>>) -> String {
    let mut out = Vec::new();
    let mut columns = 0;
    let mut after_header = false;
    for line in text.lines() {
        if line.starts_with("| Código |") {
            let base = line
                .trim_end()
                .strip_suffix("| Testes |")
                .unwrap_or_else(|| line.trim_end().trim_end_matches('|'))
                .trim_end();
            columns = base.matches('|').count() + 1;
            out.push(format!("{base} | Testes |"));
            after_header = true;
            continue;
        }
        if after_header {
            after_header = false;
            if line.starts_with("|---") {
                out.push(format!("{}|", "|---".repeat(columns)));
                continue;
            }
        }
        let code = line
            .strip_prefix("| ")
            .and_then(|rest| rest.split(' ').next())
            .filter(|c| is_code(c));
        let Some(code) = code else {
            out.push(line.to_owned());
            continue;
        };
        let base = match line.find(&format!(" | {TESTS_MARK}")) {
            Some(i) => &line[..i],
            None => line.trim_end().trim_end_matches('|').trim_end(),
        };
        let cell = tests
            .get(code)
            .filter(|_| !line.contains("(retirado"))
            .map(|names| {
                names
                    .iter()
                    .map(|n| format!("`{n}`"))
                    .collect::<Vec<_>>()
                    .join("<br>")
            })
            .unwrap_or_else(|| "—".to_owned());
        out.push(format!("{base} | {TESTS_MARK}{cell} |"));
    }
    let mut joined = out.join("\n");
    if text.ends_with('\n') {
        joined.push('\n');
    }
    joined
}

/// The "Testes" column of the use-case tables says in which file and function each case is tested,
/// and it is generated from here. After changing the tests or the cases, regenerate with
/// `TABA_UC_WRITE=1 cargo test --test uc_coverage`.
#[test]
fn the_tests_column_of_the_use_cases_is_up_to_date() {
    let tests = tests_by_code();
    let mut stale = Vec::new();
    for file in catalogue_tables() {
        let current = fs::read_to_string(&file).expect("read doc");
        let updated = with_tests_column(&current, &tests);
        if updated == current {
            continue;
        }
        if std::env::var("TABA_UC_WRITE").is_ok_and(|v| v == "1") {
            fs::write(&file, updated).expect("write doc");
        } else {
            stale.push(file.display().to_string());
        }
    }
    assert!(
        stale.is_empty(),
        "the Testes column is out of date in {stale:?}: run `TABA_UC_WRITE=1 cargo test --test uc_coverage`"
    );
}
