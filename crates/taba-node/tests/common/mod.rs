//! Base of a node's integration tests (doc 20, section 19.4, level **I**): starts a
//! real node, with real disk in a temporary folder and real HTTP on free ports.
//!
//! ```ignore
//! let node = TestNode::single().await;
//! let response = node.client_get("/health/live").await;
//! ```
//!
//! The cluster tests (level **C**) use `cluster::TestCluster`, which starts several nodes in the same
//! process, with the in-memory `Transport` and a shared manual `Clock` (doc 18, section
//! 17.5).

#![allow(dead_code)] // each test file uses a part of this module

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;

use reqwest::{Client, Response};
use tempfile::TempDir;

use taba_node::cluster::transport::http::HttpTransport;
use taba_node::cluster::transport::memory::MemoryNetwork;
use taba_node::config::{Config, MapEnv};
use taba_node::core::clock::ManualClock;
use taba_node::node::{self, ClusterDeps, Deps, NodeError, RunningNode};
use taba_node::storage::backup::ObjectStore;

pub mod cluster;
pub mod history;
pub mod otlp;
pub mod real;

pub struct TestNodeBuilder {
    env: Vec<(String, String)>,
    /// The in-memory network and the node's IP on it (with `Transport`).
    network: Option<(MemoryNetwork, String)>,
    clock: Option<Arc<ManualClock>>,
    /// The node uses the real HTTP `Transport` and listens on this IP (loopback).
    real_ip: Option<IpAddr>,
    /// The one-node profile (the default): the node starts alone, takes over its disk and the start waits for it to be active.
    profile: bool,
    /// The backup's S3 object store (the in-memory one, shared among the test's nodes).
    backup_store: Option<Arc<dyn ObjectStore>>,
    /// The program version the node announces (simulates another release).
    program_version: Option<String>,
}

impl TestNodeBuilder {
    /// The program version the node announces to the cluster, instead of the build's.
    pub fn program_version(mut self, version: &str) -> Self {
        self.program_version = Some(version.to_owned());
        self
    }

    /// Sets a parameter as a `TABA_*` variable (`("TABA_API_PORT", "0")`).
    pub fn env(mut self, name: &str, value: &str) -> Self {
        self.env.retain(|(n, _)| n != name);
        self.env.push((name.to_owned(), value.to_owned()));
        self
    }

    /// Connects the node to the in-memory network, with the IP `host` (production mode). It tries to join the
    /// cluster through the network's `balancer:8081` load balancer (`cluster.entry-address`).
    pub fn in_network(mut self, network: &MemoryNetwork, host: &str) -> Self {
        self.network = Some((network.clone(), host.to_owned()));
        self.cluster_profile()
            .env("TABA_CLUSTER_ENTRY_ADDRESS", "balancer:8081")
    }

    /// Enables the S3 backup with this store (and the `test` bucket).
    pub fn backup_store(mut self, store: Arc<dyn ObjectStore>) -> Self {
        self.backup_store = Some(store);
        self.env("TABA_BACKUP_S3_BUCKET", "test")
    }

    /// The node's clock; several nodes of a test share the same one.
    pub fn clock(mut self, clock: Arc<ManualClock>) -> Self {
        self.clock = Some(clock);
        self
    }

    /// The real node: HTTP `Transport` (with mutual TLS if `TABA_CLUSTER_TLS_DIR` points to a
    /// folder) and listens on `ip` (`127.x.y.z`, one per node, so the cluster recognizes them by IP).
    /// `entry` is the `cluster.entry-address`: the first node uses any address; the others, the
    /// cluster address of a node that is already up.
    pub fn real(self, ip: &str, entry: &str) -> Self {
        let mut this = self.cluster_profile();
        this.real_ip = Some(ip.parse().expect("test IP"));
        this.env("TABA_CLUSTER_ENTRY_ADDRESS", entry)
            .env("TABA_CLUSTER_ADVERTISE_ADDRESSES", ip)
    }

    /// The one-node profile (doc 07, section 8; the default of `TestNode::builder()`): factor 1, no TLS, no
    /// entry address, with the real `Transport`. The node starts alone and takes over its disk.
    pub fn single_node(mut self) -> Self {
        self.profile = true;
        self.real_ip = Some("127.0.0.1".parse().expect("test IP"));
        self.env("TABA_REPLICATION_FACTOR", "1")
            .env("TABA_CLUSTER_RESTORE_COLD_START_WAIT", "300ms")
            .env("TABA_TOPICS_VOLATILE_TAKEOVER_JITTER", "0s")
            .env("TABA_CLUSTER_TLS_ENABLED", "false")
    }

    /// Leaves the one-node profile: the production defaults (factor, TLS) apply again.
    fn cluster_profile(mut self) -> Self {
        self.profile = false;
        self.real_ip = None;
        self.env.retain(|(name, _)| {
            name != "TABA_REPLICATION_FACTOR"
                && name != "TABA_CLUSTER_TLS_ENABLED"
                && name != "TABA_CLUSTER_RESTORE_COLD_START_WAIT"
                && name != "TABA_TOPICS_VOLATILE_TAKEOVER_JITTER"
        });
        self
    }

    /// Production profile, **without** `Transport`: the node stays in `bootstrapping`.
    pub fn production(self) -> Self {
        self.cluster_profile()
            .env("TABA_CLUSTER_ENTRY_ADDRESS", "127.0.0.1:1")
    }

    pub async fn try_start(self) -> Result<TestNode, NodeError> {
        let dir = TempDir::new().expect("temporary folder");
        let storage = dir.path().join("taba");
        let env = MapEnv(self.env.into_iter().collect());
        let mut env = env;
        env.0
            .entry("TABA_STORAGE_DIR".into())
            .or_insert_with(|| storage.display().to_string());
        env.0
            .entry("TABA_SECRETS_DIR".into())
            .or_insert_with(|| dir.path().join("secrets").display().to_string());

        let config = Config::parse(None, &env).expect("valid test configuration");
        let clock = self
            .clock
            .unwrap_or_else(|| Arc::new(ManualClock::at_doc_epoch()));
        let real_ip = self.real_ip;
        let profile = self.profile;
        let backup_store = self.backup_store.clone();
        let program_version = self.program_version.clone();
        let cluster = self.network.map(|(network, host)| {
            let attach_to = network.clone();
            ClusterDeps {
                transport: network.transport(host.clone()),
                addresses: vec![host],
                attach: Some(Arc::new(move |addr, handler| {
                    attach_to.register(addr, handler)
                })),
            }
        });
        let deps = match real_ip {
            Some(ip) => {
                let mut deps = Deps::production(&config)?;
                deps.clock = clock.clone();
                deps.bind_ip = ip;
                deps.backup_store = backup_store.clone();
                deps.program_version = program_version.clone();
                // The cluster recognizes the caller by IP: each node calls from its own.
                if let Some(cluster) = deps.cluster.as_mut() {
                    cluster.transport = Arc::new(
                        HttpTransport::bound_to(deps.tls.as_ref(), ip).expect("transporte"),
                    );
                }
                deps
            }
            None => Deps {
                clock: clock.clone(),
                bind_ip: IpAddr::V4(Ipv4Addr::LOCALHOST),
                cluster,
                tls: None,
                backup_store: backup_store.clone(),
                program_version: program_version.clone(),
            },
        };
        let storage_dir = config.storage.dir.clone();
        let node = node::start(config, deps).await?;
        if profile {
            let shared = node.shared().clone();
            cluster::eventually("the node to be active", move || {
                shared.state() == taba_node::cluster::state::NodeState::Active
            })
            .await;
            // Whatever the disk held was restored: the messages are back before the test goes on.
            let cluster = node.cluster().expect("the node is in the cluster").clone();
            cluster::eventually("the restoration of the disk to finish", move || {
                cluster.restoration().status() == taba_node::cluster::restore::Status::Restored
            })
            .await;
            let lives = storage_dir.join(taba_node::storage::messages::MessageStore::LIVES);
            cluster::eventually(
                "the folders of the previous lives to be restored",
                move || {
                    !lives
                        .read_dir()
                        .map(|mut e| e.next().is_some())
                        .unwrap_or(false)
                },
            )
            .await;
        }
        let client = Client::builder().no_proxy().build().expect("HTTP client");
        Ok(TestNode {
            node,
            client,
            clock,
            dir,
        })
    }

    pub async fn start(self) -> TestNode {
        self.try_start().await.expect("the node should start")
    }
}

pub struct TestNode {
    pub node: RunningNode,
    pub client: Client,
    pub clock: Arc<ManualClock>,
    /// The temporary folder: `storage.dir` is `<dir>/taba`. Deleted when the test ends.
    pub dir: TempDir,
}

impl TestNode {
    /// Free ports (`0`), the one-node profile and disk in a temporary folder.
    pub fn builder() -> TestNodeBuilder {
        TestNodeBuilder {
            env: vec![
                ("TABA_API_PORT".into(), "0".into()),
                ("TABA_CLUSTER_PORT".into(), "0".into()),
                ("TABA_OPERATIONS_PORT".into(), "0".into()),
            ],
            network: None,
            clock: None,
            real_ip: None,
            profile: false,
            backup_store: None,
            program_version: None,
        }
        .single_node()
    }

    /// A node in the one-node profile, already active.
    pub async fn single() -> Self {
        Self::builder().start().await
    }

    pub async fn production() -> Self {
        Self::builder().production().start().await
    }

    pub fn storage_dir(&self) -> std::path::PathBuf {
        self.dir.path().join("taba")
    }

    pub fn client_url(&self, path: &str) -> String {
        url(self.node.client_addr(), path)
    }

    /// A route of the operations port (no token).
    pub fn operations_url(&self, path: &str) -> String {
        url(self.node.operations_addr(), path)
    }

    pub fn cluster_url(&self, path: &str) -> String {
        url(self.node.cluster_addr(), path)
    }

    pub async fn client_get(&self, path: &str) -> Response {
        self.client
            .get(self.client_url(path))
            .send()
            .await
            .expect("request")
    }

    /// `GET` on an operations route.
    pub async fn client_get_ops(&self, path_and_query: &str) -> Response {
        self.client
            .get(self.operations_url(path_and_query))
            .send()
            .await
            .expect("request")
    }

    /// The text of `GET /metrics`.
    pub async fn metrics(&self) -> String {
        let response = self
            .client
            .get(self.operations_url("/metrics"))
            .send()
            .await
            .expect("request");
        assert_eq!(response.status(), 200);
        response.text().await.expect("corpo")
    }

    /// The value of `GET /metrics/topics...` as JSON.
    pub async fn stats(&self, path_and_query: &str) -> serde_json::Value {
        let response = self
            .client
            .get(self.operations_url(path_and_query))
            .send()
            .await
            .expect("request");
        assert_eq!(response.status(), 200, "{path_and_query}");
        response.json().await.expect("corpo JSON")
    }

    pub async fn shutdown(self) {
        // The manifest of the life says when the node was last seen active; the periodic writer is slow in real time.
        if let Some(cluster) = self.node.cluster() {
            cluster.write_manifest();
        }
        self.node.shutdown().await;
    }
}

/// The value of the series `name{labels}` in a Prometheus text, with the labels as they come out (in
/// attribute order). `None` if the series does not exist.
pub fn series(text: &str, wanted: &str) -> Option<f64> {
    text.lines()
        .filter(|line| !line.starts_with('#'))
        .find_map(|line| {
            let (name, value) = line.rsplit_once(' ')?;
            (name == wanted).then(|| value.parse().ok())?
        })
}

fn url(addr: SocketAddr, path: &str) -> String {
    format!("http://{addr}{path}")
}

/// Checks the body of an `ErrorResponse` and returns the `code`.
pub async fn error_code(response: Response, expected_status: u16) -> String {
    let status = response.status().as_u16();
    let header_id = response.headers()["x-request-id"]
        .to_str()
        .unwrap()
        .to_owned();
    let body: serde_json::Value = response.json().await.expect("corpo JSON");
    // The body goes in the message: without it, an unexpected error does not say what it was.
    assert_eq!(status, expected_status, "response body: {body}");
    assert_eq!(
        body["requestId"], header_id,
        "requestId do corpo e X-Request-Id divergem"
    );
    assert!(body["message"].is_string());
    body["code"].as_str().expect("code").to_owned()
}
