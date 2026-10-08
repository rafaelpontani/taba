//! Os subcomandos `taba init`, `taba cert generate` e `taba token generate`, rodados como o
//! operador os roda (o binário de verdade), e a pasta que eles criam usada por um nó.

mod common;

use std::path::Path;
use std::process::{Command, Output};

use common::TestNode;
use serde_json::json;

fn taba(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_taba"))
        .args(args)
        .output()
        .expect("rodar o binário taba")
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn entries(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// UC: UC-SEG-019
#[test]
fn init_creates_the_certificate_and_the_root_token_and_refuses_to_run_twice() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("secrets");
    let out_arg = out.to_str().unwrap();

    let first = taba(&["init", "--out", out_arg]);
    assert!(first.status.success(), "{}", text(&first.stderr));
    let report = text(&first.stdout);
    assert!(report.contains("root-token") && report.contains(".crt") && report.contains(".key"));

    assert_eq!(entries(&out), ["root-token", "tls"]);
    let tls = entries(&out.join("tls"));
    assert_eq!(tls.len(), 2);
    assert!(tls[0].starts_with("cluster-") && tls[0].ends_with(".crt"));
    assert_eq!(
        tls[0].trim_end_matches(".crt"),
        tls[1].trim_end_matches(".key")
    );
    let token = std::fs::read_to_string(out.join("root-token")).unwrap();
    assert!(token.trim().len() >= 32);
    // O segredo não vai para a tela.
    assert!(!report.contains(token.trim()));

    // Rodar de novo não sobrescreve nada.
    let second = taba(&["init", "--out", out_arg]);
    assert!(!second.status.success());
    assert!(
        text(&second.stderr).contains("já existe"),
        "{}",
        text(&second.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(out.join("root-token")).unwrap(),
        token
    );
}

/// UC: UC-SEG-020
#[test]
fn cert_generate_adds_a_pair_named_after_its_start_date() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("tls");
    let dir_arg = dir.to_str().unwrap();

    let now = taba(&["cert", "generate", "--days", "30", "--out", dir_arg]);
    assert!(now.status.success(), "{}", text(&now.stderr));
    let later = taba(&[
        "cert",
        "generate",
        "--days",
        "30",
        "--starts-in",
        "14d",
        "--out",
        dir_arg,
    ]);
    assert!(later.status.success(), "{}", text(&later.stderr));
    let names = entries(&dir);
    assert_eq!(names.len(), 4, "{names:?}");
    assert_eq!(
        names.iter().filter(|n| n.ends_with(".crt")).count(),
        2,
        "a rotação não sobrescreve o par anterior"
    );

    let again = taba(&["cert", "generate", "--days", "30", "--out", dir_arg]);
    assert!(!again.status.success(), "mesma data de início: recusa");
    let bad = taba(&["cert", "generate", "--out", dir_arg]);
    assert_eq!(bad.status.code(), Some(2));
    assert!(text(&bad.stderr).contains("--days"));
}

/// UC: UC-SEG-021
#[test]
fn token_generate_prints_a_fresh_secret_each_time() {
    let a = taba(&["token", "generate"]);
    let b = taba(&["token", "generate"]);
    assert!(a.status.success() && b.status.success());
    let (a, b) = (text(&a.stdout), text(&b.stdout));
    assert_eq!(a.trim().len(), 43);
    assert_ne!(a, b);
}

/// UC: UC-AUT-002, UC-SEG-007
#[tokio::test]
async fn a_node_mounting_the_folder_from_init_turns_authentication_on_by_itself() {
    let tmp = tempfile::tempdir().unwrap();
    let secrets = tmp.path().join("secrets");
    let init = taba(&["init", "--out", secrets.to_str().unwrap()]);
    assert!(init.status.success(), "{}", text(&init.stderr));
    let root = std::fs::read_to_string(secrets.join("root-token")).unwrap();

    let node = TestNode::builder()
        .production()
        .env("TABA_SECRETS_DIR", secrets.to_str().unwrap())
        .start()
        .await;
    let call = |token: Option<&str>| {
        let mut request = node.client.get(node.client_url("/topics"));
        if let Some(token) = token {
            request = request.header("authorization", format!("Bearer {token}"));
        }
        async move { request.send().await.unwrap().status() }
    };
    assert_eq!(call(None).await, 401);
    assert_eq!(call(Some("outro")).await, 401);
    assert_eq!(call(Some(root.trim())).await, 200);

    let created = node
        .client
        .post(node.client_url("/tokens"))
        .header("authorization", format!("Bearer {}", root.trim()))
        .json(&json!({ "name": "app", "permissions": ["consume"] }))
        .send()
        .await
        .unwrap();
    assert_eq!(created.status(), 201);
    node.shutdown().await;
}
