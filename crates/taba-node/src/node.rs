//! A subida do nó: prepara o disco, cria o estado e abre os listeners. É o que o
//! `main` e os testes de integração usam (doc 18, seção 17.3: o `main` cria as
//! peças e as entrega aos construtores).

use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use tokio::net::TcpListener;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use crate::cluster::drain::DrainController;
use crate::cluster::members::{Member, MemberState};
use crate::cluster::service::{BootstrapError, ClusterHandler, ClusterRules, ClusterService};
use crate::cluster::state::NodeShared;
use crate::cluster::transport::{Handler, PeerAddr, Transport};
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
use crate::http::{self, AppState};
use crate::storage;
use crate::storage::db::NodeDb;
use crate::storage::messages::MessageStore;
use crate::storage::state::NodeStateStore;
use crate::storage::tokens::SqliteTokenStore;
use crate::storage::topics::SqliteTopicStore;
use crate::telemetry::Telemetry;

#[derive(Debug, thiserror::Error)]
pub enum NodeError {
    #[error("não foi possível usar storage.dir `{}`: {source}", .dir.display())]
    Storage {
        dir: std::path::PathBuf,
        source: io::Error,
    },

    #[error("não foi possível abrir a porta {name} ({addr}): {source}")]
    Bind {
        name: &'static str,
        addr: SocketAddr,
        source: io::Error,
    },
}

/// Registra o nó numa rede (a em memória dos testes): recebe o endereço dele e o `Handler`.
pub type AttachFn = Arc<dyn Fn(PeerAddr, Arc<dyn Handler>) + Send + Sync>;

/// Como o nó fala com os outros (doc 18, seção 17.5). Sem isto, um nó em `mode=production`
/// fica em `bootstrapping`: o `Transport` real (HTTP com mTLS) ainda não existe, e os testes de
/// cluster entregam o da rede em memória.
pub struct ClusterDeps {
    pub transport: Arc<dyn Transport>,
    /// Os IPs que o nó informa aos outros para se alcançarem (`addresses` do `join`).
    pub addresses: Vec<String>,
    /// Chamado, para cada IP, com o endereço do nó e o `Handler` que atende a porta do cluster,
    /// antes de o bootstrap começar. A rede em memória registra o nó aqui; a real não precisa,
    /// pois a porta já está aberta.
    pub attach: Option<AttachFn>,
}

/// O que o `main` entrega ao nó: o relógio, o endereço em que os listeners escutam e o
/// `Transport` entre nós.
pub struct Deps {
    pub clock: Arc<dyn Clock>,
    pub bind_ip: IpAddr,
    pub cluster: Option<ClusterDeps>,
}

impl Default for Deps {
    fn default() -> Self {
        Self {
            clock: Arc::new(SystemClock),
            bind_ip: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            cluster: None,
        }
    }
}

pub struct RunningNode {
    shared: Arc<NodeShared>,
    cluster: Option<Arc<ClusterService>>,
    /// O bootstrap esgotou as tentativas: o nó deve encerrar com erro (doc 04, seção 6.5).
    /// Fica aqui para o canal nunca fechar: sem falha, `fatal` espera para sempre.
    _fatal_keepalive: tokio::sync::mpsc::UnboundedSender<String>,
    fatal_rx: tokio::sync::mpsc::UnboundedReceiver<String>,
    client_addr: SocketAddr,
    operations_addr: Option<SocketAddr>,
    cluster_addr: Option<SocketAddr>,
    cancel: CancellationToken,
    tasks: JoinSet<io::Result<()>>,
    drain: Arc<DrainController>,
    /// `drain.on-stop`: ao parar, as mensagens em memória vão para o disco.
    drain_on_stop: bool,
}

impl RunningNode {
    pub fn node_id(&self) -> &str {
        &self.shared.node_id
    }

    pub fn shared(&self) -> &Arc<NodeShared> {
        &self.shared
    }

    /// O nó no cluster; só existe em `mode=production` com um `Transport`.
    pub fn cluster(&self) -> Option<&Arc<ClusterService>> {
        self.cluster.as_ref()
    }

    /// Espera o nó dizer que não consegue seguir (o bootstrap esgotou as tentativas) e devolve
    /// o motivo. Sem falha, nunca termina.
    pub async fn fatal(&mut self) -> String {
        // O canal não fecha (o nó guarda um emissor), então o `unwrap_or_default` é só pelo tipo.
        self.fatal_rx.recv().await.unwrap_or_default()
    }

    /// Endereço da API de cliente (com a porta realmente aberta).
    pub fn client_addr(&self) -> SocketAddr {
        self.client_addr
    }

    /// `None`: as rotas de operação ficam na porta da API.
    pub fn operations_addr(&self) -> Option<SocketAddr> {
        self.operations_addr
    }

    /// `None` em modo desenvolvimento: a porta do cluster não é aberta.
    pub fn cluster_addr(&self) -> Option<SocketAddr> {
        self.cluster_addr
    }

    /// Faz o drain local (se `drain.on-stop`), para os listeners e espera as chamadas em
    /// andamento terminarem. No desenvolvimento o drain é o *save*: a partida seguinte
    /// retoma do disco (doc 04, seção 6.10).
    pub async fn shutdown(mut self) {
        if self.drain_on_stop {
            self.drain.stop_by_signal().await;
        }
        self.cancel.cancel();
        while let Some(result) = self.tasks.join_next().await {
            match result {
                Ok(Ok(())) => {}
                Ok(Err(e)) => tracing::error!(error = %e, "listener terminou com erro"),
                Err(e) => tracing::error!(error = %e, "task do listener falhou"),
            }
        }
    }
}

async fn bind(name: &'static str, ip: IpAddr, port: u16) -> Result<TcpListener, NodeError> {
    let addr = SocketAddr::new(ip, port);
    TcpListener::bind(addr)
        .await
        .map_err(|source| NodeError::Bind { name, addr, source })
}

/// Sobe o nó: cria `storage.dir`, abre as portas e passa a atender. Todas as portas
/// são abertas antes de qualquer uma atender, então uma porta ocupada não deixa
/// meio nó no ar.
pub async fn start(config: Config, deps: Deps) -> Result<RunningNode, NodeError> {
    for warning in &config.warnings {
        tracing::warn!("{warning}");
    }
    storage::prepare_dir(&config.storage.dir).map_err(|source| NodeError::Storage {
        dir: config.storage.dir.clone(),
        source,
    })?;

    let dev = config.is_development();
    let client = bind("da API de cliente", deps.bind_ip, config.ports.api).await?;
    let operations = match config.ports.operations {
        Some(port) => Some(bind("de operação", deps.bind_ip, port).await?),
        None => None,
    };
    let cluster = if dev {
        None
    } else {
        Some(bind("do cluster", deps.bind_ip, config.ports.cluster).await?)
    };

    let now = deps.clock.now();
    let shared = Arc::new(NodeShared::new(
        id::new_at(deps.clock.now_millis()),
        config.mode,
        now,
    ));
    let storage_dir = config.storage.dir.clone();
    let storage_error = |source: io::Error| NodeError::Storage {
        dir: storage_dir.clone(),
        source,
    };
    // Em desenvolvimento o nó retoma o que está em disco (o drain serve de save); em
    // produção toda partida é de nó novo, e as definições vêm do cluster.
    if dev && !config.resume_on_start {
        tracing::warn!(
            "development.resume-on-start=false: apagando o conteúdo de {}",
            config.storage.dir.display()
        );
        storage::wipe_dir_contents(&config.storage.dir).map_err(storage_error)?;
    }
    let node_db = Arc::new(
        NodeDb::open(&config.storage.dir, config.storage.db_synchronous).map_err(storage_error)?,
    );
    let saves = NodeStateStore::new(node_db.clone());
    let topics = Topics::open(
        TopicRules::from_config(&config),
        Arc::new(SqliteTopicStore::new(node_db.clone())),
        deps.clock.clone(),
        dev && config.resume_on_start,
    )
    .await
    .map_err(storage_error)?;
    let topics = Arc::new(topics);
    let tokens = Tokens::open(
        TokenRules::from_config(&config),
        Arc::new(SqliteTokenStore::new(node_db)),
        deps.clock.clone(),
        dev && config.resume_on_start,
    )
    .await
    .map_err(storage_error)?;
    let auth = Arc::new(Auth::new(&config.auth, Arc::new(tokens)));
    let telemetry = Arc::new(Telemetry::from_config(&config, &shared.node_id));
    let messages = Messages::new(
        topics.clone(),
        deps.clock.clone(),
        MessageRules::from_config(&config),
        Arc::new(MessageStore::new(
            &config.storage.dir,
            config.storage.db_synchronous,
        )),
        dev && config.resume_on_start,
    );
    check_save_mark(&saves, dev && config.resume_on_start, topics.count());
    {
        let shared = shared.clone();
        messages.on_failure(Arc::new(move || {
            tracing::error!("disco cheio ou banco corrompido: o nó passa a `failed`");
            shared.set_failed();
        }));
    }
    let drain = DrainController::new(
        shared.clone(),
        messages.clone(),
        saves,
        deps.clock.clone(),
        config.drain.timeout,
    );

    let addr_of = |l: &TcpListener| {
        l.local_addr().map_err(|source| NodeError::Bind {
            name: "(endereço)",
            addr: SocketAddr::new(deps.bind_ip, 0),
            source,
        })
    };
    let client_addr = addr_of(&client)?;
    let operations_addr = operations.as_ref().map(addr_of).transpose()?;
    let cluster_addr = cluster.as_ref().map(addr_of).transpose()?;
    let cancel = CancellationToken::new();

    // Em produção, com um `Transport`, o nó entra num cluster; sem ele, fica em
    // `bootstrapping` (o `Transport` real ainda não existe).
    let cluster_node = match (cluster_addr, deps.cluster) {
        (Some(addr), Some(cluster_deps)) => {
            let local = Member {
                node_id: shared.node_id.clone(),
                addresses: cluster_deps.addresses.clone(),
                port: addr.port(),
                state: MemberState::Active,
            };
            let service = ClusterService::new(
                local,
                shared.clone(),
                cluster_deps.transport.clone(),
                deps.clock.clone(),
                telemetry.clone(),
                ClusterRules::from_config(&config),
                cancel.clone(),
            );
            Some((service, cluster_deps))
        }
        _ => None,
    };
    let state = AppState {
        clock: deps.clock.clone(),
        node: shared.clone(),
        topics,
        messages,
        drain: drain.clone(),
        cluster: cluster_node.as_ref().map(|(service, _)| service.clone()),
        allowed_ips: Arc::new(config.operations_allowed_ips.clone()),
        client_ip: Arc::new(ClientIpRules::from_config(&config.client_ip)),
        rate_limiter: Arc::new(RateLimiter::from_config(
            &config.rate_limit,
            deps.clock.clone(),
        )),
        auth: auth.clone(),
        telemetry: telemetry.clone(),
        stats_rules: Arc::new(StatsRules::from_config(&config)),
    };

    if dev {
        tracing::warn!(node_id = %shared.node_id, "mode=development: o nó é um cluster de um só, sem réplicas; não use em produção");
    } else {
        if cluster_node.is_none() {
            tracing::warn!(
                "o transporte entre nós ainda não existe: o nó fica em `bootstrapping` e /health/ready responde 503"
            );
        }
        if config.tls.dir.is_none() {
            tracing::warn!("sem pasta de certificados: a porta do cluster atende em HTTP puro");
        } else {
            tracing::warn!(
                "o TLS mútuo do cluster ainda não existe: a porta do cluster atende em HTTP puro"
            );
        }
    }

    let mut tasks = JoinSet::new();
    let (fatal_tx, fatal_rx) = tokio::sync::mpsc::unbounded_channel();
    let sweeper = state.messages.clone();
    let state_messages = state.messages.clone();
    let token_sweep_interval = config.retention.sweep_interval.min(Duration::from_secs(1));
    let reload_interval = config.auth.reload_interval;
    // Cada listener é medido com a sua porta no atributo `server.port` (doc 11).
    let mut serve = |listener: TcpListener, router: axum::Router, port: u16| {
        let cancel = cancel.clone();
        let observer = Observer {
            telemetry: telemetry.clone(),
            port,
        };
        tasks.spawn(observe::serve(listener, router, observer, cancel));
    };
    serve(
        client,
        http::client_router(state.clone(), operations.is_none()),
        client_addr.port(),
    );
    if let (Some(listener), Some(addr)) = (operations, operations_addr) {
        serve(
            listener,
            http::operations_router(state.clone()),
            addr.port(),
        );
    }
    if let (Some(listener), Some(addr)) = (cluster, cluster_addr) {
        serve(listener, http::cluster_router(state), addr.port());
    }
    if let (Some((service, cluster_deps)), Some(addr)) = (&cluster_node, cluster_addr) {
        if let Some(attach) = &cluster_deps.attach {
            for address in &cluster_deps.addresses {
                attach(
                    PeerAddr::new(address.clone(), addr.port()),
                    Arc::new(ClusterHandler(service.clone())),
                );
            }
        }
        let (service, fatal) = (service.clone(), fatal_tx.clone());
        tasks.spawn(async move {
            match service.bootstrap().await {
                Ok(()) | Err(BootstrapError::Cancelled) => Ok(()),
                Err(e) => {
                    let _ = fatal.send(e.to_string());
                    Err(io::Error::other(e.to_string()))
                }
            }
        });
    }
    {
        let cancel = cancel.clone();
        tasks.spawn(async move {
            sweeper.run_sweeper(cancel).await;
            Ok(())
        });
    }

    {
        // A fila volátil sem consumidor há `idleTimeout` é excluída (doc 15, seção 14.3).
        let cancel = cancel.clone();
        let messages = state_messages.clone();
        tasks.spawn(async move {
            messages.run_idle_sweeper(cancel).await;
            Ok(())
        });
    }
    {
        // Cada nó apaga por conta própria os tokens vencidos, sem propagar (doc 06a).
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
        // O arquivo do token raiz é relido, para trocar o segredo sem reiniciar.
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
        node_id = %shared.node_id,
        client = %client_addr,
        operations = ?operations_addr,
        cluster = ?cluster_addr,
        "nó no ar"
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
        drain_on_stop: config.drain.on_stop,
    })
}

/// Consome a marca do save (a partida a apaga, para uma queda depois dela não passar por save
/// limpo). Retomando e com tópicos em disco, sem a marca o nó avisa que o que só estava em
/// memória se perdeu (doc 04, seção 6.10).
fn check_save_mark(saves: &NodeStateStore, resuming: bool, topics: usize) {
    match saves.take_save_mark() {
        Ok(Some(at_ms)) if resuming => {
            tracing::info!(
                save_completed_at_ms = at_ms,
                "retomando do disco: o save anterior foi concluído"
            );
        }
        Ok(None) if resuming && topics > 0 => tracing::warn!(
            "o nó anterior não concluiu o save (queda ou drain interrompido): \
             o que estava só em memória se perdeu"
        ),
        Ok(_) => {}
        Err(e) => tracing::warn!(error = %e, "não foi possível ler a marca do save"),
    }
}
