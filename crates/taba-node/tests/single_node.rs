//! The one-node profile (doc 37, section 4.16; level **C**): `replication.factor=1`, no cluster TLS and no entry
//! address. The node is a cluster of one, starts alone and serves like any other.

mod common;

use serde_json::{Value, json};

use common::cluster::{eventually, wait_joined};
use common::{TestNode, error_code};
use taba_node::cluster::health::Condition;

const DEV_SECRET: &str = "dev-secret-with-at-least-32-characters";

fn profile() -> common::TestNodeBuilder {
    TestNode::builder().single_node()
}

/// UC: UC-RTN-043
#[tokio::test]
async fn a_node_with_factor_one_starts_alone_and_serves_the_plain_and_the_fifo_queue() {
    let node = profile().start().await;
    wait_joined(&node).await;
    let state = node.node.shared().clone();
    eventually("the node to be active", move || {
        state.state() == taba_node::cluster::state::NodeState::Active
    })
    .await;

    for body in [
        json!({ "name": "plain", "type": "queue" }),
        json!({ "name": "ordered", "type": "queue", "fifo": true }),
    ] {
        let created = node
            .client
            .post(node.client_url("/topics"))
            .json(&body)
            .send()
            .await
            .unwrap();
        assert_eq!(created.status(), 201, "{body}");
    }
    let published = node
        .client
        .post(node.client_url("/topics/plain/messages"))
        .header("content-type", "application/json")
        .body("{\"n\":1}")
        .send()
        .await
        .unwrap();
    assert_eq!(published.status(), 201);
    let fifo = node
        .client
        .post(node.client_url("/topics/ordered/messages?groupId=g"))
        .header("content-type", "application/json")
        .body("{\"n\":1}")
        .send()
        .await
        .unwrap();
    assert_eq!(fifo.status(), 201, "{:?}", fifo.text().await);

    let health: Value = node.stats("/health/cluster").await;
    let conditions: Vec<&str> = health["conditions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["condition"].as_str().unwrap())
        .collect();
    assert!(
        conditions.contains(&Condition::ReducedProfile.as_str()),
        "{conditions:?}"
    );
    assert!(!conditions.contains(&"quorum-lost"), "{conditions:?}");
    node.shutdown().await;
}

/// UC: UC-RTN-043
#[tokio::test]
async fn the_reduced_profile_condition_can_be_hidden() {
    let node = profile()
        .env("TABA_HEALTH_SINGLE_NODE_CONDITION", "false")
        .start()
        .await;
    wait_joined(&node).await;
    let health: Value = node.stats("/health/cluster").await;
    let text = health.to_string();
    assert!(!text.contains("reduced-profile"), "{text}");
    node.shutdown().await;
}

/// UC: UC-RTN-044
#[tokio::test]
async fn the_dev_token_is_an_admin_work_token_created_at_the_start_and_turns_authentication_on() {
    let node = profile()
        .env("TABA_AUTH_DEV_TOKEN", DEV_SECRET)
        .start()
        .await;
    wait_joined(&node).await;

    let without = node.client_get("/topics").await;
    assert_eq!(error_code(without, 401).await, "auth.unauthorized");

    let bearer = format!("Bearer tb_dev.{DEV_SECRET}");
    let list = || async {
        node.client
            .get(node.client_url("/topics"))
            .header("authorization", bearer.clone())
            .send()
            .await
            .unwrap()
            .status()
    };
    eventually_status(&list, 200).await;
    let wrong = node
        .client
        .get(node.client_url("/topics"))
        .header("authorization", format!("Bearer tb_dev.{DEV_SECRET}x"))
        .send()
        .await
        .unwrap();
    assert_eq!(wrong.status(), 401);

    // `admin` reaches the tokens routes, and the secret never shows in them.
    let tokens = node
        .client
        .get(node.client_url("/tokens"))
        .header("authorization", bearer.clone())
        .send()
        .await
        .unwrap();
    assert_eq!(tokens.status(), 200);
    let text = tokens.text().await.unwrap();
    assert!(text.contains("dev-token"), "{text}");
    assert!(!text.contains(DEV_SECRET), "{text}");
    node.shutdown().await;
}

async fn eventually_status<F, Fut>(call: &F, expected: u16)
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = reqwest::StatusCode>,
{
    for _ in 0..200 {
        if call().await.as_u16() == expected {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    panic!("the call never answered {expected}");
}
