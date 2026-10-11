//! Base dos testes de integração de um nó (doc 20, seção 19.4, nível **I**): sobe um
//! nó de verdade, com disco real numa pasta temporária e HTTP real em portas livres.
//!
//! ```ignore
//! let node = TestNode::development().await;
//! let response = node.client_get("/health/live").await;
//! ```
//!
//! Os testes de cluster (nível **C**) usam `cluster::TestCluster`, que sobe vários nós no mesmo
//! processo, com o `Transport` em memória e um `Clock` manual compartilhado (doc 18, seção
//! 17.5).

#![allow(dead_code)] // cada arquivo de teste usa uma parte deste módulo

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;

use reqwest::{Client, Response};
use tempfile::TempDir;

use taba_node::cluster::transport::http::HttpTransport;
use taba_node::cluster::transport::memory::MemoryNetwork;
use taba_node::config::{Config, MapEnv};
use taba_node::core::clock::ManualClock;
use taba_node::node::{self, ClusterDeps, Deps, NodeError, RunningNode};

pub mod cluster;
pub mod otlp;
pub mod real;

pub struct TestNodeBuilder {
    env: Vec<(String, String)>,
    /// A rede em memória e o IP do nó nela (`mode=production` com `Transport`).
    network: Option<(MemoryNetwork, String)>,
    clock: Option<Arc<ManualClock>>,
    /// O nó usa o `Transport` HTTP de verdade e escuta neste IP (loopback).
    real_ip: Option<IpAddr>,
}

impl TestNodeBuilder {
    /// Define um parâmetro como uma variável `TABA_*` (`("TABA_API_PORT", "0")`).
    pub fn env(mut self, name: &str, value: &str) -> Self {
        self.env.retain(|(n, _)| n != name);
        self.env.push((name.to_owned(), value.to_owned()));
        self
    }

    /// Liga o nó à rede em memória, com o IP `host` (modo produção). Ele tenta entrar no
    /// cluster pelo balanceador `balancer:8081` da rede (`cluster.entry-address`).
    pub fn in_network(mut self, network: &MemoryNetwork, host: &str) -> Self {
        self.network = Some((network.clone(), host.to_owned()));
        self.env("TABA_MODE", "production")
            .env("TABA_CLUSTER_ENTRY_ADDRESS", "balancer:8081")
    }

    /// O relógio do nó; vários nós de um teste compartilham o mesmo.
    pub fn clock(mut self, clock: Arc<ManualClock>) -> Self {
        self.clock = Some(clock);
        self
    }

    /// O nó de verdade: `Transport` HTTP (com TLS mútuo se `TABA_CLUSTER_TLS_DIR` aponta uma
    /// pasta) e escuta em `ip` (`127.x.y.z`, um por nó, para o cluster reconhecê-los pelo IP).
    /// `entry` é o `cluster.entry-address`: o primeiro nó usa um endereço qualquer; os outros, o
    /// endereço do cluster de um nó que já subiu.
    pub fn real(mut self, ip: &str, entry: &str) -> Self {
        self.real_ip = Some(ip.parse().expect("IP de teste"));
        self.env("TABA_MODE", "production")
            .env("TABA_CLUSTER_ENTRY_ADDRESS", entry)
            .env("TABA_CLUSTER_ADVERTISE_ADDRESSES", ip)
    }

    /// Modo produção, **sem** `Transport`: o nó fica em `bootstrapping`.
    pub fn production(self) -> Self {
        self.env("TABA_MODE", "production")
            .env("TABA_CLUSTER_ENTRY_ADDRESS", "127.0.0.1:1")
    }

    pub async fn try_start(self) -> Result<TestNode, NodeError> {
        let dir = TempDir::new().expect("pasta temporária");
        let storage = dir.path().join("taba");
        let env = MapEnv(self.env.into_iter().collect());
        let mut env = env;
        env.0
            .entry("TABA_STORAGE_DIR".into())
            .or_insert_with(|| storage.display().to_string());
        env.0
            .entry("TABA_SECRETS_DIR".into())
            .or_insert_with(|| dir.path().join("secrets").display().to_string());

        let config = Config::parse(None, &env).expect("configuração de teste válida");
        let clock = self
            .clock
            .unwrap_or_else(|| Arc::new(ManualClock::at_doc_epoch()));
        let real_ip = self.real_ip;
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
                // O cluster reconhece o chamador pelo IP: cada nó chama a partir do seu.
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
            },
        };
        let node = node::start(config, deps).await?;
        let client = Client::builder().no_proxy().build().expect("cliente HTTP");
        Ok(TestNode {
            node,
            client,
            clock,
            dir,
        })
    }

    pub async fn start(self) -> TestNode {
        self.try_start().await.expect("o nó deveria subir")
    }
}

pub struct TestNode {
    pub node: RunningNode,
    pub client: Client,
    pub clock: Arc<ManualClock>,
    /// A pasta temporária: `storage.dir` é `<dir>/taba`. Apagada quando o teste termina.
    pub dir: TempDir,
}

impl TestNode {
    /// Portas livres (`0`), `mode=development` e disco numa pasta temporária.
    pub fn builder() -> TestNodeBuilder {
        TestNodeBuilder {
            env: vec![
                ("TABA_API_PORT".into(), "0".into()),
                ("TABA_CLUSTER_PORT".into(), "0".into()),
                ("TABA_MODE".into(), "development".into()),
            ],
            network: None,
            clock: None,
            real_ip: None,
        }
    }

    pub async fn development() -> Self {
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

    /// A porta de operação, se houver; senão a da API (onde as rotas de operação ficam).
    pub fn operations_url(&self, path: &str) -> String {
        url(
            self.node
                .operations_addr()
                .unwrap_or(self.node.client_addr()),
            path,
        )
    }

    pub fn cluster_url(&self, path: &str) -> String {
        url(
            self.node
                .cluster_addr()
                .expect("o nó não abriu a porta do cluster"),
            path,
        )
    }

    pub async fn client_get(&self, path: &str) -> Response {
        self.client
            .get(self.client_url(path))
            .send()
            .await
            .expect("requisição")
    }

    /// `GET` numa rota de operação.
    pub async fn client_get_ops(&self, path_and_query: &str) -> Response {
        self.client
            .get(self.operations_url(path_and_query))
            .send()
            .await
            .expect("requisição")
    }

    /// O texto de `GET /metrics`.
    pub async fn metrics(&self) -> String {
        let response = self
            .client
            .get(self.operations_url("/metrics"))
            .send()
            .await
            .expect("requisição");
        assert_eq!(response.status(), 200);
        response.text().await.expect("corpo")
    }

    /// O valor de `GET /metrics/topics...` em JSON.
    pub async fn stats(&self, path_and_query: &str) -> serde_json::Value {
        let response = self
            .client
            .get(self.operations_url(path_and_query))
            .send()
            .await
            .expect("requisição");
        assert_eq!(response.status(), 200, "{path_and_query}");
        response.json().await.expect("corpo JSON")
    }

    pub async fn shutdown(self) {
        self.node.shutdown().await;
    }
}

/// O valor da série `nome{rótulos}` num texto do Prometheus, com os rótulos como saem (em ordem
/// de atributo). `None` se a série não existe.
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

/// Confere o corpo de um `ErrorResponse` e devolve o `code`.
pub async fn error_code(response: Response, expected_status: u16) -> String {
    let status = response.status().as_u16();
    let header_id = response.headers()["x-request-id"]
        .to_str()
        .unwrap()
        .to_owned();
    let body: serde_json::Value = response.json().await.expect("corpo JSON");
    // O corpo vai na mensagem: sem ele, um erro inesperado não diz qual foi.
    assert_eq!(status, expected_status, "corpo da resposta: {body}");
    assert_eq!(
        body["requestId"], header_id,
        "requestId do corpo e X-Request-Id divergem"
    );
    assert!(body["message"].is_string());
    body["code"].as_str().expect("code").to_owned()
}
