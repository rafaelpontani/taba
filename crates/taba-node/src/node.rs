//! The node startup: prepares the disk, creates the state and opens the listeners. It is what
//! `main` and the integration tests use (doc 18, section 17.3: `main` creates the
//! pieces and hands them to the constructors).

use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use tokio::net::TcpListener;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use crate::cluster::backup::{
    Backup, BackupArgs, BackupPort, BackupRules, ClusterTopology, Topology,
};
use crate::cluster::drain::DrainController;
use crate::cluster::members::{Member, MemberState};
use crate::cluster::replication::{self, Attached};
use crate::cluster::service::{BootstrapError, ClusterHandler, ClusterRules, ClusterService};
use crate::cluster::state::NodeShared;
use crate::cluster::sync::SyncRules;
use crate::cluster::tls::TlsStore;
use crate::cluster::transport::http::{HttpTransport, discover_address};
use crate::cluster::transport::{Handler, PeerAddr, Transport};
use crate::cluster::volatile::VolatileManager;
use crate::config::Config;
use crate::core::clock::{Clock, SystemClock};
use crate::core::id;
use crate::engine::messages::stats::StatsRules;
use crate::engine::messages::{MessageRules, Messages};
use crate::engine::tokens::{TokenRules, Tokens};
use crate::engine::topics::{TopicRules, Topics};
use crate::http::auth::Auth;
use crate::http::client_ip::ClientIpRules;
use crate::http::observe::{self, Observer};
use crate::http::ratelimit::RateLimiter;
use crate::http::{self, AppShared, AppState};
use crate::storage;
use crate::storage::backup::{BackupDb, ObjectStore, Prefixed, S3Settings, S3Store};
use crate::storage::db::NodeDb;
use crate::storage::losses::LossStore;
use crate::storage::messages::{ExcludedLimits, MessageStore};
use crate::storage::state::NodeStateStore;
use crate::storage::tokens::SqliteTokenStore;
use crate::storage::topics::SqliteTopicStore;
use crate::storage::versions::VersionStore;
use crate::telemetry::Telemetry;
use crate::telemetry::otlp::{self, Pipeline};

#[derive(Debug, thiserror::Error)]
pub enum NodeError {
    #[error("could not use storage.dir `{}`: {source}", .dir.display())]
    Storage {
        dir: std::path::PathBuf,
        source: io::Error,
    },

    #[error("could not open the port {name} ({addr}): {source}")]
    Bind {
        name: &'static str,
        addr: SocketAddr,
        source: io::Error,
    },

    #[error("OTLP export: {0}")]
    Otlp(String),

    #[error("certificados do cluster: {0}")]
    Tls(String),

    #[error("transport between nodes: {0}")]
    Transport(String),
}

/// Registers the node on a network (the in-memory one of the tests): receives its address and the `Handler`.
pub type AttachFn = Arc<dyn Fn(PeerAddr, Arc<dyn Handler>) + Send + Sync>;

/// How the node talks to the others (doc 18, section 17.5). `Deps::production` hands in the HTTP
/// `Transport` with mutual TLS; the cluster tests hand in the in-memory network's. Without it, a node in
/// `mode=production` stays in `bootstrapping`.
pub struct ClusterDeps {
    pub transport: Arc<dyn Transport>,
    /// The IPs the node tells the others so they can reach it (`addresses` of the `join`).
    pub addresses: Vec<String>,
    /// Called, for each IP, with the node's address and the `Handler` that serves the cluster port,
    /// before the bootstrap starts. The in-memory network registers the node here; the real one does not need to,
    /// since the port is already open.
    pub attach: Option<AttachFn>,
}

/// What `main` hands to the node: the clock, the address the listeners listen on and the
/// `Transport` between nodes.
pub struct Deps {
    pub clock: Arc<dyn Clock>,
    pub bind_ip: IpAddr,
    pub cluster: Option<ClusterDeps>,
    /// The cluster certificates, if whoever assembles the node has already opened them (the real `Transport` uses the
    /// same ones). Without them, the node opens the `cluster.tls.dir` folder, if there is one.
    pub tls: Option<Arc<TlsStore>>,
    /// The backup object store (the tests hand in the in-memory one). Without it, with
    /// `backup.s3.bucket`, the node opens the S3 client.
    pub backup_store: Option<Arc<dyn ObjectStore>>,
    /// The program version the node announces to the cluster; without it, the build's. The tests use it to
    /// simulate a cluster with two releases.
    pub program_version: Option<String>,
}

impl Deps {
    /// The pieces of a real node (`taba`): the system clock, the HTTP `Transport` (with mutual TLS if there is
    /// a certificates folder) and the IPs the node tells the others: `cluster.advertise-addresses` or, without
    /// it, the interface that goes out to `cluster.entry-address` (D95).
    pub fn production(config: &Config) -> Result<Self, NodeError> {
        let mut deps = Self::default();
        let tls = config
            .tls
            .dir
            .as_deref()
            .map(TlsStore::open)
            .transpose()
            .map_err(NodeError::Tls)?;
        let transport = HttpTransport::new(tls.as_ref()).map_err(NodeError::Transport)?;
        let addresses = if !config.cluster.advertise_addresses.is_empty() {
            config.cluster.advertise_addresses.clone()
        } else if config.cluster.entry_address.is_none() && config.is_single_node() {
            // A node alone has no entry address to find its interface by, and no peer that needs to reach it.
            vec!["127.0.0.1".to_owned()]
        } else {
            let entry: PeerAddr = config
                .cluster
                .entry_address
                .as_deref()
                .unwrap_or_default()
                .parse()
                .map_err(NodeError::Transport)?;
            vec![discover_address(&entry).map_err(|why| {
                NodeError::Transport(format!(
                    "{why}; set cluster.advertise-addresses to this node's IPs"
                ))
            })?]
        };
        deps.tls = tls;
        deps.cluster = Some(ClusterDeps {
            transport: Arc::new(transport),
            addresses,
            attach: None,
        });
        Ok(deps)
    }
}

impl Default for Deps {
    fn default() -> Self {
        Self {
            clock: Arc::new(SystemClock),
            bind_ip: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            cluster: None,
            tls: None,
            backup_store: None,
            program_version: None,
        }
    }
}

/// What the cluster uses of the backup without knowing the rest of the node: the service and the stop drain.
struct NodeBackupPort {
    backup: Arc<Backup>,
    drain: Arc<DrainController>,
}

impl BackupPort for NodeBackupPort {
    fn backup(&self) -> Arc<Backup> {
        self.backup.clone()
    }

    fn stop_node(
        &self,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<
                    Output = Result<serde_json::Value, crate::core::error::TabaError>,
                > + Send
                + '_,
        >,
    > {
        Box::pin(async move {
            let drained = self.drain.drain_and_wait().await;
            Ok(serde_json::json!({
                "nodeId": self.backup.node_id(),
                "drained": drained,
                "outboxPending": self.backup.status().outbox_pending,
            }))
        })
    }
}

/// Opens the backup object store: what the tests hand in, or the S3 of `backup.s3.*`.
async fn open_backup_store(
    s3: &crate::config::S3,
    injected: Option<Arc<dyn ObjectStore>>,
) -> Arc<dyn ObjectStore> {
    let raw = match injected {
        Some(store) => store,
        None => Arc::new(
            S3Store::connect(&S3Settings {
                bucket: s3.bucket.clone(),
                region: s3.region.clone(),
                endpoint: s3.endpoint.clone(),
                access_key: s3.access_key.clone(),
                secret_key: s3.secret_key.as_ref().map(|s| s.expose().to_owned()),
                path_style: s3.path_style,
            })
            .await,
        ),
    };
    Arc::new(Prefixed::new(raw, &s3.prefix))
}

pub struct RunningNode {
    shared: Arc<NodeShared>,
    cluster: Option<Arc<ClusterService>>,
    /// The bootstrap ran out of attempts: the node must exit with an error (doc 04, section 6.5).
    /// It lives here so the channel never closes: without a failure, `fatal` waits forever.
    _fatal_keepalive: tokio::sync::mpsc::UnboundedSender<String>,
    fatal_rx: tokio::sync::mpsc::UnboundedReceiver<String>,
    client_addr: SocketAddr,
    operations_addr: SocketAddr,
    cluster_addr: SocketAddr,
    cancel: CancellationToken,
    tasks: JoinSet<io::Result<()>>,
    drain: Arc<DrainController>,
    /// The message engine: when the node stops, it lets go of the topic databases.
    messages: Arc<Messages>,
    /// `drain.on-stop`: on stopping, the in-memory messages go to disk.
    drain_on_stop: bool,
    /// The manager of this node's volatile queues (only in a cluster).
    volatile: Option<Arc<VolatileManager>>,
    /// The OTLP trace and log providers (only with `OTEL_EXPORTER_OTLP_ENDPOINT`).
    otlp: Option<Arc<Pipeline>>,
    /// The optional S3 backup (only with `backup.s3.bucket`).
    backup: Option<Arc<Backup>>,
    /// The cluster only keeps a weak reference: the node keeps the backup service alive.
    _backup_port: Option<Arc<dyn BackupPort>>,
}

impl RunningNode {
    pub fn node_id(&self) -> String {
        self.shared.node_id()
    }

    pub fn shared(&self) -> &Arc<NodeShared> {
        &self.shared
    }

    /// The node in the cluster; only exists in `mode=production` with a `Transport`.
    pub fn cluster(&self) -> Option<&Arc<ClusterService>> {
        self.cluster.as_ref()
    }

    /// The `id`s of the volatile queues this node manages right now (doc 15, section 14.5); empty outside
    /// a cluster and while the node is not ready.
    pub fn volatile_managed(&self) -> Vec<String> {
        self.volatile
            .as_ref()
            .map(|manager| {
                manager
                    .managed()
                    .into_iter()
                    .map(|queue| queue.id)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Waits for the node to say it cannot go on (the bootstrap ran out of attempts) and returns
    /// the reason. Without a failure, it never ends.
    pub async fn fatal(&mut self) -> String {
        // The channel does not close (the node keeps a sender), so the `unwrap_or_default` is only for the type.
        self.fatal_rx.recv().await.unwrap_or_default()
    }

    /// Address of the client API (with the port actually opened).
    pub fn client_addr(&self) -> SocketAddr {
        self.client_addr
    }

    /// The operations port actually opened.
    pub fn operations_addr(&self) -> SocketAddr {
        self.operations_addr
    }

    /// The cluster port actually opened.
    pub fn cluster_addr(&self) -> SocketAddr {
        self.cluster_addr
    }

    /// This node's S3 backup; `None` without `backup.s3.bucket`.
    pub fn backup(&self) -> Option<&Arc<Backup>> {
        self.backup.as_ref()
    }

    /// Sends right away the spans and logs that are in the OTLP queue (for the tests).
    pub async fn flush_otlp(&self) {
        if let Some(pipeline) = self.otlp.clone() {
            let _ = tokio::task::spawn_blocking(move || pipeline.flush()).await;
        }
    }

    /// Does the local drain (if `drain.on-stop`), stops the listeners and waits for the calls in
    /// progress to finish. In development the drain is the *save*: the next start
    /// resumes from disk (doc 04, section 6.10).
    pub async fn shutdown(mut self) {
        if self.drain_on_stop {
            self.drain.stop_by_signal().await;
        }
        self.cancel.cancel();
        if let Some(pipeline) = self.otlp.take() {
            // The spans and logs still in the queue go out before the process ends.
            let _ = tokio::task::spawn_blocking(move || pipeline.shutdown()).await;
        }
        while let Some(result) = self.tasks.join_next().await {
            match result {
                Ok(Ok(())) => {}
                Ok(Err(e)) => tracing::error!(error = %e, "listener ended with an error"),
                Err(e) => tracing::error!(error = %e, "listener task failed"),
            }
        }
        // The engine and the cluster hold each other, so the files would stay open until the process ends: a node
        // started on the same disk in the same process (the tests) could not move them.
        self.messages.release_files();
    }
}

async fn bind(name: &'static str, ip: IpAddr, port: u16) -> Result<TcpListener, NodeError> {
    let addr = SocketAddr::new(ip, port);
    TcpListener::bind(addr)
        .await
        .map_err(|source| NodeError::Bind { name, addr, source })
}

/// Starts the node: creates `storage.dir`, opens the ports and starts serving. All ports
/// are opened before any of them serves, so a busy port does not leave
/// half a node up.
pub async fn start(config: Config, deps: Deps) -> Result<RunningNode, NodeError> {
    for warning in &config.warnings {
        tracing::warn!("{warning}");
    }
    storage::prepare_dir(&config.storage.dir).map_err(|source| NodeError::Storage {
        dir: config.storage.dir.clone(),
        source,
    })?;

    // The cluster certificates: at least one valid pair in the folder, otherwise the node does not start.
    let tls = match (deps.tls.clone(), config.tls.dir.as_deref()) {
        (None, None) => None,
        (Some(store), _) => Some(store),
        (None, Some(dir)) => Some(TlsStore::open(dir).map_err(NodeError::Tls)?),
    };
    let client = bind("of the client API", deps.bind_ip, config.ports.api).await?;
    let operations = bind("of operations", deps.bind_ip, config.ports.operations).await?;
    let cluster = bind("do cluster", deps.bind_ip, config.ports.cluster).await?;

    let now = deps.clock.now();
    let shared = Arc::new(NodeShared::new(id::new_at(deps.clock.now_millis()), now));
    let storage_dir = config.storage.dir.clone();
    let storage_error = |source: io::Error| NodeError::Storage {
        dir: storage_dir.clone(),
        source,
    };
    // Every start is a new node: what is on disk comes back through the restoration, and the definitions from the
    // cluster.
    let node_db = Arc::new(
        NodeDb::open(&config.storage.dir, config.storage.db_synchronous).map_err(storage_error)?,
    );
    let saves = NodeStateStore::new(node_db.clone());
    let losses = LossStore::new(node_db.clone());
    let topics = Arc::new(Topics::new(
        TopicRules::from_config(&config),
        Arc::new(SqliteTopicStore::new(node_db.clone())),
        deps.clock.clone(),
        Vec::new(),
    ));
    let tokens = Tokens::new(
        TokenRules::from_config(&config),
        Arc::new(SqliteTokenStore::new(node_db.clone())),
        deps.clock.clone(),
        Vec::new(),
    );
    let auth = Arc::new(Auth::new(&config.auth, Arc::new(tokens)));
    let telemetry = Arc::new(Telemetry::from_config(&config, &shared.node_id()));
    let message_store = Arc::new(
        MessageStore::new(&config.storage.dir, config.storage.db_synchronous).with_excluded(
            ExcludedLimits {
                max_bytes: config.storage.excluded_max_bytes,
                max_age_ms: u64::try_from(config.storage.excluded_max_age.as_millis())
                    .unwrap_or(u64::MAX),
            },
            Some(saves.clone()),
        ),
    );
    let disk_store = message_store.clone();
    let messages = if deps.cluster.is_some() {
        // In a cluster the previous life's disk waits for the restart recovery (D111).
        Messages::new_keeping_disk(
            topics.clone(),
            deps.clock.clone(),
            MessageRules::from_config(&config),
            message_store,
        )
    } else {
        Messages::new(
            topics.clone(),
            deps.clock.clone(),
            MessageRules::from_config(&config),
            message_store,
        )
    };
    // The start of the continuous recording of the excluded log is fixed when the node starts, not at the first exclusion.
    messages.excluded_since();
    {
        let shared = shared.clone();
        messages.on_failure(Arc::new(move || {
            tracing::error!("disk full or corrupt database: the node goes to `failed`");
            shared.set_failed();
        }));
    }
    {
        let shared = shared.clone();
        node_db.on_failure(Arc::new(move || {
            tracing::error!("disk full or corrupt node database: the node goes to `failed`");
            shared.set_failed();
        }));
    }
    let drain = DrainController::new(
        shared.clone(),
        messages.clone(),
        deps.clock.clone(),
        config.drain.timeout,
    );

    let addr_of = |l: &TcpListener| {
        l.local_addr().map_err(|source| NodeError::Bind {
            name: "(address)",
            addr: SocketAddr::new(deps.bind_ip, 0),
            source,
        })
    };
    let client_addr = addr_of(&client)?;
    let operations_addr = addr_of(&operations)?;
    let cluster_addr = addr_of(&cluster)?;
    let cancel = CancellationToken::new();

    // In production, with a `Transport`, the node joins a cluster; without it, it stays in
    // `bootstrapping`.
    let cluster_node = match (cluster_addr, deps.cluster) {
        (addr, Some(cluster_deps)) => {
            let local = Member {
                node_id: shared.node_id(),
                addresses: cluster_deps.addresses.clone(),
                port: addr.port(),
                state: MemberState::Active,
            };
            let mut rules = ClusterRules::from_config(&config);
            if let Some(version) = &deps.program_version {
                rules.program_version = version.clone();
            }
            let service = ClusterService::new(
                local,
                shared.clone(),
                cluster_deps.transport.clone(),
                deps.clock.clone(),
                telemetry.clone(),
                rules,
                cancel.clone(),
            );
            Some((service, cluster_deps))
        }
        _ => None,
    };
    // The cluster connects to the message engine: who the owner is, the signals and the dead-letters.
    // The versions and tombstones of the definitions come back with the definitions that are resumed.
    let version_store = VersionStore::new(node_db.clone());
    let attached = cluster_node.as_ref().map(|(service, _)| {
        replication::attach(
            service,
            &messages,
            &topics,
            &auth.tokens,
            deps.clock.clone(),
            telemetry.clone(),
            losses,
            SyncRules::from_config(&config),
            (version_store, Vec::new()),
        )
    });
    // The optional S3 backup: without `backup.s3.bucket` none of this exists and the node is the usual one.
    let backup_parts = match (&config.backup.s3, &cluster_node) {
        (Some(s3), Some((service, _))) => {
            let store = open_backup_store(s3, deps.backup_store.clone()).await;
            let db = Arc::new(BackupDb::open(&config.storage.dir).map_err(storage_error)?);
            // Every start is a new node: what the previous life's `_backup.db` kept (send queue, references) does
            // not apply to the new identity.
            db.reset().map_err(storage_error)?;
            let topology: Arc<dyn Topology> = Arc::new(ClusterTopology(service.clone()));
            let backup = Backup::new(BackupArgs {
                rules: BackupRules::from_config(&config),
                store,
                db,
                messages: messages.clone(),
                disk: disk_store,
                topics: topics.clone(),
                tokens: auth.tokens.clone(),
                clock: deps.clock.clone(),
                shared: shared.clone(),
                topology,
                cancel: cancel.clone(),
            });
            drain.set_backup(backup.clone());
            let port: Arc<dyn BackupPort> = Arc::new(NodeBackupPort {
                backup: backup.clone(),
                drain: drain.clone(),
            });
            service.set_backup(&port);
            Some((backup, port))
        }
        (Some(_), None) => {
            tracing::warn!(
                "no transport between nodes: the S3 backup needs the cluster and stays off"
            );
            None
        }
        (None, _) => None,
    };
    let backup = backup_parts.as_ref().map(|(backup, _)| backup.clone());
    let state = AppState::from(AppShared {
        clock: deps.clock.clone(),
        node: shared.clone(),
        backup: backup.clone(),
        topics,
        messages,
        drain: drain.clone(),
        cluster: cluster_node.as_ref().map(|(service, _)| service.clone()),
        replicated: attached.as_ref().map(|a| a.replication.messages.clone()),
        allowed_ips: Arc::new(config.operations_allowed_ips.clone()),
        client_ip: Arc::new(ClientIpRules::from_config(&config.client_ip)),
        rate_limiter: Arc::new(RateLimiter::from_config(
            &config.rate_limit,
            deps.clock.clone(),
        )),
        auth: auth.clone(),
        telemetry: telemetry.clone(),
        stats_rules: Arc::new(StatsRules::from_config(&config)),
        tls: tls.clone(),
    });

    let otlp = match &config.telemetry.otlp {
        Some(settings) => {
            let pipeline =
                Pipeline::start(settings, &telemetry.resource).map_err(NodeError::Otlp)?;
            let sender = otlp::metrics::Sender::new(settings).map_err(NodeError::Otlp)?;
            pipeline.attach(config.telemetry.logs_level.as_deref());
            Some((Arc::new(pipeline), sender, settings.metric_interval))
        }
        None => None,
    };

    if cluster_node.is_none() {
        tracing::warn!(
            "no transport between nodes: the node stays in `bootstrapping` and /health/ready answers 503"
        );
    }
    if tls.is_none() && config.tls.enabled {
        tracing::warn!("no certificates folder: the cluster port serves plain HTTP");
    }
    let relaxed = config.relaxations();
    if !relaxed.is_empty() {
        tracing::warn!(
            node_id = %shared.node_id(),
            relaxed = %relaxed.join(", "),
            "one-node profile: for development, not for production"
        );
    }

    let mut tasks = JoinSet::new();
    let otlp = otlp.map(|(pipeline, sender, interval)| {
        let (state, cancel) = (state.clone(), cancel.clone());
        tasks.spawn(async move {
            http::metrics::run_otlp(state, sender, interval, cancel).await;
            Ok(())
        });
        pipeline
    });
    let (fatal_tx, fatal_rx) = tokio::sync::mpsc::unbounded_channel();
    let sweeper = state.messages.clone();
    let state_messages = state.messages.clone();
    let state_topics = state.topics.clone();
    let token_sweep_interval = config.retention.sweep_interval.min(Duration::from_secs(1));
    let reload_interval = config.auth.reload_interval;
    // Each listener is measured with its port in the `server.port` attribute (doc 11).
    let mut serve = |listener: TcpListener,
                     router: axum::Router,
                     port: u16,
                     tls: Option<Arc<rustls::ServerConfig>>| {
        let cancel = cancel.clone();
        let observer = Observer::new(telemetry.clone(), port);
        match tls {
            Some(config) => tasks.spawn(observe::serve_tls(
                listener, router, observer, cancel, config,
            )),
            None => tasks.spawn(observe::serve(listener, router, observer, cancel)),
        };
    };
    let client_router = http::client_router(state.clone());
    // The hand-off received from a draining node runs against this same client API.
    if let Some(replication) = state.cluster.as_ref().and_then(|c| c.replication()) {
        replication
            .forwarder
            .set_executor(http::forward::executor(client_router.clone()));
    }
    if let Some(cluster) = &state.cluster {
        drain.set_cluster(cluster.clone());
        // `GET /cluster/status` answers with the same `NodeStatus` as `/health/cluster`.
        let parts = http::operations::StatusParts::of(&state);
        let weak = Arc::downgrade(cluster);
        cluster.set_status_source(Arc::new(move || {
            let cluster = weak.upgrade();
            http::operations::node_status(&parts, cluster.as_deref())
        }));
        // What this node sees of its topics, for the other nodes' consolidated reads.
        let reads_state = state.clone();
        cluster.set_read_source(Arc::new(move |request| {
            http::topic_stats::answer_peer(&reads_state, request)
        }));
        // The operations calls another member sends for `/nodes/{nodeId}/...` run against this node.
        cluster.set_node_calls(http::forward::executor(http::operations::local_router(
            state.clone(),
        )));
    }
    serve(client, client_router, client_addr.port(), None);
    serve(
        operations,
        http::operations_router(state.clone()),
        operations_addr.port(),
        None,
    );
    {
        let (listener, addr) = (cluster, cluster_addr);
        let server_config = tls
            .as_ref()
            .map(|store| store.server_config())
            .transpose()
            .map_err(NodeError::Tls)?;
        serve(
            listener,
            http::cluster_router(state),
            addr.port(),
            server_config,
        );
    }
    if let Some(store) = tls.clone() {
        // The folder is re-read from time to time and the certificate in use is watched.
        let (cancel, interval, warning) = (
            cancel.clone(),
            config.tls.reload_interval,
            config.tls.expiry_warning,
        );
        tasks.spawn(async move {
            store.run(interval, warning, cancel).await;
            Ok(())
        });
    }
    if let Some((service, cluster_deps)) = &cluster_node {
        let addr = cluster_addr;
        if let Some(attach) = &cluster_deps.attach {
            for address in &cluster_deps.addresses {
                attach(
                    PeerAddr::new(address.clone(), addr.port()),
                    Arc::new(ClusterHandler(service.clone())),
                );
            }
        }
        let (service, fatal) = (service.clone(), fatal_tx.clone());
        let dev_token = config.auth.dev_token.clone();
        let tokens = auth.tokens.clone();
        tasks.spawn(async move {
            match service.bootstrap().await {
                Ok(()) => {
                    // The one-node profile: the `admin` work token for examples, Compose and tests (the node is
                    // active, so what the start resumed from disk is already in place).
                    if let Some(secret) = dev_token
                        && let Err(e) = tokens.ensure_dev(secret.expose()).await
                    {
                        tracing::error!(error = %e, "could not create the dev token");
                    }
                    Ok(())
                }
                Err(BootstrapError::Cancelled) => Ok(()),
                Err(e) => {
                    let _ = fatal.send(e.to_string());
                    Err(io::Error::other(e.to_string()))
                }
            }
        });
    }
    if let Some(Attached {
        replication,
        dead_letters,
    }) = attached
    {
        let signals = replication.signals.clone();
        tasks.spawn(async move {
            signals.run().await;
            Ok(())
        });
        let messages = replication.messages.clone();
        tasks.spawn(async move {
            messages.run_dead_letters(dead_letters).await;
            Ok(())
        });
        let syncer = replication.syncer.clone();
        tasks.spawn(async move {
            syncer.run_cleanup().await;
            Ok(())
        });
    }
    {
        let cancel = cancel.clone();
        tasks.spawn(async move {
            sweeper.run_sweeper(cancel).await;
            Ok(())
        });
    }
    if let Some(backup) = backup.clone() {
        tasks.spawn(async move {
            backup.run().await;
            Ok(())
        });
    }

    if let Some((service, _)) = &cluster_node {
        let service = service.clone();
        tasks.spawn(async move {
            service.run_reunion().await;
            Ok(())
        });
    }
    if let Some((service, _)) = &cluster_node {
        let service = service.clone();
        tasks.spawn(async move {
            service.run_probe().await;
            Ok(())
        });
    }
    if let Some((service, _)) = &cluster_node {
        let service = service.clone();
        tasks.spawn(async move {
            service.run_upgrade().await;
            Ok(())
        });
    }
    let volatile = cluster_node
        .as_ref()
        .map(|(service, _)| VolatileManager::new(service.clone(), state_topics.clone()));
    if let Some(manager) = volatile.clone() {
        // In a cluster, whoever deletes the idle volatile queue is its managing node (doc 15, section 14.5).
        let cancel = cancel.clone();
        tasks.spawn(async move {
            manager.run(cancel).await;
            Ok(())
        });
    } else {
        // A single node: a volatile queue with no consumer for `idleTimeout` is deleted (doc 15, section 14.3).
        let cancel = cancel.clone();
        let messages = state_messages.clone();
        tasks.spawn(async move {
            messages.run_idle_sweeper(cancel).await;
            Ok(())
        });
    }
    {
        // Each node deletes the expired tokens on its own, without propagating (doc 06a).
        let (cancel, auth) = (cancel.clone(), auth.clone());
        tasks.spawn(async move {
            let mut ticker = tokio::time::interval(token_sweep_interval);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                tokio::select! {
                    () = cancel.cancelled() => return Ok(()),
                    _ = ticker.tick() => { auth.tokens.sweep().await; }
                }
            }
        });
    }
    if auth.root.has_file() {
        // The root token file is re-read, to change the secret without restarting.
        let (cancel, auth) = (cancel.clone(), auth.clone());
        tasks.spawn(async move {
            let mut ticker = tokio::time::interval(reload_interval);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                tokio::select! {
                    () = cancel.cancelled() => return Ok(()),
                    _ = ticker.tick() => auth.root.reload(),
                }
            }
        });
    }

    tracing::info!(
        node_id = %shared.node_id(),
        client = %client_addr,
        operations = %operations_addr,
        cluster = %cluster_addr,
        "node is up"
    );
    Ok(RunningNode {
        shared,
        cluster: cluster_node.map(|(service, _)| service),
        _fatal_keepalive: fatal_tx,
        fatal_rx,
        client_addr,
        operations_addr,
        cluster_addr,
        cancel,
        tasks,
        drain,
        messages: state_messages,
        drain_on_stop: config.drain.on_stop,
        volatile,
        otlp,
        backup,
        _backup_port: backup_parts.map(|(_, port)| port),
    })
}
