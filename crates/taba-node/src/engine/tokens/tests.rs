use std::sync::Arc;
use std::time::Duration;

use super::*;
use crate::config::{Config, MapEnv};
use crate::core::clock::ManualClock;
use crate::storage::tokens::MemoryTokenStore;

struct Harness {
    tokens: Tokens,
    store: Arc<MemoryTokenStore>,
    clock: Arc<ManualClock>,
}

fn harness() -> Harness {
    let env = MapEnv::new([("TABA_REPLICATION_FACTOR", "1")]);
    let rules = TokenRules::from_config(&Config::parse(None, &env).unwrap());
    let store = Arc::new(MemoryTokenStore::default());
    let clock = Arc::new(ManualClock::at_doc_epoch());
    let tokens = Tokens::new(rules, store.clone(), clock.clone(), Vec::new());
    Harness {
        tokens,
        store,
        clock,
    }
}

fn request(name: &str, permissions: &[&str]) -> CreateToken {
    CreateToken {
        name: name.into(),
        permissions: permissions.iter().map(|p| (*p).to_owned()).collect(),
        ..Default::default()
    }
}

async fn create(h: &Harness, name: &str) -> Created {
    h.tokens.create(request(name, &["publish"])).await.unwrap()
}

fn code_of(error: TabaError) -> (ErrorCode, Vec<String>) {
    (
        error.code,
        error.details.iter().map(|d| d.field.clone()).collect(),
    )
}

/// UC: UC-AUT-022, UC-AUT-020
#[tokio::test]
async fn creating_returns_the_full_token_once_and_it_authenticates() {
    let h = harness();
    let created = h
        .tokens
        .create(CreateToken {
            topic_prefixes: vec!["pedidos".into()],
            ..request("servico-pedidos", &["create", "publish", "publish"])
        })
        .await
        .unwrap();
    assert!(created.token.starts_with("tb_"));
    assert!(created.token.contains(&created.record.id));
    assert_eq!(
        created.record.permissions,
        [Permission::Publish, Permission::Create],
        "no repetition, in the fixed order"
    );
    assert_eq!(created.record.created_ms, 1_791_288_000_000);

    let record = h.tokens.authenticate(&created.token).expect("valid token");
    assert_eq!(record.id, created.record.id);
    // Reading never returns the secret: the record only has the hash.
    let (_, secret) = token::parse(&created.token).unwrap();
    assert!(!format!("{:?}", h.tokens.get(&record.id).unwrap()).contains(secret));
}

/// UC: UC-AUT-008
#[tokio::test]
async fn every_invalid_token_is_just_none() {
    let h = harness();
    let created = create(&h, "a").await;
    let (id, secret) = token::parse(&created.token).unwrap();
    let other = create(&h, "b").await;
    let (_, other_secret) = token::parse(&other.token).unwrap();

    for bad in [
        String::new(),
        "lixo".into(),
        format!("tb_{id}.{other_secret}"),
        format!("tb_{id}.{secret}x"),
        format!("tb_00000000-0000-7000-8000-000000000000.{secret}"),
        format!("tb_{id}"),
    ] {
        assert!(h.tokens.authenticate(&bad).is_none(), "{bad:?}");
    }
}

/// UC: UC-AUT-023
#[tokio::test]
async fn invalid_requests_are_400_with_the_field() {
    let h = harness();
    for (req, code, field) in [
        (
            request("", &["publish"]),
            ErrorCode::ValidationInvalidName,
            "name",
        ),
        (
            request("with space", &["publish"]),
            ErrorCode::ValidationInvalidName,
            "name",
        ),
        (
            request(&"a".repeat(101), &["publish"]),
            ErrorCode::ValidationInvalidName,
            "name",
        ),
        (
            request("ok", &[]),
            ErrorCode::ValidationFailed,
            "permissions",
        ),
        (
            request("ok", &["publish", "voar"]),
            ErrorCode::ValidationFailed,
            "permissions[1]",
        ),
        (
            CreateToken {
                topic_prefixes: vec!["with space".into()],
                ..request("ok", &["publish"])
            },
            ErrorCode::ValidationFailed,
            "topicPrefixes[0]",
        ),
        (
            CreateToken {
                topic_prefixes: (0..21).map(|i| format!("p{i}")).collect(),
                ..request("ok", &["publish"])
            },
            ErrorCode::ValidationFailed,
            "topicPrefixes",
        ),
        (
            CreateToken {
                expires_ms: Some(1),
                ..request("ok", &["publish"])
            },
            ErrorCode::ValidationOutOfRange,
            "expiresAt",
        ),
    ] {
        let (got, fields) = code_of(h.tokens.create(req).await.unwrap_err());
        assert_eq!(got, code, "{field}");
        assert!(fields.iter().any(|f| f == field), "{field}: {fields:?}");
    }
    assert_eq!(h.tokens.count(), 0);
    assert!(h.store.ids().is_empty());
}

/// UC: UC-AUT-032
#[tokio::test]
async fn a_failed_write_leaves_no_visible_token() {
    let h = harness();
    h.store.set_failing(true);
    let error = h
        .tokens
        .create(request("a", &["publish"]))
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::InternalError);
    assert_eq!(h.tokens.count(), 0);

    h.store.set_failing(false);
    let created = create(&h, "a").await;
    h.store.set_failing(true);
    assert!(h.tokens.revoke(&created.record.id).await.is_err());
    assert!(
        h.tokens.authenticate(&created.token).is_some(),
        "revoking that failed on disk does not revoke"
    );
}

/// UC: UC-AUT-018, UC-AUT-008
#[tokio::test]
async fn an_expired_token_is_refused_and_the_sweep_erases_it() {
    let h = harness();
    let created = h
        .tokens
        .create(CreateToken {
            expires_ms: Some(1_791_288_000_000 + 60_000),
            ..request("short", &["consume"])
        })
        .await
        .unwrap();
    let forever = create(&h, "long").await;
    assert!(h.tokens.authenticate(&created.token).is_some());

    h.clock.advance(Duration::from_secs(60));
    assert!(h.tokens.authenticate(&created.token).is_none(), "vencido");
    assert!(h.tokens.get(&created.record.id).is_err());
    assert_eq!(h.tokens.list(ListQuery::default()).unwrap().items.len(), 1);
    assert_eq!(h.tokens.count(), 2, "still in memory until the sweep");

    assert_eq!(h.tokens.sweep().await, 1);
    assert_eq!(h.tokens.count(), 1);
    assert_eq!(h.store.ids(), std::slice::from_ref(&forever.record.id));
    assert_eq!(h.tokens.sweep().await, 0);
}

/// UC: UC-AUT-028
#[tokio::test]
async fn patch_changes_only_the_name_and_the_expiry() {
    let h = harness();
    let created = create(&h, "old").await;
    let id = created.record.id.clone();

    let updated = h
        .tokens
        .update(
            &id,
            UpdateToken {
                name: Some("new".into()),
                expires_ms: Patch::Value(1_791_288_000_000 + 1000),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(updated.name, "new");
    assert_eq!(updated.expires_ms, Some(1_791_288_001_000));
    assert_eq!(updated.secret_hash, created.record.secret_hash);
    assert!(
        h.tokens.authenticate(&created.token).is_some(),
        "the secret keeps applying"
    );

    let cleared = h
        .tokens
        .update(
            &id,
            UpdateToken {
                expires_ms: Patch::Null,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(cleared.expires_ms, None);
    assert_eq!(cleared.name, "new", "absent does not touch");

    for field in ["permissions", "topicPrefixes", "id"] {
        let error = h
            .tokens
            .update(
                &id,
                UpdateToken {
                    immutable_present: vec![field.into()],
                    ..Default::default()
                },
            )
            .await
            .unwrap_err();
        assert_eq!(
            code_of(error),
            (ErrorCode::ValidationImmutableField, vec![field.into()])
        );
    }
    let past = h
        .tokens
        .update(
            &id,
            UpdateToken {
                expires_ms: Patch::Value(5),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
    assert_eq!(past.code, ErrorCode::ValidationOutOfRange);
    assert_eq!(
        h.tokens
            .update("does-not-exist", UpdateToken::default())
            .await
            .unwrap_err()
            .code,
        ErrorCode::TokenNotFound
    );
}

/// UC: UC-AUT-029
#[tokio::test]
async fn revoking_makes_the_token_unknown_and_a_second_revoke_is_404() {
    let h = harness();
    let created = create(&h, "a").await;
    h.tokens.revoke(&created.record.id).await.unwrap();
    assert!(h.tokens.authenticate(&created.token).is_none());
    assert!(h.store.ids().is_empty());
    assert_eq!(
        h.tokens.revoke(&created.record.id).await.unwrap_err().code,
        ErrorCode::TokenNotFound
    );
    assert_eq!(
        h.tokens.get(&created.record.id).unwrap_err().code,
        ErrorCode::TokenNotFound
    );
}

/// UC: UC-AUT-026
#[tokio::test]
async fn listing_is_paginated_by_id_or_creation_and_filters_by_name() {
    let h = harness();
    let mut ids = Vec::new();
    for name in ["alfa", "beta", "alfa-2", "gama", "delta"] {
        ids.push(create(&h, name).await.record.id);
        h.clock.advance(Duration::from_millis(5));
    }
    let page = |sort: Option<&str>, name: Option<&str>, cursor: Option<String>, limit: &str| {
        h.tokens.list(ListQuery {
            limit: Some(limit.into()),
            cursor,
            sort: sort.map(Into::into),
            name: name.map(Into::into),
        })
    };

    let first = page(None, None, None, "2").unwrap();
    assert_eq!(
        first.items.iter().map(|t| t.id.clone()).collect::<Vec<_>>(),
        ids[..2]
    );
    let second = page(None, None, first.next_cursor.clone(), "2").unwrap();
    assert_eq!(
        second
            .items
            .iter()
            .map(|t| t.id.clone())
            .collect::<Vec<_>>(),
        ids[2..4]
    );
    let third = page(None, None, second.next_cursor.clone(), "2").unwrap();
    assert_eq!(third.items.len(), 1);
    assert!(third.next_cursor.is_none());

    let newest = page(Some("-createdAt"), None, None, "10").unwrap();
    let mut reversed = ids.clone();
    reversed.reverse();
    assert_eq!(
        newest
            .items
            .iter()
            .map(|t| t.id.clone())
            .collect::<Vec<_>>(),
        reversed
    );

    let named = page(None, Some("alfa"), None, "10").unwrap();
    assert_eq!(named.items.len(), 2);

    // `name` is not an ordering key (it is changeable).
    let (code, fields) = code_of(page(Some("name"), None, None, "10").err().unwrap());
    assert_eq!(
        (code, fields),
        (ErrorCode::ValidationFailed, vec!["query.sort".to_owned()])
    );
    // The cursor only applies to the `sort` and filter that generated it.
    let stale = page(Some("createdAt"), None, first.next_cursor, "2")
        .err()
        .unwrap();
    assert_eq!(stale.code, ErrorCode::ValidationInvalidCursor);
    assert_eq!(
        page(None, None, Some("lixo".into()), "2")
            .err()
            .unwrap()
            .code,
        ErrorCode::ValidationInvalidCursor
    );
}

/// UC: UC-AUT-032
#[tokio::test]
async fn the_store_keeps_the_tokens_that_were_created() {
    let h = harness();
    let created = create(&h, "a").await;
    let env = MapEnv::new([("TABA_REPLICATION_FACTOR", "1")]);
    let rules = TokenRules::from_config(&Config::parse(None, &env).unwrap());

    let stored = h.tokens.stored().await.unwrap();
    assert!(stored.iter().any(|r| r.id == created.record.id));

    // A registry built from nothing does not see them: the node takes them over only through the restoration.
    let fresh = Tokens::new(rules, h.store.clone(), h.clock.clone(), Vec::new());
    assert_eq!(fresh.count(), 0);
}
