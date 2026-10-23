//! What keeps the node from starting (doc 10; UC-CFG-004). The validation of the configuration itself
//! is tested next to the code; here is what only shows up when opening ports and disk.

mod common;

use std::net::TcpListener;

use common::TestNode;
use taba_node::config::{Config, MapEnv};
use taba_node::node::NodeError;

/// UC: UC-SEG-001, UC-CFG-004
#[tokio::test]
async fn an_occupied_api_port_stops_startup_naming_the_port() {
    let occupied = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = occupied.local_addr().unwrap().port().to_string();

    let result = TestNode::builder()
        .env("TABA_API_PORT", &port)
        .try_start()
        .await;

    let Err(error) = result else {
        panic!("the node should not start with the port taken")
    };
    assert!(matches!(error, NodeError::Bind { .. }), "{error}");
    assert!(error.to_string().contains(&port), "{error}");
}

/// UC: UC-SEG-001, UC-CFG-004
#[tokio::test]
async fn an_occupied_cluster_port_stops_startup_in_production() {
    let occupied = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = occupied.local_addr().unwrap().port().to_string();

    let result = TestNode::builder()
        .production()
        .env("TABA_CLUSTER_PORT", &port)
        .try_start()
        .await;

    let Err(error) = result else {
        panic!("the node should not start with the cluster port taken")
    };
    assert!(error.to_string().contains("cluster"), "{error}");
}

/// UC: UC-ING-001
#[test]
fn a_cluster_without_the_entry_address_does_not_start() {
    // The configuration is refused before opening any port; `taba` exits with code 2.
    let env = MapEnv::new([("TABA_API_PORT", "0")]);
    let error = Config::parse(None, &env).unwrap_err();
    assert!(
        error.to_string().contains("cluster.entry-address"),
        "{error}"
    );
}

/// UC: UC-CFG-004
#[tokio::test]
async fn a_storage_dir_that_cannot_be_created_stops_startup() {
    let tmp = tempfile::tempdir().unwrap();
    let blocker = tmp.path().join("file");
    std::fs::write(&blocker, "I am not a folder").unwrap();
    let storage = blocker.join("taba");

    let result = TestNode::builder()
        .env("TABA_STORAGE_DIR", &storage.display().to_string())
        .try_start()
        .await;

    let Err(error) = result else {
        panic!("the node should not start without being able to create storage.dir")
    };
    assert!(matches!(error, NodeError::Storage { .. }), "{error}");
    assert!(error.to_string().contains("storage.dir"), "{error}");
}

#[tokio::test]
async fn startup_creates_storage_dir_when_missing() {
    let node = TestNode::single().await;
    assert!(node.storage_dir().is_dir());
    node.shutdown().await;
}
