//! The transport: carries the request bytes to the server and brings the response back. The core (building and reading
//! HTTP) is in `http`; here there is only TCP and TLS (`rustls`, TLS 1.2 or later).

use std::io::{self, BufReader, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, ServerName};
use rustls::{ClientConfig, ClientConnection, RootCertStore, StreamOwned};

use crate::http::{self, ReadError, Request, Response, Url};

#[derive(Debug, Clone, Copy)]
pub struct Timeouts {
    /// To open the connection.
    pub connect: Duration,
    /// Total of the call, from the open connection until the response is read.
    pub total: Duration,
}

/// Why the call failed, and whether the server may have received the request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportError {
    /// Failure before sending any byte of the request (DNS, refusal, TLS handshake).
    Connect(String),
    /// Time ran out; `sent` says whether the request had already been written.
    Timeout { sent: bool },
    /// Network failure after starting to send.
    Io(String),
    /// The response is not valid HTTP.
    Protocol(String),
}

pub trait Transport: Send + Sync {
    fn send(
        &self,
        url: &Url,
        request: &Request,
        timeouts: &Timeouts,
    ) -> Result<Response, TransportError>;
}

/// Plain TCP or with `rustls`. One connection per call.
pub struct TcpTransport {
    ca_pem: Option<Vec<u8>>,
    tls: Mutex<Option<Arc<ClientConfig>>>,
}

impl TcpTransport {
    /// `ca_pem` is a CA certificate (PEM) to trust, besides the system roots.
    pub fn new(ca_pem: Option<Vec<u8>>) -> Self {
        TcpTransport {
            ca_pem,
            tls: Mutex::new(None),
        }
    }

    fn tls_config(&self) -> Result<Arc<ClientConfig>, TransportError> {
        let mut cached = self.tls.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(config) = cached.as_ref() {
            return Ok(config.clone());
        }
        let config =
            Arc::new(build_tls_config(self.ca_pem.as_deref()).map_err(TransportError::Connect)?);
        *cached = Some(config.clone());
        Ok(config)
    }
}

/// The TLS configuration: the system roots plus the given CA. It never turns certificate
/// verification off (UC-CNT-030).
pub fn build_tls_config(ca_pem: Option<&[u8]>) -> Result<ClientConfig, String> {
    let mut roots = RootCertStore::empty();
    for cert in rustls_native_certs::load_native_certs().certs {
        let _ = roots.add(cert);
    }
    if let Some(pem) = ca_pem {
        let mut found = 0;
        for cert in CertificateDer::pem_slice_iter(pem) {
            let cert = cert.map_err(|e| format!("invalid CA certificate: {e}"))?;
            roots
                .add(cert)
                .map_err(|e| format!("CA certificate refused: {e}"))?;
            found += 1;
        }
        if found == 0 {
            return Err("the CA file has no PEM certificate".into());
        }
    }
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let builder = ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13, &rustls::version::TLS12])
        .map_err(|e| e.to_string())?;
    Ok(builder.with_root_certificates(roots).with_no_client_auth())
}

/// Reads and writes with a single deadline for the whole call.
struct Deadlined {
    sock: TcpStream,
    deadline: Instant,
}

impl Deadlined {
    fn remaining(&self) -> io::Result<Duration> {
        let left = self.deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            Err(io::ErrorKind::TimedOut.into())
        } else {
            Ok(left)
        }
    }
}

impl Read for Deadlined {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.sock.set_read_timeout(Some(self.remaining()?))?;
        self.sock.read(buf)
    }
}

impl Write for Deadlined {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.sock.set_write_timeout(Some(self.remaining()?))?;
        self.sock.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.sock.flush()
    }
}

fn is_timeout(e: &io::Error) -> bool {
    matches!(
        e.kind(),
        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
    )
}

fn connect(url: &Url, timeouts: &Timeouts) -> Result<TcpStream, TransportError> {
    let addrs = (url.host.as_str(), url.port)
        .to_socket_addrs()
        .map_err(|e| TransportError::Connect(format!("could not find {}: {e}", url.host)))?;
    let mut last = None;
    for addr in addrs {
        match TcpStream::connect_timeout(&addr, timeouts.connect) {
            Ok(sock) => {
                let _ = sock.set_nodelay(true);
                return Ok(sock);
            }
            Err(e) if is_timeout(&e) => return Err(TransportError::Timeout { sent: false }),
            Err(e) => last = Some(e),
        }
    }
    Err(TransportError::Connect(match last {
        Some(e) => format!("could not connect to {}:{}: {e}", url.host, url.port),
        None => format!("{} has no address", url.host),
    }))
}

impl Transport for TcpTransport {
    fn send(
        &self,
        url: &Url,
        request: &Request,
        timeouts: &Timeouts,
    ) -> Result<Response, TransportError> {
        let sock = connect(url, timeouts)?;
        let mut stream = Deadlined {
            sock,
            deadline: Instant::now() + timeouts.total,
        };
        let bytes = http::encode_request(&url.host_header(), request);
        if url.https {
            let name = ServerName::try_from(url.host.clone()).map_err(|_| {
                TransportError::Connect(format!("`{}` is not a valid server name", url.host))
            })?;
            let mut conn = ClientConnection::new(self.tls_config()?, name)
                .map_err(|e| TransportError::Connect(e.to_string()))?;
            // The handshake ends before any byte of the request: failing here is "did not send".
            while conn.is_handshaking() {
                conn.complete_io(&mut stream).map_err(|e| {
                    if is_timeout(&e) {
                        TransportError::Timeout { sent: false }
                    } else {
                        TransportError::Connect(format!("TLS failure: {e}"))
                    }
                })?;
            }
            exchange(StreamOwned::new(conn, stream), &bytes)
        } else {
            exchange(stream, &bytes)
        }
    }
}

fn exchange<S: Read + Write>(mut stream: S, bytes: &[u8]) -> Result<Response, TransportError> {
    if let Err(e) = stream.write_all(bytes).and_then(|_| stream.flush()) {
        return Err(if is_timeout(&e) {
            TransportError::Timeout { sent: true }
        } else {
            TransportError::Io(format!("failed to send: {e}"))
        });
    }
    let mut reader = BufReader::new(stream);
    match http::read_response(&mut reader) {
        Ok(response) => Ok(response),
        Err(ReadError::Io { error, .. }) if is_timeout(&error) => {
            Err(TransportError::Timeout { sent: true })
        }
        Err(ReadError::Io { error, .. }) => Err(TransportError::Io(format!(
            "failed to read the response: {error}"
        ))),
        Err(ReadError::Malformed(why)) => Err(TransportError::Protocol(why.to_owned())),
    }
}

#[cfg(test)]
mod tests;
