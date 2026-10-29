use super::*;
use crate::config::Secret;
use crate::core::token::generate;

fn record(permissions: Vec<Permission>, prefixes: &[&str]) -> Arc<TokenRecord> {
    Arc::new(
        generate(
            "01927f3a-0000-7000-8000-000000000001".into(),
            "t".into(),
            permissions,
            prefixes.iter().map(|p| (*p).to_owned()).collect(),
            1,
            None,
        )
        .record,
    )
}

fn secret(text: &str) -> Secret {
    // `Secret` is only created through the configuration; `serde` is not on it, so it uses the parse.
    use crate::config::{Config, MapEnv};
    let env = MapEnv::new([
        ("TABA_CLUSTER_ENTRY_ADDRESS", "127.0.0.1:1"),
        ("TABA_AUTH_ROOT_TOKEN", text),
    ]);
    match Config::parse(None, &env).unwrap().auth.root_token {
        RootToken::Inline(secret) => secret,
        other => panic!("expected the inline token, got {other:?}"),
    }
}

const ROOT_A: &str = "raiz-raiz-raiz-raiz-raiz-raiz-raiz-aaaa";
const ROOT_B: &str = "raiz-raiz-raiz-raiz-raiz-raiz-raiz-bbbb";

/// UC: UC-AUT-017
#[test]
fn root_and_open_reach_everything_and_a_token_only_what_it_has() {
    for all in [Principal::Root, Principal::Open] {
        assert!(all.allows(Permission::Admin));
        assert!(all.reaches("anything"));
        assert!(all.prefixes().is_empty());
        assert!(all.covers_distribution_prefix("x"));
    }
    assert_eq!(Principal::Root.log_id(), "root");

    let token = Principal::Token(record(vec![Permission::Consume], &["pedidos"]));
    assert!(token.allows(Permission::Consume));
    assert!(!token.allows(Permission::Publish));
    assert!(token.reaches("pedidos-eu"));
    assert!(!token.reaches("estoque"));
    assert_eq!(token.log_id(), "01927f3a-0000-7000-8000-000000000001");
}

/// UC: UC-AUT-014
#[test]
fn a_distribution_prefix_must_sit_inside_the_token_prefixes() {
    let token = Principal::Token(record(vec![Permission::Create], &["pedidos", "fin-"]));
    assert!(token.covers_distribution_prefix("pedidos"));
    assert!(token.covers_distribution_prefix("pedidos-br"));
    assert!(token.covers_distribution_prefix("fin-x"));
    assert!(
        !token.covers_distribution_prefix("ped"),
        "would reach queues outside the token"
    );
    assert!(!token.covers_distribution_prefix("estoque"));
    assert!(
        !token.covers_distribution_prefix(""),
        "the empty prefix would match everything"
    );

    let open = Principal::Token(record(vec![Permission::Create], &[]));
    assert!(open.covers_distribution_prefix("anything"));
}

/// UC: UC-AUT-009, UC-AUT-010, UC-AUT-011, UC-AUT-024
#[test]
fn each_route_asks_for_its_own_permission() {
    let want = |method: Method, route: &str| match need(&method, route) {
        Some(Need::Permission(p)) => p.as_str().to_string(),
        Some(Need::Any) => "any".into(),
        Some(Need::Root) => "root".into(),
        None => "none".into(),
    };
    assert_eq!(want(Method::POST, "/topics"), "create");
    assert_eq!(want(Method::GET, "/topics"), "any");
    assert_eq!(want(Method::GET, "/topics/{topic}"), "any");
    assert_eq!(want(Method::PATCH, "/topics/{topic}"), "manage");
    assert_eq!(want(Method::DELETE, "/topics/{topic}"), "manage");
    assert_eq!(want(Method::POST, "/topics/{topic}/messages"), "publish");
    assert_eq!(want(Method::GET, "/topics/{topic}/messages"), "consume");
    assert_eq!(
        want(Method::DELETE, "/topics/{topic}/messages/{id}"),
        "consume"
    );
    assert_eq!(
        want(Method::PUT, "/topics/{topic}/messages/{id}/visibility"),
        "consume"
    );
    assert_eq!(
        want(Method::POST, "/topics/{topic}/messages/batch"),
        "publish"
    );
    assert_eq!(
        want(Method::POST, "/topics/{topic}/messages/ack"),
        "consume"
    );
    assert_eq!(
        want(Method::POST, "/topics/{topic}/messages/visibility"),
        "consume"
    );
    assert_eq!(want(Method::GET, "/tokens"), "admin");
    assert_eq!(want(Method::DELETE, "/tokens/{id}"), "admin");
    assert_eq!(want(Method::GET, "/health/live"), "none");
}

/// UC: UC-AUT-039
#[test]
fn the_monitoring_routes_split_between_any_token_and_the_root_token() {
    let want = |method: Method, route: &str| match need(&method, route) {
        Some(Need::Permission(p)) => p.as_str().to_string(),
        Some(Need::Any) => "any".into(),
        Some(Need::Root) => "root".into(),
        None => "none".into(),
    };
    for route in [
        "/auth/me",
        "/nodes",
        "/stats/topics",
        "/stats/topics/{topic}",
    ] {
        assert_eq!(want(Method::GET, route), "any", "{route}");
    }
    assert_eq!(want(Method::GET, "/stats/topics/{topic}/groups"), "any");
    for route in [
        "/cluster/status",
        "/nodes/{nodeId}/metrics",
        "/nodes/{nodeId}/drain",
        "/nodes/{nodeId}/backup",
        "/nodes/{nodeId}/backup/flush",
        "/cluster/backup",
        "/cluster/backup/flush",
    ] {
        assert_eq!(want(Method::POST, route), "admin", "{route}");
    }
    assert_eq!(want(Method::POST, "/cluster/stop"), "root");
    assert_eq!(want(Method::DELETE, "/topics/{topic}/messages"), "manage");
}

/// UC: UC-AUT-041
#[test]
fn every_route_of_the_client_document_has_its_requirement_decided() {
    // A route is matched by the template it was registered with, and the documents register it with the template
    // they show: a renamed parameter would silently make `need()` miss the route, and any valid token would reach
    // it. So every documented route, but the probes (open on purpose), must have an entry.
    let document: serde_json::Value = serde_json::from_str(&crate::http::openapi::document(
        crate::http::openapi::Api::Client,
    ))
    .unwrap();
    let mut unknown = Vec::new();
    for (route, item) in document["paths"].as_object().unwrap() {
        for method in item.as_object().unwrap().keys() {
            let method = Method::from_bytes(method.to_uppercase().as_bytes()).unwrap();
            let open = route == "/health/live" || route == "/health/ready";
            if need(&method, route).is_none() != open {
                unknown.push(format!("{method} {route}"));
            }
        }
    }
    assert!(
        unknown.is_empty(),
        "routes whose requirement is not in `need()` (or probes that have one): {unknown:?}"
    );
}

/// UC: UC-AUT-001
#[test]
fn without_a_root_token_nothing_matches() {
    let root = RootTokens::from_config(&RootToken::None);
    assert!(!root.matches(""));
    assert!(!root.matches("anything"));
    assert!(!root.has_file());
}

/// UC: UC-AUT-002
#[test]
fn an_inline_root_token_matches_only_itself() {
    let root = RootTokens::from_config(&RootToken::Inline(secret(ROOT_A)));
    assert!(root.matches(ROOT_A));
    assert!(!root.matches(ROOT_B));
    assert!(!root.matches(&ROOT_A[..ROOT_A.len() - 1]));
    assert!(!root.matches(""));
}

/// UC: UC-AUT-006
#[test]
fn the_root_token_file_takes_two_lines_and_is_reloaded() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("root-token");
    std::fs::write(&path, format!("{ROOT_A}\n")).unwrap();
    let root = RootTokens::from_config(&RootToken::File(path.clone()));
    assert!(root.has_file());
    assert!(root.matches(ROOT_A));
    assert!(!root.matches(ROOT_B));

    // During the swap the current and the previous both apply.
    std::fs::write(&path, format!("{ROOT_B}\n{ROOT_A}\n")).unwrap();
    root.reload();
    assert!(root.matches(ROOT_A) && root.matches(ROOT_B));

    // Afterwards, only the new one; and a third line is ignored.
    std::fs::write(
        &path,
        format!("{ROOT_B}\n\n   \nthird-line-third-line-ignored\n"),
    )
    .unwrap();
    root.reload();
    assert!(root.matches(ROOT_B));
    assert!(!root.matches(ROOT_A));
}

/// UC: UC-AUT-006
#[test]
fn a_failed_reload_keeps_the_previous_secret() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("root-token");
    std::fs::write(&path, ROOT_A).unwrap();
    let root = RootTokens::from_config(&RootToken::File(path.clone()));
    std::fs::remove_file(&path).unwrap();
    root.reload();
    assert!(
        root.matches(ROOT_A),
        "file vanished: it carries on with what it had"
    );
    std::fs::write(&path, "   \n").unwrap();
    root.reload();
    assert!(root.matches(ROOT_A), "empty file: same");
}
