use std::time::Duration;

use super::super::*;

fn meta(id: &str) -> MessageMeta {
    MessageMeta {
        id: id.into(),
        content_type: "application/json".into(),
        group_id: None,
        deduplication_id: None,
        published_ms: 1_000,
        delivery_count: 0,
        invisible_until_ms: 0,
        receipt_token: None,
        dead_lettered_ms: None,
        dead_letter_reason: None,
        position: 0,
    }
}

fn store(dir: &Path) -> MessageStore {
    MessageStore::new(dir, DbSynchronous::Normal)
}

/// Lê direto do banco, sem a fila: o que já foi gravado.
fn committed(store: &MessageStore, topic: &str, id: &str) -> bool {
    let disk = store.disk(topic, false).unwrap().unwrap();
    disk.read(|conn| {
        conn.query_row(
            "SELECT COUNT(*) FROM messages WHERE id = ?1",
            params![id],
            |row| row.get::<_, i64>(0),
        )
    })
    .unwrap()
        == 1
}

/// Dentro de um escopo adiado a escrita só entra na fila: o banco ainda não a tem, mas quem lê pelo `id` (de
/// qualquer thread) já vê a linha e o corpo, e quem lê por faixa espera a fila. Depois do commit, está no banco.
///
/// UC: UC-ARM-013
#[test]
fn a_queued_write_is_seen_by_every_read_before_its_commit() {
    let tmp = tempfile::tempdir().unwrap();
    let store = store(tmp.path());
    store.put("orders", &meta("seed"), b"s").unwrap();
    let ((), written) = store.deferred(|| {
        store.put("orders", &meta("a1"), b"body").unwrap();
        let mut delivered = meta("seed");
        delivered.delivery_count = 1;
        store.update_delivery("orders", &delivered).unwrap();
        store.remove("orders", "seed").unwrap();
    });
    assert!(!committed(&store, "orders", "a1"), "still in the queue");
    std::thread::scope(|s| {
        s.spawn(|| {
            assert_eq!(store.get("orders", "a1").unwrap(), Some(meta("a1")));
            assert_eq!(store.body("orders", "a1").unwrap(), b"body");
            assert!(store.has_body("orders", "a1"));
            assert_eq!(store.get("orders", "seed").unwrap(), None);
            assert!(store.get_live("orders", "seed").unwrap().is_none());
        })
        .join()
        .unwrap();
    });
    // A leitura por faixa espera a fila: a contagem já é a do commit.
    assert_eq!(store.count("orders").unwrap(), 1);
    written.wait().unwrap();
    assert!(committed(&store, "orders", "a1"));
    assert!(!committed(&store, "orders", "seed"));
}

/// Várias threads escrevendo no mesmo tópico ao mesmo tempo, cada uma esperando a rodada em que a sua escrita entrou:
/// as escritas de cada uma chegam em ordem e nenhuma se perde.
///
/// UC: UC-ARM-013
#[test]
fn concurrent_writers_share_commits_and_lose_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let store = store(tmp.path());
    store.put("orders", &meta("seed"), b"s").unwrap();
    let threads = 8;
    let each = 200;
    std::thread::scope(|s| {
        for t in 0..threads {
            let store = &store;
            s.spawn(move || {
                for i in 0..each {
                    let id = format!("t{t}-{i:04}");
                    store.put("orders", &meta(&id), id.as_bytes()).unwrap();
                    let mut delivered = meta(&id);
                    delivered.delivery_count = 1;
                    store.update_delivery("orders", &delivered).unwrap();
                    if i % 2 == 0 {
                        store.remove("orders", &id).unwrap();
                    }
                }
            });
        }
    });
    let reopened = MessageStore::new(tmp.path(), DbSynchronous::Normal);
    drop(store);
    assert_eq!(reopened.count("orders").unwrap(), 1 + threads * each / 2);
    for t in 0..threads {
        for i in (1..each).step_by(2) {
            let id = format!("t{t}-{i:04}");
            let got = reopened.get("orders", &id).unwrap().expect("kept");
            assert_eq!(got.delivery_count, 1, "{id}");
            assert_eq!(reopened.body("orders", &id).unwrap(), id.as_bytes());
        }
    }
}

/// Uma escrita recusada pelo banco é erro só para quem a fez: as outras do mesmo commit ficam, e quem esperou um lote
/// adiado com ela recebe o erro.
///
/// UC: UC-ARM-013
#[test]
fn a_refused_write_fails_only_its_caller_and_the_rest_of_the_round_commits() {
    let tmp = tempfile::tempdir().unwrap();
    let store = store(tmp.path());
    store.put("orders", &meta("seed"), b"s").unwrap();
    store
        .disk("orders", false)
        .unwrap()
        .unwrap()
        .conn()
        .execute_batch(
            "CREATE TRIGGER refuse BEFORE INSERT ON messages WHEN NEW.id = 'bad' \
             BEGIN SELECT RAISE(ABORT, 'refused'); END",
        )
        .unwrap();
    assert!(store.put("orders", &meta("bad"), b"x").is_err());
    let ((), written) = store.deferred(|| {
        for id in ["a1", "bad", "a2"] {
            store.put("orders", &meta(id), b"x").unwrap();
        }
    });
    assert!(written.wait().is_err(), "the batch had a refused write");
    assert!(committed(&store, "orders", "a1"));
    assert!(committed(&store, "orders", "a2"));
    assert!(!committed(&store, "orders", "bad"));
    // A fila continua: a próxima escrita grava.
    store.put("orders", &meta("after"), b"y").unwrap();
    assert!(committed(&store, "orders", "after"));
}

/// As leituras pelo `id` vão pelas conexões só de leitura: não esperam a conexão de escrita, nem quando ela está
/// presa numa escrita longa.
///
/// UC: UC-ARM-013
#[test]
fn a_read_by_id_does_not_wait_for_the_writer() {
    let tmp = tempfile::tempdir().unwrap();
    let store = store(tmp.path());
    store.put("orders", &meta("a1"), b"x").unwrap();
    let disk = store.disk("orders", false).unwrap().unwrap();
    // A conexão de escrita fica presa (uma escrita longa); a leitura vai pelas conexões de leitura.
    let held = disk.conn();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::scope(|s| {
        s.spawn(|| {
            let found = store.get("orders", "a1").unwrap().is_some()
                && store.body("orders", "a1").unwrap() == b"x";
            tx.send(found).unwrap();
        });
        let found = rx.recv_timeout(Duration::from_secs(10));
        drop(held);
        assert_eq!(found, Ok(true), "the read did not wait for the writer");
    });
}

/// Um banco de antes tem os índices `messages_group_dedup` e `messages_delete_at` inteiros: ao abrir, eles viram
/// parciais (só as linhas com `deduplication_id` e com `delete_at`), e as linhas curtas do ack FIFO ainda saem pela
/// varredura.
///
/// UC: UC-ARM-030
#[test]
fn the_indexes_of_fifo_columns_become_partial_when_an_old_database_opens() {
    let tmp = tempfile::tempdir().unwrap();
    {
        let store = store(tmp.path());
        store.put("orders", &meta("a1"), b"x").unwrap();
        let disk = store.disk("orders", false).unwrap().unwrap();
        disk.conn()
            .execute_batch(
                "DROP INDEX messages_group_dedup; DROP INDEX messages_delete_at;
                 CREATE INDEX messages_group_dedup ON messages (group_id, deduplication_id);
                 CREATE INDEX messages_delete_at ON messages (delete_at);",
            )
            .unwrap();
    }
    let store = store(tmp.path());
    let mut m = meta("a2");
    m.group_id = Some("g".into());
    m.deduplication_id = Some("k".into());
    store.tombstone("orders", &m, 5_000).unwrap();
    let disk = store.disk("orders", false).unwrap().unwrap();
    for name in ["messages_group_dedup", "messages_delete_at"] {
        let sql: String = disk
            .conn()
            .query_row(
                "SELECT sql FROM sqlite_master WHERE name = ?1",
                params![name],
                |row| row.get(0),
            )
            .unwrap();
        assert!(sql.contains("WHERE"), "{name}: {sql}");
    }
    assert_eq!(store.sweep_tombstones("orders", 6_000).unwrap(), 1);
    assert!(store.get("orders", "a2").unwrap().is_none());
    assert!(store.get("orders", "a1").unwrap().is_some());
}

fn ids(rows: Vec<ScanRow>) -> Vec<String> {
    rows.into_iter().map(|row| row.id).collect()
}

/// As páginas e a `expiry` não esperam a fila (D193): com a conexão de escrita presa e escritas na fila, a página
/// já mostra a mensagem nova, tira a removida, filtra pelos metadados novos e pelos pares da fila (um gravado e um
/// apagado), e a paginação em pedaços passa por tudo. Depois do commit, as mesmas respostas.
///
/// UC: UC-ARM-013
#[test]
fn a_page_merges_the_queue_without_waiting_for_the_writer() {
    let tmp = tempfile::tempdir().unwrap();
    let store = store(tmp.path());
    for id in ["m01", "m02", "m03", "m04", "m05"] {
        store.put("orders", &meta(id), b"x").unwrap();
    }
    store
        .expiry_apply("orders", &[(true, 900, "m05".into())])
        .unwrap();
    let ((), written) = store.deferred(|| {
        store.remove("orders", "m02").unwrap();
        let mut delivered = meta("m03");
        delivered.delivery_count = 1;
        store.update_delivery("orders", &delivered).unwrap();
        store.put("orders", &meta("m06"), b"y").unwrap();
        store
            .expiry_apply(
                "orders",
                &[(false, 900, "m05".into()), (true, 950, "m04".into())],
            )
            .unwrap();
    });
    let page = |query: PageQuery| ids(store.page("orders", &query).unwrap());
    let check = || {
        let all = PageQuery {
            limit: 64,
            ..PageQuery::default()
        };
        assert_eq!(page(all.clone()), ["m01", "m03", "m04", "m05", "m06"]);
        let delivered = PageQuery {
            delivered: true,
            ..all.clone()
        };
        assert_eq!(page(delivered), ["m03"]);
        let off_table = PageQuery {
            off_table: true,
            ..all.clone()
        };
        assert_eq!(page(off_table), ["m01", "m03", "m05", "m06"]);
        let mut walked = Vec::new();
        let mut after = None;
        loop {
            let rows = page(PageQuery {
                after: after.clone(),
                limit: 2,
                ..PageQuery::default()
            });
            let full = rows.len() == 2;
            after = rows.last().cloned();
            walked.extend(rows);
            if !full {
                break;
            }
        }
        assert_eq!(walked, ["m01", "m03", "m04", "m05", "m06"]);
        let newest = PageQuery {
            desc: true,
            limit: 2,
            ..PageQuery::default()
        };
        assert_eq!(page(newest), ["m06", "m05"]);
        assert_eq!(store.expiry_find("orders", "m04").unwrap(), Some(950));
        assert_eq!(store.expiry_find("orders", "m05").unwrap(), None);
        assert_eq!(
            store.expiry_earliest("orders", 10).unwrap(),
            [(950, "m04".to_owned())]
        );
    };
    let disk = store.disk("orders", false).unwrap().unwrap();
    let held = disk.conn();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::scope(|s| {
        s.spawn(|| {
            check();
            tx.send(()).unwrap();
        });
        let done = rx.recv_timeout(Duration::from_secs(10));
        drop(held);
        assert_eq!(done, Ok(()), "the page did not wait for the writer");
    });
    assert!(!committed(&store, "orders", "m06"), "still in the queue");
    written.wait().unwrap();
    assert!(committed(&store, "orders", "m06"));
    check();
}

/// Uma escrita desligada ([`Written::detach`]) chega ao banco sem ninguém esperar por ela.
///
/// UC: UC-ARM-013
#[test]
fn a_detached_write_reaches_the_database_without_anyone_waiting() {
    let tmp = tempfile::tempdir().unwrap();
    let store = store(tmp.path());
    let ((), written) = store.deferred(|| store.put("orders", &meta("a1"), b"x").unwrap());
    written.detach();
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while !committed(&store, "orders", "a1") {
        assert!(std::time::Instant::now() < deadline, "never written");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Depois de `release_files` (o nó parou), uma leitura atrasada não reabre o banco: a pasta do tópico fica livre
/// para ser movida (a vida anterior de um nó iniciado no mesmo disco), e uma escrita é recusada.
///
/// UC: UC-ARM-013
#[test]
fn a_released_store_does_not_open_a_database_again() {
    let tmp = tempfile::tempdir().unwrap();
    let store = store(tmp.path());
    store.put("orders", &meta("a1"), b"x").unwrap();
    store.release_files();
    assert_eq!(store.get("orders", "a1").unwrap(), None);
    assert_eq!(store.count("orders").unwrap(), 0);
    assert!(store.put("orders", &meta("a2"), b"y").is_err());
    std::fs::rename(tmp.path().join("orders"), tmp.path().join("moved"))
        .expect("nothing holds the folder");
    let moved = MessageStore::new(tmp.path(), DbSynchronous::Normal);
    assert!(moved.get("moved", "a1").unwrap().is_some());
}
