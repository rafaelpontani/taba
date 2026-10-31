//! Medição descartável (doc/plans/corpo-no-sqlite-e-consumo.md): custo por mensagem das instruções SQL do
//! armazenamento, com variações de índices, de transação e de `inline-max`. Não vai para o repositório.
use std::time::Instant;

use rusqlite::{Connection, params};

const N: usize = 100_000;
const BATCH: usize = 100;

const TABLE: &str = "
    CREATE TABLE messages (
        id TEXT PRIMARY KEY NOT NULL, content_type TEXT NOT NULL, group_id TEXT,
        published_at INTEGER NOT NULL, delivery_count INTEGER NOT NULL DEFAULT 0,
        invisible_until INTEGER NOT NULL DEFAULT 0, receipt_token TEXT, dead_lettered_at INTEGER,
        dead_letter_reason TEXT, age_base INTEGER NOT NULL, deduplication_id TEXT, delete_at INTEGER,
        position INTEGER NOT NULL DEFAULT 0, body BLOB
    ) WITHOUT ROWID;
    CREATE TABLE bodies (id TEXT PRIMARY KEY NOT NULL, body BLOB NOT NULL);
    CREATE TABLE excluded (id TEXT PRIMARY KEY NOT NULL, position INTEGER NOT NULL, at_ms INTEGER NOT NULL) WITHOUT ROWID;
    CREATE TABLE expiry (invisible_until INTEGER NOT NULL, id TEXT NOT NULL, PRIMARY KEY (invisible_until, id)) WITHOUT ROWID;
";

struct Variant {
    name: &'static str,
    indexes: &'static str,
    excluded_indexes: &'static str,
}

const ALL_MSG: &str = "
    CREATE INDEX messages_invisible_until ON messages (invisible_until);
    CREATE INDEX messages_age_base ON messages (age_base);
    CREATE INDEX messages_group ON messages (group_id);
    CREATE INDEX messages_group_dedup ON messages (group_id, deduplication_id);
    CREATE INDEX messages_delete_at ON messages (delete_at);
    CREATE INDEX messages_position ON messages (position);
";
const ALL_EXC: &str = "
    CREATE INDEX excluded_at ON excluded (at_ms);
    CREATE INDEX excluded_position ON excluded (position);
";

fn open(path: &std::path::Path, v: &Variant) -> Connection {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let path = &path.with_file_name(format!("t{}.db", COUNTER.fetch_add(1, Ordering::SeqCst)));
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(path.with_extension("db-wal"));
    let _ = std::fs::remove_file(path.with_extension("db-shm"));
    let conn = Connection::open(path).unwrap();
    conn.pragma_update(None, "journal_mode", "WAL").unwrap();
    conn.pragma_update(None, "synchronous", "NORMAL").unwrap();
    conn.pragma_update(None, "cache_size", -32 * 1024i64)
        .unwrap();
    conn.pragma_update(None, "wal_autocheckpoint", 10_000i64)
        .unwrap();
    conn.execute_batch(TABLE).unwrap();
    conn.execute_batch(v.indexes).unwrap();
    conn.execute_batch(v.excluded_indexes).unwrap();
    conn
}

fn id(n: usize) -> String {
    format!("01JABCDEFGH{n:015}")
}

const INSERT: &str = "INSERT OR REPLACE INTO messages (id, content_type, group_id, published_at, delivery_count, \
    invisible_until, receipt_token, dead_lettered_at, dead_letter_reason, age_base, deduplication_id, position, body) \
    VALUES (?1, 'application/octet-stream', NULL, ?2, 0, 0, NULL, NULL, NULL, ?2, NULL, ?3, ?4)";

fn publish(conn: &mut Connection, body: &[u8], inline_max: usize, batch: usize) -> f64 {
    let start = Instant::now();
    let mut n = 0;
    while n < N {
        let tx = conn.transaction().unwrap();
        for k in n..(n + batch).min(N) {
            let inline = body.len() <= inline_max;
            tx.prepare_cached(INSERT)
                .unwrap()
                .execute(params![
                    id(k),
                    1_000 + k as i64,
                    (k as i64).wrapping_mul(7919),
                    inline.then_some(body)
                ])
                .unwrap();
            if !inline {
                tx.prepare_cached("INSERT OR REPLACE INTO bodies (id, body) VALUES (?1, ?2)")
                    .unwrap()
                    .execute(params![id(k), body])
                    .unwrap();
            }
        }
        tx.commit().unwrap();
        n += batch;
    }
    start.elapsed().as_secs_f64() * 1e6 / N as f64
}

fn deliver(conn: &mut Connection, batch: usize) -> f64 {
    let start = Instant::now();
    let mut n = 0;
    while n < N {
        let tx = conn.transaction().unwrap();
        for k in n..(n + batch).min(N) {
            // leitura da linha e do corpo, como `get` + `body`
            let _: Vec<u8> = tx
                .prepare_cached(
                    "SELECT COALESCE(body, (SELECT body FROM bodies WHERE bodies.id = messages.id)) FROM messages WHERE id = ?1",
                )
                .unwrap()
                .query_row(params![id(k)], |r| r.get(0))
                .unwrap();
            tx.prepare_cached("UPDATE messages SET delivery_count = ?2, invisible_until = ?3, receipt_token = ?4 WHERE id = ?1")
                .unwrap()
                .execute(params![id(k), 1, 31_000 + k as i64, "0123456789abcdefghijklmno"])
                .unwrap();
        }
        tx.commit().unwrap();
        n += batch;
    }
    start.elapsed().as_secs_f64() * 1e6 / N as f64
}

fn ack(conn: &mut Connection, batch: usize) -> f64 {
    let start = Instant::now();
    let mut n = 0;
    while n < N {
        let tx = conn.transaction().unwrap();
        for k in n..(n + batch).min(N) {
            tx.prepare_cached("INSERT OR IGNORE INTO excluded (id, position, at_ms) SELECT id, position, ?2 FROM messages WHERE id = ?1")
                .unwrap()
                .execute(params![id(k), 99_000 + k as i64])
                .unwrap();
            tx.prepare_cached("DELETE FROM messages WHERE id = ?1")
                .unwrap()
                .execute(params![id(k)])
                .unwrap();
            tx.prepare_cached("DELETE FROM bodies WHERE id = ?1")
                .unwrap()
                .execute(params![id(k)])
                .unwrap();
        }
        tx.commit().unwrap();
        n += batch;
    }
    start.elapsed().as_secs_f64() * 1e6 / N as f64
}

fn main() {
    if std::env::args().any(|a| a == "excluded") {
        excluded_only();
        return;
    }
    let dir = std::env::temp_dir().join("sqlite_cost");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("t.db");
    let variants = [
        Variant {
            name: "atual (6 índices + 2 em excluded)",
            indexes: ALL_MSG,
            excluded_indexes: ALL_EXC,
        },
        Variant {
            name: "só invisible_until + position (sem age/group/delete_at)",
            indexes: "CREATE INDEX messages_invisible_until ON messages (invisible_until); CREATE INDEX messages_position ON messages (position);",
            excluded_indexes: ALL_EXC,
        },
        Variant {
            name: "nenhum índice em messages",
            indexes: "",
            excluded_indexes: ALL_EXC,
        },
        Variant {
            name: "atual, sem índices em excluded",
            indexes: ALL_MSG,
            excluded_indexes: "",
        },
        Variant {
            name: "nenhum índice (messages e excluded)",
            indexes: "",
            excluded_indexes: "",
        },
    ];
    for body_len in [100usize, 1024] {
        let body = vec![7u8; body_len];
        println!("\n=== corpo de {body_len} B, {N} mensagens (µs por mensagem) ===");
        println!(
            "{:<58} {:>9} {:>9} {:>9}",
            "variante (inline-max 256, 1 tx por msg)", "publicar", "entregar", "ack"
        );
        for v in &variants {
            let mut conn = open(&path, v);
            let p = publish(&mut conn, &body, 256, 1);
            let d = deliver(&mut conn, 1);
            let a = ack(&mut conn, 1);
            println!("{:<58} {:>9.1} {:>9.1} {:>9.1}", v.name, p, d, a);
        }
        println!(
            "{:<58} {:>9} {:>9} {:>9}",
            "variante atual, lote de 100 por transação", "publicar", "entregar", "ack"
        );
        let mut conn = open(&path, &variants[0]);
        let p = publish(&mut conn, &body, 256, BATCH);
        let d = deliver(&mut conn, BATCH);
        let a = ack(&mut conn, BATCH);
        println!(
            "{:<58} {:>9.1} {:>9.1} {:>9.1}",
            "atual + lote 100", p, d, a
        );
        let mut conn = open(&path, &variants[4]);
        let p = publish(&mut conn, &body, 256, BATCH);
        let d = deliver(&mut conn, BATCH);
        let a = ack(&mut conn, BATCH);
        println!(
            "{:<58} {:>9.1} {:>9.1} {:>9.1}",
            "sem índices + lote 100", p, d, a
        );
    }
    println!("\n=== inline-max (corpo de 1024 B, índices atuais, 1 tx por msg) ===");
    let body = vec![7u8; 1024];
    for inline_max in [0usize, 256, 2048] {
        let mut conn = open(&path, &variants[0]);
        let p = publish(&mut conn, &body, inline_max, 1);
        let d = deliver(&mut conn, 1);
        let a = ack(&mut conn, 1);
        println!("inline-max {inline_max:>5}: publicar {p:.1}  entregar {d:.1}  ack {a:.1}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// Só o insert no log de excluídos (o que cada ack em memória paga), em lote.
#[allow(dead_code)]
pub fn excluded_only() {
    let dir = std::env::temp_dir().join("sqlite_cost2");
    std::fs::create_dir_all(&dir).unwrap();
    println!(
        "\n=== log de excluídos: só o INSERT, {N} linhas, position aleatória (µs por linha) ==="
    );
    for (name, idx) in [
        ("PK + at_ms + position (atual)", ALL_EXC),
        (
            "PK + at_ms",
            "CREATE INDEX excluded_at ON excluded (at_ms);",
        ),
        ("só PK", ""),
    ] {
        for batch in [100usize, 1000] {
            let v = Variant {
                name: "x",
                indexes: "",
                excluded_indexes: idx,
            };
            let mut conn = open(&dir.join("t.db"), &v);
            let start = Instant::now();
            let mut n = 0usize;
            while n < N {
                let tx = conn.transaction().unwrap();
                for k in n..(n + batch).min(N) {
                    let pos = (k as i64).wrapping_mul(0x9E37_79B9_7F4A_7C15u64 as i64);
                    tx.prepare_cached(
                        "INSERT OR IGNORE INTO excluded (id, position, at_ms) VALUES (?1, ?2, ?3)",
                    )
                    .unwrap()
                    .execute(params![id(k), pos, 99_000 + k as i64])
                    .unwrap();
                }
                tx.commit().unwrap();
                n += batch;
            }
            println!(
                "{name:<32} lote {batch:>4}: {:.1}",
                start.elapsed().as_secs_f64() * 1e6 / N as f64
            );
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}
