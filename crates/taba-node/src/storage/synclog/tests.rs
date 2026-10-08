use super::*;

fn headers() -> Vec<(String, String)> {
    vec![("content-type".into(), "application/json".into())]
}

/// UC: UC-SIN-010, UC-SIN-013
#[test]
fn events_are_kept_in_arrival_order_and_read_oldest_first() {
    let dir = tempfile::tempdir().unwrap();
    let log = SyncLog::create(dir.path()).unwrap();
    assert!(log.is_empty().unwrap());
    let first = log
        .append("POST", "/cluster/a", &headers(), b"{\"n\":1}")
        .unwrap();
    let second = log
        .append("POST", "/cluster/b", &headers(), b"{\"n\":2}")
        .unwrap();
    assert!(second > first, "o número de sequência cresce");
    assert_eq!(log.len().unwrap(), 2);

    let oldest = log.oldest().unwrap().unwrap();
    assert_eq!(oldest.path, "/cluster/a");
    assert_eq!(oldest.headers, headers());
    assert_eq!(oldest.body, b"{\"n\":1}");
    // Não há posição guardada: o que resta na tabela é o que falta processar.
    log.delete(oldest.seq).unwrap();
    assert_eq!(log.oldest().unwrap().unwrap().path, "/cluster/b");
    log.delete(second).unwrap();
    assert!(log.oldest().unwrap().is_none());
    assert!(log.is_empty().unwrap());
}

/// UC: UC-SIN-010
#[test]
fn the_database_lives_in_the_sync_folder_and_goes_away_with_discard() {
    let dir = tempfile::tempdir().unwrap();
    let log = SyncLog::create(dir.path()).unwrap();
    assert!(dir.path().join("_sync").join("sync.db").is_file());
    log.append("POST", "/x", &[], b"{}").unwrap();
    assert!(log.size_bytes().unwrap() > 0);
    log.discard().unwrap();
    assert!(!dir.path().join("_sync").exists());

    // Uma sincronização que ficou pela metade não deixa lixo para a seguinte.
    let first = SyncLog::create(dir.path()).unwrap();
    first.append("POST", "/velho", &[], b"{}").unwrap();
    drop(first);
    let again = SyncLog::create(dir.path()).unwrap();
    assert!(again.is_empty().unwrap());
}

/// UC: UC-SIN-016
#[test]
fn the_ids_of_the_load_are_kept_by_block() {
    let dir = tempfile::tempdir().unwrap();
    let log = SyncLog::create(dir.path()).unwrap();
    log.mark_loaded(
        "00000000000000aa",
        &[("t1".into(), "b".into()), ("t1".into(), "a".into())],
    )
    .unwrap();
    log.mark_loaded("00000000000000bb", &[("t2".into(), "c".into())])
        .unwrap();
    assert_eq!(
        log.loaded_in("00000000000000aa").unwrap(),
        vec![
            ("t1".to_owned(), "a".to_owned()),
            ("t1".to_owned(), "b".to_owned())
        ]
    );
    assert_eq!(log.loaded_in("00000000000000bb").unwrap().len(), 1);
    assert!(log.loaded_in("00000000000000cc").unwrap().is_empty());
}
