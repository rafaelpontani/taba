//! The replication in the simulated cluster (doc 05, section 7.2; doc 13; doc 14; doc 20b, level **C**):
//! topic definitions on all the nodes, write, delivery and ack by quorum on the responsible nodes.

mod common;

use std::time::Duration;

use serde_json::{Value, json};

use common::cluster::{NO_VOTE, TestCluster, eventually};
use common::{TestNode, error_code};
use taba_node::core::clock::Clock;
use taba_node::core::ring::message_position;

fn messages_of(node: &TestNode) -> std::sync::Arc<taba_node::engine::messages::Messages> {
    node.node
        .cluster()
        .and_then(|c| c.replication())
        .map(|r| r.messages.messages().clone())
        .expect("the node is in the cluster")
}

async fn create_queue(node: &TestNode, name: &str) -> reqwest::Response {
    node.client
        .post(node.client_url("/topics"))
        .json(&json!({"name": name, "type": "queue"}))
        .send()
        .await
        .expect("request")
}

async fn publish(node: &TestNode, topic: &str, body: Value) -> String {
    let response = node
        .client
        .post(node.client_url(&format!("/topics/{topic}/messages")))
        .json(&body)
        .send()
        .await
        .expect("request");
    assert_eq!(response.status(), 201, "{:?}", response.text().await);
    let value: Value = response.json().await.unwrap();
    value["id"].as_str().unwrap().to_owned()
}

async fn consume(node: &TestNode, topic: &str, query: &str) -> Vec<Value> {
    let response = node
        .client
        .get(node.client_url(&format!("/topics/{topic}/messages?{query}")))
        .send()
        .await
        .expect("request");
    assert_eq!(response.status(), 200, "{:?}", response.text().await);
    let value: Value = response.json().await.unwrap();
    value["messages"].as_array().cloned().unwrap_or_default()
}

/// UC: UC-DEF-025, UC-DEF-029
#[tokio::test]
async fn topic_definitions_reach_every_node_before_the_api_answers() {
    let cluster = TestCluster::start(3).await;

    let response = create_queue(cluster.node(1), "orders").await;
    assert_eq!(response.status(), 201);
    // Already at the `201` every node knows the definition: the commit was confirmed on all.
    for number in 1..=3 {
        let node = cluster.node(number);
        let got = node
            .client
            .get(node.client_url("/topics/orders"))
            .send()
            .await
            .unwrap();
        assert_eq!(got.status(), 200, "node {number}");
    }
    let ids: Vec<Value> = {
        let mut ids = Vec::new();
        for number in 1..=3 {
            let node = cluster.node(number);
            let body: Value = node
                .client
                .get(node.client_url("/topics/orders"))
                .send()
                .await
                .unwrap()
                .json()
                .await
                .unwrap();
            ids.push(body["id"].clone());
        }
        ids
    };
    assert!(
        ids.windows(2).all(|w| w[0] == w[1]),
        "the same id on all: {ids:?}"
    );

    // The name already exists: `409` without leaving anything pending.
    let again = create_queue(cluster.node(2), "orders").await;
    assert_eq!(error_code(again, 409).await, "topic.already-exists");

    // Changing and removing through other nodes.
    let patch = cluster
        .node(2)
        .client
        .patch(cluster.node(2).client_url("/topics/orders"))
        .json(&json!({"tags": {"time": "payments"}}))
        .send()
        .await
        .unwrap();
    assert_eq!(patch.status(), 200);
    for number in 1..=3 {
        let node = cluster.node(number);
        let body: Value = node
            .client
            .get(node.client_url("/topics/orders"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(body["tags"]["time"], "payments", "node {number}");
    }
    let del = cluster
        .node(3)
        .client
        .delete(cluster.node(3).client_url("/topics/orders"))
        .send()
        .await
        .unwrap();
    assert_eq!(del.status(), 204);
    for number in 1..=3 {
        let node = cluster.node(number);
        let got = node
            .client
            .get(node.client_url("/topics/orders"))
            .send()
            .await
            .unwrap();
        assert_eq!(got.status(), 404, "node {number}");
    }
    cluster.shutdown().await;
}

/// UC: UC-DEF-015, UC-DEF-016
#[tokio::test]
async fn a_new_node_pulls_the_topic_table_after_it_joins() {
    let mut cluster = TestCluster::start(2).await;
    assert_eq!(create_queue(cluster.node(1), "orders").await.status(), 201);
    assert_eq!(
        create_queue(cluster.node(2), "payments").await.status(),
        201
    );

    cluster.add_node(false).await;

    let node = cluster.node(3);
    for topic in ["orders", "payments"] {
        let got = node
            .client
            .get(node.client_url(&format!("/topics/{topic}")))
            .send()
            .await
            .unwrap();
        assert_eq!(got.status(), 200, "{topic}");
    }
    // The topic listing came by stream from one of the members, not in the join response.
    assert!(cluster.network.count("list-topics") >= 1);
    assert!(cluster.network.count("list-tokens") >= 1);
    cluster.shutdown().await;
}

/// UC: UC-ESC-002, UC-ESC-003, UC-ESC-004, UC-ESC-005, UC-ENT-001, UC-ANE-008
#[tokio::test]
async fn a_publication_reaches_every_responsible_node_and_only_the_primary_owns_it() {
    let cluster = TestCluster::start(3).await;
    create_queue(cluster.node(1), "orders").await;

    let mut ids = Vec::new();
    for n in 0..12 {
        ids.push(publish(cluster.node(1 + n % 3), "orders", json!({"n": n})).await);
    }
    for id in &ids {
        // The factor is 3 and there are 3 nodes: all keep the message (full durability comes next).
        for number in 1..=3 {
            let messages = messages_of(cluster.node(number));
            let topic = "orders".to_owned();
            let id = id.clone();
            eventually("the replica to keep the message", move || {
                messages.message_info(&topic, &id).unwrap().is_some()
            })
            .await;
        }
        let ring = cluster.node(1).node.cluster().unwrap().membership().ring();
        let primary = ring.primary(message_position(id)).unwrap().to_owned();
        for number in 1..=3 {
            let info = messages_of(cluster.node(number))
                .message_info("orders", id)
                .unwrap()
                .unwrap();
            assert_eq!(
                info.owned,
                cluster.node_id(number) == primary,
                "node {number}, message {id}"
            );
        }
    }
    // The body is the same on all the copies.
    let id = &ids[0];
    for number in 1..=3 {
        let (_, body) = messages_of(cluster.node(number))
            .read_message("orders", id)
            .unwrap()
            .unwrap();
        assert_eq!(body, br#"{"n":0}"#);
    }
    cluster.shutdown().await;
}

/// UC: UC-CNS-014, UC-ENT-017, UC-ENT-002, UC-INV-006, UC-INV-014
#[tokio::test]
async fn a_get_on_any_node_delivers_what_the_other_nodes_own() {
    let cluster = TestCluster::start(3).await;
    create_queue(cluster.node(1), "orders").await;
    let mut published = Vec::new();
    for n in 0..9 {
        published.push(publish(cluster.node(1), "orders", json!({"n": n})).await);
    }

    // A single node serves the `GET`: what it does not own comes from a `take` to the others.
    let mut got: Vec<Value> = Vec::new();
    for _ in 0..40 {
        let more = consume(
            cluster.node(2),
            "orders",
            "maxMessages=10&visibilityTimeout=60",
        )
        .await;
        got.extend(more);
        if got.len() >= 9 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    let mut ids: Vec<String> = got
        .iter()
        .map(|m| m["id"].as_str().unwrap().to_owned())
        .collect();
    ids.sort();
    let mut expected = published.clone();
    expected.sort();
    assert_eq!(ids, expected, "each message delivered once");
    for message in &got {
        assert_eq!(message["deliveryCount"], 1);
        assert!(message["receiptToken"].is_string());
    }

    // When the client received the message, the quorum (owner plus one copy) already had the token.
    for message in &got {
        let id = message["id"].as_str().unwrap();
        let token = message["receiptToken"].as_str().unwrap();
        let with_token = (1..=3)
            .filter(|n| {
                messages_of(cluster.node(*n))
                    .message_info("orders", id)
                    .unwrap()
                    .is_some_and(|i| i.meta.receipt_token.as_deref() == Some(token))
            })
            .count();
        assert!(
            with_token >= 2,
            "{id}: only {with_token} copies with the token"
        );
    }

    // Nothing more is delivered while they are invisible.
    let none = consume(cluster.node(3), "orders", "maxMessages=10").await;
    assert!(none.is_empty());
    cluster.shutdown().await;
}

/// UC: UC-INV-008, UC-INV-010
#[tokio::test]
async fn an_ack_on_any_node_removes_the_message_everywhere_and_a_stale_token_is_refused() {
    let cluster = TestCluster::start(3).await;
    create_queue(cluster.node(1), "orders").await;
    let id = publish(cluster.node(1), "orders", json!({"x": 1})).await;

    let mut got = Vec::new();
    for _ in 0..40 {
        got = consume(
            cluster.node(1),
            "orders",
            "maxMessages=1&visibilityTimeout=60",
        )
        .await;
        if !got.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert_eq!(got.len(), 1);
    let token = got[0]["receiptToken"].as_str().unwrap().to_owned();

    // The wrong token: `409`, and nothing changes.
    let stale = cluster
        .node(3)
        .client
        .delete(
            cluster
                .node(3)
                .client_url(&format!("/topics/orders/messages/{id}?receiptToken=other")),
        )
        .send()
        .await
        .unwrap();
    assert_eq!(error_code(stale, 409).await, "message.stale-receipt");
    assert!(
        messages_of(cluster.node(1))
            .message_info("orders", &id)
            .unwrap()
            .is_some()
    );

    // The ack through another node removes the message from all.
    let ack = cluster
        .node(3)
        .client
        .delete(cluster.node(3).client_url(&format!(
            "/topics/orders/messages/{id}?receiptToken={token}"
        )))
        .send()
        .await
        .unwrap();
    assert_eq!(ack.status(), 204);
    for number in 1..=3 {
        let messages = messages_of(cluster.node(number));
        let id = id.clone();
        eventually("the message to disappear from the replica", move || {
            messages.message_info("orders", &id).unwrap().is_none()
        })
        .await;
    }
    // Ack again: the message no longer exists.
    let again = cluster
        .node(2)
        .client
        .delete(cluster.node(2).client_url(&format!(
            "/topics/orders/messages/{id}?receiptToken={token}"
        )))
        .send()
        .await
        .unwrap();
    assert_eq!(error_code(again, 404).await, "message.not-found");
    cluster.shutdown().await;
}

/// UC: UC-MSG-016, UC-ESC-010, UC-ESC-013, UC-ESC-014, UC-ESC-016, UC-ESC-019, UC-ESC-020, UC-ESC-022, UC-ESC-024
#[tokio::test]
async fn with_a_node_down_only_the_publications_it_owns_fail_and_the_replica_catches_up() {
    let cluster = TestCluster::start_with(3, NO_VOTE).await;
    create_queue(cluster.node(1), "orders").await;
    // Node 3 falls off the network (isolated): rf 3 and quorum 2 withstand one node out, but only if it is not the
    // message's primary.
    cluster.network.isolate("10.0.0.3");

    let (mut ok_ids, mut failed) = (Vec::new(), 0);
    for n in 0..14 {
        let response = cluster
            .node(1)
            .client
            .post(cluster.node(1).client_url("/topics/orders/messages"))
            .json(&json!({"n": n}))
            .send()
            .await
            .unwrap();
        match response.status().as_u16() {
            201 => {
                let body: Value = response.json().await.unwrap();
                ok_ids.push(body["id"].as_str().unwrap().to_owned());
            }
            503 => {
                assert!(response.headers().contains_key("retry-after"));
                assert_eq!(
                    error_code(response, 503).await,
                    "cluster.quorum-unavailable"
                );
                failed += 1;
            }
            other => panic!("status inesperado {other}"),
        }
    }
    assert!(
        !ok_ids.is_empty() && failed > 0,
        "ok {} failed {failed}",
        ok_ids.len()
    );

    // Nodes 1 and 2 have exactly what was accepted; the refused publishes left nothing
    // (the register was undone).
    for number in [1, 2] {
        let messages = messages_of(cluster.node(number));
        let expected = ok_ids.len() as u64;
        eventually("only the accepted ones kept", move || {
            messages.pending() == expected
        })
        .await;
    }
    assert_eq!(messages_of(cluster.node(3)).pending(), 0);

    // The coordinator counted the rollback and, after the commit ceiling, the delayed replica's warning.
    tokio::time::sleep(Duration::from_millis(700)).await;
    let metrics = cluster.node(1).metrics().await;
    assert!(
        common::series(&metrics, "taba_write_rollbacks_total").is_some_and(|n| n >= 1.0),
        "{metrics}"
    );
    assert!(common::series(&metrics, "taba_write_commit_timeouts_total").is_some_and(|n| n >= 1.0));
    assert!(
        common::series(
            &metrics,
            "taba_write_coordinated_total{taba_outcome=\"failed\"}"
        )
        .is_some_and(|n| n >= 1.0)
    );

    // The node comes back: the register and the commit in the background reach it, and the block returns to the full factor.
    cluster.network.restore("10.0.0.3");
    let third = messages_of(cluster.node(3));
    let expected = ok_ids.clone();
    eventually("the delayed node to receive everything", move || {
        expected
            .iter()
            .all(|id| third.message_info("orders", id).unwrap().is_some())
    })
    .await;
    cluster.shutdown().await;
}

/// UC: UC-ESC-015
#[tokio::test]
async fn a_lost_call_is_retried_before_the_quorum_gives_up() {
    let cluster = TestCluster::start(3).await;
    create_queue(cluster.node(1), "orders").await;
    // Loses the first two register calls to each node: with three attempts it goes through.
    for host in ["10.0.0.2", "10.0.0.3"] {
        cluster.network.lose_kind(host, "write-register", 2);
    }
    let id = publish(cluster.node(1), "orders", json!({"a": 1})).await;
    let messages = messages_of(cluster.node(1));
    assert!(
        messages.message_info("orders", &id).unwrap().is_some() || {
            eventually("the message to appear", || {
                messages.message_info("orders", &id).unwrap().is_some()
            })
            .await;
            true
        }
    );
    cluster.shutdown().await;
}

/// UC: UC-ACK-008, UC-ACK-009, UC-INV-012
#[tokio::test]
async fn an_ack_that_cannot_reach_the_primary_fails_and_the_message_stays() {
    let cluster = TestCluster::start_with(3, NO_VOTE).await;
    create_queue(cluster.node(1), "orders").await;
    let id = publish(cluster.node(1), "orders", json!({"x": 1})).await;
    let mut got = Vec::new();
    for _ in 0..40 {
        got = consume(
            cluster.node(1),
            "orders",
            "maxMessages=1&visibilityTimeout=60",
        )
        .await;
        if !got.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    let token = got[0]["receiptToken"].as_str().unwrap().to_owned();
    let primary = cluster
        .node(1)
        .node
        .cluster()
        .unwrap()
        .membership()
        .ring()
        .primary(message_position(&id))
        .unwrap()
        .to_owned();
    let primary_number = (1..=3).find(|n| cluster.node_id(*n) == primary).unwrap();
    // The ack goes through a node that is not the primary, and the primary vanishes from the network.
    let via = (1..=3).find(|n| *n != primary_number).unwrap();
    cluster.network.isolate(&format!("10.0.0.{primary_number}"));
    let ack = cluster
        .node(via)
        .client
        .delete(cluster.node(via).client_url(&format!(
            "/topics/orders/messages/{id}?receiptToken={token}"
        )))
        .send()
        .await
        .unwrap();
    assert_eq!(error_code(ack, 503).await, "cluster.quorum-unavailable");
    assert!(
        messages_of(cluster.node(primary_number))
            .message_info("orders", &id)
            .unwrap()
            .is_some(),
        "the message is still there"
    );

    // The primary comes back and the same ack goes through.
    cluster.network.restore(&format!("10.0.0.{primary_number}"));
    let ack = cluster
        .node(via)
        .client
        .delete(cluster.node(via).client_url(&format!(
            "/topics/orders/messages/{id}?receiptToken={token}"
        )))
        .send()
        .await
        .unwrap();
    assert_eq!(ack.status(), 204);
    cluster.shutdown().await;
}

/// UC: UC-INV-007, UC-INV-016
#[tokio::test]
async fn the_visibility_can_be_changed_and_an_expired_one_is_redelivered_without_propagation() {
    let cluster = TestCluster::start(3).await;
    create_queue(cluster.node(1), "orders").await;
    let id = publish(cluster.node(2), "orders", json!({"x": 1})).await;
    let mut got = Vec::new();
    for _ in 0..40 {
        got = consume(
            cluster.node(3),
            "orders",
            "maxMessages=1&visibilityTimeout=60",
        )
        .await;
        if !got.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    let token = got[0]["receiptToken"].as_str().unwrap().to_owned();

    // The `PUT` through another node extends the deadline on all the copies, without changing the counter.
    let put = cluster
        .node(1)
        .client
        .put(cluster.node(1).client_url(&format!(
            "/topics/orders/messages/{id}/visibility?receiptToken={token}"
        )))
        .json(&json!({"visibilityTimeout": 600}))
        .send()
        .await
        .unwrap();
    assert_eq!(put.status(), 200);
    let wanted = cluster.clock.now_millis() + 600_000;
    for number in 1..=3 {
        let messages = messages_of(cluster.node(number));
        let id = id.clone();
        eventually("the new deadline on all the copies", move || {
            messages
                .message_info("orders", &id)
                .unwrap()
                .is_some_and(|i| i.meta.invisible_until_ms == wanted && i.meta.delivery_count == 1)
        })
        .await;
    }

    // After the deadline the message comes back on its own, through the owner, and the next delivery raises the counter.
    cluster.clock.advance(Duration::from_secs(601));
    let mut again = Vec::new();
    for _ in 0..40 {
        again = consume(
            cluster.node(2),
            "orders",
            "maxMessages=1&visibilityTimeout=60",
        )
        .await;
        if !again.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert_eq!(again.len(), 1);
    assert_eq!(again[0]["id"], id);
    assert_eq!(again[0]["deliveryCount"], 2);
    assert_ne!(again[0]["receiptToken"], got[0]["receiptToken"]);
    cluster.shutdown().await;
}

/// UC: UC-ESC-029, UC-INV-019
#[tokio::test]
async fn a_message_that_spent_its_deliveries_moves_to_the_dead_letter_on_every_node() {
    let cluster = TestCluster::start(3).await;
    let created = cluster
        .node(1)
        .client
        .post(cluster.node(1).client_url("/topics"))
        .json(&json!({"name": "orders", "type": "queue", "maxDeliveries": 2}))
        .send()
        .await
        .unwrap();
    assert_eq!(created.status(), 201);
    let id = publish(cluster.node(1), "orders", json!({"x": 1})).await;

    for round in 1..=2 {
        let mut got = Vec::new();
        for _ in 0..40 {
            got = consume(
                cluster.node(2),
                "orders",
                "maxMessages=1&visibilityTimeout=60",
            )
            .await;
            if !got.is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        assert_eq!(got.len(), 1, "entrega {round}");
        assert_eq!(got[0]["deliveryCount"], round);
        cluster.clock.advance(Duration::from_secs(61));
    }

    // The second invisibility expired with the deliveries spent: the owner sends it to the dead-letter, and
    // all the copies make the move.
    for number in 1..=3 {
        let messages = messages_of(cluster.node(number));
        let id = id.clone();
        eventually("the move to the dead-letter", move || {
            messages
                .message_info("orders-dead-letter", &id)
                .unwrap()
                .is_some()
                && messages.message_info("orders", &id).unwrap().is_none()
        })
        .await;
    }
    let mut in_dead_letter = Vec::new();
    for _ in 0..40 {
        in_dead_letter = consume(cluster.node(3), "orders-dead-letter", "maxMessages=1").await;
        if !in_dead_letter.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert_eq!(in_dead_letter.len(), 1);
    assert_eq!(in_dead_letter[0]["id"], id);
    assert_eq!(in_dead_letter[0]["deadLetterReason"], "max-deliveries");
    cluster.shutdown().await;
}

/// UC: UC-CNS-030, UC-ENT-023, UC-ENT-024, UC-ENT-025
#[tokio::test]
async fn a_long_poll_on_one_node_is_woken_by_a_message_that_arrives_at_another() {
    let cluster = TestCluster::start(3).await;
    create_queue(cluster.node(1), "orders").await;

    // The listener waits on node 2; the message enters through node 1 and the owner may be any one.
    let waiter = {
        let client = cluster.node(2).client.clone();
        let url = cluster
            .node(2)
            .client_url("/topics/orders/messages?waitTime=10&maxMessages=1&visibilityTimeout=60");
        tokio::spawn(async move { client.get(url).send().await.unwrap().json::<Value>().await })
    };
    // Node 2's interest reaches the others by signal, only once.
    let calls = cluster.network.clone();
    eventually("the listener signal", move || calls.count("channel") >= 2).await;
    let id = publish(cluster.node(1), "orders", json!({"x": 1})).await;

    let body = tokio::time::timeout(Duration::from_secs(5), waiter)
        .await
        .expect("o long polling deveria acordar")
        .unwrap()
        .unwrap();
    assert_eq!(body["messages"][0]["id"], id);
    cluster.shutdown().await;
}

const ROOT: &str = "raiz-raiz-raiz-raiz-raiz-raiz-raiz-0001";

async fn create_token(node: &TestNode) -> (String, String) {
    let response = node
        .client
        .post(node.client_url("/tokens"))
        .header("authorization", format!("Bearer {ROOT}"))
        .json(&json!({"name": "app", "permissions": ["consume"], "topicPrefixes": []}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 201);
    let body: Value = response.json().await.unwrap();
    (
        body["id"].as_str().unwrap().to_owned(),
        body["token"].as_str().unwrap().to_owned(),
    )
}

async fn status_with(node: &TestNode, token: &str) -> u16 {
    node.client
        .get(node.client_url("/topics"))
        .header("authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap()
        .status()
        .as_u16()
}

/// UC: UC-DEF-034, UC-DEF-022, UC-ESC-030
#[tokio::test]
async fn tokens_reach_every_node_and_a_new_node_pulls_them() {
    let mut cluster = TestCluster::start_with(2, &[("TABA_AUTH_ROOT_TOKEN", ROOT)]).await;
    let (revoked_id, revoked) = create_token(cluster.node(1)).await;
    let (_, kept) = create_token(cluster.node(2)).await;

    // The token created by one node counts on both (the creation only answers with all confirming).
    for number in [1, 2] {
        assert_eq!(status_with(cluster.node(number), &revoked).await, 200);
        assert_eq!(status_with(cluster.node(number), &kept).await, 200);
    }

    // The revocation through another node counts on all.
    let response = cluster
        .node(2)
        .client
        .delete(cluster.node(2).client_url(&format!("/tokens/{revoked_id}")))
        .header("authorization", format!("Bearer {ROOT}"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 204);
    for number in [1, 2] {
        assert_eq!(status_with(cluster.node(number), &revoked).await, 401);
        assert_eq!(status_with(cluster.node(number), &kept).await, 200);
    }

    // The new node pulls the tokens along with the topics: what is valid, is valid; what was revoked, is not.
    cluster.add_node(false).await;
    assert_eq!(status_with(cluster.node(3), &kept).await, 200);
    assert_eq!(status_with(cluster.node(3), &revoked).await, 401);
    cluster.shutdown().await;
}

/// UC: UC-RET-006
#[tokio::test]
async fn every_node_expires_its_own_copies_by_ttl_without_propagating() {
    let cluster = TestCluster::start(3).await;
    let created = cluster
        .node(1)
        .client
        .post(cluster.node(1).client_url("/topics"))
        .json(&json!({"name": "orders", "type": "queue", "ttl": 60}))
        .send()
        .await
        .unwrap();
    assert_eq!(created.status(), 201);
    let id = publish(cluster.node(1), "orders", json!({"x": 1})).await;
    for number in 1..=3 {
        let messages = messages_of(cluster.node(number));
        let id = id.clone();
        eventually("the stored copy", move || {
            messages.message_info("orders", &id).unwrap().is_some()
        })
        .await;
    }
    cluster.network.clear_calls();

    // Time passes for all; each node's sweep deletes its copy, without a call between nodes.
    cluster.clock.advance(Duration::from_secs(61));
    for number in 1..=3 {
        let messages = messages_of(cluster.node(number));
        let id = id.clone();
        eventually("the expired copy deleted", move || {
            messages.message_info("orders", &id).unwrap().is_none()
        })
        .await;
    }
    assert!(
        cluster
            .network
            .calls()
            .iter()
            .all(|c| !c.request.starts_with("write") && !c.request.starts_with("ack")),
        "the expiration is local and deterministic"
    );
    cluster.shutdown().await;
}

/// The nodes responsible for a message, in order (primary, secondary, tertiary), as a number.
fn owners_of(cluster: &TestCluster, id: &str) -> Vec<usize> {
    let ring = cluster.node(1).node.cluster().unwrap().membership().ring();
    ring.owners(message_position(id), 3)
        .into_iter()
        .map(|owner| (1..=3).find(|n| cluster.node_id(*n) == owner).unwrap())
        .collect()
}

fn host(number: usize) -> String {
    format!("10.0.0.{number}")
}

/// Delivers the message through the `GET` on the given node and returns the `receiptToken`.
async fn deliver_at(node: &TestNode) -> String {
    let mut got = Vec::new();
    for _ in 0..80 {
        got = consume(node, "orders", "maxMessages=1&visibilityTimeout=300").await;
        if !got.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert_eq!(got.len(), 1);
    got[0]["receiptToken"].as_str().unwrap().to_owned()
}

/// UC: UC-INV-008, UC-INV-010
///
/// The message no longer exists on any owner: the ack answers `404`, even if the answers of the two
/// owners that are not the primary arrive before its own. The error that counts is the primary's, and not the
/// `503` of the quorum count (on a slow node the arrival order changes).
#[tokio::test]
async fn an_ack_of_a_gone_message_answers_404_even_when_the_primary_answers_last() {
    let cluster = TestCluster::start(3).await;
    create_queue(cluster.node(1), "orders").await;
    let id = publish(cluster.node(1), "orders", json!({"x": 1})).await;
    let token = deliver_at(cluster.node(1)).await;
    let ack = cluster
        .node(1)
        .client
        .delete(cluster.node(1).client_url(&format!(
            "/topics/orders/messages/{id}?receiptToken={token}"
        )))
        .send()
        .await
        .unwrap();
    assert_eq!(ack.status(), 204);
    for number in 1..=3 {
        let messages = messages_of(cluster.node(number));
        let id = id.clone();
        eventually("the message to disappear from the replica", move || {
            messages.message_info("orders", &id).unwrap().is_none()
        })
        .await;
    }

    let primary = owners_of(&cluster, &id)[0];
    cluster
        .network
        .set_delay_host_kind(&host(primary), "ack-register", Duration::from_millis(300));
    let via = (1..=3).find(|n| *n != primary).unwrap();
    let again = cluster
        .node(via)
        .client
        .delete(cluster.node(via).client_url(&format!(
            "/topics/orders/messages/{id}?receiptToken={token}"
        )))
        .send()
        .await
        .unwrap();
    assert_eq!(error_code(again, 404).await, "message.not-found");
    cluster.shutdown().await;
}

/// UC: UC-MSG-014, UC-MSG-015, UC-ESC-004, UC-ESC-006, UC-ESC-007, UC-ESC-011
#[tokio::test]
async fn the_answer_comes_only_after_the_quorum_committed_whichever_node_took_the_call() {
    let cluster = TestCluster::start(3).await;
    create_queue(cluster.node(1), "orders").await;
    for n in 0..12usize {
        let via = 1 + n % 3;
        let id = publish(cluster.node(via), "orders", json!({"n": n})).await;
        // In the response the primary already has the message and owns it, and the quorum of 2 already has it
        // committed: without waiting.
        let owners = owners_of(&cluster, &id);
        let primary = messages_of(cluster.node(owners[0]))
            .message_info("orders", &id)
            .unwrap()
            .expect("the primary already has it");
        assert!(primary.owned);
        let holders = (1..=3)
            .filter(|n| {
                messages_of(cluster.node(*n))
                    .message_info("orders", &id)
                    .unwrap()
                    .is_some()
            })
            .count();
        assert!(holders >= 2, "message {n}: only {holders} nodes have it");
    }
    cluster.shutdown().await;
}

/// UC: UC-ACK-010, UC-ESC-020, UC-ESC-021, UC-ESC-024
#[tokio::test]
async fn an_ack_decided_by_the_quorum_is_not_undone_by_a_replica_that_is_down() {
    let cluster = TestCluster::start_with(3, NO_VOTE).await;
    create_queue(cluster.node(1), "orders").await;
    let id = publish(cluster.node(1), "orders", json!({"x": 1})).await;
    let owners = owners_of(&cluster, &id);
    let (primary, secondary, tertiary) = (owners[0], owners[1], owners[2]);
    let token = deliver_at(cluster.node(primary)).await;
    for number in 1..=3 {
        let messages = messages_of(cluster.node(number));
        let id = id.clone();
        let token = token.clone();
        eventually("the delivery on all the copies", move || {
            messages
                .message_info("orders", &id)
                .unwrap()
                .is_some_and(|i| i.meta.receipt_token.as_deref() == Some(token.as_str()))
        })
        .await;
    }

    // The tertiary falls: the ack (primary + secondary) is decided and is not undone.
    cluster.network.isolate(&host(tertiary));
    let ack = cluster
        .node(secondary)
        .client
        .delete(cluster.node(secondary).client_url(&format!(
            "/topics/orders/messages/{id}?receiptToken={token}"
        )))
        .send()
        .await
        .unwrap();
    assert_eq!(ack.status(), 204);
    for number in [primary, secondary] {
        let messages = messages_of(cluster.node(number));
        let id = id.clone();
        eventually("the ack in the quorum", move || {
            messages.message_info("orders", &id).unwrap().is_none()
        })
        .await;
    }
    assert!(
        messages_of(cluster.node(tertiary))
            .message_info("orders", &id)
            .unwrap()
            .is_some(),
        "the node that fell still has the copy"
    );

    // When it comes back, the coordinator, which insisted in the background, reaches it.
    cluster.network.restore(&host(tertiary));
    let messages = messages_of(cluster.node(tertiary));
    let wanted = id.clone();
    eventually("the ack to reach the delayed replica", move || {
        messages.message_info("orders", &wanted).unwrap().is_none()
    })
    .await;
    cluster.shutdown().await;
}

/// UC: UC-CNS-019, UC-CNS-021, UC-INV-012
#[tokio::test]
async fn a_get_whose_registrations_fail_answers_503_and_leaves_the_messages_ready() {
    let cluster = TestCluster::start(3).await;
    create_queue(cluster.node(1), "orders").await;
    let mut ids = Vec::new();
    for n in 0..9 {
        ids.push(publish(cluster.node(1), "orders", json!({"n": n})).await);
    }
    let owner = owners_of(&cluster, &ids[0])[0];

    // No invisibility register reaches another node: the quorum closes for no one.
    for number in 1..=3 {
        cluster
            .network
            .lose_kind(&host(number), "visibility-register", 1_000_000);
    }
    let response = cluster
        .node(owner)
        .client
        .get(
            cluster
                .node(owner)
                .client_url("/topics/orders/messages?maxMessages=10&visibilityTimeout=60"),
        )
        .send()
        .await
        .unwrap();
    assert!(response.headers().contains_key("retry-after"));
    assert_eq!(error_code(response, 503).await, "delivery.unavailable");

    // The messages that failed never entered the response and stay ready, as they were.
    for number in 1..=3 {
        cluster
            .network
            .lose_kind(&host(number), "visibility-register", 0);
    }
    let mut got = Vec::new();
    for _ in 0..80 {
        got.extend(
            consume(
                cluster.node(owner),
                "orders",
                "maxMessages=10&visibilityTimeout=60",
            )
            .await,
        );
        if !got.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert!(!got.is_empty());
    assert!(got.iter().all(|m| m["deliveryCount"] == 1));
    cluster.shutdown().await;
}

/// UC: UC-CNS-023, UC-ESC-017, UC-ACK-008
#[tokio::test]
async fn a_rollback_that_does_not_arrive_leaves_a_pending_that_expires_by_itself() {
    let cluster = TestCluster::start(3).await;
    create_queue(cluster.node(1), "orders").await;
    let id = publish(cluster.node(1), "orders", json!({"x": 1})).await;
    let owners = owners_of(&cluster, &id);
    let (primary, secondary, tertiary) = (owners[0], owners[1], owners[2]);
    let token = deliver_at(cluster.node(primary)).await;

    // The ack enters through the tertiary: the register does not reach the primary, the quorum does not close, and the
    // rollback does not reach the secondary (which had already registered).
    cluster
        .network
        .lose_kind(&host(primary), "ack-register", 1_000_000);
    cluster
        .network
        .lose_kind(&host(secondary), "ack-rollback", 1_000_000);
    let ack_url = format!("/topics/orders/messages/{id}?receiptToken={token}");
    let first = cluster
        .node(tertiary)
        .client
        .delete(cluster.node(tertiary).client_url(&ack_url))
        .send()
        .await
        .unwrap();
    assert_eq!(error_code(first, 503).await, "cluster.quorum-unavailable");
    let replica = cluster
        .node(secondary)
        .node
        .cluster()
        .unwrap()
        .replication()
        .unwrap()
        .replica
        .clone();
    assert_eq!(
        replica.pending(),
        1,
        "the register stayed pending on the secondary"
    );
    for number in 1..=3 {
        assert!(
            messages_of(cluster.node(number))
                .message_info("orders", &id)
                .unwrap()
                .is_some(),
            "the message is still on all"
        );
    }

    // `write.pending.ttl` (30 s) passes and the pending one disappears on its own; the same ack now goes through.
    cluster.network.lose_kind(&host(primary), "ack-register", 0);
    cluster
        .network
        .lose_kind(&host(secondary), "ack-rollback", 0);
    cluster.clock.advance(Duration::from_secs(31));
    assert_eq!(replica.pending(), 0);
    let again = cluster
        .node(tertiary)
        .client
        .delete(cluster.node(tertiary).client_url(&ack_url))
        .send()
        .await
        .unwrap();
    assert_eq!(again.status(), 204);
    cluster.shutdown().await;
}

#[tokio::test]
async fn a_lost_ready_signal_is_corrected_by_the_periodic_repeat() {
    let cluster = TestCluster::start(3).await;
    create_queue(cluster.node(1), "orders").await;
    // No signal reaches anyone while the messages are published.
    for number in 1..=3 {
        cluster
            .network
            .lose_kind(&host(number), "channel", 1_000_000);
    }
    let mut published = Vec::new();
    for n in 0..9 {
        published.push(publish(cluster.node(1), "orders", json!({"n": n})).await);
    }
    // Node 2 only knows its own; the rest appears when the state is repeated.
    for number in 1..=3 {
        cluster.network.lose_kind(&host(number), "channel", 0);
    }
    let mut got: Vec<Value> = Vec::new();
    for _ in 0..200 {
        got.extend(
            consume(
                cluster.node(2),
                "orders",
                "maxMessages=10&visibilityTimeout=300",
            )
            .await,
        );
        if got.len() >= published.len() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(
        got.len(),
        published.len(),
        "o estado repetido corrigiu a tabela"
    );
    cluster.shutdown().await;
}

/// UC: UC-CHS-005
///
/// The ack was decided and the replicas applied it, but the register on the owner expired (`write.pending.ttl`) while
/// the commit to it was on the way, and another operation took the message in the meantime. The coordinator registers
/// the ack again, gets a conflict and must keep trying until the other operation is gone, not give up: the owner
/// would keep a message that no longer exists on the replicas (PRB-076, found by the chaos run).
#[tokio::test]
async fn an_ack_the_replicas_applied_reaches_the_owner_even_if_another_operation_took_the_message()
{
    use taba_node::cluster::protocol::{AckRegisterRequest, OperationRef, Step};

    let cluster = TestCluster::start_with(3, NO_VOTE).await;
    create_queue(cluster.node(1), "orders").await;
    let id = publish(cluster.node(1), "orders", json!({"x": 1})).await;
    let mut got = Vec::new();
    for _ in 0..40 {
        got = consume(
            cluster.node(1),
            "orders",
            "maxMessages=1&visibilityTimeout=600",
        )
        .await;
        if !got.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    let token = got[0]["receiptToken"].as_str().unwrap().to_owned();
    let owner_id = cluster
        .node(1)
        .node
        .cluster()
        .unwrap()
        .membership()
        .ring()
        .primary(message_position(&id))
        .unwrap()
        .to_owned();
    let owner = (1..=3).find(|n| cluster.node_id(*n) == owner_id).unwrap();
    let via = (1..=3).find(|n| *n != owner).unwrap();
    let owner_host = common::cluster::host_of(owner);
    let replica = cluster
        .node(owner)
        .node
        .cluster()
        .and_then(|c| c.replication())
        .map(|r| r.replica.clone())
        .unwrap();

    // The commit to the owner takes 1 s: the register lands at once, and the commit is still on the way.
    cluster
        .network
        .set_delay_host_kind(&owner_host, "ack-commit", Duration::from_millis(1000));
    let client = cluster.node(via);
    let url = client.client_url(&format!(
        "/topics/orders/messages/{id}?receiptToken={token}"
    ));
    let http = client.client.clone();
    let ack = tokio::spawn(async move { http.delete(url).send().await });

    // While the commit is on the way: the register of the owner expires, and another operation takes the message.
    tokio::time::sleep(Duration::from_millis(300)).await;
    cluster.clock.advance(Duration::from_secs(31));
    replica
        .ack(
            "orders",
            &id,
            Step::Register(AckRegisterRequest {
                operation_id: "another-operation".into(),
                coordinator_id: "test".into(),
                receipt_token: String::new(),
                delete_at_ms: None,
                discard: true,
            }),
        )
        .await
        .expect("the other operation registers");

    // The commit reaches the owner, finds nothing and registers again against the other operation. It must keep trying.
    tokio::time::sleep(Duration::from_millis(1500)).await;
    replica
        .ack(
            "orders",
            &id,
            Step::Rollback(OperationRef {
                operation_id: "another-operation".into(),
            }),
        )
        .await
        .expect("the other operation ends");

    let _ = ack.await;
    let owner_messages = messages_of(cluster.node(owner));
    eventually("the owner to apply the ack too", move || {
        owner_messages
            .message_info("orders", &id)
            .unwrap()
            .is_none()
    })
    .await;
    cluster.shutdown().await;
}
