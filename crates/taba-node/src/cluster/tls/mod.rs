//! The mutual TLS of the cluster port (doc 06, section 8.0.1). Trust is that of the certificates
//! themselves (*pinning*), read from **one folder** with several pairs at once, reloaded
//! by itself:
//!
//! - **Whom the node accepts:** whoever presents a certificate equal to one in the folder, or issued by one
//!   of them (a CA put in the folder), as long as the folder's certificate and the presented one have not
//!   expired (`notAfter`). The start date does not count, so a node accepts the new certificate
//!   before anyone starts presenting it. The host name is not verified.
//! - **Which one the node presents:** the pair with the latest start that has already become valid and has not
//!   expired, chosen on each handshake (so rotation restarts nothing).
//! - **Reload:** the whole folder or nothing. An unreadable file, an incomplete pair or an invalid certificate
//!   keep the previous set (and become `error` in the log).
//!
//! The same [`TlsStore`] serves the cluster port ([`TlsStore::server_config`]) and the outgoing
//! calls ([`TlsStore::client_config`]); since the certificate is chosen at the handshake, both
//! ends follow the reload.

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, WebPkiSupportedAlgorithms};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName, UnixTime};
use rustls::server::danger::{ClientCertVerified, ClientCertVerifier};
use rustls::sign::CertifiedKey;
use rustls::{ClientConfig, DigitallySignedStruct, DistinguishedName, Error, ServerConfig};
use rustls::{SignatureScheme, client::ResolvesClientCert, server::ResolvesServerCert};
use tokio_util::sync::CancellationToken;
use x509_parser::prelude::{FromDer, X509Certificate};

/// How often the expiry warning repeats.
const WARNING_EVERY: Duration = Duration::from_secs(3600);

/// How many intermediate certificates follow the chain of a presented certificate.
const MAX_CHAIN: usize = 4;

/// A certificate in the folder.
pub(crate) struct Entry {
    name: String,
    der: CertificateDer<'static>,
    not_before: i64,
    not_after: i64,
    /// The complete pair (chain and key), if the folder has its `.key`.
    key: Option<Arc<CertifiedKey>>,
}

/// The folder's content at an instant.
pub struct CertSet {
    entries: Vec<Entry>,
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

fn parse<'a>(der: &'a CertificateDer<'_>) -> Result<X509Certificate<'a>, String> {
    X509Certificate::from_der(der.as_ref())
        .map(|(_, cert)| cert)
        .map_err(|e| format!("unreadable certificate: {e}"))
}

impl CertSet {
    /// Reads the whole folder. Any problem is an error: the set only changes when everything is valid, and the
    /// start requires at least one pair.
    pub fn load(dir: &Path, provider: &CryptoProvider) -> Result<Self, String> {
        let mut crts: Vec<PathBuf> = Vec::new();
        let mut keys: Vec<PathBuf> = Vec::new();
        let listing = std::fs::read_dir(dir)
            .map_err(|e| format!("could not read `{}`: {e}", dir.display()))?;
        for item in listing {
            let path = item
                .map_err(|e| format!("could not read `{}`: {e}", dir.display()))?
                .path();
            if !path.is_file() {
                continue;
            }
            match path.extension().and_then(|e| e.to_str()) {
                Some("crt") => crts.push(path),
                Some("key") => keys.push(path),
                _ => {}
            }
        }
        crts.sort();
        // A `.key` without the `.crt` of the same name is an incomplete pair.
        for key in &keys {
            if !key.with_extension("crt").is_file() {
                return Err(format!(
                    "incomplete pair: `{}` has no .crt of the same name",
                    key.display()
                ));
            }
        }
        let mut entries = Vec::new();
        for crt in &crts {
            let name = crt
                .file_stem()
                .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
            let chain = CertificateDer::pem_file_iter(crt)
                .map_err(|e| format!("`{}` unreadable: {e}", crt.display()))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| format!("`{}` unreadable: {e}", crt.display()))?;
            let Some(der) = chain.first().cloned() else {
                return Err(format!("`{}` has no certificate", crt.display()));
            };
            let (not_before, not_after) = {
                let cert = parse(&der).map_err(|e| format!("`{}`: {e}", crt.display()))?;
                (
                    cert.validity().not_before.timestamp(),
                    cert.validity().not_after.timestamp(),
                )
            };
            let key_path = crt.with_extension("key");
            let key = if key_path.is_file() {
                let private = PrivateKeyDer::from_pem_file(&key_path)
                    .map_err(|e| format!("`{}` unreadable: {e}", key_path.display()))?;
                let signing = provider
                    .key_provider
                    .load_private_key(private)
                    .map_err(|e| format!("`{}` invalid: {e}", key_path.display()))?;
                let certified = CertifiedKey::new(chain, signing);
                certified.keys_match().map_err(|e| {
                    format!(
                        "`{}` is not the key of `{}`: {e}",
                        key_path.display(),
                        crt.display()
                    )
                })?;
                Some(Arc::new(certified))
            } else {
                None
            };
            entries.push(Entry {
                name,
                der,
                not_before,
                not_after,
                key,
            });
        }
        if !entries.iter().any(|e| e.key.is_some()) {
            return Err(format!(
                "`{}` has no <name>.crt + <name>.key pair",
                dir.display()
            ));
        }
        Ok(Self { entries })
    }

    /// Is the certificate `cert` one of those the node accepts? Equal to one in the folder or issued by one of them
    /// (directly or through intermediates), without being expired (neither it, nor the folder's).
    pub fn trusts(
        &self,
        cert: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        now: i64,
    ) -> bool {
        let mut current = cert.clone();
        for _ in 0..=MAX_CHAIN {
            let Ok(parsed) = parse(&current) else {
                return false;
            };
            if parsed.validity().not_after.timestamp() <= now {
                return false;
            }
            let pinned = self
                .entries
                .iter()
                .filter(|e| e.not_after > now)
                .any(|e| e.der.as_ref() == current.as_ref() || self.issued_by(&parsed, e));
            if pinned {
                return true;
            }
            // It is not from the folder nor issued by one of its own: goes on through the intermediate that issued it.
            let next = intermediates.iter().find(|candidate| {
                parse(candidate).is_ok_and(|issuer| {
                    issuer.subject().as_raw() == parsed.issuer().as_raw()
                        && parsed.verify_signature(Some(issuer.public_key())).is_ok()
                })
            });
            match next {
                Some(next) => current = next.clone().into_owned(),
                None => return false,
            }
        }
        false
    }

    fn issued_by(&self, cert: &X509Certificate<'_>, issuer: &Entry) -> bool {
        parse(&issuer.der).is_ok_and(|issuer| {
            issuer.subject().as_raw() == cert.issuer().as_raw()
                && cert.verify_signature(Some(issuer.public_key())).is_ok()
        })
    }

    /// The pair the node presents: the one with the latest start that has already become valid and has not
    /// expired.
    pub(crate) fn presented(&self, now: i64) -> Option<&Entry> {
        self.entries
            .iter()
            .filter(|e| e.key.is_some() && e.not_before <= now && e.not_after > now)
            .max_by_key(|e| e.not_before)
    }

    /// The names of the folder's pairs (for the logs and the tests).
    pub fn names(&self) -> Vec<&str> {
        self.entries.iter().map(|e| e.name.as_str()).collect()
    }
}

/// The certificates folder and what is taken from it, with the reload.
pub struct TlsStore {
    dir: PathBuf,
    provider: Arc<CryptoProvider>,
    set: RwLock<Arc<CertSet>>,
}

impl std::fmt::Debug for TlsStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "TlsStore({})", self.dir.display())
    }
}

fn provider() -> Arc<CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

impl TlsStore {
    /// Reads the folder for the first time; without at least one valid pair, it is an error (the node does not start).
    pub fn open(dir: &Path) -> Result<Arc<Self>, String> {
        let provider = provider();
        let set = CertSet::load(dir, &provider)?;
        Ok(Arc::new(Self {
            dir: dir.to_owned(),
            provider,
            set: RwLock::new(Arc::new(set)),
        }))
    }

    fn current(&self) -> Arc<CertSet> {
        self.set.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Reloads the folder. If it is not entirely valid, it keeps the previous set and returns the error.
    pub fn reload(&self) -> Result<(), String> {
        let set = CertSet::load(&self.dir, &self.provider)?;
        *self.set.write().unwrap_or_else(|e| e.into_inner()) = Arc::new(set);
        Ok(())
    }

    /// How many seconds remain until the in-use certificate's `notAfter`; `None` without one.
    pub fn expires_in(&self) -> Option<i64> {
        let now = unix_now();
        self.current()
            .presented(now)
            .map(|entry| entry.not_after - now)
    }

    /// The DER of the certificate the node would present now (for the tests).
    pub fn presented_der(&self) -> Option<Vec<u8>> {
        self.current()
            .presented(unix_now())
            .map(|e| e.der.as_ref().to_vec())
    }

    /// Re-evaluates the `interval` folder every `interval` and warns when the in-use certificate is
    /// within `expiry_warning` of expiring, until `cancel`.
    pub async fn run(
        self: Arc<Self>,
        interval: Duration,
        expiry_warning: Duration,
        cancel: CancellationToken,
    ) {
        let mut ticker = tokio::time::interval(interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        ticker.tick().await;
        let mut last_warning: Option<std::time::Instant> = None;
        loop {
            // The warning repeats at most hourly, not on every reload.
            if last_warning.is_none_or(|at| at.elapsed() >= WARNING_EVERY)
                && self.check_expiry(expiry_warning)
            {
                last_warning = Some(std::time::Instant::now());
            }
            tokio::select! {
                () = cancel.cancelled() => return,
                _ = ticker.tick() => {
                    match self.reload() {
                        Ok(()) => {}
                        Err(why) => tracing::error!(
                            dir = %self.dir.display(),
                            "cluster certificate reload refused; the previous set applies: {why}"
                        ),
                    }
                }
            }
        }
    }

    /// Warns if the in-use certificate is within `warning` of expiring (or if none is in use).
    /// Returns whether it warned.
    fn check_expiry(&self, warning: Duration) -> bool {
        match self.expires_in() {
            Some(left) if left <= i64::try_from(warning.as_secs()).unwrap_or(i64::MAX) => {
                tracing::warn!(
                    expires_in_secs = left,
                    "the in-use cluster certificate is about to expire: generate a new one (taba cert generate)"
                );
                true
            }
            Some(_) => false,
            None => {
                tracing::error!(
                    "no cluster certificate is valid right now: the node presents no certificate"
                );
                true
            }
        }
    }

    /// The configuration of whoever **receives** on the cluster port: demands the other side's certificate.
    pub fn server_config(self: &Arc<Self>) -> Result<Arc<ServerConfig>, String> {
        let mut config = ServerConfig::builder_with_provider(self.provider.clone())
            .with_safe_default_protocol_versions()
            .map_err(|e| e.to_string())?
            .with_client_cert_verifier(Arc::new(Verifier(self.clone())))
            .with_cert_resolver(Arc::new(Resolver(self.clone())));
        config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
        Ok(Arc::new(config))
    }

    /// The configuration of whoever **opens** a connection: trusts the certificates in the folder (without checking
    /// the host name) and presents its own.
    pub fn client_config(self: &Arc<Self>) -> Result<ClientConfig, String> {
        let config = ClientConfig::builder_with_provider(self.provider.clone())
            .with_safe_default_protocol_versions()
            .map_err(|e| e.to_string())?
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(Verifier(self.clone())))
            .with_client_cert_resolver(Arc::new(Resolver(self.clone())));
        Ok(config)
    }

    fn algorithms(&self) -> WebPkiSupportedAlgorithms {
        self.provider.signature_verification_algorithms
    }

    fn accept(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
    ) -> Result<(), Error> {
        if self.current().trusts(end_entity, intermediates, unix_now()) {
            Ok(())
        } else {
            Err(Error::InvalidCertificate(
                rustls::CertificateError::UnknownIssuer,
            ))
        }
    }
}

/// The verifier of both ends: accepts what the folder accepts.
#[derive(Debug)]
struct Verifier(Arc<TlsStore>);

impl ServerCertVerifier for Verifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, Error> {
        self.0.accept(end_entity, intermediates)?;
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.0.algorithms())
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.0.algorithms())
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.algorithms().supported_schemes()
    }
}

impl ClientCertVerifier for Verifier {
    fn root_hint_subjects(&self) -> &[DistinguishedName] {
        &[]
    }

    fn verify_client_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        _now: UnixTime,
    ) -> Result<ClientCertVerified, Error> {
        self.0.accept(end_entity, intermediates)?;
        Ok(ClientCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.0.algorithms())
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.0.algorithms())
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.algorithms().supported_schemes()
    }
}

/// Picks the pair to present on each handshake.
#[derive(Debug)]
struct Resolver(Arc<TlsStore>);

impl Resolver {
    fn pick(&self) -> Option<Arc<CertifiedKey>> {
        self.0
            .current()
            .presented(unix_now())
            .and_then(|entry| entry.key.clone())
    }
}

impl ResolvesServerCert for Resolver {
    fn resolve(&self, _hello: rustls::server::ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        self.pick()
    }
}

impl ResolvesClientCert for Resolver {
    fn resolve(
        &self,
        _root_hint_subjects: &[&[u8]],
        _sigschemes: &[SignatureScheme],
    ) -> Option<Arc<CertifiedKey>> {
        self.pick()
    }

    fn has_certs(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests;
