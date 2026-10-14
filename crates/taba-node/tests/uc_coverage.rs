//! Verificação de cobertura dos casos de uso (doc 20, seção 19.5).
//!
//! Lê os códigos `UC-<ÁREA>-<NNN>` das tabelas de `doc/20*.md` e os códigos citados nos
//! testes, na linha de comentário `UC:` logo acima de cada função de teste, e falha
//! quando:
//!
//! 1. um teste cita um código que não existe (ou que está `(retirado)`);
//! 2. um código aparece duas vezes na documentação.
//!
//! A terceira regra, "todo caso documentado tem ao menos um teste", só falha com
//! `TABA_UC_STRICT=1`: a documentação tem mais de oitocentos casos e a implementação
//! está no começo. Sem a variável o teste imprime o placar (`cargo test -- --nocapture`).

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

/// O catálogo dos casos de uso em tabela (`| UC-XXX-NNN | caso | fonte | nível | testes |`): o
/// histórico em `doc/20*.md`, se ainda existir.
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

/// Os casos que só existem como cartões (`### UC-XXX-NNN`) nos catálogos gerados `26a` e `26b`:
/// valem quando o histórico `doc/20*.md` já não está no repositório.
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

/// Código -> `true` se o caso está `(retirado)`. Também devolve os códigos repetidos.
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
        "nenhum catálogo de casos de uso encontrado"
    );

    for file in files {
        let text = fs::read_to_string(&file).expect("ler doc");
        for line in text.lines() {
            // Só linhas de tabela cujo primeiro campo é o código: `| UC-MSG-003 | ... |`.
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

/// Os `.rs` e `.cs` de uma pasta de conector, em qualquer profundidade, sem as pastas de build e de saída.
fn connector_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        if path.is_dir() {
            if !["target", "bin", "obj", "dist"]
                .iter()
                .any(|skip| name == *skip)
            {
                connector_files(&path, out);
            }
        } else if path.extension().is_some_and(|e| e == "rs" || e == "cs") {
            out.push(path);
        }
    }
}

/// Os fontes que podem citar casos de uso, cada um com a pasta a que o caminho mostrado é relativo:
/// o `src` e o `tests` deste crate (relativos ao crate) e todos os de cada conector em `connectors/<linguagem>`
/// (relativos à raiz do repositório).
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
    out
}

/// Código -> arquivos que o citam.
fn cited_codes() -> BTreeMap<String, BTreeSet<String>> {
    let files = source_files();

    let marker = format!("{} UC:", "///");
    let mut cited: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (file, base) in files {
        let text = fs::read_to_string(&file).expect("ler fonte");
        for line in text.lines() {
            let Some(list) = line.trim_start().strip_prefix(marker.as_str()) else {
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
        "códigos repetidos na documentação: {duplicated:?}"
    );

    let mut problems = Vec::new();
    for (code, files) in &cited {
        let files = files.iter().cloned().collect::<Vec<_>>().join(", ");
        if !code.starts_with(CODE_PREFIX) || !is_code(code) {
            problems.push(format!(
                "`{code}` (em {files}) não tem o formato UC-<ÁREA>-<NNN>"
            ));
        } else {
            match documented.get(code) {
                None => problems.push(format!(
                    "`{code}` (em {files}) não existe no catálogo de casos de uso (doc/pt-BR/26*.md)"
                )),
                Some(true) => problems.push(format!("`{code}` (em {files}) está (retirado)")),
                Some(false) => {}
            }
        }
    }
    assert!(
        problems.is_empty(),
        "testes citam casos de uso inválidos:\n  - {}",
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
        "casos de uso com ao menos um teste: {covered} de {} ({} retirados)",
        active.len(),
        documented.len() - active.len()
    );

    if std::env::var("TABA_UC_STRICT").is_ok_and(|v| v == "1") {
        assert!(
            uncovered.is_empty(),
            "{} casos de uso sem teste, por exemplo: {:?}",
            uncovered.len(),
            &uncovered[..uncovered.len().min(10)]
        );
    }
}

/// Marca o começo da célula gerada da coluna "Testes", para regerar a coluna sem duplicá-la.
const TESTS_MARK: &str = "<!--t-->";

/// Código -> os testes que o citam, como `arquivo::função` (o arquivo relativo ao crate, ou à raiz do repositório para os conectores), em ordem.
/// A linha `/// UC:` vale para a primeira função que vem depois dela, pulando atributos e outras
/// linhas de documentação.
fn tests_by_code() -> BTreeMap<String, BTreeSet<String>> {
    let mut files = source_files();
    files.sort();

    let marker = format!("{} UC:", "///");
    let mut tests: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (file, base) in files {
        let shown = file
            .strip_prefix(&base)
            .unwrap_or(&file)
            .display()
            .to_string()
            .replace('\\', "/");
        let text = fs::read_to_string(&file).expect("ler fonte");
        let is_csharp = file.extension().is_some_and(|e| e == "cs");
        let mut pending: Vec<String> = Vec::new();
        for line in text.lines() {
            let line = line.trim_start();
            if let Some(list) = line.strip_prefix(marker.as_str()) {
                pending.extend(
                    list.split(',')
                        .map(str::trim)
                        .filter(|c| !c.is_empty())
                        .map(str::to_owned),
                );
                continue;
            }
            if pending.is_empty()
                || line.is_empty()
                || line.starts_with("#[")
                || (is_csharp && line.starts_with('['))
                || line.starts_with("//")
            {
                continue;
            }
            // Rust: `fn nome(`. C#: `public async Task nome(` (o último termo antes do parêntese).
            let name = if is_csharp {
                line.split('(')
                    .next()
                    .filter(|_| line.contains('('))
                    .and_then(|head| head.split_whitespace().last())
            } else {
                line.split("fn ")
                    .nth(1)
                    .and_then(|after| after.split(['(', '<']).next())
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

/// O texto de `doc/20*.md` com a coluna "Testes" de cada tabela de casos de uso.
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

/// A coluna "Testes" das tabelas dos casos de uso diz em que arquivo e função cada caso é testado,
/// e é gerada daqui. Depois de mexer nos testes ou nos casos, regere com
/// `TABA_UC_WRITE=1 cargo test --test uc_coverage`.
#[test]
fn the_tests_column_of_the_use_cases_is_up_to_date() {
    let tests = tests_by_code();
    let mut stale = Vec::new();
    for file in catalogue_tables() {
        let current = fs::read_to_string(&file).expect("ler doc");
        let updated = with_tests_column(&current, &tests);
        if updated == current {
            continue;
        }
        if std::env::var("TABA_UC_WRITE").is_ok_and(|v| v == "1") {
            fs::write(&file, updated).expect("gravar doc");
        } else {
            stale.push(file.display().to_string());
        }
    }
    assert!(
        stale.is_empty(),
        "a coluna Testes está desatualizada em {stale:?}: rode `TABA_UC_WRITE=1 cargo test --test uc_coverage`"
    );
}
