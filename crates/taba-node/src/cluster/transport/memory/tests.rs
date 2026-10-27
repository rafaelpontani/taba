use std::sync::Mutex;
use std::time::{Duration, Instant};

use super::*;
use crate::cluster::protocol::{Ack, OperationRef};
use crate::core::error::{ErrorCode, TabaError};

/// Answers everything with an `Ack` and records who called.
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
                Request::Rollback(_) => Err(TabaError::new(ErrorCode::ClusterConflict, "no")),
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
        .expect("the third reaches both");
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
        .expect("the third gets through");
}

#[tokio::test]
async fn a_silent_node_never_answers() {
    let (network, _) = network_with(&["10.0.0.1", "10.0.0.2"]);
    let one = network.transport("10.0.0.1");
    network.set_silent("10.0.0.2", true);
    let waited =
        tokio::time::timeout(Duration::from_millis(50), one.call(&addr("10.0.0.2"), op())).await;
    assert!(waited.is_err(), "only the caller's deadline interrupts");
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
        assert_eq!(handler.callers.lock().unwrap().len(), 2, "round-robin");
    }

    network.unregister(&addr("10.0.0.1"));
    network.isolate("10.0.0.2");
    for _ in 0..3 {
        client.call(&balancer, op()).await.unwrap();
    }
    assert_eq!(
        handlers[2].callers.lock().unwrap().len(),
        5,
        "only node 3 is left"
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
        .expect("another type gets through");
    assert!(one.call(&addr("10.0.0.2"), op()).await.is_err());
    one.call(&addr("10.0.0.2"), op())
        .await
        .expect("only two were lost");
}

/// UC: UC-CHS-003
#[tokio::test]
async fn a_one_way_cut_stops_one_direction_only_and_heals() {
    let (network, _) = network_with(&["10.0.0.1", "10.0.0.2"]);
    let (one, two) = (network.transport("10.0.0.1"), network.transport("10.0.0.2"));
    network.cut_one_way("10.0.0.1", "10.0.0.2");
    assert!(
        one.call(&addr("10.0.0.2"), op())
            .await
            .unwrap_err()
            .is_network()
    );
    two.call(&addr("10.0.0.1"), op())
        .await
        .expect("the other direction is open");

    network.heal_one_way("10.0.0.1", "10.0.0.2");
    one.call(&addr("10.0.0.2"), op()).await.unwrap();
}

/// UC: UC-CHS-003
#[tokio::test]
async fn a_lost_reply_runs_the_call_and_fails_the_caller() {
    let (network, handlers) = network_with(&["10.0.0.1", "10.0.0.2"]);
    let one = network.transport("10.0.0.1");
    network.lose_replies("10.0.0.1", "10.0.0.2", true);
    let error = one.call(&addr("10.0.0.2"), op()).await.unwrap_err();
    assert!(error.is_network(), "{error}");
    assert_eq!(
        handlers[1].callers.lock().unwrap().len(),
        1,
        "the handler did run"
    );

    network.lose_replies("10.0.0.1", "10.0.0.2", false);
    one.call(&addr("10.0.0.2"), op()).await.unwrap();
}

/// UC: UC-CHS-003
#[tokio::test]
async fn the_loss_of_a_link_repeats_from_the_seed() {
    async fn outcome(seed: u64) -> Vec<bool> {
        let (network, _) = network_with(&["10.0.0.1", "10.0.0.2"]);
        network.seed_links(seed);
        network.set_link_loss("10.0.0.1", "10.0.0.2", 50);
        let one = network.transport("10.0.0.1");
        let mut result = Vec::new();
        for _ in 0..40 {
            result.push(one.call(&addr("10.0.0.2"), op()).await.is_ok());
        }
        result
    }
    let first = outcome(7).await;
    assert_eq!(
        first,
        outcome(7).await,
        "the same seed loses the same calls"
    );
    assert!(first.contains(&true) && first.contains(&false), "{first:?}");
    assert_ne!(first, outcome(8).await, "another seed loses others");
}

/// UC: UC-CHS-003
#[tokio::test]
async fn heal_all_removes_every_programmed_failure() {
    let (network, _) = network_with(&["10.0.0.1", "10.0.0.2", "10.0.0.3"]);
    let one = network.transport("10.0.0.1");
    network.partition("10.0.0.1", "10.0.0.2");
    network.cut_one_way("10.0.0.1", "10.0.0.3");
    network.isolate("10.0.0.3");
    network.set_link_loss("10.0.0.1", "10.0.0.2", 100);
    network.heal_all();
    one.call(&addr("10.0.0.2"), op()).await.unwrap();
    one.call(&addr("10.0.0.3"), op()).await.unwrap();
}
