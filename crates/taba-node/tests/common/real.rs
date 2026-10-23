//! The real cluster of the security tests (doc 20c, UC-SEG): nodes in the same process, but
//! linked by TCP on `127.0.0.x` with the HTTP `Transport` and, if there is a folder, mutual TLS. Each node
//! has its own loopback IP, because the cluster recognizes the caller by it.
//!
//! It also brings a raw TLS client, to see what the cluster port accepts in the handshake.

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

/// The IP of node `number` (starting at 1).
pub fn ip_of(number: usize) -> String {
    format!("127.0.0.{}", 10 + number)
}

/// A new certificate folder, with a pair valid from now for 30 days.
pub fn cert_dir() -> TempDir {
    let dir = tempfile::tempdir().expect("temporary folder");
    new_pair(dir.path(), None, 30);
    dir
}

/// Generates a pair and returns the path of the `.crt`. The name carries the start date, so a second pair
/// of the same day is renamed to `name` (if given).
pub fn new_pair(dir: &Path, starts_in: Option<Duration>, days: u64) -> PathBuf {
    generate_cert(dir, OffsetDateTime::now_utc(), days, starts_in)
        .expect("gerar certificado")
        .certificate
}

/// Generates a pair in a separate folder and copies it to `dir` with the name `name` (`<name>.crt` and `.key`).
pub fn add_named_pair(dir: &Path, name: &str, starts_in: Option<Duration>, days: u64) -> Vec<u8> {
    let scratch = tempfile::tempdir().expect("temporary folder");
    let files = generate_cert(scratch.path(), OffsetDateTime::now_utc(), days, starts_in)
        .expect("gerar certificado");
    std::fs::copy(&files.certificate, dir.join(format!("{name}.crt"))).unwrap();
    std::fs::copy(&files.key, dir.join(format!("{name}.key"))).unwrap();
    std::fs::read(&files.certificate).unwrap()
}

/// Copies only the `.crt` of all the pairs from `from` to `to` (to trust them without presenting them).
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
    /// Starts `count` nodes, one after the other, each only after the previous one is in the cluster.
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

    /// Starts the next node and waits for it to join. The first is the `cluster.first-node`.
    pub async fn add_node(&mut self, env: &[(&str, &str)]) -> &TestNode {
        let number = self.nodes.len() + 1;
        let entry = match self.nodes.first() {
            Some(first) => first.node.cluster_addr().to_string(),
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
        let node = builder.try_start().await.expect("the node should start");
        self.nodes.push(node);
        wait_joined(self.nodes.last().unwrap()).await;
        self.nodes.last().unwrap()
    }

    pub fn node(&self, number: usize) -> &TestNode {
        &self.nodes[number - 1]
    }

    /// The address of the node's cluster port.
    pub fn cluster_addr(&self, number: usize) -> SocketAddr {
        self.node(number).node.cluster_addr()
    }

    /// Waits for all the nodes to see `size` members.
    pub async fn settled(&self, size: usize) {
        let nodes: Vec<_> = self
            .nodes
            .iter()
            .map(|n| n.node.cluster().cloned())
            .collect();
        eventually("the member table equal on all", move || {
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

// ---- raw TLS client ----

/// Accepts any server certificate (the test wants to see what the server demands of whoever calls).
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

/// A client with no certificate at all, which trusts any server.
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

/// A client with the certificates in `dir` (presents the valid pair and trusts what the folder brings).
pub fn client_of(dir: &Path) -> (ClientConfig, Arc<TlsStore>) {
    let store = TlsStore::open(dir).expect("valid certificate folder");
    (store.client_config().expect("TLS configuration"), store)
}

/// What a `GET` over the TLS connection returned.
#[derive(Debug)]
pub struct Reply {
    pub status: u16,
    pub body: String,
    /// The DER of the certificate the server presented.
    pub server_cert: Vec<u8>,
}

/// An open TLS connection (HTTP/1.1, `keep-alive`).
pub struct Connection {
    stream: tokio_rustls::client::TlsStream<TcpStream>,
    pub server_cert: Vec<u8>,
}

/// Opens the connection and does the handshake. With TLS 1.3, the server only refuses the client's certificate
/// after the handshake on the client side: whoever tests the refusal must make a request.
pub async fn connect(addr: SocketAddr, config: ClientConfig) -> Result<Connection, String> {
    let connector = tokio_rustls::TlsConnector::from(Arc::new(config));
    let tcp = TcpStream::connect(addr).await.map_err(|e| e.to_string())?;
    let name = ServerName::try_from(addr.ip().to_string()).map_err(|e| e.to_string())?;
    let stream = tokio::time::timeout(Duration::from_secs(5), connector.connect(name, tcp))
        .await
        .map_err(|_| "handshake without response".to_owned())?
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
    /// A `GET path` over the open connection.
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
                .map_err(|_| "no response".to_owned())?
                .map_err(|e| e.to_string())?;
            if n == 0 {
                return Err("connection closed without response".into());
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
            .ok_or_else(|| format!("unreadable response: {head}"))?;
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

/// Opens a connection and does a `GET`; `Err` if the handshake or the request fails.
pub async fn get(addr: SocketAddr, config: ClientConfig, path: &str) -> Result<Reply, String> {
    connect(addr, config).await?.get(path).await
}
