use std::sync::Mutex;
use std::time::{Duration, Instant};

use super::*;
use crate::cluster::protocol::{Ack, OperationRef};
use crate::core::error::{ErrorCode, TabaError};

/// Atende tudo com um `Ack` e guarda quem chamou.
#[derive(Default)]
struct Echo {
    callers: Mutex<Vec<String>>,
}

impl Handler for Echo {
    fn handle<'a>(
        &'a self,
        caller: &'a str,
        request: Request,
    ) -> BoxFuture<'a, Result<Response, TabaError>> {
        Box::pin(async move {
            self.callers.lock().unwrap().push(caller.to_owned());
            match request {
                Request::Rollback(_) => Err(TabaError::new(ErrorCode::ClusterConflict, "não")),
                _ => Ok(Response::Ack(Ack {
                    operation_id: "x".into(),
                    accepted: true,
                })),
            }
        })
    }
}

fn op() -> Request {
    Request::Commit(OperationRef {
        operation_id: "x".into(),
    })
}

fn addr(host: &str) -> PeerAddr {
    PeerAddr::new(host, 8081)
}

fn network_with(hosts: &[&str]) -> (MemoryNetwork, Vec<Arc<Echo>>) {
    let network = MemoryNetwork::new();
    let handlers: Vec<Arc<Echo>> = hosts.iter().map(|_| Arc::new(Echo::default())).collect();
    for (host, handler) in hosts.iter().zip(&handlers) {
        network.register(addr(host), handler.clone());
    }
    (network, handlers)
}

#[tokio::test]
async fn a_call_reaches_the_handler_with_the_caller_address() {
    let (network, handlers) = network_with(&["10.0.0.1", "10.0.0.2"]);
    let from = network.transport("10.0.0.1");
    let response = from.call(&addr("10.0.0.2"), op()).await.unwrap();
    assert!(matches!(
        response,
        Response::Ack(Ack { accepted: true, .. })
    ));
    assert_eq!(*handlers[1].callers.lock().unwrap(), ["10.0.0.1"]);
    assert_eq!(
        network.calls(),
        [Call {
            from: "10.0.0.1".into(),
            to: addr("10.0.0.2"),
            request: "commit"
        }]
    );
}

#[tokio::test]
async fn an_error_from_the_handler_is_a_remote_error() {
    let (network, _) = network_with(&["10.0.0.1", "10.0.0.2"]);
    let error = network
        .transport("10.0.0.1")
        .call(
            &addr("10.0.0.2"),
            Request::Rollback(OperationRef {
                operation_id: "x".into(),
            }),
        )
        .await
        .unwrap_err();
    assert!(matches!(&error, CallError::Remote(e) if e.code == ErrorCode::ClusterConflict));
    assert!(!error.is_network());
}

#[tokio::test]
async fn a_node_that_is_down_refuses_the_connection() {
    let (network, _) = network_with(&["10.0.0.1", "10.0.0.2"]);
    let from = network.transport("10.0.0.1");
    network.unregister(&addr("10.0.0.2"));
    let error = from.call(&addr("10.0.0.2"), op()).await.unwrap_err();
    assert!(error.is_network(), "{error}");
}

#[tokio::test]
async fn a_partition_cuts_both_directions_and_heals() {
    let (network, _) = network_with(&["10.0.0.1", "10.0.0.2", "10.0.0.3"]);
    let (one, two, three) = (
        network.transport("10.0.0.1"),
        network.transport("10.0.0.2"),
        network.transport("10.0.0.3"),
    );
    network.partition("10.0.0.1", "10.0.0.2");
    assert!(
        one.call(&addr("10.0.0.2"), op())
            .await
            .unwrap_err()
            .is_network()
    );
    assert!(
        two.call(&addr("10.0.0.1"), op())
            .await
            .unwrap_err()
            .is_network()
    );
    one.call(&addr("10.0.0.3"), op())
        .await
        .expect("a terceira alcança os dois");
    three.call(&addr("10.0.0.2"), op()).await.unwrap();

    network.heal("10.0.0.1", "10.0.0.2");
    one.call(&addr("10.0.0.2"), op()).await.unwrap();
}

#[tokio::test]
async fn an_isolated_node_still_talks_to_itself_only() {
    let (network, _) = network_with(&["10.0.0.1", "10.0.0.2"]);
    let (one, two) = (network.transport("10.0.0.1"), network.transport("10.0.0.2"));
    network.isolate("10.0.0.2");
    assert!(
        one.call(&addr("10.0.0.2"), op())
            .await
            .unwrap_err()
            .is_network()
    );
    assert!(
        two.call(&addr("10.0.0.1"), op())
            .await
            .unwrap_err()
            .is_network()
    );
    two.call(&addr("10.0.0.2"), op()).await.unwrap();
    network.restore("10.0.0.2");
    one.call(&addr("10.0.0.2"), op()).await.unwrap();
}

#[tokio::test]
async fn the_next_calls_to_a_node_can_be_lost() {
    let (network, _) = network_with(&["10.0.0.1", "10.0.0.2"]);
    let one = network.transport("10.0.0.1");
    network.lose_next("10.0.0.2", 2);
    assert!(one.call(&addr("10.0.0.2"), op()).await.is_err());
    assert!(one.call(&addr("10.0.0.2"), op()).await.is_err());
    one.call(&addr("10.0.0.2"), op())
        .await
        .expect("a terceira passa");
}

#[tokio::test]
async fn a_silent_node_never_answers() {
    let (network, _) = network_with(&["10.0.0.1", "10.0.0.2"]);
    let one = network.transport("10.0.0.1");
    network.set_silent("10.0.0.2", true);
    let waited =
        tokio::time::timeout(Duration::from_millis(50), one.call(&addr("10.0.0.2"), op())).await;
    assert!(waited.is_err(), "só o prazo de quem chama interrompe");
    network.set_silent("10.0.0.2", false);
    one.call(&addr("10.0.0.2"), op()).await.unwrap();
}

#[tokio::test]
async fn the_delay_is_added_to_every_call() {
    let (network, _) = network_with(&["10.0.0.1", "10.0.0.2"]);
    let one = network.transport("10.0.0.1");
    network.set_delay(Duration::from_millis(40));
    let started = Instant::now();
    one.call(&addr("10.0.0.2"), op()).await.unwrap();
    assert!(started.elapsed() >= Duration::from_millis(40));
}

#[tokio::test]
async fn the_delay_of_a_kind_slows_only_that_kind_of_call() {
    let (network, _) = network_with(&["10.0.0.1", "10.0.0.2"]);
    let one = network.transport("10.0.0.1");
    network.set_delay_kind("commit", Duration::from_millis(60));
    let started = Instant::now();
    one.call(&addr("10.0.0.2"), op()).await.unwrap();
    assert!(started.elapsed() >= Duration::from_millis(60));

    let started = Instant::now();
    one.call(&addr("10.0.0.2"), Request::ListNodes)
        .await
        .unwrap();
    assert!(started.elapsed() < Duration::from_millis(60));
}

#[tokio::test]
async fn a_balancer_spreads_the_calls_over_the_live_nodes() {
    let (network, handlers) = network_with(&["10.0.0.1", "10.0.0.2", "10.0.0.3"]);
    let balancer = PeerAddr::new("lb", 8081);
    network.add_balancer(
        balancer.clone(),
        vec![addr("10.0.0.1"), addr("10.0.0.2"), addr("10.0.0.3")],
    );
    let client = network.transport("10.0.0.9");
    for _ in 0..6 {
        client.call(&balancer, op()).await.unwrap();
    }
    for handler in &handlers {
        assert_eq!(handler.callers.lock().unwrap().len(), 2, "rodízio");
    }

    network.unregister(&addr("10.0.0.1"));
    network.isolate("10.0.0.2");
    for _ in 0..3 {
        client.call(&balancer, op()).await.unwrap();
    }
    assert_eq!(
        handlers[2].callers.lock().unwrap().len(),
        5,
        "só o nó 3 sobra"
    );

    network.unregister(&addr("10.0.0.3"));
    assert!(client.call(&balancer, op()).await.unwrap_err().is_network());
}

#[tokio::test]
async fn the_call_log_can_be_counted_and_cleared() {
    let (network, _) = network_with(&["10.0.0.1", "10.0.0.2"]);
    let one = network.transport("10.0.0.1");
    one.call(&addr("10.0.0.2"), Request::ListNodes)
        .await
        .unwrap();
    one.call(&addr("10.0.0.2"), op()).await.unwrap();
    assert_eq!(network.count("list-nodes"), 1);
    assert_eq!(network.count("commit"), 1);
    network.clear_calls();
    assert!(network.calls().is_empty());
}

#[tokio::test]
async fn only_the_chosen_kind_of_call_is_lost() {
    let (network, _) = network_with(&["10.0.0.1", "10.0.0.2"]);
    let one = network.transport("10.0.0.1");
    network.lose_kind("10.0.0.2", "commit", 2);
    assert!(one.call(&addr("10.0.0.2"), op()).await.is_err());
    one.call(&addr("10.0.0.2"), Request::ListNodes)
        .await
        .expect("outro tipo passa");
    assert!(one.call(&addr("10.0.0.2"), op()).await.is_err());
    one.call(&addr("10.0.0.2"), op())
        .await
        .expect("só duas foram perdidas");
}
