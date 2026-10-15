//! The node that manages the volatile queues and `delivery.get.max-attempts` in the cluster (doc 15, sections 14.5 and
//! 14.6; doc 13, section 12.2; doc 20a, UC-VOL and UC-CNS; level **C**).

mod common;

use std::time::Duration;

use serde_json::{Value, json};

use common::TestNode;
use common::cluster::{NO_VOTE, TestCluster, address_of, eventually, host_of, settled};
use taba_node::cluster::protocol::{Request, Response};
use taba_node::cluster::transport::{CallError, Transport};
use taba_node::core::error::ErrorCode;
use taba_node::core::ring::topic_position;

/// The clock is manual: spreading out the first query (in clock time) would only delay the test.
const NO_JITTER: (&str, &str) = ("TABA_TOPICS_VOLATILE_TAKEOVER_JITTER", "0s");
const IDLE_RETRY: (&str, &str) = ("TABA_TOPICS_VOLATILE_IDLE_CHECK_RETRY_INTERVAL", "5s");

async fn volatile_queue(node: &TestNode, name: &str, idle_timeout: u64) -> String {
    let response = node
        .client
        .post(node.client_url("/topics"))
        .json(&json!({
            "name": name, "type": "queue", "volatile": true, "idleTimeout": idle_timeout
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 201, "{:?}", response.text().await);
    response.json::<Value>().await.unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

async fn exists(node: &TestNode, name: &str) -> bool {
    node.client_get(&format!("/topics/{name}")).await.status() == 200
}

async fn get(node: &TestNode, name: &str, query: &str) -> reqwest::Response {
    node.client_get(&format!("/topics/{name}/messages?{query}"))
        .await
}

async fn wait_gone(cluster: &TestCluster, numbers: &[usize], name: &str) {
    for _ in 0..400 {
        let mut gone = true;
        for number in numbers {
            gone &= !exists(cluster.node(*number), name).await;
        }
        if gone {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("queue `{name}` was not deleted");
}

/// Who manages the queue: the primary of the position of the hash of the topic's `id`.
fn manager_of(cluster: &TestCluster, topic_id: &str) -> usize {
    let owners = cluster
        .node(1)
        .node
        .cluster()
        .unwrap()
        .owners_of(topic_position(topic_id))
        .unwrap();
    (1..=3)
        .find(|n| cluster.node_id(*n) == owners[0].node_id)
        .unwrap()
}

/// A node that is not the manager.
fn other_than(manager: usize) -> usize {
    if manager == 1 { 2 } else { 1 }
}

async fn idle_of(
    cluster: &TestCluster,
    from: usize,
    to: usize,
    id: &str,
) -> Result<Value, CallError> {
    let caller = cluster.network.transport(host_of(from));
    match caller
        .call(
            &address_of(cluster, to),
            Request::Idle {
                topic_id: id.to_owned(),
            },
        )
        .await?
    {
        Response::Idle(idle) => Ok(serde_json::to_value(idle).unwrap()),
        other => panic!("unexpected response: {other:?}"),
    }
}

/// UC: UC-VOL-001, UC-VOL-006, UC-VOL-008, UC-VOL-009
#[tokio::test]
async fn the_manager_asks_every_node_and_deletes_only_when_none_had_a_get() {
    let cluster = TestCluster::start_with(3, &[NO_JITTER]).await;
    settled(&cluster).await;
    let id = volatile_queue(cluster.node(1), "efemera", 60).await;
    let manager = manager_of(&cluster, &id);
    let consumer = other_than(manager);

    // A `GET` on a node that is not the manager, 30 s after the creation, is what postpones the deletion.
    cluster.clock.advance(Duration::from_secs(30));
    assert_eq!(
        get(cluster.node(consumer), "efemera", "waitTime=0")
            .await
            .status(),
        200
    );

    // With the 60 s since the creation passed, the manager's query finds the `GET` from 40 s ago: the queue stays.
    cluster.clock.advance(Duration::from_secs(40));
    tokio::time::sleep(Duration::from_millis(1500)).await;
    for number in 1..=3 {
        assert!(
            exists(cluster.node(number), "efemera").await,
            "on node {number}"
        );
    }

    // With no `GET` for 60 s, the manager deletes the queue on all the nodes.
    cluster.clock.advance(Duration::from_secs(30));
    wait_gone(&cluster, &[1, 2, 3], "efemera").await;
    cluster.shutdown().await;
}

/// UC: UC-VOL-006, UC-VOL-023
#[tokio::test]
async fn many_queues_are_managed_by_different_nodes_and_all_expire() {
    let cluster = TestCluster::start_with(3, &[NO_JITTER]).await;
    settled(&cluster).await;
    let mut managers = std::collections::BTreeSet::new();
    let names: Vec<String> = (0..12).map(|n| format!("queue-{n}")).collect();
    for name in &names {
        let id = volatile_queue(cluster.node(1), name, 60).await;
        managers.insert(manager_of(&cluster, &id));
    }
    assert!(managers.len() >= 2, "{managers:?}");
    cluster.clock.advance(Duration::from_secs(61));
    for name in &names {
        wait_gone(&cluster, &[1, 2, 3], name).await;
    }
    cluster.shutdown().await;
}

/// UC: UC-VOL-010, UC-VOL-015
#[tokio::test]
async fn a_node_that_does_not_answer_or_a_manager_that_is_down_keeps_the_queue_alive() {
    let mut env = NO_VOTE.to_vec();
    env.extend([IDLE_RETRY, NO_JITTER]);
    let cluster = TestCluster::start_with(3, &env).await;
    settled(&cluster).await;
    let id = volatile_queue(cluster.node(1), "efemera", 60).await;
    let manager = manager_of(&cluster, &id);
    let silent = other_than(manager);

    // A node does not answer (and is still in the table): when in doubt, the queue stays alive.
    cluster.network.isolate(&host_of(silent));
    cluster.clock.advance(Duration::from_secs(61));
    tokio::time::sleep(Duration::from_millis(1500)).await;
    for number in (1..=3).filter(|n| *n != silent) {
        assert!(exists(cluster.node(number), "efemera").await);
    }
    // The queue keeps accepting publishes: the one that falls in a block whose primary is the mute node waits
    // for it (`503`, PRB-044), the others close.
    let published = cluster
        .node(manager)
        .client
        .post(cluster.node(manager).client_url("/topics/efemera/messages"))
        .json(&json!({"a": 1}))
        .send()
        .await
        .unwrap();
    assert!([201, 503].contains(&published.status().as_u16()));

    // The node came back: on the new attempt (after the interval) the queue is deleted.
    cluster.network.restore(&host_of(silent));
    cluster.clock.advance(Duration::from_secs(6));
    wait_gone(&cluster, &[1, 2, 3], "efemera").await;

    // The manager itself down and still in the table: none of its queues is deleted.
    let id = volatile_queue(cluster.node(1), "other", 60).await;
    let manager = manager_of(&cluster, &id);
    let alive = other_than(manager);
    cluster.network.isolate(&host_of(manager));
    cluster.clock.advance(Duration::from_secs(120));
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert!(exists(cluster.node(alive), "other").await);
    cluster.shutdown().await;
}

/// UC: UC-VOL-017, UC-FAL-018
#[tokio::test]
async fn when_the_manager_leaves_the_new_primary_of_its_positions_takes_over() {
    let cluster = TestCluster::start_with(3, &[NO_JITTER]).await;
    settled(&cluster).await;
    let id = volatile_queue(cluster.node(1), "efemera", 60).await;
    let manager = manager_of(&cluster, &id);
    let survivors: Vec<usize> = (1..=3).filter(|n| *n != manager).collect();

    // The manager falls and is excluded from the cluster; the new primary of the position takes over the queue, without inherited
    // state, and the query it makes already shows the real deadline.
    let old = cluster.node_id(manager);
    cluster.network.isolate(&host_of(manager));
    eventually("the manager to be excluded", || {
        survivors
            .iter()
            .all(|n| !cluster.member_ids(*n).contains(&old))
    })
    .await;
    cluster.clock.advance(Duration::from_secs(61));
    wait_gone(&cluster, &survivors, "efemera").await;
    cluster.shutdown().await;
}

/// UC: UC-CLR-007, UC-VOL-011
#[tokio::test]
async fn the_idle_route_answers_a_duration_and_says_when_it_counts_from_the_start() {
    let cluster = TestCluster::start(3).await;
    settled(&cluster).await;
    let id = volatile_queue(cluster.node(1), "efemera", 3000).await;

    // Without a `GET` on this node: the duration is the time since it started.
    cluster.clock.advance(Duration::from_secs(7));
    let idle = idle_of(&cluster, 1, 2, &id).await.unwrap();
    assert_eq!(idle["topicId"], id);
    assert_eq!(idle["sinceStart"], true);
    assert!(idle["idleSeconds"].as_u64().unwrap() >= 7);

    // With a `GET`, the duration is the one from before and `sinceStart` is false.
    assert_eq!(
        get(cluster.node(2), "efemera", "waitTime=0").await.status(),
        200
    );
    cluster.clock.advance(Duration::from_secs(5));
    let idle = idle_of(&cluster, 1, 2, &id).await.unwrap();
    assert_eq!(idle["sinceStart"], false);
    assert_eq!(idle["idleSeconds"], 5);

    // Unknown topic: `404`.
    match idle_of(&cluster, 1, 2, "00000000-0000-7000-8000-000000000000").await {
        Err(CallError::Remote(error)) => assert_eq!(error.code, ErrorCode::TopicNotFound),
        other => panic!("esperava `topic.not-found`: {other:?}"),
    }
    cluster.shutdown().await;
}

async fn publish_many(node: &TestNode, topic: &str, count: usize) {
    for n in 0..count {
        let response = node
            .client
            .post(node.client_url(&format!("/topics/{topic}/messages")))
            .json(&json!({ "n": n }))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 201);
    }
}

fn messages_of(node: &TestNode) -> std::sync::Arc<taba_node::engine::messages::Messages> {
    node.node
        .cluster()
        .and_then(|c| c.replication())
        .map(|r| r.messages.messages().clone())
        .expect("the node is in the cluster")
}

fn delivered(body: &Value) -> usize {
    body["messages"].as_array().unwrap().len()
}

/// UC: UC-CNS-020, UC-CNS-022
#[tokio::test]
async fn a_get_tries_the_next_ids_after_a_failed_registration_up_to_the_attempt_limit() {
    let cluster = TestCluster::start(3).await;
    common::cluster::create_queue(cluster.node(1), "orders").await;
    publish_many(cluster.node(1), "orders", 30).await;

    // An invisibility register that does not close does not bring the `GET` down: it goes on to the next
    // ID and delivers a message from another.
    for number in 2..=3 {
        cluster
            .network
            .lose_kind(&host_of(number), "visibility-register", 1);
    }
    let response = get(
        cluster.node(1),
        "orders",
        "maxMessages=1&visibilityTimeout=60",
    )
    .await;
    assert_eq!(response.status(), 200);
    let body: Value = response.json().await.unwrap();
    assert_eq!(delivered(&body), 1);
    for number in 2..=3 {
        cluster
            .network
            .lose_kind(&host_of(number), "visibility-register", 0);
    }
    // The quorum closes without the replica that lost the register: it keeps registering and committing in the
    // background, every 200 ms (`write.retry.backoff`), until it succeeds. On a slow node that
    // replica is still in the middle when the next phase starts and, losing all the calls, would end up
    // adding attempts to the count. Waits for all to finish before going on.
    let id = body["messages"][0]["id"].as_str().unwrap().to_owned();
    let token = body["messages"][0]["receiptToken"]
        .as_str()
        .unwrap()
        .to_owned();
    for number in 1..=3 {
        let messages = messages_of(cluster.node(number));
        let (id, token) = (id.clone(), token.clone());
        eventually("the replica to register the delivery", move || {
            messages
                .message_info("orders", &id)
                .unwrap()
                .is_some_and(|info| info.meta.receipt_token.as_deref() == Some(token.as_str()))
        })
        .await;
    }

    // With no register closing, the `GET` stops after `delivery.get.max-attempts` failures
    // (3 by default, on each node that delivers) and answers `503`, instead of going through the whole queue.
    for number in 1..=3 {
        cluster
            .network
            .lose_kind(&host_of(number), "visibility-register", 1_000_000);
    }
    cluster.network.clear_calls();
    let response = get(
        cluster.node(1),
        "orders",
        "maxMessages=1&visibilityTimeout=60",
    )
    .await;
    assert_eq!(response.status(), 503);
    let attempts = cluster.network.count("visibility-register");
    // 3 failures × 2 peers × 3 attempts (`write.retry.attempts`) × 3 nodes that deliver.
    assert!(attempts <= 3 * 2 * 3 * 3, "{attempts} register calls");
    cluster.shutdown().await;
}

type Managed = Vec<std::collections::BTreeSet<String>>;

/// Who manages each queue now, node by node (the `id`s each derives from its own table).
fn managed_sets(cluster: &TestCluster, count: usize) -> Managed {
    (1..=count)
        .map(|n| {
            cluster
                .node(n)
                .node
                .volatile_managed()
                .into_iter()
                .collect()
        })
        .collect()
}

/// The node (from 1 to `count`) that is the primary of the position of the `id` in node 1's table.
fn primary_of(cluster: &TestCluster, count: usize, id: &str) -> usize {
    let owners = cluster
        .node(1)
        .node
        .cluster()
        .unwrap()
        .owners_of(topic_position(id))
        .unwrap();
    (1..=count)
        .find(|n| cluster.node_id(*n) == owners[0].node_id)
        .unwrap()
}

async fn many_volatile_queues(cluster: &TestCluster, count: usize) -> Vec<String> {
    let mut ids = Vec::new();
    for n in 0..count {
        ids.push(volatile_queue(cluster.node(1), &format!("queue-{n:02}"), 3600).await);
    }
    ids
}

/// UC: UC-VOL-007, UC-VOL-016, UC-ING-023
#[tokio::test]
async fn the_managed_set_is_derived_from_the_table_and_a_new_node_manages_only_once_ready() {
    let mut cluster = TestCluster::start(3).await;
    settled(&cluster).await;
    let ids = many_volatile_queues(&cluster, 24).await;

    // Each queue has a single manager, the primary of its position, and each node derives it from its own table.
    eventually("each queue to have the manager the ring dictates", || {
        let sets = managed_sets(&cluster, 3);
        ids.iter().all(|id| {
            let holders: Vec<usize> = (1..=3).filter(|n| sets[*n - 1].contains(id)).collect();
            holders == [primary_of(&cluster, 3, id)]
        })
    })
    .await;

    // Creating and deleting a topic changes the set on its own.
    let extra = volatile_queue(cluster.node(2), "queue-extra", 3600).await;
    let holder = primary_of(&cluster, 3, &extra);
    eventually("the new queue to enter the manager's set", || {
        managed_sets(&cluster, 3)[holder - 1].contains(&extra)
    })
    .await;
    let deleted = cluster
        .node(3)
        .client
        .delete(cluster.node(3).client_url("/topics/queue-extra"))
        .send()
        .await
        .unwrap();
    assert_eq!(deleted.status(), 204);
    eventually("the deleted queue to leave the set", || {
        managed_sets(&cluster, 3)
            .iter()
            .all(|set| !set.contains(&extra))
    })
    .await;

    // Node 4 joins and syncs: it is in the table and on the ring, but does not manage anything yet, and the queues
    // of its stretch belong to no one until it is ready (none is deleted in that interval).
    let fourth = common::cluster::start_isolated(&mut cluster, &[]).await;
    let gate = common::cluster::pause_sync(&cluster, fourth, "replay");
    common::cluster::wait_phase(cluster.node(fourth), "replay").await;
    eventually("the three nodes to have node 4 in the table", || {
        (1..=3).all(|n| cluster.member_ids(n).len() == 4)
    })
    .await;
    let moved: Vec<&String> = ids
        .iter()
        .filter(|id| primary_of(&cluster, 4, id) == 4)
        .collect();
    assert!(!moved.is_empty(), "the ring gave queues to the new node");
    eventually(
        "the queues of node 4's stretch to be left without a manager",
        || {
            let sets = managed_sets(&cluster, 4);
            sets[3].is_empty()
                && moved
                    .iter()
                    .all(|id| (0..4).all(|n| !sets[n].contains(*id)))
                && ids
                    .iter()
                    .filter(|id| !moved.contains(id))
                    .all(|id| (0..3).any(|n| sets[n].contains(id)))
        },
    )
    .await;

    // With node 4 ready, it is also the manager of the queues of its stretch, and the others let go of them.
    gate.add_permits(1);
    common::cluster::wait_joined(cluster.node(fourth)).await;
    eventually("node 4 to manage the queues of its stretch", || {
        let sets = managed_sets(&cluster, 4);
        ids.iter().all(|id| {
            let holders: Vec<usize> = (1..=4).filter(|n| sets[*n - 1].contains(id)).collect();
            holders == [primary_of(&cluster, 4, id)]
        }) && moved.iter().all(|id| sets[3].contains(*id))
    })
    .await;
    cluster.shutdown().await;
}

/// UC: UC-VOL-018
#[tokio::test]
async fn the_managed_set_of_every_node_goes_back_when_the_excluded_node_returns() {
    let cluster = TestCluster::start(3).await;
    settled(&cluster).await;
    let ids = many_volatile_queues(&cluster, 24).await;
    eventually("each node's set to stabilize", || {
        managed_sets(&cluster, 3)
            .iter()
            .map(|s| s.len())
            .sum::<usize>()
            == ids.len()
    })
    .await;
    let before = managed_sets(&cluster, 3);

    // Node 3 is excluded: the other two split its queues.
    cluster.network.isolate(&host_of(3));
    eventually("the two nodes to be left with all the queues", || {
        let sets = managed_sets(&cluster, 2);
        cluster.member_ids(1).len() == 2
            && cluster.member_ids(2).len() == 2
            && ids
                .iter()
                .all(|id| sets.iter().filter(|s| s.contains(id)).count() == 1)
    })
    .await;

    // After the freezing, the node comes back with the same ID, the ring is the one from before and the set too.
    cluster.network.restore(&host_of(3));
    cluster.clock.advance(Duration::from_secs(301));
    common::cluster::wait_joined(cluster.node(3)).await;
    eventually("each node's set to go back to the previous one", || {
        managed_sets(&cluster, 3) == before
    })
    .await;
    cluster.shutdown().await;
}
