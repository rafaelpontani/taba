//! The simulated cluster of the **C**-level tests (doc 20, section 19.4): several real nodes in the
//! same process, each with disk in a temporary folder and real HTTP on free ports, linked
//! by the in-memory network (`MemoryNetwork`, with programmable loss, delay and partition) and by a
//! shared manual `Clock`.
//!
//! Node `n` has the IP `10.0.0.n` on the in-memory network. The load balancer (`cluster.entry-address`) is
//! `balancer:8081` and splits the calls among the nodes that are already up.

#![allow(dead_code)]

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Semaphore;

use serde_json::json;
use taba_node::cluster::channel::Channels;
use taba_node::cluster::members::MemberTable;
use taba_node::cluster::replication::{Definitions, Replication, Signals};
use taba_node::cluster::transport::PeerAddr;
use taba_node::cluster::transport::memory::MemoryNetwork;
use taba_node::core::clock::ManualClock;
use taba_node::node::NodeError;
use taba_node::storage::backup::MemoryStore;

use super::{TestNode, TestNodeBuilder};

pub const BALANCER_HOST: &str = "balancer";
pub const BALANCER_PORT: u16 = 8081;

/// Short deadlines: the tests run in real time (only the `Clock` is manual).
pub const FAST: &[(&str, &str)] = &[
    ("TABA_WRITE_RETRY_ATTEMPTS", "3"),
    ("TABA_WRITE_RETRY_BACKOFF", "10ms"),
    ("TABA_WRITE_COMMIT_TIMEOUT", "400ms"),
    ("TABA_CLUSTER_BOOTSTRAP_RETRY_INTERVAL", "50ms"),
    ("TABA_CLUSTER_CHANNEL_KEEPALIVE_INTERVAL", "200ms"),
    ("TABA_CLUSTER_CHANNEL_RECONNECT_BACKOFF", "50ms"),
];

/// The backup deadlines in the tests: the `Clock` is the manual one (the `backup.delay` window passes when the
/// test advances the clock); the real-time deadlines (heartbeat, death) are short.
pub const BACKUP_FAST: &[(&str, &str)] = &[
    ("TABA_BACKUP_DELAY", "60s"),
    ("TABA_BACKUP_META_INTERVAL", "300ms"),
    ("TABA_BACKUP_SEGMENT_MAX_AGE", "300ms"),
    ("TABA_BACKUP_HEARTBEAT_INTERVAL", "100ms"),
    ("TABA_BACKUP_DEAD_AFTER", "1500ms"),
    ("TABA_BACKUP_DRAIN_TIMEOUT", "8s"),
    ("TABA_BACKUP_RECONCILE_INTERVAL", "1s"),
    ("TABA_BACKUP_RESTORE_LOCK_TTL", "1500ms"),
];

pub fn host_of(number: usize) -> String {
    format!("10.0.0.{number}")
}

/// The deadline, in seconds, of `eventually` and `eventually_async`.
pub const WAIT: u64 = 30;

/// Keeps a port busy between the fall of a node and its return, so the system does not hand it to a node of another
/// test of the same process (known issue "A restarted test node that loses its port to another test").
pub struct PortHold(std::sync::Mutex<Option<std::net::TcpListener>>);

impl PortHold {
    /// Binds the port right after the node shut down. If someone took it in that instant, there is nothing to hold.
    pub fn new(port: u16) -> Self {
        Self(std::sync::Mutex::new(
            std::net::TcpListener::bind(("127.0.0.1", port)).ok(),
        ))
    }

    /// Frees the port just before the node binds it again.
    pub fn release(&self) {
        self.0.lock().unwrap().take();
    }
}

pub fn balancer() -> PeerAddr {
    PeerAddr::new(BALANCER_HOST, BALANCER_PORT)
}

/// Waits for the condition to become true (up to `WAIT` s). The nodes work in their own tasks. The deadline is
/// generous on purpose: it only costs time when the test has already failed, and a slow or overloaded runner
/// (arm64 on CI) takes much longer than the developer sees on their machine.
pub async fn eventually(what: &str, mut condition: impl FnMut() -> bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(WAIT);
    while !condition() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "waited {WAIT} s for: {what}"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// The same, for a condition that needs `await` (for example, reading `/metrics`).
pub async fn eventually_async<F, Fut>(what: &str, mut condition: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let deadline = tokio::time::Instant::now() + Duration::from_secs(WAIT);
    while !condition().await {
        assert!(
            tokio::time::Instant::now() < deadline,
            "waited {WAIT} s for: {what}"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

pub struct TestCluster {
    pub network: MemoryNetwork,
    pub clock: Arc<ManualClock>,
    pub nodes: Vec<TestNode>,
    /// Extra parameters for all the nodes (for example, the root token).
    pub env: Vec<(String, String)>,
    /// The backup's in-memory S3, shared by all the nodes (`start_with_backup`).
    pub s3: Option<Arc<MemoryStore>>,
    /// The disk usage (%) the nodes with backup see: the test raises it to simulate the disk
    /// filling up, without depending on the machine's disk.
    pub disk_usage: Arc<std::sync::atomic::AtomicU32>,
}

impl TestCluster {
    /// A network without nodes, with the load balancer up (no node behind it).
    pub fn empty() -> Self {
        // `TABA_TEST_LOG=debug` (or `taba_node=trace,...`) turns the node logs on to investigate a test.
        if let Ok(level) = std::env::var("TABA_TEST_LOG") {
            taba_node::telemetry::init_logging(Some(&level), taba_node::config::LogFormat::Text);
        }
        let network = MemoryNetwork::new();
        network.add_balancer(balancer(), Vec::new());
        Self {
            network,
            clock: Arc::new(ManualClock::at_doc_epoch()),
            nodes: Vec::new(),
            env: Vec::new(),
            s3: None,
            disk_usage: Arc::new(std::sync::atomic::AtomicU32::new(10)),
        }
    }

    /// Connects the test's disk measure to the node's backup.
    fn arm_backup(&self, node: &TestNode) {
        if let Some(backup) = node.node.backup() {
            let usage = self.disk_usage.clone();
            backup.set_disk_probe(Arc::new(move || {
                usage.load(std::sync::atomic::Ordering::SeqCst)
            }));
        }
    }

    /// The disk usage (%) the nodes see from now on.
    pub fn set_disk_usage(&self, percent: u32) {
        self.disk_usage
            .store(percent, std::sync::atomic::Ordering::SeqCst);
    }

    /// Like `start_with`, with the S3 backup enabled on the nodes, in an in-memory store that the test
    /// also sees (`cluster.s3`). The backup deadlines are short; the `Clock` is the manual one.
    pub async fn start_with_backup(
        count: usize,
        env: &[(&str, &str)],
        s3: Arc<MemoryStore>,
    ) -> Self {
        let mut cluster = Self::empty();
        cluster.s3 = Some(s3);
        cluster.env = BACKUP_FAST
            .iter()
            .chain(env)
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        for number in 1..=count {
            cluster.add_node(number == 1).await;
        }
        cluster
    }

    /// Starts `count` nodes, one after the other, each only after the previous one is in the cluster.
    /// The first is the `cluster.first-node`.
    pub async fn start(count: usize) -> Self {
        Self::start_with(count, &[]).await
    }

    /// Like `start`, with extra `TABA_*` parameters on all the nodes.
    pub async fn start_with(count: usize, env: &[(&str, &str)]) -> Self {
        let mut cluster = Self::empty();
        cluster.env = env
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        for number in 1..=count {
            cluster.add_node(number == 1).await;
        }
        cluster
    }

    /// A node's builder with the short deadlines, the clock and the cluster network.
    pub fn builder(&self, number: usize, first: bool) -> TestNodeBuilder {
        let mut builder = TestNode::builder()
            .in_network(&self.network, &host_of(number))
            .clock(self.clock.clone());
        for (name, value) in FAST {
            builder = builder.env(name, value);
        }
        for (name, value) in &self.env {
            builder = builder.env(name, value);
        }
        if let Some(s3) = &self.s3 {
            builder = builder.backup_store(s3.clone());
        }
        if first {
            builder = builder.env("TABA_CLUSTER_FIRST_NODE", "true");
        }
        builder
    }

    /// The load balancer starts splitting the calls among the first `count` nodes in the list. A node
    /// only enters it after having the table (otherwise the load balancer would pick it for the listing).
    pub fn publish_backends(&self, count: usize) {
        let backends: Vec<PeerAddr> = self
            .nodes
            .iter()
            .take(count)
            .enumerate()
            .map(|(i, n)| PeerAddr::new(host_of(i + 1), n.node.cluster_addr().port()))
            .collect();
        self.network.add_balancer(balancer(), backends);
    }

    /// Starts the next node (`10.0.0.<n>`) and waits for it to join the cluster.
    pub async fn add_node(&mut self, first: bool) -> &TestNode {
        let number = self.nodes.len() + 1;
        let node = self
            .builder(number, first)
            .try_start()
            .await
            .expect("the node should start");
        self.arm_backup(&node);
        self.nodes.push(node);
        wait_joined(self.nodes.last().expect("just added")).await;
        self.publish_backends(self.nodes.len());
        self.nodes.last().expect("just added")
    }

    /// Starts a node without waiting for it to join (when the test wants to see the join fail or take long).
    /// It enters the node list, but not the load balancer (see `publish_backends`).
    pub async fn spawn_node(&mut self, builder: TestNodeBuilder) -> Result<usize, NodeError> {
        let node = builder.try_start().await?;
        self.arm_backup(&node);
        self.nodes.push(node);
        Ok(self.nodes.len())
    }

    /// Starts node `number` again, at the same address as the one that just fell (`configure` puts the port
    /// in the builder). The port is ephemeral, drawn by the system, and in that interval another connection or
    /// another test may take it; on a slow runner the window is much larger. So it retries while the
    /// error is `AddrInUse`, up to `WAIT` s.
    pub async fn respawn_node(
        &mut self,
        number: usize,
        configure: impl Fn(TestNodeBuilder) -> TestNodeBuilder,
    ) -> Result<usize, NodeError> {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(WAIT);
        loop {
            let builder = configure(self.builder(number, false));
            match self.spawn_node(builder).await {
                Err(NodeError::Bind { source, .. })
                    if source.kind() == std::io::ErrorKind::AddrInUse
                        && tokio::time::Instant::now() < deadline =>
                {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
                other => return other,
            }
        }
    }

    pub fn node(&self, number: usize) -> &TestNode {
        &self.nodes[number - 1]
    }

    /// The member table the node has.
    pub fn table(&self, number: usize) -> Option<MemberTable> {
        self.node(number)
            .node
            .cluster()
            .and_then(|c| c.membership().table())
    }

    /// The IDs of the members the node sees, in order.
    pub fn member_ids(&self, number: usize) -> Vec<String> {
        self.table(number)
            .map(|t| t.members.into_iter().map(|m| m.node_id).collect())
            .unwrap_or_default()
    }

    pub fn node_id(&self, number: usize) -> String {
        self.node(number).node.node_id().to_owned()
    }

    pub async fn shutdown(self) {
        for node in self.nodes {
            node.shutdown().await;
        }
    }
}

/// Waits for the node to have the member table and be `active`.
pub async fn wait_joined(node: &TestNode) {
    let shared = node.node.shared().clone();
    eventually("the node to join the cluster", move || {
        shared.state() == taba_node::cluster::state::NodeState::Active
    })
    .await;
}

/// For the tests that bring a node down and want it to **stay in the table**: the channel only gives a node
/// up for lost (and opens the exclusion vote) after a very large number of attempts, or after the queue
/// holds a frame for a long time. The reconnect backoff is left alone: a healthy channel that drops on a
/// slow runner must come back quickly.
pub const NO_VOTE: &[(&str, &str)] = &[
    ("TABA_CLUSTER_CHANNEL_RECONNECT_RETRIES", "100000"),
    ("TABA_CLUSTER_CHANNEL_QUEUE_MAX_AGE", "600s"),
];

// ---- Shortcuts for the C-level tests ----

pub fn address_of(cluster: &TestCluster, number: usize) -> PeerAddr {
    PeerAddr::new(
        host_of(number),
        cluster.node(number).node.cluster_addr().port(),
    )
}

pub fn replication_of(node: &TestNode) -> Arc<Replication> {
    node.node
        .cluster()
        .and_then(|c| c.replication())
        .expect("the node is in the cluster")
        .clone()
}

pub fn channels_of(cluster: &TestCluster, number: usize) -> Arc<Channels> {
    replication_of(cluster.node(number)).channels.clone()
}

pub fn definitions_of(cluster: &TestCluster, number: usize) -> Arc<Definitions> {
    replication_of(cluster.node(number)).definitions.clone()
}

pub fn signals_of(cluster: &TestCluster, number: usize) -> Arc<Signals> {
    replication_of(cluster.node(number)).signals.clone()
}

/// The sum of the `name{...}` series of the `/metrics` text whose label contains `label`.
pub fn total(text: &str, name: &str, label: &str) -> f64 {
    text.lines()
        .filter(|line| !line.starts_with('#'))
        .filter(|line| {
            line.strip_prefix(name)
                .is_some_and(|rest| rest.starts_with('{') || rest.starts_with(' '))
        })
        .filter(|line| line.contains(label))
        .filter_map(|line| line.rsplit_once(' ')?.1.parse::<f64>().ok())
        .sum()
}

/// Waits for all the node's channels to be connected, with the outgoing queue empty and the peer's
/// heartbeat (active, with the current table version) already received.
pub async fn settled(cluster: &TestCluster) {
    let version = cluster.table(1).unwrap().version;
    for number in 1..=cluster.nodes.len() {
        let channels = channels_of(cluster, number);
        let expected = cluster.nodes.len() - 1;
        eventually("the channels connected and empty", move || {
            let peers = channels.peers();
            peers.len() == expected
                && peers.iter().all(|p| {
                    p.up && p.queue == 0
                        && p.version == Some(version)
                        && p.state.as_deref() == Some("active")
                })
        })
        .await;
    }
}

/// A topic name whose definition owner (the primary of its position on the ring) is node `number`.
pub fn name_owned_by(cluster: &TestCluster, number: usize, prefix: &str) -> String {
    let membership = cluster.node(1).node.cluster().unwrap().membership();
    let wanted = cluster.node_id(number);
    (0..2000)
        .map(|i| format!("{prefix}{i}"))
        .find(|name| {
            membership
                .ring()
                .primary(taba_node::core::ring::definition_position("topic", name))
                .is_some_and(|owner| owner == wanted)
        })
        .expect("some name falls on the node")
}

pub async fn create_queue(node: &TestNode, name: &str) {
    let response = node
        .client
        .post(node.client_url("/topics"))
        .json(&json!({"name": name, "type": "queue"}))
        .send()
        .await
        .expect("request");
    assert_eq!(response.status(), 201);
}

/// Starts node `number` isolated from the network: the bootstrap tries and fails until the test gives the network back, which
/// gives time to arm the synchronization pause.
pub async fn start_isolated(cluster: &mut TestCluster, env: &[(&str, &str)]) -> usize {
    let number = cluster.nodes.len() + 1;
    cluster.network.isolate(&host_of(number));
    let mut builder = cluster.builder(number, false);
    for (name, value) in env {
        builder = builder.env(name, value);
    }
    cluster
        .spawn_node(builder)
        .await
        .expect("the node should start")
}

/// Makes the node's synchronization stop on entering `phase` and gives it back to the network. Returns the gate:
/// a permit on it lets the synchronization continue.
pub fn pause_sync(cluster: &TestCluster, number: usize, phase: &'static str) -> Arc<Semaphore> {
    let gate = Arc::new(Semaphore::new(0));
    cluster
        .node(number)
        .node
        .cluster()
        .and_then(|c| c.replication())
        .expect("replication")
        .syncer
        .pause_at(phase, gate.clone());
    cluster.network.restore(&host_of(number));
    gate
}

pub async fn wait_phase(node: &TestNode, phase: &str) {
    let wanted = format!("taba_sync_phase{{taba_phase=\"{phase}\"}}");
    eventually_async("the synchronization phase", || async {
        super::series(&node.metrics().await, &wanted) == Some(1.0)
    })
    .await;
}

/// No vote: the channel only opens the vote after many attempts in a row.
pub const MANY_RETRIES: (&str, &str) = ("TABA_CLUSTER_CHANNEL_RECONNECT_RETRIES", "1000");
/// The heartbeat only when the test says so: the first call of each pair still carries it.
pub const QUIET: (&str, &str) = ("TABA_CLUSTER_CHANNEL_KEEPALIVE_INTERVAL", "30s");
