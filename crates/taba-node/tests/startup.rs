//! O que impede o nó de subir (doc 10; UC-CFG-004). A validação da configuração em si
//! é testada junto do código; aqui está o que só aparece ao abrir portas e disco.

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
        panic!("o nó não deveria subir com a porta ocupada")
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
        panic!("o nó não deveria subir com a porta do cluster ocupada")
    };
    assert!(error.to_string().contains("cluster"), "{error}");
}

/// UC: UC-SEG-005
#[tokio::test]
async fn in_development_the_cluster_port_is_ignored_even_when_occupied() {
    let occupied = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = occupied.local_addr().unwrap().port().to_string();

    let node = TestNode::builder()
        .env("TABA_CLUSTER_PORT", &port)
        .start()
        .await;
    assert_eq!(node.node.cluster_addr(), None);
    node.shutdown().await;
}

/// UC: UC-ING-001
#[test]
fn production_without_the_entry_address_does_not_start() {
    // A configuração é recusada antes de abrir qualquer porta; o `taba` sai com código 2.
    let env = MapEnv::new([("TABA_MODE", "production"), ("TABA_API_PORT", "0")]);
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
    let blocker = tmp.path().join("arquivo");
    std::fs::write(&blocker, "não sou uma pasta").unwrap();
    let storage = blocker.join("taba");

    let result = TestNode::builder()
        .env("TABA_STORAGE_DIR", &storage.display().to_string())
        .try_start()
        .await;

    let Err(error) = result else {
        panic!("o nó não deveria subir sem poder criar storage.dir")
    };
    assert!(matches!(error, NodeError::Storage { .. }), "{error}");
    assert!(error.to_string().contains("storage.dir"), "{error}");
}

#[tokio::test]
async fn startup_creates_storage_dir_when_missing() {
    let node = TestNode::development().await;
    assert!(node.storage_dir().is_dir());
    node.shutdown().await;
}
