//! O cluster de verdade dos testes de segurança (doc 20c, UC-SEG): nós no mesmo processo, mas
//! ligados por TCP em `127.0.0.x` com o `Transport` HTTP e, se houver pasta, TLS mútuo. Cada nó
//! tem o próprio IP de loopback, porque o cluster reconhece o chamador por ele.
//!
//! Também traz um cliente TLS cru, para ver o que a porta do cluster aceita no handshake.

#![allow(dead_code)]

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, DigitallySignedStruct, SignatureScheme};
use tempfile::TempDir;
use time::OffsetDateTime;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use taba_node::cluster::tls::TlsStore;
use taba_node::secrets::generate_cert;

use super::TestNode;
use super::cluster::{FAST, eventually, wait_joined};

/// O IP do nó `number` (a partir de 1).
pub fn ip_of(number: usize) -> String {
    format!("127.0.0.{}", 10 + number)
}

/// Uma pasta de certificados nova, com um par que vale desde já por 30 dias.
pub fn cert_dir() -> TempDir {
    let dir = tempfile::tempdir().expect("pasta temporária");
    new_pair(dir.path(), None, 30);
    dir
}

/// Gera um par e devolve o caminho do `.crt`. O nome traz a data de início, então um segundo par
/// do mesmo dia é renomeado para `name` (se informado).
pub fn new_pair(dir: &Path, starts_in: Option<Duration>, days: u64) -> PathBuf {
    generate_cert(dir, OffsetDateTime::now_utc(), days, starts_in)
        .expect("gerar certificado")
        .certificate
}

/// Gera um par em uma pasta à parte e o copia para `dir` com o nome `name` (`<name>.crt` e `.key`).
pub fn add_named_pair(dir: &Path, name: &str, starts_in: Option<Duration>, days: u64) -> Vec<u8> {
    let scratch = tempfile::tempdir().expect("pasta temporária");
    let files = generate_cert(scratch.path(), OffsetDateTime::now_utc(), days, starts_in)
        .expect("gerar certificado");
    std::fs::copy(&files.certificate, dir.join(format!("{name}.crt"))).unwrap();
    std::fs::copy(&files.key, dir.join(format!("{name}.key"))).unwrap();
    std::fs::read(&files.certificate).unwrap()
}

/// Copia só o `.crt` de todos os pares de `from` para `to` (para confiar neles sem apresentá-los).
pub fn trust_certs_of(from: &Path, to: &Path) {
    for entry in std::fs::read_dir(from).unwrap().flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "crt") {
            let name = format!("confiar-{}", path.file_name().unwrap().to_string_lossy());
            std::fs::copy(&path, to.join(name)).unwrap();
        }
    }
}

pub struct RealCluster {
    pub nodes: Vec<TestNode>,
    pub tls_dir: Option<PathBuf>,
}

impl RealCluster {
    /// Sobe `count` nós, um depois do outro, cada um só depois de o anterior estar no cluster.
    pub async fn start(count: usize, tls_dir: Option<&Path>) -> Self {
        taba_node::telemetry::init_logging(Some("info"), taba_node::config::LogFormat::Text);
        let mut cluster = Self {
            nodes: Vec::new(),
            tls_dir: tls_dir.map(Path::to_owned),
        };
        for number in 1..=count {
            cluster.add_node(&[]).await;
            assert_eq!(cluster.nodes.len(), number);
        }
        cluster
    }

    /// Sobe o próximo nó e espera ele entrar. O primeiro é o `cluster.first-node`.
    pub async fn add_node(&mut self, env: &[(&str, &str)]) -> &TestNode {
        let number = self.nodes.len() + 1;
        let entry = match self.nodes.first() {
            Some(first) => first
                .node
                .cluster_addr()
                .expect("porta do cluster")
                .to_string(),
            None => "127.0.0.1:1".to_owned(),
        };
        let mut builder = TestNode::builder().real(&ip_of(number), &entry);
        for (name, value) in FAST {
            builder = builder.env(name, value);
        }
        if let Some(dir) = &self.tls_dir {
            builder = builder.env("TABA_CLUSTER_TLS_DIR", &dir.display().to_string());
        }
        if number == 1 {
            builder = builder.env("TABA_CLUSTER_FIRST_NODE", "true");
        }
        for (name, value) in env {
            builder = builder.env(name, value);
        }
        let node = builder.try_start().await.expect("o nó deveria subir");
        self.nodes.push(node);
        wait_joined(self.nodes.last().unwrap()).await;
        self.nodes.last().unwrap()
    }

    pub fn node(&self, number: usize) -> &TestNode {
        &self.nodes[number - 1]
    }

    /// O endereço da porta do cluster do nó.
    pub fn cluster_addr(&self, number: usize) -> SocketAddr {
        self.node(number)
            .node
            .cluster_addr()
            .expect("porta do cluster")
    }

    /// Espera todos os nós enxergarem `size` membros.
    pub async fn settled(&self, size: usize) {
        let nodes: Vec<_> = self
            .nodes
            .iter()
            .map(|n| n.node.cluster().cloned())
            .collect();
        eventually("a tabela de membros igual em todos", move || {
            nodes.iter().all(|c| {
                c.as_ref()
                    .and_then(|c| c.membership().table())
                    .is_some_and(|t| t.members.len() == size)
            })
        })
        .await;
    }

    pub async fn shutdown(self) {
        for node in self.nodes {
            node.shutdown().await;
        }
    }
}

// ---- cliente TLS cru ----

/// Aceita qualquer certificado do servidor (o teste quer ver o que o servidor exige de quem chama).
#[derive(Debug)]
struct AcceptAny(rustls::crypto::WebPkiSupportedAlgorithms);

impl ServerCertVerifier for AcceptAny {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.0)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.0)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.supported_schemes()
    }
}

/// Um cliente sem certificado nenhum, que confia em qualquer servidor.
pub fn anonymous_client() -> ClientConfig {
    let provider = rustls::crypto::ring::default_provider();
    let algorithms = provider.signature_verification_algorithms;
    ClientConfig::builder_with_provider(Arc::new(provider))
        .with_safe_default_protocol_versions()
        .unwrap()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(AcceptAny(algorithms)))
        .with_no_client_auth()
}

/// Um cliente com os certificados de `dir` (apresenta o par válido e confia no que a pasta traz).
pub fn client_of(dir: &Path) -> (ClientConfig, Arc<TlsStore>) {
    let store = TlsStore::open(dir).expect("pasta de certificados válida");
    (store.client_config().expect("configuração TLS"), store)
}

/// O que um `GET` pela conexão TLS devolveu.
#[derive(Debug)]
pub struct Reply {
    pub status: u16,
    pub body: String,
    /// O DER do certificado que o servidor apresentou.
    pub server_cert: Vec<u8>,
}

/// Uma conexão TLS aberta (HTTP/1.1, `keep-alive`).
pub struct Connection {
    stream: tokio_rustls::client::TlsStream<TcpStream>,
    pub server_cert: Vec<u8>,
}

/// Abre a conexão e faz o handshake. Com TLS 1.3, o servidor só recusa o certificado do cliente
/// depois do handshake do lado do cliente: quem testa a recusa deve fazer uma requisição.
pub async fn connect(addr: SocketAddr, config: ClientConfig) -> Result<Connection, String> {
    let connector = tokio_rustls::TlsConnector::from(Arc::new(config));
    let tcp = TcpStream::connect(addr).await.map_err(|e| e.to_string())?;
    let name = ServerName::try_from(addr.ip().to_string()).map_err(|e| e.to_string())?;
    let stream = tokio::time::timeout(Duration::from_secs(5), connector.connect(name, tcp))
        .await
        .map_err(|_| "handshake sem resposta".to_owned())?
        .map_err(|e| e.to_string())?;
    let server_cert = stream
        .get_ref()
        .1
        .peer_certificates()
        .and_then(|c| c.first())
        .map(|c| c.as_ref().to_vec())
        .unwrap_or_default();
    Ok(Connection {
        stream,
        server_cert,
    })
}

impl Connection {
    /// Um `GET path` pela conexão aberta.
    pub async fn get(&mut self, path: &str) -> Result<Reply, String> {
        let request =
            format!("GET {path} HTTP/1.1\r\nHost: cluster\r\nConnection: keep-alive\r\n\r\n");
        self.stream
            .write_all(request.as_bytes())
            .await
            .map_err(|e| e.to_string())?;
        let mut data = Vec::new();
        let mut chunk = [0u8; 4096];
        let (head_end, length) = loop {
            let n = tokio::time::timeout(Duration::from_secs(5), self.stream.read(&mut chunk))
                .await
                .map_err(|_| "sem resposta".to_owned())?
                .map_err(|e| e.to_string())?;
            if n == 0 {
                return Err("conexão fechada sem resposta".into());
            }
            data.extend_from_slice(&chunk[..n]);
            if let Some(end) = find(&data, b"\r\n\r\n") {
                let head = String::from_utf8_lossy(&data[..end]).to_lowercase();
                let length = head
                    .lines()
                    .find_map(|l| l.strip_prefix("content-length:"))
                    .and_then(|v| v.trim().parse::<usize>().ok())
                    .unwrap_or(0);
                break (end + 4, length);
            }
        };
        while data.len() < head_end + length {
            let n = self
                .stream
                .read(&mut chunk)
                .await
                .map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            data.extend_from_slice(&chunk[..n]);
        }
        let head = String::from_utf8_lossy(&data[..head_end]).into_owned();
        let status = head
            .split_whitespace()
            .nth(1)
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| format!("resposta ilegível: {head}"))?;
        Ok(Reply {
            status,
            body: String::from_utf8_lossy(&data[head_end..]).into_owned(),
            server_cert: self.server_cert.clone(),
        })
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Abre uma conexão e faz um `GET`; `Err` se o handshake ou a requisição falham.
pub async fn get(addr: SocketAddr, config: ClientConfig, path: &str) -> Result<Reply, String> {
    connect(addr, config).await?.get(path).await
}
