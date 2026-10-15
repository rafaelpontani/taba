//! The FIFO queue in the cluster (doc 17, sections 16.3 to 16.7; doc 20a, UC-FIF; level **C**): the group decides
//! the block, the publish is coordinated by the group's primary, the quorum includes the primary and the
//! secondary, and the deduplication and the order survive the change of primary.

mod common;

use std::time::Duration;

use reqwest::Response;
use serde_json::{Value, json};

use common::TestNode;
use common::cluster::{TestCluster, host_of, settled, total};
use taba_node::core::ring::fifo_position;

const TOPIC: &str = "orders";

async fn fifo_queue(node: &TestNode, name: &str) -> String {
    let response = node
        .client
        .post(node.client_url("/topics"))
        .json(&json!({ "name": name, "type": "queue", "fifo": true }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 201, "{:?}", response.text().await);
    response.json::<Value>().await.unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

async fn publish(node: &TestNode, group: &str, dedup: Option<&str>, body: &str) -> Response {
    let mut query = format!("?groupId={group}");
    if let Some(key) = dedup {
        query.push_str(&format!("&deduplicationId={key}"));
    }
    node.client
        .post(node.client_url(&format!("/topics/{TOPIC}/messages{query}")))
        .header("content-type", "application/json")
        .body(body.to_owned())
        .send()
        .await
        .unwrap()
}

async fn get(node: &TestNode, max: u32) -> Vec<Value> {
    let response = node
        .client_get(&format!(
            "/topics/{TOPIC}/messages?maxMessages={max}&visibilityTimeout=600"
        ))
        .await;
    assert_eq!(response.status(), 200);
    response.json::<Value>().await.unwrap()["messages"]
        .as_array()
        .unwrap()
        .clone()
}

/// Waits for the `GET` to bring something (the notice between nodes takes a moment).
async fn get_some(node: &TestNode, max: u32) -> Vec<Value> {
    for _ in 0..80 {
        let got = get(node, max).await;
        if !got.is_empty() {
            return got;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Vec::new()
}

async fn ack(node: &TestNode, message: &Value) -> Response {
    node.client
        .delete(node.client_url(&format!(
            "/topics/{TOPIC}/messages/{}?receiptToken={}",
            message["id"].as_str().unwrap(),
            message["receiptToken"].as_str().unwrap()
        )))
        .send()
        .await
        .unwrap()
}

/// The number of the node that is the group's primary (and the secondary's), by node 1's table.
fn owners_of_group(cluster: &TestCluster, topic_id: &str, group: &str) -> Vec<usize> {
    let service = cluster.node(1).node.cluster().unwrap();
    let owners = service.owners_of(fifo_position(topic_id, group)).unwrap();
    owners
        .iter()
        .map(|m| {
            (1..=3)
                .find(|n| cluster.node_id(*n) == m.node_id)
                .expect("it is one of the test's nodes")
        })
        .collect()
}

/// UC: UC-FIF-026, UC-FIF-010, UC-FIF-002, UC-FIF-015, UC-FIF-044, UC-FIF-047, UC-FWD-011
#[tokio::test]
async fn a_publication_from_any_node_is_coordinated_by_the_primary_of_the_group() {
    let cluster = TestCluster::start(3).await;
    settled(&cluster).await;
    let topic_id = fifo_queue(cluster.node(1), TOPIC).await;
    let owners = owners_of_group(&cluster, &topic_id, "g1");
    let primary = owners[0];
    let others: Vec<usize> = (1..=3).filter(|n| *n != primary).collect();

    // Publishing on the primary itself does not forward; on the others, one forward each.
    cluster.network.clear_calls();
    let first = publish(cluster.node(primary), "g1", None, r#"{"n":1}"#).await;
    assert_eq!(first.status(), 201);
    assert_eq!(cluster.network.count("fifo-publish"), 0);
    let mut ids = vec![
        first.json::<Value>().await.unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned(),
    ];
    for (round, node) in others.iter().enumerate() {
        let before = cluster.network.count("fifo-publish");
        let response = publish(
            cluster.node(*node),
            "g1",
            None,
            &format!(r#"{{"n":{}}}"#, round + 2),
        )
        .await;
        assert_eq!(response.status(), 201);
        assert_eq!(cluster.network.count("fifo-publish"), before + 1);
        let body: Value = response.json().await.unwrap();
        assert_eq!(body["duplicate"], false);
        ids.push(body["id"].as_str().unwrap().to_owned());
    }
    // The group's `id`s grow in the order the primary accepted.
    assert!(ids.windows(2).all(|w| w[0] < w[1]), "{ids:?}");

    // Repeating the content in the same group, from any node, returns the original.
    let again = publish(cluster.node(others[0]), "g1", None, r#"{"n":1}"#).await;
    assert_eq!(again.status(), 200);
    let body: Value = again.json().await.unwrap();
    assert_eq!(
        (body["id"].as_str().unwrap(), body["duplicate"].as_bool()),
        (ids[0].as_str(), Some(true))
    );

    // The delivery is one at a time, in order, from any node.
    // The receipt carries the group's position: a malformed receipt is `409`, from any node.
    let stale = cluster
        .node(others[0])
        .client
        .delete(cluster.node(others[0]).client_url(&format!(
            "/topics/{TOPIC}/messages/{}?receiptToken=malformado",
            ids[0]
        )))
        .send()
        .await
        .unwrap();
    assert_eq!(code_of(stale, 409).await, "message.stale-receipt");

    for expected in &ids {
        let got = get_some(cluster.node(others[1]), 10).await;
        assert_eq!(got.len(), 1, "one group, one message in flight: {got:?}");
        assert_eq!(got[0]["id"], *expected);
        assert_eq!(ack(cluster.node(others[0]), &got[0]).await.status(), 204);
    }
    assert!(get(cluster.node(1), 10).await.is_empty());

    // `taba.fifo.forward` counts the forwards to the group's primary (one per node that is not it).
    let mut forwarded = 0.0;
    for number in 1..=3 {
        forwarded += total(
            &cluster.node(number).metrics().await,
            "taba_fifo_forward_total",
            "taba_outcome=\"ok\"",
        );
    }
    assert_eq!(forwarded, 3.0, "two forwarded publishes and the duplicate");
    cluster.shutdown().await;
}

async fn code_of(response: Response, status: u16) -> String {
    assert_eq!(response.status(), status);
    response.json::<Value>().await.unwrap()["code"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn messages_of(node: &TestNode) -> std::sync::Arc<taba_node::engine::messages::Messages> {
    node.node
        .cluster()
        .and_then(|c| c.replication())
        .map(|r| r.messages.messages().clone())
        .expect("the node is in the cluster")
}

/// UC: UC-FIF-030, UC-FIF-032, UC-FIF-031
#[tokio::test]
async fn a_primary_or_a_secondary_that_is_down_but_still_in_the_table_stops_the_group() {
    let cluster = TestCluster::start_with(3, common::cluster::NO_VOTE).await;
    settled(&cluster).await;
    let topic_id = fifo_queue(cluster.node(1), TOPIC).await;
    let owners = owners_of_group(&cluster, &topic_id, "g1");
    let (primary, secondary, third) = (owners[0], owners[1], owners[2]);

    // Primary down and still in the table: the group's publish, from any node, is `503`.
    cluster.network.isolate(&host_of(primary));
    let blocked = publish(cluster.node(third), "g1", None, r#"{"n":1}"#).await;
    assert_eq!(code_of(blocked, 503).await, "fifo.primary-unavailable");
    cluster.network.restore(&host_of(primary));

    // Secondary down and still in the table: the group's write, delivery and ack answer
    // `503`, even with the tertiary healthy (the quorum includes the secondary).
    let ok = publish(cluster.node(primary), "g1", None, r#"{"n":1}"#).await;
    assert_eq!(ok.status(), 201);
    let delivered = get_some(cluster.node(primary), 10).await;
    assert_eq!(delivered.len(), 1);
    cluster.network.isolate(&host_of(secondary));
    let write = publish(cluster.node(primary), "g1", None, r#"{"n":2}"#).await;
    assert_eq!(code_of(write, 503).await, "cluster.quorum-unavailable");
    let ack_result = ack(cluster.node(primary), &delivered[0]).await;
    assert_eq!(ack_result.status(), 503);
    cluster.network.restore(&host_of(secondary));

    // It came back: everything goes on (nothing was left pending).
    assert_eq!(ack(cluster.node(third), &delivered[0]).await.status(), 204);
    let after = publish(cluster.node(third), "g1", None, r#"{"n":3}"#).await;
    assert_eq!(after.status(), 201);
    cluster.shutdown().await;
}

/// UC: UC-FIF-033, UC-FIF-034, UC-FIF-035
#[tokio::test]
async fn when_the_primary_leaves_the_secondary_takes_the_group_without_reordering_or_forgetting() {
    let cluster = TestCluster::start(3).await;
    settled(&cluster).await;
    let topic_id = fifo_queue(cluster.node(1), TOPIC).await;
    let owners = owners_of_group(&cluster, &topic_id, "g1");
    let (primary, secondary, third) = (owners[0], owners[1], owners[2]);

    let mut ids = Vec::new();
    for n in 1..=3 {
        let response = publish(cluster.node(third), "g1", None, &format!(r#"{{"n":{n}}}"#)).await;
        assert_eq!(response.status(), 201);
        ids.push(
            response.json::<Value>().await.unwrap()["id"]
                .as_str()
                .unwrap()
                .to_owned(),
        );
    }
    // m1 is confirmed, and m2 goes out in flight (the invisibility lasts 600 s).
    let first = get_some(cluster.node(third), 10).await;
    assert_eq!(first[0]["id"], ids[0]);
    assert_eq!(ack(cluster.node(third), &first[0]).await.status(), 204);
    let second = get_some(cluster.node(third), 10).await;
    assert_eq!(second[0]["id"], ids[1]);

    // The primary falls and is excluded; the secondary inherits the group.
    let old = cluster.node_id(primary);
    cluster.network.isolate(&host_of(primary));
    common::cluster::eventually("the primary to be excluded", || {
        [secondary, third]
            .iter()
            .all(|n| !cluster.member_ids(*n).contains(&old))
    })
    .await;

    // Without reordering: m1 does not come back, and m2 (in flight) stays in flight; nothing is delivered yet.
    assert!(get(cluster.node(secondary), 10).await.is_empty());
    assert!(get(cluster.node(third), 10).await.is_empty());

    // The deduplication was inherited: repeating m3 returns the original.
    let duplicate = publish(cluster.node(secondary), "g1", None, r#"{"n":3}"#).await;
    assert_eq!(duplicate.status(), 200);
    let body: Value = duplicate.json().await.unwrap();
    assert_eq!(body["id"], ids[2]);
    assert_eq!(body["duplicate"], true);
    // And m1, already confirmed, too: the short row exists on all the nodes.
    let again = publish(cluster.node(third), "g1", None, r#"{"n":1}"#).await;
    assert_eq!(again.status(), 200);

    // The group's `id` keeps growing.
    let next = publish(cluster.node(third), "g1", None, r#"{"n":4}"#).await;
    assert_eq!(next.status(), 201);
    let next_id = next.json::<Value>().await.unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(next_id > ids[2]);

    // With the invisibility expired, m2 comes back as the head of the group, then m3 and m4, one at a time.
    cluster.clock.advance(Duration::from_secs(601));
    let redelivered = get_some(cluster.node(third), 10).await;
    assert_eq!(redelivered.len(), 1);
    assert_eq!(redelivered[0]["id"], ids[1]);
    assert_eq!(
        ack(cluster.node(secondary), &redelivered[0]).await.status(),
        204
    );
    for expected in [&ids[2], &next_id] {
        let got = get_some(cluster.node(secondary), 10).await;
        assert_eq!(got[0]["id"], *expected);
        assert_eq!(ack(cluster.node(third), &got[0]).await.status(), 204);
    }
    cluster.shutdown().await;
}

/// UC: UC-FIF-009
#[tokio::test]
async fn simultaneous_publications_with_the_same_key_create_one_message() {
    let cluster = TestCluster::start(3).await;
    settled(&cluster).await;
    fifo_queue(cluster.node(1), TOPIC).await;
    let mut set = tokio::task::JoinSet::new();
    for round in 0..9 {
        let node = cluster.node(1 + round % 3);
        let client = node.client.clone();
        let url = node.client_url(&format!(
            "/topics/{TOPIC}/messages?groupId=g7&deduplicationId=same"
        ));
        set.spawn(async move {
            let response = client
                .post(url)
                .header("content-type", "application/json")
                .body(r#"{"a":1}"#)
                .send()
                .await
                .unwrap();
            let status = response.status().as_u16();
            let body: Value = response.json().await.unwrap();
            (status, body["id"].as_str().unwrap().to_owned())
        });
    }
    let mut results = Vec::new();
    while let Some(done) = set.join_next().await {
        results.push(done.unwrap());
    }
    let created = results.iter().filter(|(status, _)| *status == 201).count();
    assert_eq!(created, 1, "{results:?}");
    assert!(results.iter().all(|(_, id)| *id == results[0].1));
    cluster.shutdown().await;
}

/// UC: UC-FIF-028
#[tokio::test]
async fn a_node_that_is_not_the_primary_of_the_group_answers_421() {
    use taba_node::cluster::protocol::{FIFO_PLACEHOLDER_ID, MessageRecord, Request};
    use taba_node::cluster::transport::{CallError, Transport};

    let cluster = TestCluster::start(3).await;
    settled(&cluster).await;
    let topic_id = fifo_queue(cluster.node(1), TOPIC).await;
    let owners = owners_of_group(&cluster, &topic_id, "g1");
    let not_primary = owners[1];
    let record = MessageRecord {
        id: FIFO_PLACEHOLDER_ID.into(),
        topic: TOPIC.into(),
        body: br#"{"a":1}"#.to_vec(),
        content_type: "application/json".into(),
        group_id: Some("g1".into()),
        deduplication_id: None,
        delivery_count: 0,
        invisible_until_ms: None,
        receipt_token: None,
        dead_lettered_ms: None,
        dead_letter_reason: None,
        delete_at_ms: None,
    };
    let caller = cluster.network.transport(host_of(owners[0]));
    let reply = caller
        .call(
            &common::cluster::address_of(&cluster, not_primary),
            Request::FifoPublish {
                topic: TOPIC.into(),
                record,
            },
        )
        .await;
    match reply {
        Err(CallError::Remote(error)) => {
            assert_eq!(
                error.code,
                taba_node::core::error::ErrorCode::FifoMisdirected
            );
        }
        other => panic!("esperava `fifo.misdirected`: {other:?}"),
    }
    let _ = messages_of(cluster.node(1));
    cluster.shutdown().await;
}

/// A publish made straight to the node's engine, as the group's primary would decide it.
fn draft(
    node: &TestNode,
    group: &str,
    key: &str,
    body: &str,
) -> taba_node::engine::messages::Draft {
    let messages = messages_of(node);
    let view = messages.queue_view(TOPIC).expect("the queue exists");
    messages
        .draft_fifo_publish(
            &view,
            &taba_node::engine::messages::PublishRequest {
                body: body.as_bytes().to_vec(),
                content_type: Some("application/json".into()),
                group_id: Some(group.to_owned()),
                deduplication_id: Some(key.to_owned()),
            },
        )
        .expect("rascunho")
}

fn is_duplicate_of(draft: &taba_node::engine::messages::Draft, id: &str) -> bool {
    matches!(draft, taba_node::engine::messages::Draft::Duplicate { id: original } if original == id)
}

/// UC: UC-FIF-029, UC-FIF-038, UC-FIF-041, UC-SIN-022
#[tokio::test]
async fn the_ack_line_lives_in_every_node_and_a_new_primary_only_coordinates_once_ready() {
    let mut cluster = TestCluster::start(2).await;
    settled(&cluster).await;
    fifo_queue(cluster.node(1), TOPIC).await;
    let sent = publish(cluster.node(1), "g1", Some("k1"), r#"{"n":1}"#).await;
    let first_id = sent.json::<Value>().await.unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        publish(cluster.node(2), "g1", Some("k2"), r#"{"n":2}"#)
            .await
            .status(),
        201
    );
    // The first one's ack leaves the short row (`deleteAt`) on all the nodes, decided by quorum.
    let got = get_some(cluster.node(2), 10).await;
    assert_eq!(got[0]["id"], first_id);
    assert_eq!(ack(cluster.node(1), &got[0]).await.status(), 204);
    for number in [1, 2] {
        common::cluster::eventually("the short row on all the nodes", || {
            is_duplicate_of(
                &draft(cluster.node(number), "g1", "k1", r#"{"n":1}"#),
                &first_id,
            )
        })
        .await;
    }

    // A third node joins and stops before becoming ready; it is already in the table.
    let third = common::cluster::start_isolated(&mut cluster, &[]).await;
    let gate = common::cluster::pause_sync(&cluster, third, "replay");
    common::cluster::wait_phase(cluster.node(third), "replay").await;
    let topic_id = {
        let view: Value = cluster
            .node(1)
            .client_get(&format!("/topics/{TOPIC}"))
            .await
            .json()
            .await
            .unwrap();
        view["id"].as_str().unwrap().to_owned()
    };
    let service = cluster.node(1).node.cluster().unwrap();
    let new_id = cluster.node_id(third);
    let group = (0..400)
        .map(|n| format!("new-{n}"))
        .find(|group| {
            service
                .owners_of(fifo_position(&topic_id, group))
                .unwrap()
                .first()
                .is_some_and(|m| m.node_id == new_id)
        })
        .expect("some group has the new node as primary");

    // While the new primary is not ready, the group's publishes are `503`, and the old
    // primaries do not coordinate for it.
    for number in [1, 2] {
        let blocked = publish(cluster.node(number), &group, None, r#"{"a":1}"#).await;
        assert_eq!(code_of(blocked, 503).await, "fifo.primary-unavailable");
    }

    // With the node ready, it has the short row of the confirmed message, carried by the synchronization.
    gate.add_permits(1);
    common::cluster::wait_joined(cluster.node(third)).await;
    common::cluster::eventually("the short row on the new node", || {
        is_duplicate_of(
            &draft(cluster.node(third), "g1", "k1", r#"{"n":1}"#),
            &first_id,
        )
    })
    .await;
    let ok = publish(cluster.node(1), &group, None, r#"{"a":1}"#).await;
    assert_eq!(ok.status(), 201);
    cluster.shutdown().await;
}

/// UC: UC-FIF-025
#[tokio::test]
async fn the_dead_letter_of_a_fifo_stays_in_the_block_of_the_group() {
    let cluster = TestCluster::start(3).await;
    settled(&cluster).await;
    let created = cluster
        .node(1)
        .client
        .post(cluster.node(1).client_url("/topics"))
        .json(&json!({ "name": TOPIC, "type": "queue", "fifo": true, "maxDeliveries": 1 }))
        .send()
        .await
        .unwrap();
    assert_eq!(created.status(), 201);
    let topic_id = created.json::<Value>().await.unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let owners = owners_of_group(&cluster, &topic_id, "g1");
    let sent = publish(cluster.node(owners[2]), "g1", None, r#"{"x":1}"#).await;
    let id = sent.json::<Value>().await.unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let got = get_some(cluster.node(owners[2]), 10).await;
    assert_eq!(got[0]["id"], id);
    // The delivery spent the only `maxDeliveries`: when the invisibility expires, it goes to the dead-letter.
    cluster.clock.advance(Duration::from_secs(601));
    let dead = format!("{TOPIC}-dead-letter");
    for number in 1..=3 {
        let messages = messages_of(cluster.node(number));
        let (dead, id) = (dead.clone(), id.clone());
        common::cluster::eventually("the move to the dead-letter", move || {
            messages.message_info(&dead, &id).unwrap().is_some()
        })
        .await;
    }
    // A single operation in the block of the origin queue's group: the stored position is the group's.
    let wanted = fifo_position(&topic_id, "g1");
    for number in 1..=3 {
        let info = messages_of(cluster.node(number))
            .message_info(&dead, &id)
            .unwrap()
            .unwrap();
        assert_eq!(info.meta.position, wanted, "on node {number}");
    }
    // The dead-letter is a plain queue: it is not FIFO and delivers without an occupied group.
    let in_dead_letter: Value = cluster
        .node(1)
        .client_get(&format!("/topics/{dead}/messages?maxMessages=5"))
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(in_dead_letter["messages"][0]["id"], id);
    cluster.shutdown().await;
}

/// UC: UC-FIF-043
#[tokio::test]
async fn a_distributor_keeps_the_order_by_group_in_a_fifo_target() {
    let cluster = TestCluster::start(3).await;
    settled(&cluster).await;
    fifo_queue(cluster.node(1), TOPIC).await;
    let distributor = cluster
        .node(1)
        .client
        .post(cluster.node(1).client_url("/topics"))
        .json(&json!({ "name": "all", "type": "distributor", "prefixes": [TOPIC] }))
        .send()
        .await
        .unwrap();
    assert_eq!(distributor.status(), 201, "{:?}", distributor.text().await);
    let mut ids = Vec::new();
    for (round, node) in [1usize, 2, 3, 1].iter().enumerate() {
        let response = cluster
            .node(*node)
            .client
            .post(
                cluster
                    .node(*node)
                    .client_url("/topics/all/messages?groupId=g1"),
            )
            .header("content-type", "application/json")
            .body(format!(r#"{{"n":{round}}}"#))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let body: Value = response.json().await.unwrap();
        ids.push(body["deliveries"][0]["id"].as_str().unwrap().to_owned());
    }
    assert!(ids.windows(2).all(|w| w[0] < w[1]), "{ids:?}");
    for expected in &ids {
        let got = get_some(cluster.node(2), 10).await;
        assert_eq!(got.len(), 1);
        assert_eq!(got[0]["id"], *expected);
        assert_eq!(ack(cluster.node(3), &got[0]).await.status(), 204);
    }
    cluster.shutdown().await;
}

/// UC: UC-FIF-042, UC-CNS-037
#[tokio::test]
async fn auto_create_accepts_fifo_only_when_the_replication_factor_allows_it() {
    let cluster = TestCluster::start(3).await;
    settled(&cluster).await;
    let auto = cluster
        .node(2)
        .client_get("/topics/auto/messages?autoCreate=true&fifo=true")
        .await;
    assert_eq!(auto.status(), 200);
    for number in 1..=3 {
        let topic: Value = cluster
            .node(number)
            .client_get("/topics/auto")
            .await
            .json()
            .await
            .unwrap();
        assert_eq!(topic["fifo"], true, "on node {number}");
    }
    cluster.shutdown().await;

    let single = TestCluster::start_with(2, &[("TABA_REPLICATION_FACTOR", "1")]).await;
    settled(&single).await;
    let refused = single
        .node(1)
        .client_get("/topics/auto/messages?autoCreate=true&fifo=true")
        .await;
    assert_eq!(code_of(refused, 422).await, "topic.fifo-needs-replication");
    let created = single
        .node(1)
        .client
        .post(single.node(1).client_url("/topics"))
        .json(&json!({ "name": "queue", "type": "queue", "fifo": true }))
        .send()
        .await
        .unwrap();
    assert_eq!(code_of(created, 422).await, "topic.fifo-needs-replication");
    single.shutdown().await;
}
