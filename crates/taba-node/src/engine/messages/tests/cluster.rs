//! The message engine as owner or as replica (doc 13, section 12.7; doc 14, section 13.3): the
//! building blocks the cluster uses, without a network.

use std::collections::HashSet;
use std::sync::Mutex;

use super::*;
use crate::storage::messages::{TOPIC_DB_FILE, dir_name};

/// The owner is whoever is in the list; the rest is a replica.
#[derive(Default)]
pub(super) struct Owners(pub(super) Mutex<HashSet<String>>);

impl Placement for Owners {
    fn is_owner(&self, _topic: &str, id: &str, _group: Option<&str>, _position: u64) -> bool {
        self.0.lock().unwrap().contains(id)
    }
}

struct Cluster {
    h: Harness,
    owners: Arc<Owners>,
    dead_letters: tokio::sync::mpsc::UnboundedReceiver<DeadLetterJob>,
    ready: Arc<Mutex<Vec<(String, bool)>>>,
}

fn clustered() -> Cluster {
    clustered_with(&[])
}

fn clustered_with(env: &[(&str, &str)]) -> Cluster {
    let h = harness_with(env);
    let owners = Arc::new(Owners::default());
    let (tx, dead_letters) = tokio::sync::mpsc::unbounded_channel();
    let ready = Arc::new(Mutex::new(Vec::new()));
    let seen = ready.clone();
    h.messages.enable_cluster(ClusterHooks {
        placement: owners.clone(),
        dead_letters: tx,
        ready: Arc::new(move |topic, has| seen.lock().unwrap().push((topic.to_owned(), has))),
    });
    Cluster {
        h,
        owners,
        dead_letters,
        ready,
    }
}

impl Cluster {
    /// Prepares a publication and says whether this node is its owner.
    fn draft(&self, topic: &str, body: &str, owner: bool) -> (MessageMeta, Vec<u8>) {
        let view = self.h.topics.get(topic).unwrap();
        let (meta, body) = self
            .h
            .messages
            .draft_publish(&view, &json_body(body))
            .unwrap();
        if owner {
            self.owners.0.lock().unwrap().insert(meta.id.clone());
        }
        (meta, body.to_vec())
    }

    fn publish(&self, topic: &str, body: &str, owner: bool) -> String {
        let (meta, body) = self.draft(topic, body, owner);
        let id = meta.id.clone();
        self.h
            .messages
            .apply_publish(topic, meta, body.into())
            .unwrap();
        id
    }

    fn view(&self, topic: &str) -> TopicView {
        self.h.topics.get(topic).unwrap()
    }
}

async fn queue_in(c: &Cluster, name: &str) {
    c.h.topics.create(queue(name)).await.unwrap();
}

/// UC: UC-ENT-001
#[tokio::test]
async fn only_the_owner_puts_a_message_in_the_delivery_lists() {
    let c = clustered();
    queue_in(&c, "orders").await;
    let mine = c.publish("orders", "1", true);
    let theirs = c.publish("orders", "2", false);

    let info = c.h.messages.message_info("orders", &mine).unwrap().unwrap();
    assert!(info.owned);
    let info =
        c.h.messages
            .message_info("orders", &theirs)
            .unwrap()
            .unwrap();
    assert!(!info.owned, "the replica stores but is not the owner");

    let candidates =
        c.h.messages
            .take_candidates(&c.view("orders"), 10, 60)
            .unwrap();
    assert_eq!(candidates.len(), 1, "the ready list only has what is mine");
    assert_eq!(candidates[0].meta.id, mine);
    assert_eq!(c.h.messages.pending(), 2, "both are stored");
}

/// UC: UC-ENT-013, UC-INV-006
#[tokio::test]
async fn a_candidate_is_reserved_until_the_quorum_commits_or_aborts() {
    let c = clustered();
    queue_in(&c, "orders").await;
    let id = c.publish("orders", "1", true);
    let view = c.view("orders");

    let mut taken = c.h.messages.take_candidates(&view, 5, 60).unwrap();
    assert_eq!(taken.len(), 1);
    assert_eq!(taken[0].meta.delivery_count, 1);
    assert!(taken[0].meta.receipt_token.is_some());
    assert_eq!(
        taken[0].meta.invisible_until_ms,
        c.h.clock.now_millis() + 60_000
    );
    // Reserved: a second `GET` does not take it, even before the commit.
    assert!(
        c.h.messages
            .take_candidates(&view, 5, 60)
            .unwrap()
            .is_empty()
    );

    // The quorum did not close: it goes back to being ready, with the previous counter.
    let candidate = taken.remove(0);
    c.h.messages.abort_take("orders", &candidate);
    let again = c.h.messages.take_candidates(&view, 5, 60).unwrap();
    assert_eq!(again.len(), 1);
    assert_eq!(again[0].meta.id, id);
    assert_eq!(again[0].meta.delivery_count, 1, "the counter did not go up");
    assert_eq!(
        c.h.messages
            .message_info("orders", &id)
            .unwrap()
            .unwrap()
            .meta
            .delivery_count,
        0
    );

    // The commit writes the deadline and the counter into the metadata.
    let candidate = &again[0];
    c.h.messages
        .apply_visibility(
            "orders",
            &id,
            candidate.meta.invisible_until_ms,
            Some((1, candidate.meta.receipt_token.as_deref().unwrap())),
        )
        .unwrap();
    let meta =
        c.h.messages
            .message_info("orders", &id)
            .unwrap()
            .unwrap()
            .meta;
    assert_eq!(meta.delivery_count, 1);
    assert_eq!(meta.invisible_until_ms, candidate.meta.invisible_until_ms);
    assert_eq!(meta.receipt_token, candidate.meta.receipt_token);
    c.h.messages
        .finish_take("orders", &[candidate], Instant::now());
}

/// UC: UC-INV-010, UC-INV-003
#[tokio::test]
async fn the_registration_of_a_visibility_is_checked_against_the_message() {
    let c = clustered();
    queue_in(&c, "orders").await;
    let id = c.publish("orders", "1", true);
    let check = |kind, token: &str| c.h.messages.check_visibility("orders", &id, kind, token);

    // A delivery needs the message to be ready; once delivered it is invisible.
    assert!(check(VisibilityCheck::Delivery, "x").is_ok());
    c.h.messages
        .apply_visibility(
            "orders",
            &id,
            c.h.clock.now_millis() + 60_000,
            Some((1, "tok")),
        )
        .unwrap();
    assert_eq!(
        check(VisibilityCheck::Delivery, "x").unwrap_err().code,
        ErrorCode::ClusterConflict
    );
    // The change and the ack require the last delivery's token.
    assert!(check(VisibilityCheck::Receipt, "tok").is_ok());
    assert_eq!(
        check(VisibilityCheck::Receipt, "old").unwrap_err().code,
        ErrorCode::MessageStaleReceipt
    );
    assert_eq!(
        c.h.messages
            .check_visibility("orders", "nao-exists", VisibilityCheck::Receipt, "tok")
            .unwrap_err()
            .code,
        ErrorCode::MessageNotFound
    );
}

/// UC: UC-INV-033
#[tokio::test]
async fn on_a_plain_queue_a_copy_that_is_not_the_owner_only_checks_that_the_message_exists() {
    // D199: the owner delivers alone and tells the copies after; an ack or a change that reaches a copy before the
    // delivery does is not refused there (the owner's check, which the quorum always includes, is the one that decides).
    let c = clustered();
    queue_in(&c, "orders").await;
    let id = c.publish("orders", "1", false);
    c.h.messages
        .apply_visibility(
            "orders",
            &id,
            c.h.clock.now_millis() + 60_000,
            Some((1, "tok")),
        )
        .unwrap();
    let check = |kind, token: &str| c.h.messages.check_visibility("orders", &id, kind, token);
    assert!(check(VisibilityCheck::Receipt, "newer-delivery").is_ok());
    assert!(check(VisibilityCheck::Delivery, "x").is_ok());
    assert_eq!(
        c.h.messages
            .check_visibility("orders", "nao-exists", VisibilityCheck::Receipt, "tok")
            .unwrap_err()
            .code,
        ErrorCode::MessageNotFound
    );
}

/// UC: UC-INV-033
#[tokio::test]
async fn a_replicated_delivery_never_takes_the_copy_back_to_an_earlier_delivery() {
    let c = clustered();
    queue_in(&c, "orders").await;
    let id = c.publish("orders", "1", false);
    let until = c.h.clock.now_millis() + 60_000;
    let applied =
        c.h.messages
            .apply_deliveries("orders", &[(id.clone(), until, 2, "second".to_owned())]);
    assert!(applied.iter().all(Result::is_ok));
    // The first delivery arrives late: the copy keeps the second.
    let late = c.h.messages.apply_deliveries(
        "orders",
        &[
            (id.clone(), until - 30_000, 1, "first".to_owned()),
            // A message the copy no longer has (the ack got there first): nothing to do.
            (
                "01900000-0000-7000-8000-00000000000f".to_owned(),
                until,
                1,
                "x".to_owned(),
            ),
        ],
    );
    assert!(late.iter().all(Result::is_ok), "{late:?}");
    let info = c.h.messages.message_info("orders", &id).unwrap().unwrap();
    assert_eq!(info.meta.delivery_count, 2);
    assert_eq!(info.meta.receipt_token.as_deref(), Some("second"));
    assert_eq!(info.meta.invisible_until_ms, until);
}

/// UC: UC-INV-007, UC-INV-033
#[tokio::test]
async fn a_replicated_delivery_older_than_a_change_of_deadline_does_not_bring_the_deadline_back() {
    // KI-028: on a copy, the change of deadline (`PUT .../visibility`) of a delivery can get there before the delivery
    // itself (D199). The delivery that arrives after it is older: it brings its counter, not its deadline.
    let c = clustered();
    queue_in(&c, "orders").await;
    let id = c.publish("orders", "1", false);
    let delivered = c.h.clock.now_millis() + 60_000;
    let changed = c.h.clock.now_millis() + 600_000;
    c.h.messages
        .apply_change("orders", &id, changed, "tok")
        .unwrap();
    let late =
        c.h.messages
            .apply_deliveries("orders", &[(id.clone(), delivered, 1, "tok".to_owned())]);
    assert!(late.iter().all(Result::is_ok), "{late:?}");
    let info = c.h.messages.message_info("orders", &id).unwrap().unwrap();
    assert_eq!(info.meta.invisible_until_ms, changed, "the change stays");
    assert_eq!(info.meta.delivery_count, 1, "the counter of the delivery");

    // The same delivery repeated after the change (a resend) does not undo it either.
    let again =
        c.h.messages
            .apply_deliveries("orders", &[(id.clone(), delivered, 1, "tok".to_owned())]);
    assert!(again.iter().all(Result::is_ok));
    let info = c.h.messages.message_info("orders", &id).unwrap().unwrap();
    assert_eq!(info.meta.invisible_until_ms, changed);

    // A newer delivery (another token, a greater counter) is applied as before.
    let newer = c.h.messages.apply_deliveries(
        "orders",
        &[(id.clone(), delivered + 1, 2, "tok2".to_owned())],
    );
    assert!(newer.iter().all(Result::is_ok));
    let info = c.h.messages.message_info("orders", &id).unwrap().unwrap();
    assert_eq!(info.meta.invisible_until_ms, delivered + 1);
    assert_eq!(info.meta.delivery_count, 2);
}

/// UC: UC-INV-008
#[tokio::test]
async fn the_ack_removes_the_copy_and_only_the_owner_counts_it() {
    let c = clustered();
    queue_in(&c, "orders").await;
    let mine = c.publish("orders", "1", true);
    let theirs = c.publish("orders", "2", false);

    c.h.messages.apply_ack("orders", &mine).unwrap();
    c.h.messages.apply_ack("orders", &theirs).unwrap();
    assert!(
        c.h.messages
            .message_info("orders", &mine)
            .unwrap()
            .is_none()
    );
    assert!(
        c.h.messages
            .message_info("orders", &theirs)
            .unwrap()
            .is_none()
    );
    // Repeating the commit does nothing.
    c.h.messages.apply_ack("orders", &mine).unwrap();

    let snapshot =
        c.h.messages
            .topic_snapshot("orders", &StatsRules::from_config(&config()))
            .unwrap();
    assert_eq!(
        snapshot.counters.acked, 1,
        "only the owner's enters the statistics"
    );
    assert_eq!(snapshot.counters.published, 1);
}

fn config() -> Config {
    Config::parse(None, &MapEnv::new([("TABA_REPLICATION_FACTOR", "1")])).unwrap()
}

/// UC: UC-RET-006
#[tokio::test]
async fn each_node_expires_its_own_replicas_by_ttl() {
    let c = clustered();
    c.h.topics
        .create(CreateTopic {
            ttl: Some(60),
            ..queue("orders")
        })
        .await
        .unwrap();
    let mine = c.publish("orders", "1", true);
    let theirs = c.publish("orders", "2", false);

    c.h.clock.advance(Duration::from_secs(61));
    c.h.messages.sweep();
    assert!(
        c.h.messages
            .message_info("orders", &mine)
            .unwrap()
            .is_none()
    );
    assert!(
        c.h.messages
            .message_info("orders", &theirs)
            .unwrap()
            .is_none()
    );
    assert_eq!(c.h.messages.pending(), 0);
}

/// UC: UC-ENT-004, UC-INV-029, UC-INV-030
#[tokio::test]
async fn when_the_ring_changes_the_owner_is_recomputed_from_the_metadata() {
    let c = clustered();
    queue_in(&c, "orders").await;
    let was_mine = c.publish("orders", "1", true);
    let was_theirs = c.publish("orders", "2", false);
    // The one that was mine is invisible; the one that was a replica already had the deadline written.
    let until = c.h.clock.now_millis() + 120_000;
    c.h.messages
        .apply_visibility("orders", &was_mine, until, Some((1, "t1")))
        .unwrap();
    c.h.messages
        .apply_visibility("orders", &was_theirs, until, Some((1, "t2")))
        .unwrap();

    // The ring changed: I lost one and gained the other.
    {
        let mut owners = c.owners.0.lock().unwrap();
        owners.remove(&was_mine);
        owners.insert(was_theirs.clone());
    }
    c.h.messages.reown();
    assert!(
        !c.h.messages
            .message_info("orders", &was_mine)
            .unwrap()
            .unwrap()
            .owned
    );
    assert!(
        c.h.messages
            .message_info("orders", &was_theirs)
            .unwrap()
            .unwrap()
            .owned
    );

    // The new owner has it invisible by the metadata and returns it when the deadline expires.
    let view = c.view("orders");
    assert!(
        c.h.messages
            .take_candidates(&view, 5, 60)
            .unwrap()
            .is_empty()
    );
    c.h.clock.advance(Duration::from_secs(121));
    let back = c.h.messages.take_candidates(&view, 5, 60).unwrap();
    assert_eq!(back.len(), 1);
    assert_eq!(back[0].meta.id, was_theirs);
    assert_eq!(
        back[0].meta.delivery_count, 2,
        "the counter comes from the metadata"
    );
}

/// UC: UC-CAN-007
#[tokio::test]
async fn the_ready_signal_fires_only_when_the_state_changes() {
    let c = clustered();
    queue_in(&c, "orders").await;
    let view = c.view("orders");
    c.publish("orders", "1", true);
    c.publish("orders", "2", true);
    assert_eq!(
        *c.ready.lock().unwrap(),
        [("orders".to_owned(), true)],
        "empty → not empty, only once"
    );
    // Emptying the list is the other transition.
    let taken = c.h.messages.take_candidates(&view, 10, 60).unwrap();
    assert_eq!(taken.len(), 2);
    assert_eq!(
        *c.ready.lock().unwrap(),
        [("orders".to_owned(), true), ("orders".to_owned(), false)]
    );
    // A replica does not count: it does not enter the ready list.
    c.publish("orders", "3", false);
    assert_eq!(c.ready.lock().unwrap().len(), 2);
}

/// UC: UC-INV-019, UC-RET-011
#[tokio::test]
async fn the_owner_queues_the_dead_letter_move_once_and_can_try_again_after_a_failure() {
    let mut c = clustered();
    c.h.topics
        .create(CreateTopic {
            max_deliveries: Some(1),
            ..queue("orders")
        })
        .await
        .unwrap();
    let id = c.publish("orders", "1", true);
    let view = c.view("orders");
    let taken = c.h.messages.take_candidates(&view, 1, 60).unwrap();
    let candidate = &taken[0];
    c.h.messages
        .apply_visibility(
            "orders",
            &id,
            candidate.meta.invisible_until_ms,
            Some((1, candidate.meta.receipt_token.as_deref().unwrap())),
        )
        .unwrap();

    // The invisibility expired with the delivery used up: the move is queued once, and the
    // message does not become ready for another `GET` while the quorum has not decided.
    c.h.clock.advance(Duration::from_secs(61));
    c.h.messages.sweep();
    c.h.messages.sweep();
    let job = c.dead_letters.try_recv().expect("a queued move");
    assert_eq!(
        job,
        DeadLetterJob {
            topic: "orders".into(),
            id: id.clone()
        }
    );
    assert!(c.dead_letters.try_recv().is_err(), "only once");
    assert!(
        c.h.messages
            .take_candidates(&view, 1, 60)
            .unwrap()
            .is_empty()
    );

    // The quorum did not close: the message is handled again on the next round.
    c.h.messages.dead_letter_done("orders", &id, false);
    c.h.messages.sweep();
    assert!(c.dead_letters.try_recv().is_ok(), "tries again");

    // The commit moves the copy to the dead-letter, with the instant the owner fixed.
    let at = c.h.clock.now_millis();
    c.h.messages.apply_dead_letter("orders", &id, at).unwrap();
    assert!(c.h.messages.message_info("orders", &id).unwrap().is_none());
    let moved =
        c.h.messages
            .message_info("orders-dead-letter", &id)
            .unwrap()
            .unwrap();
    assert_eq!(moved.meta.dead_lettered_ms, Some(at));
    assert_eq!(moved.meta.delivery_count, 0);
    assert!(moved.owned);
}

/// A delivery whose commit reaches the owner after its own invisibility already ended (the clock moved between the
/// reservation and the commit) must not put a message that used up its deliveries back in the ready list: the
/// `maxDeliveries`-th delivery is the last (PRB-075, found by the chaos run).
///
/// UC: UC-RET-011, UC-CHS-004
#[tokio::test]
async fn a_commit_that_arrives_after_its_invisibility_does_not_deliver_past_the_limit() {
    let mut c = clustered();
    c.h.topics
        .create(CreateTopic {
            max_deliveries: Some(2),
            ..queue("orders")
        })
        .await
        .unwrap();
    let id = c.publish("orders", "1", true);
    let view = c.view("orders");

    // First delivery, committed in time.
    let first = c.h.messages.take_candidates(&view, 1, 60).unwrap();
    c.h.messages
        .apply_visibility(
            "orders",
            &id,
            first[0].meta.invisible_until_ms,
            Some((1, first[0].meta.receipt_token.as_deref().unwrap())),
        )
        .unwrap();

    // Second delivery (the last one allowed): reserved, and the clock moves past its invisibility before the commit.
    c.h.clock.advance(Duration::from_secs(61));
    let second = c.h.messages.take_candidates(&view, 1, 60).unwrap();
    assert_eq!(second[0].meta.delivery_count, 2);
    c.h.clock.advance(Duration::from_secs(61));
    c.h.messages
        .apply_visibility(
            "orders",
            &id,
            second[0].meta.invisible_until_ms,
            Some((2, second[0].meta.receipt_token.as_deref().unwrap())),
        )
        .unwrap();

    // A third delivery never happens: the message goes to the dead-letter.
    assert!(
        c.h.messages
            .take_candidates(&view, 1, 60)
            .unwrap()
            .is_empty(),
        "delivered a third time"
    );
    // The message waits an instant and the next settle sends it to the dead-letter.
    c.h.clock.advance(Duration::from_secs(1));
    c.h.messages.sweep();
    assert_eq!(
        c.dead_letters.try_recv().expect("a queued move"),
        DeadLetterJob {
            topic: "orders".into(),
            id
        }
    );
}

/// UC: UC-ESC-023
#[tokio::test]
async fn applying_a_commit_twice_changes_nothing() {
    let c = clustered();
    queue_in(&c, "orders").await;
    let (meta, body) = c.draft("orders", "1", true);
    c.h.messages
        .apply_publish("orders", meta.clone(), body.clone().into())
        .unwrap();
    c.h.messages
        .apply_publish("orders", meta, body.into())
        .unwrap();
    assert_eq!(c.h.messages.pending(), 1);
    let snapshot =
        c.h.messages
            .topic_snapshot("orders", &StatsRules::from_config(&config()))
            .unwrap();
    assert_eq!(snapshot.counters.published, 1);
}

/// UC: UC-FIF-027
#[tokio::test]
async fn the_id_of_a_group_keeps_growing_even_if_the_clock_of_the_new_primary_is_behind() {
    let c = clustered();
    c.h.topics.create(fifo("f")).await.unwrap();
    let view = c.view("f");
    let request = |key: &str| PublishRequest {
        body: b"{}".to_vec().into(),
        content_type: Some("application/json".into()),
        group_id: Some("g".into()),
        deduplication_id: Some(key.to_owned()),
        ..Default::default()
    };
    let Draft::New { mut meta, body } =
        c.h.messages
            .draft_fifo_publish(&view, &request("k0"))
            .unwrap()
    else {
        panic!("expected a new message");
    };
    // A previous primary, with its clock ahead, already gave an `id` one minute ahead.
    let ahead = c.h.clock.now_millis() + 60_000;
    meta.id = id::new_at(ahead);
    meta.published_ms = ahead;
    let ahead_id = meta.id.clone();
    c.h.messages.apply_publish("f", meta, body).unwrap();

    // This node, with its clock behind, still generates greater `id`s; in the same millisecond too.
    let mut last = ahead_id;
    for key in ["k1", "k2", "k3"] {
        let Draft::New { meta, .. } =
            c.h.messages
                .draft_fifo_publish(&view, &request(key))
                .unwrap()
        else {
            panic!("expected a new message");
        };
        assert!(meta.id > last, "{} should come after {last}", meta.id);
        last = meta.id;
    }
}

/// UC: UC-ENT-013, UC-ENT-008
#[tokio::test]
async fn the_owner_reserves_and_aborts_with_small_lists_as_with_large_ones() {
    let mut runs: Vec<Vec<String>> = Vec::new();
    for env in [
        vec![
            ("TABA_DELIVERY_READY_LIST_SIZE", "2"),
            ("TABA_DELIVERY_INVISIBLE_LIST_SIZE", "1"),
        ],
        vec![],
    ] {
        let c = clustered_with(&env);
        queue_in(&c, "orders").await;
        for i in 0..8 {
            // A replica between every two of mine: the window must skip it.
            c.publish("orders", &format!("\"r{i}\""), false);
            c.publish("orders", &i.to_string(), true);
        }
        let view = c.view("orders");
        let body = |t: &Candidate| String::from_utf8(t.body.to_vec()).unwrap();

        // Deadlines that do not fit the invisible list.
        let mut seen = Vec::new();
        let mut taken = Vec::new();
        for visibility in [60, 120, 180] {
            taken.extend(c.h.messages.take_candidates(&view, 2, visibility).unwrap());
        }
        seen.extend(taken.iter().map(body));
        assert_eq!(taken.len(), 6);
        assert!(
            seen.iter().all(|b| !b.contains('r')),
            "no replica is delivered"
        );

        // The quorum fails for one that only the table can know: it goes back to ready, once.
        let aborted = taken.remove(5);
        c.h.messages.abort_take("orders", &aborted);
        let again = c.h.messages.take_candidates(&view, 10, 60).unwrap();
        assert_eq!(again.len(), 3, "the two left and the aborted one");
        assert!(again.iter().any(|t| t.meta.id == aborted.meta.id));
        seen.extend(again.iter().map(body));
        assert!(
            c.h.messages
                .take_candidates(&view, 10, 60)
                .unwrap()
                .is_empty()
        );
        runs.push(seen);
    }
    assert_eq!(runs[0], runs[1]);
}

/// UC: UC-INV-027
#[tokio::test]
async fn a_failed_table_write_undoes_a_reservation_of_the_owner() {
    let c = clustered_with(&[("TABA_DELIVERY_INVISIBLE_LIST_SIZE", "1")]);
    queue_in(&c, "orders").await;
    for i in 0..3 {
        c.publish("orders", &i.to_string(), true);
    }
    let view = c.view("orders");
    c.h.messages.take_candidates(&view, 1, 30).unwrap();

    c.h.store
        .expiry_apply("orders", &[(true, 1, "x".into()), (false, 1, "x".into())])
        .unwrap();
    let db = c.h.dir.path().join(dir_name("orders")).join(TOPIC_DB_FILE);
    let conn = rusqlite::Connection::open(&db).unwrap();
    conn.execute("DROP TABLE expiry", []).unwrap();

    // The second reservation falls after the first on the list, so it needs the table: it fails.
    assert!(c.h.messages.take_candidates(&view, 1, 60).is_err());

    conn.execute(
        "CREATE TABLE expiry (invisible_until INTEGER NOT NULL, id TEXT NOT NULL, \
         PRIMARY KEY (invisible_until, id)) WITHOUT ROWID",
        [],
    )
    .unwrap();
    // The message was not left reserved, and its counter did not go up.
    let again = c.h.messages.take_candidates(&view, 1, 60).unwrap();
    assert_eq!(again.len(), 1);
    assert_eq!(again[0].meta.delivery_count, 1);
}

/// UC: UC-ARM-001
#[tokio::test]
async fn asking_whether_a_message_is_in_memory_does_not_hold_the_map_of_topics_while_it_waits_for_the_topic()
 {
    // PRB-088: a replica asking whether a message is in memory, a topic being deleted and a publication that looks
    // for memory to free in the other topics (it is over `memory.limit`) stopped every thread of a node.
    let h = harness_with(&[("TABA_MEMORY_LIMIT", "3")]);
    h.topics.create(queue("orders")).await.unwrap();
    h.topics.create(queue("other")).await.unwrap();
    let id = publish_ok(&h, "orders", "1");
    publish_ok(&h, "other", "2");
    // The publication holds its topic, as `publish_held` does.
    let state = h.messages.state_of("orders");
    let held = lock(&state);

    // The replica asks about a message of that topic: it has to wait for the topic.
    let m = h.messages.clone();
    let asking = std::thread::spawn(move || m.is_in_memory("orders", &id));
    std::thread::sleep(Duration::from_millis(100));
    // A topic is deleted: it waits to write in the map of topics.
    let m = h.messages.clone();
    let dropping = std::thread::spawn(move || m.drop_topic("other"));
    std::thread::sleep(Duration::from_millis(100));
    // Still holding its topic, the publication lists the other topics to demote messages. If the question held the
    // map while waiting for the topic, this read would queue behind the deletion for good.
    let (tx, rx) = std::sync::mpsc::channel();
    let m = h.messages.clone();
    std::thread::spawn(move || {
        tx.send(m.existing_states().len()).ok();
    });
    let listed = rx.recv_timeout(Duration::from_secs(5));
    drop(held);
    assert!(
        listed.is_ok(),
        "the list of topics waited for the deletion, which waited for the question, which waited for the topic"
    );
    asking.join().unwrap();
    dropping.join().unwrap();
}
