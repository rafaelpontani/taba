//! O motor de mensagens como dono ou como réplica (doc 13, seção 12.7; doc 14, seção 13.3): os
//! blocos de construção que o cluster usa, sem rede.

use std::collections::HashSet;
use std::sync::Mutex;

use super::*;

/// Dono é quem está na lista; o resto é réplica.
#[derive(Default)]
struct Owners(Mutex<HashSet<String>>);

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
    let h = harness();
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
    /// Prepara uma publicação e diz se este nó é o dono dela.
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
        (meta, body)
    }

    fn publish(&self, topic: &str, body: &str, owner: bool) -> String {
        let (meta, body) = self.draft(topic, body, owner);
        let id = meta.id.clone();
        self.h.messages.apply_publish(topic, meta, body).unwrap();
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
    queue_in(&c, "pedidos").await;
    let mine = c.publish("pedidos", "1", true);
    let theirs = c.publish("pedidos", "2", false);

    let info =
        c.h.messages
            .message_info("pedidos", &mine)
            .unwrap()
            .unwrap();
    assert!(info.owned);
    let info =
        c.h.messages
            .message_info("pedidos", &theirs)
            .unwrap()
            .unwrap();
    assert!(!info.owned, "a réplica guarda mas não é dona");

    let candidates =
        c.h.messages
            .take_candidates(&c.view("pedidos"), 10, 60)
            .unwrap();
    assert_eq!(candidates.len(), 1, "a lista de prontas só tem o que é meu");
    assert_eq!(candidates[0].meta.id, mine);
    assert_eq!(c.h.messages.pending(), 2, "as duas estão guardadas");
}

/// UC: UC-ENT-013, UC-INV-006
#[tokio::test]
async fn a_candidate_is_reserved_until_the_quorum_commits_or_aborts() {
    let c = clustered();
    queue_in(&c, "pedidos").await;
    let id = c.publish("pedidos", "1", true);
    let view = c.view("pedidos");

    let mut taken = c.h.messages.take_candidates(&view, 5, 60).unwrap();
    assert_eq!(taken.len(), 1);
    assert_eq!(taken[0].meta.delivery_count, 1);
    assert!(taken[0].meta.receipt_token.is_some());
    assert_eq!(
        taken[0].meta.invisible_until_ms,
        c.h.clock.now_millis() + 60_000
    );
    // Reservada: um segundo `GET` não a pega, mesmo antes do commit.
    assert!(
        c.h.messages
            .take_candidates(&view, 5, 60)
            .unwrap()
            .is_empty()
    );

    // O quórum não fechou: ela volta a ser pronta, com o contador de antes.
    let candidate = taken.remove(0);
    c.h.messages.abort_take("pedidos", &candidate);
    let again = c.h.messages.take_candidates(&view, 5, 60).unwrap();
    assert_eq!(again.len(), 1);
    assert_eq!(again[0].meta.id, id);
    assert_eq!(again[0].meta.delivery_count, 1, "o contador não subiu");
    assert_eq!(
        c.h.messages
            .message_info("pedidos", &id)
            .unwrap()
            .unwrap()
            .meta
            .delivery_count,
        0
    );

    // O commit grava o prazo e o contador nos metadados.
    let candidate = &again[0];
    c.h.messages
        .apply_visibility(
            "pedidos",
            &id,
            candidate.meta.invisible_until_ms,
            Some((1, candidate.meta.receipt_token.as_deref().unwrap())),
        )
        .unwrap();
    let meta =
        c.h.messages
            .message_info("pedidos", &id)
            .unwrap()
            .unwrap()
            .meta;
    assert_eq!(meta.delivery_count, 1);
    assert_eq!(meta.invisible_until_ms, candidate.meta.invisible_until_ms);
    assert_eq!(meta.receipt_token, candidate.meta.receipt_token);
    c.h.messages
        .finish_take("pedidos", &[candidate], Instant::now());
}

/// UC: UC-INV-010, UC-INV-003
#[tokio::test]
async fn the_registration_of_a_visibility_is_checked_against_the_message() {
    let c = clustered();
    queue_in(&c, "pedidos").await;
    let id = c.publish("pedidos", "1", true);
    let check = |kind, token: &str| c.h.messages.check_visibility("pedidos", &id, kind, token);

    // Uma entrega precisa da mensagem pronta; depois de entregue ela está invisível.
    assert!(check(VisibilityCheck::Delivery, "x").is_ok());
    c.h.messages
        .apply_visibility(
            "pedidos",
            &id,
            c.h.clock.now_millis() + 60_000,
            Some((1, "tok")),
        )
        .unwrap();
    assert_eq!(
        check(VisibilityCheck::Delivery, "x").unwrap_err().code,
        ErrorCode::ClusterConflict
    );
    // A alteração e o ack exigem o token da última entrega.
    assert!(check(VisibilityCheck::Receipt, "tok").is_ok());
    assert_eq!(
        check(VisibilityCheck::Receipt, "velho").unwrap_err().code,
        ErrorCode::MessageStaleReceipt
    );
    assert_eq!(
        c.h.messages
            .check_visibility("pedidos", "nao-existe", VisibilityCheck::Receipt, "tok")
            .unwrap_err()
            .code,
        ErrorCode::MessageNotFound
    );
}

/// UC: UC-INV-008
#[tokio::test]
async fn the_ack_removes_the_copy_and_only_the_owner_counts_it() {
    let c = clustered();
    queue_in(&c, "pedidos").await;
    let mine = c.publish("pedidos", "1", true);
    let theirs = c.publish("pedidos", "2", false);

    c.h.messages.apply_ack("pedidos", &mine).unwrap();
    c.h.messages.apply_ack("pedidos", &theirs).unwrap();
    assert!(
        c.h.messages
            .message_info("pedidos", &mine)
            .unwrap()
            .is_none()
    );
    assert!(
        c.h.messages
            .message_info("pedidos", &theirs)
            .unwrap()
            .is_none()
    );
    // Repetir o commit não faz nada.
    c.h.messages.apply_ack("pedidos", &mine).unwrap();

    let snapshot =
        c.h.messages
            .topic_snapshot("pedidos", &StatsRules::from_config(&config()))
            .unwrap();
    assert_eq!(
        snapshot.counters.acked, 1,
        "só a do dono entra na estatística"
    );
    assert_eq!(snapshot.counters.published, 1);
}

fn config() -> Config {
    Config::parse(None, &MapEnv::new([("TABA_MODE", "development")])).unwrap()
}

/// UC: UC-RET-006
#[tokio::test]
async fn each_node_expires_its_own_replicas_by_ttl() {
    let c = clustered();
    c.h.topics
        .create(CreateTopic {
            ttl: Some(60),
            ..queue("pedidos")
        })
        .await
        .unwrap();
    let mine = c.publish("pedidos", "1", true);
    let theirs = c.publish("pedidos", "2", false);

    c.h.clock.advance(Duration::from_secs(61));
    c.h.messages.sweep();
    assert!(
        c.h.messages
            .message_info("pedidos", &mine)
            .unwrap()
            .is_none()
    );
    assert!(
        c.h.messages
            .message_info("pedidos", &theirs)
            .unwrap()
            .is_none()
    );
    assert_eq!(c.h.messages.pending(), 0);
}

/// UC: UC-ENT-004, UC-INV-029, UC-INV-030
#[tokio::test]
async fn when_the_ring_changes_the_owner_is_recomputed_from_the_metadata() {
    let c = clustered();
    queue_in(&c, "pedidos").await;
    let was_mine = c.publish("pedidos", "1", true);
    let was_theirs = c.publish("pedidos", "2", false);
    // A que era minha está invisível; a que era réplica já tinha o prazo gravado.
    let until = c.h.clock.now_millis() + 120_000;
    c.h.messages
        .apply_visibility("pedidos", &was_mine, until, Some((1, "t1")))
        .unwrap();
    c.h.messages
        .apply_visibility("pedidos", &was_theirs, until, Some((1, "t2")))
        .unwrap();

    // O anel mudou: eu perdi uma e ganhei a outra.
    {
        let mut owners = c.owners.0.lock().unwrap();
        owners.remove(&was_mine);
        owners.insert(was_theirs.clone());
    }
    c.h.messages.reown();
    assert!(
        !c.h.messages
            .message_info("pedidos", &was_mine)
            .unwrap()
            .unwrap()
            .owned
    );
    assert!(
        c.h.messages
            .message_info("pedidos", &was_theirs)
            .unwrap()
            .unwrap()
            .owned
    );

    // A nova dona a tem invisível pelos metadados e a devolve quando o prazo vence.
    let view = c.view("pedidos");
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
        "o contador vem dos metadados"
    );
}

/// UC: UC-CAN-007
#[tokio::test]
async fn the_ready_signal_fires_only_when_the_state_changes() {
    let c = clustered();
    queue_in(&c, "pedidos").await;
    let view = c.view("pedidos");
    c.publish("pedidos", "1", true);
    c.publish("pedidos", "2", true);
    assert_eq!(
        *c.ready.lock().unwrap(),
        [("pedidos".to_owned(), true)],
        "vazio → não vazio, uma vez só"
    );
    // Esvaziar a lista é a outra transição.
    let taken = c.h.messages.take_candidates(&view, 10, 60).unwrap();
    assert_eq!(taken.len(), 2);
    assert_eq!(
        *c.ready.lock().unwrap(),
        [("pedidos".to_owned(), true), ("pedidos".to_owned(), false)]
    );
    // Réplica não conta: não entra na lista de prontas.
    c.publish("pedidos", "3", false);
    assert_eq!(c.ready.lock().unwrap().len(), 2);
}

/// UC: UC-INV-019, UC-RET-011
#[tokio::test]
async fn the_owner_queues_the_dead_letter_move_once_and_can_try_again_after_a_failure() {
    let mut c = clustered();
    c.h.topics
        .create(CreateTopic {
            max_deliveries: Some(1),
            ..queue("pedidos")
        })
        .await
        .unwrap();
    let id = c.publish("pedidos", "1", true);
    let view = c.view("pedidos");
    let taken = c.h.messages.take_candidates(&view, 1, 60).unwrap();
    let candidate = &taken[0];
    c.h.messages
        .apply_visibility(
            "pedidos",
            &id,
            candidate.meta.invisible_until_ms,
            Some((1, candidate.meta.receipt_token.as_deref().unwrap())),
        )
        .unwrap();

    // A invisibilidade venceu com a entrega gasta: o movimento é enfileirado uma vez, e a
    // mensagem não fica pronta para outro `GET` enquanto o quórum não decide.
    c.h.clock.advance(Duration::from_secs(61));
    c.h.messages.sweep();
    c.h.messages.sweep();
    let job = c.dead_letters.try_recv().expect("um movimento enfileirado");
    assert_eq!(
        job,
        DeadLetterJob {
            topic: "pedidos".into(),
            id: id.clone()
        }
    );
    assert!(c.dead_letters.try_recv().is_err(), "só uma vez");
    assert!(
        c.h.messages
            .take_candidates(&view, 1, 60)
            .unwrap()
            .is_empty()
    );

    // O quórum não fechou: a mensagem volta a ser tratada na próxima volta.
    c.h.messages.dead_letter_done("pedidos", &id, false);
    c.h.messages.sweep();
    assert!(c.dead_letters.try_recv().is_ok(), "tenta de novo");

    // O commit move a cópia para a dead-letter, com o instante que o dono fixou.
    let at = c.h.clock.now_millis();
    c.h.messages.apply_dead_letter("pedidos", &id, at).unwrap();
    assert!(c.h.messages.message_info("pedidos", &id).unwrap().is_none());
    let moved =
        c.h.messages
            .message_info("pedidos-dead-letter", &id)
            .unwrap()
            .unwrap();
    assert_eq!(moved.meta.dead_lettered_ms, Some(at));
    assert_eq!(moved.meta.delivery_count, 0);
    assert!(moved.owned);
}

/// UC: UC-ESC-023
#[tokio::test]
async fn applying_a_commit_twice_changes_nothing() {
    let c = clustered();
    queue_in(&c, "pedidos").await;
    let (meta, body) = c.draft("pedidos", "1", true);
    c.h.messages
        .apply_publish("pedidos", meta.clone(), body.clone())
        .unwrap();
    c.h.messages.apply_publish("pedidos", meta, body).unwrap();
    assert_eq!(c.h.messages.pending(), 1);
    let snapshot =
        c.h.messages
            .topic_snapshot("pedidos", &StatsRules::from_config(&config()))
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
        body: b"{}".to_vec(),
        content_type: Some("application/json".into()),
        group_id: Some("g".into()),
        deduplication_id: Some(key.to_owned()),
    };
    let Draft::New { mut meta, body } =
        c.h.messages
            .draft_fifo_publish(&view, &request("k0"))
            .unwrap()
    else {
        panic!("esperava uma mensagem nova");
    };
    // Um primário anterior, com o relógio adiantado, já deu um `id` de um minuto à frente.
    let ahead = c.h.clock.now_millis() + 60_000;
    meta.id = id::new_at(ahead);
    meta.published_ms = ahead;
    let ahead_id = meta.id.clone();
    c.h.messages.apply_publish("f", meta, body).unwrap();

    // Este nó, com o relógio atrasado, ainda gera `id` maiores; no mesmo milissegundo também.
    let mut last = ahead_id;
    for key in ["k1", "k2", "k3"] {
        let Draft::New { meta, .. } =
            c.h.messages
                .draft_fifo_publish(&view, &request(key))
                .unwrap()
        else {
            panic!("esperava uma mensagem nova");
        };
        assert!(meta.id > last, "{} deveria vir depois de {last}", meta.id);
        last = meta.id;
    }
}
