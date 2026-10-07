//! Base dos testes de integração de um nó (doc 20, seção 19.4, nível **I**): sobe um
//! nó de verdade, com disco real numa pasta temporária e HTTP real em portas livres.
//!
//! ```ignore
//! let node = TestNode::development().await;
//! let response = node.client_get("/health/live").await;
//! ```
//!
//! Os testes de cluster (nível **C**) vão acrescentar aqui um harness que sobe vários
//! nós no mesmo processo, com `Transport` em memória e `Clock` manual (doc 18, seção
//! 17.5). Ele depende do trait `Transport`, que entra com o protocolo de cluster.

#![allow(dead_code)] // cada arquivo de teste usa uma parte deste módulo

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;

use reqwest::{Client, Response};
use tempfile::TempDir;

use taba_node::config::{Config, MapEnv};
use taba_node::core::clock::ManualClock;
use taba_node::node::{self, Deps, NodeError, RunningNode};

pub struct TestNodeBuilder {
    env: Vec<(String, String)>,
}

impl TestNodeBuilder {
    /// Define um parâmetro como uma variável `TABA_*` (`("TABA_API_PORT", "0")`).
    pub fn env(mut self, name: &str, value: &str) -> Self {
        self.env.retain(|(n, _)| n != name);
        self.env.push((name.to_owned(), value.to_owned()));
        self
    }

    /// Modo produção. Ainda sem protocolo de cluster, então o nó fica em `bootstrapping`.
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
        let clock = Arc::new(ManualClock::at_doc_epoch());
        let deps = Deps {
            clock: clock.clone(),
            bind_ip: IpAddr::V4(Ipv4Addr::LOCALHOST),
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

    pub async fn shutdown(self) {
        self.node.shutdown().await;
    }
}

fn url(addr: SocketAddr, path: &str) -> String {
    format!("http://{addr}{path}")
}

/// Confere o corpo de um `ErrorResponse` e devolve o `code`.
pub async fn error_code(response: Response, expected_status: u16) -> String {
    assert_eq!(response.status().as_u16(), expected_status);
    let header_id = response.headers()["x-request-id"]
        .to_str()
        .unwrap()
        .to_owned();
    let body: serde_json::Value = response.json().await.expect("corpo JSON");
    assert_eq!(
        body["requestId"], header_id,
        "requestId do corpo e X-Request-Id divergem"
    );
    assert!(body["message"].is_string());
    body["code"].as_str().expect("code").to_owned()
}
