use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Barrier, Mutex};
use std::time::Duration;

use taba_client::testing::MockTransport;
use taba_client::Config;

use super::*;

struct Scripted {
    seen: Mutex<Vec<String>>,
}

impl Backend for Scripted {
    fn call(&self, call: &Call) -> Result<Reply, Error> {
        self.seen
            .lock()
            .unwrap()
            .push(format!("{} {}", call.method, call.path));
        Ok(Reply {
            status: 200,
            headers: Vec::new(),
            body: b"{}".to_vec(),
            request_id: None,
            recovered: false,
        })
    }
}

fn wait(rx: &mpsc::Receiver<Msg>) -> Msg {
    rx.recv_timeout(Duration::from_secs(5)).expect("a reply")
}

/// TUI: TUI-FRM-08
#[test]
fn a_reply_comes_back_marked_with_its_request_generation_and_call() {
    let backend = Arc::new(Scripted {
        seen: Mutex::new(Vec::new()),
    });
    let (tx, rx) = mpsc::channel();
    let workers = Workers::start(backend.clone(), tx, 2);
    workers.submit(41, 7, ApiCall::DrainStart("n1".into()));
    let Msg::Reply {
        id,
        generation,
        call,
        result,
    } = wait(&rx)
    else {
        panic!("not a reply");
    };
    assert_eq!((id, generation), (41, 7));
    assert_eq!(call, ApiCall::DrainStart("n1".into()));
    assert!(result.is_ok());
    assert_eq!(
        *backend.seen.lock().unwrap(),
        vec!["POST /nodes/n1/drain".to_owned()]
    );
}

/// TUI: TUI-FRM-08
#[test]
fn calls_run_at_the_same_time_and_a_slow_one_does_not_hold_the_others() {
    struct Meeting {
        barrier: Barrier,
        met: AtomicUsize,
    }
    impl Backend for Meeting {
        fn call(&self, _: &Call) -> Result<Reply, Error> {
            // Every call waits for the others: this only finishes if they run in parallel.
            self.barrier.wait();
            self.met.fetch_add(1, Ordering::SeqCst);
            Err(Error::from_message("client.network", "done"))
        }
    }
    let backend = Arc::new(Meeting {
        barrier: Barrier::new(THREADS),
        met: AtomicUsize::new(0),
    });
    let (tx, rx) = mpsc::channel();
    let workers = Workers::start(backend.clone(), tx, THREADS);
    for id in 0..THREADS as u64 {
        workers.submit(id, 0, ApiCall::Nodes);
    }
    for _ in 0..THREADS {
        let Msg::Reply { result, .. } = wait(&rx) else {
            panic!()
        };
        assert!(result.is_err());
    }
    assert_eq!(backend.met.load(Ordering::SeqCst), THREADS);
}

/// TUI: TUI-CLU-05
#[test]
fn the_real_client_runs_the_call_through_the_scripted_transport() {
    let transport = MockTransport::new();
    transport.reply(200, &[], r#"{"drainState":"draining","pendingMessages":3}"#);
    transport.reply(
        503,
        &[],
        r#"{"code":"cluster.quorum-unavailable","message":"no quorum"}"#,
    );
    let mut config = Config::new("http://127.0.0.1:1");
    config.token = Some("root-token".into());
    config.max_attempts = 1;
    let client =
        Client::with_transport(config, Box::new(transport.clone()), Arc::new(|_| {})).unwrap();
    let (tx, rx) = mpsc::channel();
    let workers = Workers::start(Arc::new(client), tx, 1);
    workers.submit(1, 0, ApiCall::DrainStatus("abc".into()));
    let Msg::Reply { result, .. } = wait(&rx) else {
        panic!()
    };
    assert!(result.unwrap().text().contains("draining"));
    workers.submit(2, 0, ApiCall::StopCluster);
    let Msg::Reply { result, .. } = wait(&rx) else {
        panic!()
    };
    let error = result.unwrap_err();
    assert_eq!(
        (error.status, error.code.as_str()),
        (503, "cluster.quorum-unavailable")
    );

    let seen = transport.seen();
    assert_eq!(seen[0].request.method, "GET");
    assert!(seen[0].request.target.ends_with("/nodes/abc/drain"));
    assert_eq!(seen[0].header("authorization"), Some("Bearer root-token"));
    assert_eq!(seen[1].request.method, "POST");
    assert!(seen[1].request.target.ends_with("/cluster/stop"));
}

#[test]
fn the_threads_end_when_the_workers_are_dropped() {
    let backend = Arc::new(Scripted {
        seen: Mutex::new(Vec::new()),
    });
    let (tx, rx) = mpsc::channel();
    let workers = Workers::start(backend, tx, 2);
    drop(workers);
    // With the workers gone and every sender dropped, the channel closes instead of hanging.
    assert!(matches!(
        rx.recv_timeout(Duration::from_secs(5)),
        Err(mpsc::RecvTimeoutError::Disconnected)
    ));
}
