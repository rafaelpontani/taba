//! The interface's core against a real node (doc 34, section 10, "Contra um nó"). Optional: it only runs when
//! `TABA_LIVE_URL` points at a node (for example one started with `TABA_REPLICATION_FACTOR=1`), with
//! `TABA_LIVE_TOKEN` if the node asks for a token. It drives `update` and the workers, with no terminal, and never
//! drains or stops the node. Run it with:
//!
//!     TABA_LIVE_URL=http://127.0.0.1:18080 cargo test -p taba-cli --test live_node -- --nocapture

#![cfg(feature = "tui")]

use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use taba_cli::tui::api::ApiCall;
use taba_cli::tui::effect::Effect;
use taba_cli::tui::i18n::Lang;
use taba_cli::tui::model::{ClusterView, Modal, Model, Options, Report};
use taba_cli::tui::msg::Msg;
use taba_cli::tui::theme::Theme;
use taba_cli::tui::update::update;
use taba_cli::tui::worker::Workers;
use taba_client::{Client, Config};

struct Live {
    model: Model,
    workers: Workers,
    replies: mpsc::Receiver<Msg>,
    started: Instant,
}

impl Live {
    fn open(url: &str, token: Option<String>) -> Live {
        let mut config = Config::new(url);
        config.token = token;
        config.max_attempts = 1;
        let client = Client::new(config).expect("client");
        let (tx, replies) = mpsc::channel();
        let workers = Workers::start(Arc::new(client), tx, 4);
        let model = Model::new(Options {
            lang: Lang::EnUs,
            theme: Theme::default(),
            profile: "live".to_owned(),
            url: url.to_owned(),
            retention_secs: 60,
            interval_secs: 1,
            profiles: taba_cli::profile::ConfigFile::default(),
            choose_profile: false,
        });
        Live {
            model,
            workers,
            replies,
            started: Instant::now(),
        }
    }

    fn feed(&mut self, msg: Msg) {
        for effect in update(&mut self.model, msg) {
            if let Effect::Call {
                id,
                generation,
                call,
            } = effect
            {
                self.workers.submit(id, generation, call);
            }
        }
    }

    fn key(&mut self, code: KeyCode) {
        self.feed(Msg::Key(KeyEvent::new(code, KeyModifiers::NONE)));
    }

    /// Ticks the clock and delivers the replies until `done` holds.
    fn pump(&mut self, what: &str, done: impl Fn(&Model) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while !done(&self.model) {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            let now = u64::try_from(self.started.elapsed().as_millis()).unwrap();
            self.feed(Msg::Tick { now, wall_ms: 0 });
            while let Ok(reply) = self.replies.try_recv() {
                self.feed(reply);
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }
}

#[test]
fn the_core_reads_a_real_node_and_shows_its_detail_charts_and_a_failed_action() {
    let Ok(url) = std::env::var("TABA_LIVE_URL") else {
        eprintln!("TABA_LIVE_URL is not set: skipped");
        return;
    };
    let mut live = Live::open(&url, std::env::var("TABA_LIVE_TOKEN").ok());

    // Who the token is, then the cluster status.
    live.pump("the status", |m| m.status.data.is_some());
    let status = live.model.status.data.clone().unwrap();
    assert!(live.model.is_root());
    assert!(!status.nodes.is_empty());
    assert!(status
        .nodes
        .iter()
        .all(|n| n.reachable && n.state == "active"));
    assert_eq!(live.model.health().1, "ok");
    println!(
        "status: {} node(s), quorum {}",
        status.nodes.len(),
        status.quorum
    );

    // The detail loads the metrics (Prometheus text) and the backup (not configured is not an error).
    live.key(KeyCode::Enter);
    assert_eq!(live.model.cluster.view, ClusterView::Detail);
    live.pump("metrics and backup", |m| {
        m.cluster.metrics.data.is_some() && m.cluster.backup.data.is_some()
    });
    let metrics = live.model.cluster.metrics.data.clone().unwrap();
    assert!(metrics.sum("http_server_active_requests").is_some());
    println!("metrics: {} samples", metrics.samples.len());
    assert!(live.model.cluster.backup.error.is_none());

    // The sampler fills the history while the screen is open.
    live.key(KeyCode::Esc);
    live.key(KeyCode::Char('m'));
    assert_eq!(live.model.cluster.view, ClusterView::Monitor);
    let node = live
        .model
        .cluster
        .node
        .clone()
        .unwrap_or_else(|| status.nodes[0].node_id.clone());
    live.pump("two samples", |m| {
        m.history
            .series(&node, taba_cli::tui::history::Series::MemoryUsed)
            .is_some_and(|r| r.len() >= 2)
    });
    let csv = taba_cli::tui::screens::cluster::export_csv(&live.model, &node, 0);
    assert!(csv.lines().count() >= 3, "{csv}");
    println!("csv:\n{csv}");

    // An action the server refuses is reported with its code, and nothing changes (no S3 backup here).
    live.key(KeyCode::Esc);
    live.key(KeyCode::Char('B'));
    assert!(matches!(live.model.modal, Some(Modal::Confirm(_))));
    live.key(KeyCode::Right);
    live.key(KeyCode::Enter);
    live.pump("the flush answer", |m| {
        m.modal.is_some() && m.busy.is_none()
    });
    match &live.model.modal {
        Some(Modal::Report(Report::Failure { call, fail })) => {
            assert_eq!(call, &ApiCall::FlushCluster);
            assert_eq!(fail.code, "backup.not-configured");
        }
        Some(Modal::Report(Report::Flush(report))) => {
            // A node with S3 configured: the report lists the nodes.
            assert!(!report.nodes.is_empty());
        }
        other => panic!("unexpected: {other:?}"),
    }
}
