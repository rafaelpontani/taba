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

/// Código -> `true` se o caso está `(retirado)`. Também devolve os códigos repetidos.
fn documented_codes() -> (BTreeMap<String, bool>, Vec<String>) {
    let mut codes = BTreeMap::new();
    let mut duplicated = Vec::new();
    let mut files: Vec<PathBuf> = fs::read_dir(root().join("doc"))
        .expect("pasta doc/")
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or_default();
            name.starts_with("20") && name.ends_with(".md")
        })
        .collect();
    files.sort();
    assert!(!files.is_empty(), "nenhum doc/20*.md encontrado");

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

/// Código -> arquivos que o citam.
fn cited_codes() -> BTreeMap<String, BTreeSet<String>> {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    rust_files(&crate_dir.join("src"), &mut files);
    rust_files(&crate_dir.join("tests"), &mut files);

    let marker = format!("{} UC:", "///");
    let mut cited: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for file in files {
        let text = fs::read_to_string(&file).expect("ler fonte");
        for line in text.lines() {
            let Some(list) = line.trim_start().strip_prefix(marker.as_str()) else {
                continue;
            };
            for code in list.split(',').map(str::trim).filter(|c| !c.is_empty()) {
                let name = file
                    .strip_prefix(crate_dir)
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
                None => problems.push(format!("`{code}` (em {files}) não existe em doc/20*.md")),
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
