//! O TLS mútuo da porta do cluster (doc 06, seção 8.0.1). A confiança é a dos próprios
//! certificados (*pinning*), lidos de **uma pasta** com vários pares ao mesmo tempo, recarregada
//! sozinha:
//!
//! - **Quem o nó aceita:** quem apresenta um certificado igual a um da pasta, ou emitido por um
//!   deles (uma CA posta na pasta), desde que o certificado da pasta e o apresentado ainda não
//!   tenham expirado (`notAfter`). A data de início não conta, para um nó aceitar o certificado
//!   novo antes de qualquer um passar a apresentá-lo. O nome do host não é verificado.
//! - **Qual o nó apresenta:** o par de início mais recente que já começou a valer e ainda não
//!   expirou, escolhido a cada handshake (então a rotação não reinicia nada).
//! - **Recarga:** a pasta inteira ou nada. Arquivo ilegível, par incompleto ou certificado
//!   inválido mantêm o conjunto anterior (e viram `error` no log).
//!
//! O mesmo [`TlsStore`] serve a porta do cluster ([`TlsStore::server_config`]) e as chamadas de
//! saída ([`TlsStore::client_config`]); como o certificado é escolhido no handshake, as duas
//! pontas acompanham a recarga.

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

/// De quanto em quanto tempo o aviso de expiração se repete.
const WARNING_EVERY: Duration = Duration::from_secs(3600);

/// Quantos certificados intermediários seguem a cadeia de um certificado apresentado.
const MAX_CHAIN: usize = 4;

/// Um certificado da pasta.
pub(crate) struct Entry {
    name: String,
    der: CertificateDer<'static>,
    not_before: i64,
    not_after: i64,
    /// O par completo (cadeia e chave), se a pasta tem o `.key` dele.
    key: Option<Arc<CertifiedKey>>,
}

/// O conteúdo da pasta num instante.
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
        .map_err(|e| format!("certificado ilegível: {e}"))
}

impl CertSet {
    /// Lê a pasta inteira. Qualquer problema é erro: o conjunto só muda quando tudo é válido, e a
    /// partida exige ao menos um par.
    pub fn load(dir: &Path, provider: &CryptoProvider) -> Result<Self, String> {
        let mut crts: Vec<PathBuf> = Vec::new();
        let mut keys: Vec<PathBuf> = Vec::new();
        let listing = std::fs::read_dir(dir)
            .map_err(|e| format!("não foi possível ler `{}`: {e}", dir.display()))?;
        for item in listing {
            let path = item
                .map_err(|e| format!("não foi possível ler `{}`: {e}", dir.display()))?
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
        // Um `.key` sem o `.crt` do mesmo nome é um par incompleto.
        for key in &keys {
            if !key.with_extension("crt").is_file() {
                return Err(format!(
                    "par incompleto: `{}` não tem o .crt de mesmo nome",
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
                .map_err(|e| format!("`{}` ilegível: {e}", crt.display()))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| format!("`{}` ilegível: {e}", crt.display()))?;
            let Some(der) = chain.first().cloned() else {
                return Err(format!("`{}` não tem nenhum certificado", crt.display()));
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
                    .map_err(|e| format!("`{}` ilegível: {e}", key_path.display()))?;
                let signing = provider
                    .key_provider
                    .load_private_key(private)
                    .map_err(|e| format!("`{}` inválida: {e}", key_path.display()))?;
                let certified = CertifiedKey::new(chain, signing);
                certified.keys_match().map_err(|e| {
                    format!(
                        "`{}` não é a chave de `{}`: {e}",
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
                "`{}` não tem nenhum par <nome>.crt + <nome>.key",
                dir.display()
            ));
        }
        Ok(Self { entries })
    }

    /// O certificado `cert` é de quem o nó aceita? Igual a um da pasta ou emitido por um deles
    /// (direto ou por intermediários), sem estar expirado (nem ele, nem o da pasta).
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
            // Não é da pasta nem foi emitido por um dela: segue pelo intermediário que o emitiu.
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

    /// O par que o nó apresenta: o de início mais recente que já começou a valer e ainda não
    /// expirou.
    pub(crate) fn presented(&self, now: i64) -> Option<&Entry> {
        self.entries
            .iter()
            .filter(|e| e.key.is_some() && e.not_before <= now && e.not_after > now)
            .max_by_key(|e| e.not_before)
    }

    /// Os nomes dos pares da pasta (para os logs e os testes).
    pub fn names(&self) -> Vec<&str> {
        self.entries.iter().map(|e| e.name.as_str()).collect()
    }
}

/// A pasta de certificados e o que dela se tira, com a recarga.
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
    /// Lê a pasta pela primeira vez; sem ao menos um par válido, é erro (o nó não sobe).
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

    /// Relê a pasta. Se ela não está inteira válida, mantém o conjunto anterior e devolve o erro.
    pub fn reload(&self) -> Result<(), String> {
        let set = CertSet::load(&self.dir, &self.provider)?;
        *self.set.write().unwrap_or_else(|e| e.into_inner()) = Arc::new(set);
        Ok(())
    }

    /// Quantos segundos faltam para o `notAfter` do certificado em uso; `None` sem nenhum.
    pub fn expires_in(&self) -> Option<i64> {
        let now = unix_now();
        self.current()
            .presented(now)
            .map(|entry| entry.not_after - now)
    }

    /// O DER do certificado que o nó apresentaria agora (para os testes).
    pub fn presented_der(&self) -> Option<Vec<u8>> {
        self.current()
            .presented(unix_now())
            .map(|e| e.der.as_ref().to_vec())
    }

    /// Reavalia a pasta de `interval` em `interval` e avisa quando o certificado em uso está a
    /// `expiry_warning` de expirar, até `cancel`.
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
            // O aviso se repete no máximo de hora em hora, não a cada recarga.
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
                            "recarga dos certificados do cluster recusada; vale o conjunto anterior: {why}"
                        ),
                    }
                }
            }
        }
    }

    /// Avisa se o certificado em uso está a `warning` de expirar (ou se não há nenhum em uso).
    /// Devolve se avisou.
    fn check_expiry(&self, warning: Duration) -> bool {
        match self.expires_in() {
            Some(left) if left <= i64::try_from(warning.as_secs()).unwrap_or(i64::MAX) => {
                tracing::warn!(
                    expires_in_secs = left,
                    "o certificado do cluster em uso está perto de expirar: gere um novo (taba cert generate)"
                );
                true
            }
            Some(_) => false,
            None => {
                tracing::error!(
                    "nenhum certificado do cluster está valendo agora: o nó não apresenta certificado"
                );
                true
            }
        }
    }

    /// A configuração de quem **recebe** na porta do cluster: exige o certificado do outro lado.
    pub fn server_config(self: &Arc<Self>) -> Result<Arc<ServerConfig>, String> {
        let mut config = ServerConfig::builder_with_provider(self.provider.clone())
            .with_safe_default_protocol_versions()
            .map_err(|e| e.to_string())?
            .with_client_cert_verifier(Arc::new(Verifier(self.clone())))
            .with_cert_resolver(Arc::new(Resolver(self.clone())));
        config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
        Ok(Arc::new(config))
    }

    /// A configuração de quem **abre** uma conexão: confia nos certificados da pasta (sem conferir
    /// o nome do host) e apresenta o seu.
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

/// O verificador das duas pontas: aceita o que a pasta aceita.
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

/// Escolhe o par a apresentar a cada handshake.
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
