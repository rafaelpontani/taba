//! A pasta de segredos (doc 06, seção 8.0.1; doc 06a, seção 8.1.3): o que `taba init`,
//! `taba cert generate` e `taba token generate` criam. Só depende de `core`.
//!
//! - **Certificado do cluster:** um par `cluster-<data de início>.crt` e `.key`, autoassinado e
//!   válido para servidor e cliente (o nó confia no certificado em si, sem CA; o nome do host
//!   não é verificado). O nome traz a data de início, para gerações sucessivas não se
//!   sobrescreverem.
//! - **Token raiz:** 256 bits aleatórios em base64 sem preenchimento (43 caracteres), no arquivo
//!   `root-token`.
//!
//! Nada é sobrescrito: se o arquivo já existe, o comando falha e o operador decide.

use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rcgen::{CertificateParams, DnType, ExtendedKeyUsagePurpose, KeyPair};
use time::OffsetDateTime;

/// Folga no `notBefore` para a diferença de relógio entre os nós, quando o início é "agora".
const CLOCK_SKEW: Duration = Duration::from_secs(5 * 60);
/// Validade padrão do primeiro certificado do `taba init`: cinco anos.
pub const DEFAULT_DAYS: u64 = 1825;
const ROOT_TOKEN_FILE: &str = "root-token";
const TLS_DIR: &str = "tls";

/// O que o `cert generate` criou.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CertFiles {
    pub certificate: PathBuf,
    pub key: PathBuf,
}

/// O que o `init` criou.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitFiles {
    pub certificate: CertFiles,
    pub root_token: PathBuf,
}

/// Um segredo aleatório de 256 bits, em base64 sem preenchimento (43 caracteres): o token raiz.
pub fn generate_root_token() -> String {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).expect("o sistema não tem fonte de números aleatórios");
    URL_SAFE_NO_PAD.encode(bytes)
}

fn already_exists(path: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::AlreadyExists,
        format!(
            "`{}` já existe; apague-o ou escolha outra pasta (nada é sobrescrito)",
            path.display()
        ),
    )
}

/// Cria o arquivo **novo** com o conteúdo, só para o dono quando o sistema permite (as chaves e
/// o token raiz são segredos). Falha se ele já existe.
fn write_new(path: &Path, content: &[u8]) -> io::Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(|e| {
        if e.kind() == io::ErrorKind::AlreadyExists {
            already_exists(path)
        } else {
            e
        }
    })?;
    file.write_all(content)?;
    file.sync_all()
}

/// Gera o par `cluster-<AAAAMMDD do início>.crt` e `.key` em `dir`, autoassinado, válido por
/// `days` dias a partir do início. O início é `now + starts_in`; sem `starts_in`, é `now` menos
/// uma folga de alguns minutos para a diferença de relógio.
pub fn generate_cert(
    dir: &Path,
    now: OffsetDateTime,
    days: u64,
    starts_in: Option<Duration>,
) -> io::Result<CertFiles> {
    if days == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "--days deve ser maior que zero",
        ));
    }
    let skew = time::Duration::try_from(CLOCK_SKEW).map_err(io::Error::other)?;
    let not_before = match starts_in {
        Some(wait) => now + time::Duration::try_from(wait).map_err(io::Error::other)?,
        None => now - skew,
    };
    let days = i64::try_from(days)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "--days grande demais"))?;
    let not_after = not_before
        .checked_add(time::Duration::days(days))
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "--days grande demais"))?;

    let key = KeyPair::generate().map_err(io::Error::other)?;
    let mut params = CertificateParams::new(Vec::<String>::new()).map_err(io::Error::other)?;
    params.not_before = not_before;
    params.not_after = not_after;
    params
        .distinguished_name
        .push(DnType::CommonName, "taba-cluster");
    params.extended_key_usages = vec![
        ExtendedKeyUsagePurpose::ServerAuth,
        ExtendedKeyUsagePurpose::ClientAuth,
    ];
    let certificate = params.self_signed(&key).map_err(io::Error::other)?;

    std::fs::create_dir_all(dir)?;
    let name = format!(
        "cluster-{:04}{:02}{:02}",
        not_before.year(),
        u8::from(not_before.month()),
        not_before.day()
    );
    let files = CertFiles {
        certificate: dir.join(format!("{name}.crt")),
        key: dir.join(format!("{name}.key")),
    };
    // Confere os dois antes de gravar qualquer um: não deixa um `.crt` sem `.key`.
    for path in [&files.certificate, &files.key] {
        if path.exists() {
            return Err(already_exists(path));
        }
    }
    write_new(&files.key, key.serialize_pem().as_bytes())?;
    write_new(&files.certificate, certificate.pem().as_bytes())?;
    Ok(files)
}

/// `taba init --out <pasta>`: cria a pasta de segredos, com o primeiro certificado em `tls/` e o
/// token raiz em `root-token`.
pub fn init(out: &Path, now: OffsetDateTime, days: u64) -> io::Result<InitFiles> {
    let root_token = out.join(ROOT_TOKEN_FILE);
    if root_token.exists() {
        return Err(already_exists(&root_token));
    }
    std::fs::create_dir_all(out)?;
    let certificate = generate_cert(&out.join(TLS_DIR), now, days, None)?;
    write_new(
        &root_token,
        format!("{}\n", generate_root_token()).as_bytes(),
    )?;
    Ok(InitFiles {
        certificate,
        root_token,
    })
}

#[cfg(test)]
mod tests;
