use super::*;

fn sample(permissions: Vec<Permission>, prefixes: &[&str]) -> NewToken {
    generate(
        "01927f3a-0000-7000-8000-000000000001".into(),
        "servico".into(),
        permissions,
        prefixes.iter().map(|p| (*p).to_owned()).collect(),
        1000,
        None,
    )
}

/// UC: UC-AUT-020
#[test]
fn the_token_is_tb_id_dot_a_256_bit_secret() {
    let new = sample(vec![Permission::Publish], &[]);
    let (id, secret) = parse(&new.token).expect("formato tb_<id>.<segredo>");
    assert_eq!(id, "01927f3a-0000-7000-8000-000000000001");
    assert_eq!(id, new.record.id);
    // 32 bytes in unpadded base64: 43 characters.
    assert_eq!(secret.len(), 43);
    assert_eq!(URL_SAFE_NO_PAD.decode(secret).unwrap().len(), 32);
}

/// UC: UC-AUT-020
#[test]
fn only_a_salted_hash_is_kept_and_it_verifies_the_secret() {
    let new = sample(vec![Permission::Consume], &[]);
    let (_, secret) = parse(&new.token).unwrap();
    assert!(!new.record.secret_hash.contains(secret));
    assert!(new.record.matches_secret(secret));
    assert!(!new.record.matches_secret("another-secret"));
    assert!(!new.record.matches_secret(""));
}

/// UC: UC-AUT-020
#[test]
fn every_token_has_its_own_salt_so_equal_secrets_hash_differently() {
    let a = sample(vec![Permission::Consume], &[]);
    let b = sample(vec![Permission::Consume], &[]);
    assert_ne!(a.record.salt, b.record.salt);
    assert_ne!(a.token, b.token);
    // The same secret with another salt gives another hash.
    let (_, secret) = parse(&a.token).unwrap();
    let salt = URL_SAFE_NO_PAD.decode(&b.record.salt).unwrap();
    assert_ne!(
        URL_SAFE_NO_PAD.encode(hash(&salt, secret)),
        a.record.secret_hash
    );
}

/// UC: UC-AUT-008
#[test]
fn malformed_tokens_do_not_parse() {
    for bad in [
        "",
        "tb_",
        "tb_abc",
        "tb_.segredo",
        "tb_id.",
        "abc.def",
        "Bearer x",
    ] {
        assert!(parse(bad).is_none(), "{bad:?}");
    }
    assert_eq!(parse("tb_a.b.c"), Some(("a", "b.c")));
}

/// UC: UC-AUT-009
#[test]
fn manage_includes_create_and_admin_includes_manage_and_nothing_else_does() {
    let manager = sample(vec![Permission::Manage], &[]).record;
    assert!(manager.allows(Permission::Manage));
    assert!(manager.allows(Permission::Create));
    assert!(!manager.allows(Permission::Admin));
    assert!(manager.allows(Permission::Publish));
    assert!(manager.allows(Permission::Consume));

    let admin = sample(vec![Permission::Admin], &[]).record;
    assert!(admin.allows(Permission::Admin));
    assert!(admin.allows(Permission::Manage));
    assert!(admin.allows(Permission::Create));
    assert!(admin.allows(Permission::Publish));
    assert!(admin.allows(Permission::Consume));

    let creator = sample(vec![Permission::Create], &[]).record;
    assert!(creator.allows(Permission::Create));
    assert!(!creator.allows(Permission::Manage));
    assert!(!creator.allows(Permission::Admin));
    assert!(!creator.allows(Permission::Publish));
    assert!(!creator.allows(Permission::Consume));
}

/// UC: UC-AUT-009
#[test]
fn the_name_admin_of_schema_one_reads_as_manage() {
    assert_eq!(Permission::parse_legacy("admin"), Some(Permission::Manage));
    assert_eq!(Permission::parse_legacy("manage"), Some(Permission::Manage));
    assert_eq!(
        Permission::parse_legacy("publish"),
        Some(Permission::Publish)
    );
    assert_eq!(Permission::parse("admin"), Some(Permission::Admin));
}

/// UC: UC-AUT-012, UC-AUT-015
#[test]
fn prefixes_limit_the_topics_a_token_reaches() {
    let open = sample(vec![Permission::Consume], &[]).record;
    assert!(open.reaches("anything"));

    let scoped = sample(vec![Permission::Consume], &["pedidos", "fin-"]).record;
    assert!(scoped.reaches("pedidos"));
    assert!(scoped.reaches("pedidos-eu"));
    assert!(
        scoped.reaches("pedidos-dead-letter"),
        "the dead-letter starts with the queue name"
    );
    assert!(scoped.reaches("fin-x"));
    assert!(!scoped.reaches("estoque"));
    assert!(!scoped.reaches("Orders"), "capitals count");
}

/// UC: UC-AUT-018
#[test]
fn a_token_expires_at_the_given_instant() {
    let mut record = sample(vec![Permission::Consume], &[]).record;
    assert!(
        !record.expired_at(u64::MAX),
        "with no deadline, it never expires"
    );
    record.expires_ms = Some(5000);
    assert!(!record.expired_at(4999));
    assert!(record.expired_at(5000));
}

#[test]
fn constant_time_eq_compares_whole_values() {
    assert!(constant_time_eq(b"abc", b"abc"));
    assert!(!constant_time_eq(b"abc", b"abd"));
    assert!(!constant_time_eq(b"abc", b"abcd"));
    assert!(constant_time_eq(b"", b""));
}

#[test]
fn permissions_parse_by_their_documented_names() {
    for p in Permission::ALL {
        assert_eq!(Permission::parse(p.as_str()), Some(p));
    }
    assert_eq!(Permission::parse("write"), None);
    assert_eq!(Permission::parse("Admin"), None);
}
