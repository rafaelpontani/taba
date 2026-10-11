//! O transporte: leva os bytes da requisição ao servidor e traz a resposta. O núcleo (montar e ler
//! HTTP) está em `http`; aqui só há TCP e TLS (`rustls`, TLS 1.2 ou superior).

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
    /// Para abrir a conexão.
    pub connect: Duration,
    /// Total da chamada, da conexão aberta até a resposta lida.
    pub total: Duration,
}

/// Por que a chamada falhou, e se o servidor pode ter recebido o pedido.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportError {
    /// Falha antes de enviar qualquer byte da requisição (DNS, recusa, handshake de TLS).
    Connect(String),
    /// O tempo acabou; `sent` diz se a requisição já tinha sido escrita.
    Timeout { sent: bool },
    /// Falha de rede depois de começar a enviar.
    Io(String),
    /// A resposta não é HTTP válido.
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

/// TCP puro ou com `rustls`. Uma conexão por chamada.
pub struct TcpTransport {
    ca_pem: Option<Vec<u8>>,
    tls: Mutex<Option<Arc<ClientConfig>>>,
}

impl TcpTransport {
    /// `ca_pem` é um certificado de CA (PEM) a confiar, além das raízes do sistema.
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

/// A configuração de TLS: as raízes do sistema mais a CA dada. Nunca desliga a verificação do
/// certificado (UC-CNT-030).
pub fn build_tls_config(ca_pem: Option<&[u8]>) -> Result<ClientConfig, String> {
    let mut roots = RootCertStore::empty();
    for cert in rustls_native_certs::load_native_certs().certs {
        let _ = roots.add(cert);
    }
    if let Some(pem) = ca_pem {
        let mut found = 0;
        for cert in CertificateDer::pem_slice_iter(pem) {
            let cert = cert.map_err(|e| format!("certificado de CA inválido: {e}"))?;
            roots
                .add(cert)
                .map_err(|e| format!("certificado de CA recusado: {e}"))?;
            found += 1;
        }
        if found == 0 {
            return Err("o arquivo de CA não tem nenhum certificado PEM".into());
        }
    }
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let builder = ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13, &rustls::version::TLS12])
        .map_err(|e| e.to_string())?;
    Ok(builder.with_root_certificates(roots).with_no_client_auth())
}

/// Lê e escreve com um prazo único para a chamada inteira.
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
        .map_err(|e| TransportError::Connect(format!("não achou {}: {e}", url.host)))?;
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
        Some(e) => format!("não conectou a {}:{}: {e}", url.host, url.port),
        None => format!("{} não tem endereço", url.host),
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
                TransportError::Connect(format!("`{}` não é um nome de servidor válido", url.host))
            })?;
            let mut conn = ClientConnection::new(self.tls_config()?, name)
                .map_err(|e| TransportError::Connect(e.to_string()))?;
            // O handshake termina antes de qualquer byte da requisição: falhar aqui é "não enviou".
            while conn.is_handshaking() {
                conn.complete_io(&mut stream).map_err(|e| {
                    if is_timeout(&e) {
                        TransportError::Timeout { sent: false }
                    } else {
                        TransportError::Connect(format!("falha de TLS: {e}"))
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
            TransportError::Io(format!("falha ao enviar: {e}"))
        });
    }
    let mut reader = BufReader::new(stream);
    match http::read_response(&mut reader) {
        Ok(response) => Ok(response),
        Err(ReadError::Io { error, .. }) if is_timeout(&error) => {
            Err(TransportError::Timeout { sent: true })
        }
        Err(ReadError::Io { error, .. }) => Err(TransportError::Io(format!(
            "falha ao ler a resposta: {error}"
        ))),
        Err(ReadError::Malformed(why)) => Err(TransportError::Protocol(why.to_owned())),
    }
}

#[cfg(test)]
mod tests;
