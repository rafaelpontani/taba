use super::*;

fn open() -> (tempfile::TempDir, BackupDb) {
    let tmp = tempfile::tempdir().unwrap();
    let db = BackupDb::open(tmp.path()).unwrap();
    (tmp, db)
}

fn segment(key: &str, total: u64) -> SegmentRow {
    SegmentRow {
        key: key.into(),
        topic: "t".into(),
        topic_id: "tid".into(),
        total,
        live: total,
        bytes: 100,
        created_ms: 1,
    }
}

/// UC: UC-BKP-008, UC-BKP-009
#[test]
fn the_outbox_keeps_only_ids_and_hands_them_out_oldest_first() {
    let (_tmp, db) = open();
    db.enqueue("t", "b", 20).unwrap();
    db.enqueue("t", "a", 10).unwrap();
    db.enqueue("t", "a", 99).unwrap();
    assert_eq!(db.outbox_len().unwrap(), 2, "repetir não duplica");
    assert_eq!(
        db.outbox(15, 10).unwrap(),
        [("t".to_owned(), "a".to_owned())]
    );
    assert_eq!(db.outbox(u64::MAX, 10).unwrap().len(), 2);
    assert!(db.unqueue("t", "a").unwrap());
    assert!(!db.unqueue("t", "a").unwrap());
}

/// UC: UC-BKP-008, UC-BKP-012
#[test]
fn committing_a_segment_records_the_references_and_empties_the_outbox() {
    let (_tmp, db) = open();
    for id in ["a", "b", "c"] {
        db.enqueue("t", id, 1).unwrap();
    }
    db.commit_segment(
        &segment("seg-1", 2),
        &[("a".into(), 8, 30), ("b".into(), 38, 31)],
    )
    .unwrap();
    assert_eq!(db.outbox_len().unwrap(), 1, "só a `c` ainda espera");
    assert_eq!(
        db.saved("t", "b").unwrap(),
        Some(SavedRef {
            segment: "seg-1".into(),
            offset: 38,
            len: 31,
            saved_at_ms: 1
        })
    );
    assert_eq!(db.saved("t", "c").unwrap(), None);
    assert_eq!(db.segment_totals().unwrap(), (1, 100));
}

/// UC: UC-BKP-008, UC-BKP-014
#[test]
fn a_segment_is_reported_dead_only_when_its_last_message_dies() {
    let (_tmp, db) = open();
    db.enqueue("t", "a", 1).unwrap();
    db.enqueue("t", "b", 1).unwrap();
    db.commit_segment(
        &segment("seg-1", 2),
        &[("a".into(), 8, 1), ("b".into(), 9, 1)],
    )
    .unwrap();
    assert_eq!(db.mark_dead("t", "a").unwrap(), None, "ainda há uma viva");
    assert_eq!(db.segment("seg-1").unwrap().unwrap().live, 1);
    assert_eq!(db.mark_dead("t", "b").unwrap().as_deref(), Some("seg-1"));
    db.drop_segment("seg-1").unwrap();
    assert!(db.segment("seg-1").unwrap().is_none());
    assert_eq!(
        db.mark_dead("t", "zzz").unwrap(),
        None,
        "desconhecida: nenhum segmento para apagar"
    );
    assert!(
        db.dirty(10)
            .unwrap()
            .iter()
            .any(|d| d.id == "zzz" && d.removed),
        "mas a remoção vai ao diário, por garantia (mensagem herdada)"
    );
}

/// UC: UC-BKP-013
#[test]
fn only_saved_messages_enter_the_journal_and_a_death_is_a_removal() {
    let (_tmp, db) = open();
    db.enqueue("t", "pendente", 1).unwrap();
    db.enqueue("t", "salva", 1).unwrap();
    db.commit_segment(&segment("seg-1", 1), &[("salva".into(), 8, 1)])
        .unwrap();
    db.touch("t", "pendente").unwrap();
    db.touch("t", "salva").unwrap();
    db.touch("t", "salva").unwrap();
    let dirty = db.dirty(10).unwrap();
    assert_eq!(
        dirty.len(),
        1,
        "a pendente vai com o estado atual, sem diário"
    );
    assert_eq!(dirty[0].id, "salva");
    assert!(!dirty[0].removed);

    db.mark_dead("t", "salva").unwrap();
    let dirty = db.dirty(10).unwrap();
    assert_eq!(dirty.len(), 1);
    assert!(dirty[0].removed, "o ack vira remoção");
    db.clear_dirty(&dirty).unwrap();
    assert_eq!(db.dirty_len().unwrap(), 0);
}

/// UC: UC-BKP-008
#[test]
fn sequence_numbers_never_repeat_and_a_topic_can_be_forgotten() {
    let (tmp, db) = open();
    let first = db.next_seq().unwrap();
    assert_eq!(db.next_seq().unwrap(), first + 1);
    db.enqueue("t", "a", 1).unwrap();
    db.enqueue("outro", "b", 1).unwrap();
    db.drop_topic("t").unwrap();
    assert_eq!(db.outbox_len().unwrap(), 1);
    drop(db);
    let again = BackupDb::open(tmp.path()).unwrap();
    assert_eq!(
        again.next_seq().unwrap(),
        first + 2,
        "a contagem sobrevive ao reinício"
    );
    again.reset().unwrap();
    assert_eq!(again.outbox_len().unwrap(), 0);
}

/// UC: UC-BKP-013, UC-BKP-014
#[test]
fn a_message_that_died_during_the_upload_is_not_alive_and_is_journaled_as_removed() {
    let (_tmp, db) = open();
    db.enqueue("t", "viva", 1).unwrap();
    db.enqueue("t", "morta", 1).unwrap();
    // O ack chega com o segmento em voo: a linha do outbox some antes do commit.
    db.mark_dead("t", "morta").unwrap();
    let committed = db
        .commit_segment(
            &segment("seg-1", 2),
            &[("viva".into(), 8, 1), ("morta".into(), 9, 1)],
        )
        .unwrap();
    assert_eq!(committed.live, 1);
    assert_eq!(committed.dead, ["morta"]);
    assert_eq!(db.segment("seg-1").unwrap().unwrap().live, 1);
    assert_eq!(db.saved("t", "morta").unwrap(), None);
    let dirty = db.dirty(10).unwrap();
    assert!(dirty.iter().any(|d| d.id == "morta" && d.removed));
}

/// UC: UC-BKP-036
#[test]
fn a_replica_keeps_the_reference_but_never_the_journal_or_the_segment_count() {
    let (_tmp, db) = open();
    db.set_saved(
        "t",
        "a",
        &SavedRef {
            segment: "seg-9".into(),
            offset: 8,
            len: 10,
            saved_at_ms: 777,
        },
    )
    .unwrap();
    assert_eq!(db.saved("t", "a").unwrap().unwrap().saved_at_ms, 777);
    assert_eq!(db.saved_of_topic("t").unwrap().len(), 1);
    db.forget("t", "a").unwrap();
    assert_eq!(db.saved("t", "a").unwrap(), None);
    assert_eq!(db.dirty_len().unwrap(), 0, "só o dono escreve o diário");
}
