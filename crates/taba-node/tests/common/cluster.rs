//! O cluster simulado dos testes de nível **C** (doc 20, seção 19.4): vários nós de verdade no
//! mesmo processo, cada um com disco numa pasta temporária e HTTP real em portas livres, ligados
//! pela rede em memória (`MemoryNetwork`, com perda, atraso e partição programáveis) e por um
//! `Clock` manual compartilhado.
//!
//! O nó `n` tem o IP `10.0.0.n` na rede em memória. O balanceador (`cluster.entry-address`) é
//! `balancer:8081` e reparte as chamadas entre os nós que já subiram.

#![allow(dead_code)]

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Semaphore;

use serde_json::json;
use taba_node::cluster::channel::Channels;
use taba_node::cluster::members::MemberTable;
use taba_node::cluster::replication::{Replication, Signals};
use taba_node::cluster::transport::PeerAddr;
use taba_node::cluster::transport::memory::MemoryNetwork;
use taba_node::core::clock::ManualClock;
use taba_node::node::NodeError;
use taba_node::storage::backup::MemoryStore;

use super::{TestNode, TestNodeBuilder};

pub const BALANCER_HOST: &str = "balancer";
pub const BALANCER_PORT: u16 = 8081;

/// Prazos curtos: os testes rodam em tempo real (só o `Clock` é manual).
pub const FAST: &[(&str, &str)] = &[
    ("TABA_WRITE_RETRY_ATTEMPTS", "3"),
    ("TABA_WRITE_RETRY_BACKOFF", "10ms"),
    ("TABA_WRITE_COMMIT_TIMEOUT", "400ms"),
    ("TABA_CLUSTER_BOOTSTRAP_RETRY_INTERVAL", "50ms"),
    ("TABA_CLUSTER_CHANNEL_KEEPALIVE_INTERVAL", "200ms"),
    ("TABA_CLUSTER_CHANNEL_RECONNECT_BACKOFF", "50ms"),
];

/// Os prazos do backup nos testes: o `Clock` é o manual (a janela de `backup.delay` passa quando o
/// teste avança o relógio); os prazos de tempo real (batimento, morte) são curtos.
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

/// O prazo, em segundos, de `eventually` e `eventually_async`.
pub const WAIT: u64 = 30;

pub fn balancer() -> PeerAddr {
    PeerAddr::new(BALANCER_HOST, BALANCER_PORT)
}

/// Espera a condição ficar verdadeira (até `WAIT` s). Os nós trabalham em tarefas próprias. O prazo é
/// folgado de propósito: só custa tempo quando o teste já falhou, e um runner lento ou sobrecarregado
/// (arm64 no CI) leva muito mais que o desenvolvedor vê na sua máquina.
pub async fn eventually(what: &str, mut condition: impl FnMut() -> bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(WAIT);
    while !condition() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "esperei {WAIT} s por: {what}"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// O mesmo, para uma condição que precisa de `await` (por exemplo, ler o `/metrics`).
pub async fn eventually_async<F, Fut>(what: &str, mut condition: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let deadline = tokio::time::Instant::now() + Duration::from_secs(WAIT);
    while !condition().await {
        assert!(
            tokio::time::Instant::now() < deadline,
            "esperei {WAIT} s por: {what}"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

pub struct TestCluster {
    pub network: MemoryNetwork,
    pub clock: Arc<ManualClock>,
    pub nodes: Vec<TestNode>,
    /// Parâmetros extras de todos os nós (por exemplo, o token raiz).
    pub env: Vec<(String, String)>,
    /// O S3 em memória do backup, compartilhado por todos os nós (`start_with_backup`).
    pub s3: Option<Arc<MemoryStore>>,
    /// O uso do disco (%) que os nós com backup enxergam: o teste o sobe para simular o disco
    /// enchendo, sem depender do disco da máquina.
    pub disk_usage: Arc<std::sync::atomic::AtomicU32>,
}

impl TestCluster {
    /// Uma rede sem nós, com o balanceador no ar (sem nó por trás).
    pub fn empty() -> Self {
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

    /// Liga ao backup do nó a medida de disco do teste.
    fn arm_backup(&self, node: &TestNode) {
        if let Some(backup) = node.node.backup() {
            let usage = self.disk_usage.clone();
            backup.set_disk_probe(Arc::new(move || {
                usage.load(std::sync::atomic::Ordering::SeqCst)
            }));
        }
    }

    /// O uso do disco (%) que os nós enxergam daqui em diante.
    pub fn set_disk_usage(&self, percent: u32) {
        self.disk_usage
            .store(percent, std::sync::atomic::Ordering::SeqCst);
    }

    /// Como `start_with`, com o backup em S3 ligado nos nós, num depósito em memória que o teste
    /// também enxerga (`cluster.s3`). Os prazos do backup são curtos; o `Clock` é o manual.
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

    /// Sobe `count` nós, um depois do outro, cada um só depois de o anterior estar no cluster.
    /// O primeiro é o `cluster.first-node`.
    pub async fn start(count: usize) -> Self {
        Self::start_with(count, &[]).await
    }

    /// Como `start`, com parâmetros `TABA_*` a mais em todos os nós.
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

    /// O construtor de um nó com os prazos curtos, o relógio e a rede do cluster.
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

    /// O balanceador passa a repartir as chamadas entre os `count` primeiros nós da lista. Um nó
    /// só entra nele depois de ter a tabela (senão o balanceador o escolheria para a listagem).
    pub fn publish_backends(&self, count: usize) {
        let backends: Vec<PeerAddr> = self
            .nodes
            .iter()
            .take(count)
            .enumerate()
            .map(|(i, n)| {
                PeerAddr::new(
                    host_of(i + 1),
                    n.node.cluster_addr().expect("porta do cluster").port(),
                )
            })
            .collect();
        self.network.add_balancer(balancer(), backends);
    }

    /// Sobe o próximo nó (`10.0.0.<n>`) e espera ele entrar no cluster.
    pub async fn add_node(&mut self, first: bool) -> &TestNode {
        let number = self.nodes.len() + 1;
        let node = self
            .builder(number, first)
            .try_start()
            .await
            .expect("o nó deveria subir");
        self.arm_backup(&node);
        self.nodes.push(node);
        wait_joined(self.nodes.last().expect("recém-adicionado")).await;
        self.publish_backends(self.nodes.len());
        self.nodes.last().expect("recém-adicionado")
    }

    /// Sobe um nó sem esperar ele entrar (quando o teste quer ver o ingresso falhar ou demorar).
    /// Entra na lista de nós, mas não no balanceador (veja `publish_backends`).
    pub async fn spawn_node(&mut self, builder: TestNodeBuilder) -> Result<usize, NodeError> {
        let node = builder.try_start().await?;
        self.arm_backup(&node);
        self.nodes.push(node);
        Ok(self.nodes.len())
    }

    /// Sobe o nó `number` de novo, no mesmo endereço do que acabou de cair (`configure` põe a porta
    /// no construtor). A porta é efêmera, sorteada pelo sistema, e nesse intervalo outra conexão ou
    /// outro teste pode tomá-la; num runner lento a janela é bem maior. Por isso repete enquanto o
    /// erro for `AddrInUse`, até `WAIT` s.
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

    /// A tabela de membros que o nó tem.
    pub fn table(&self, number: usize) -> Option<MemberTable> {
        self.node(number)
            .node
            .cluster()
            .and_then(|c| c.membership().table())
    }

    /// Os IDs dos membros que o nó enxerga, em ordem.
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

/// Espera o nó ter a tabela de membros e ficar `active`.
pub async fn wait_joined(node: &TestNode) {
    let shared = node.node.shared().clone();
    eventually("o nó entrar no cluster", move || {
        shared.state() == taba_node::cluster::state::NodeState::Active
    })
    .await;
}

/// Para os testes que derrubam um nó e querem que ele **continue na tabela**: o canal só dá a nó
/// por perdido (e abre a votação de exclusão) depois de muito tempo.
pub const NO_VOTE: &[(&str, &str)] = &[("TABA_CLUSTER_CHANNEL_RECONNECT_BACKOFF", "600s")];

// ---- Atalhos dos testes de nível C ----

pub fn address_of(cluster: &TestCluster, number: usize) -> PeerAddr {
    PeerAddr::new(
        host_of(number),
        cluster.node(number).node.cluster_addr().unwrap().port(),
    )
}

pub fn replication_of(node: &TestNode) -> Arc<Replication> {
    node.node
        .cluster()
        .and_then(|c| c.replication())
        .expect("o nó está no cluster")
        .clone()
}

pub fn channels_of(cluster: &TestCluster, number: usize) -> Arc<Channels> {
    replication_of(cluster.node(number)).channels.clone()
}

pub fn signals_of(cluster: &TestCluster, number: usize) -> Arc<Signals> {
    replication_of(cluster.node(number)).signals.clone()
}

/// A soma das séries `name{...}` do texto do `/metrics` cujo rótulo contém `label`.
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

/// Espera todos os canais do nó estarem conectados, com a fila de saída vazia e o sinal de vida do
/// par (ativo, com a versão da tabela atual) já recebido.
pub async fn settled(cluster: &TestCluster) {
    let version = cluster.table(1).unwrap().version;
    for number in 1..=cluster.nodes.len() {
        let channels = channels_of(cluster, number);
        let expected = cluster.nodes.len() - 1;
        eventually("os canais conectados e vazios", move || {
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

/// Um nome de tópico cujo dono da definição (o primário da posição dele no anel) é o nó `number`.
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
        .expect("algum nome cai no nó")
}

pub async fn create_queue(node: &TestNode, name: &str) {
    let response = node
        .client
        .post(node.client_url("/topics"))
        .json(&json!({"name": name, "type": "queue"}))
        .send()
        .await
        .expect("requisição");
    assert_eq!(response.status(), 201);
}

/// Sobe o nó `number` isolado da rede: o bootstrap tenta e falha até o teste devolver a rede, o
/// que dá tempo de armar a pausa da sincronização.
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
        .expect("o nó deveria subir")
}

/// Faz a sincronização do nó parar ao entrar em `phase` e o devolve à rede. Devolve o portão:
/// uma licença nele deixa a sincronização seguir.
pub fn pause_sync(cluster: &TestCluster, number: usize, phase: &'static str) -> Arc<Semaphore> {
    let gate = Arc::new(Semaphore::new(0));
    cluster
        .node(number)
        .node
        .cluster()
        .and_then(|c| c.replication())
        .expect("replicação")
        .syncer
        .pause_at(phase, gate.clone());
    cluster.network.restore(&host_of(number));
    gate
}

pub async fn wait_phase(node: &TestNode, phase: &str) {
    let wanted = format!("taba_sync_phase{{taba_phase=\"{phase}\"}}");
    eventually_async("a fase da sincronização", || async {
        super::series(&node.metrics().await, &wanted) == Some(1.0)
    })
    .await;
}

/// Sem votação: o canal só abre a votação depois de muitas tentativas seguidas.
pub const MANY_RETRIES: (&str, &str) = ("TABA_CLUSTER_CHANNEL_RECONNECT_RETRIES", "1000");
/// O sinal de vida só quando o teste manda: a primeira chamada de cada par ainda o leva.
pub const QUIET: (&str, &str) = ("TABA_CLUSTER_CHANNEL_KEEPALIVE_INTERVAL", "30s");
