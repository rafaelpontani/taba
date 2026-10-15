//! The secrets folder (doc 06, section 8.0.1; doc 06a, section 8.1.3): what `taba init`,
//! `taba cert generate` and `taba token generate` create. It only depends on `core`.
//!
//! - **Cluster certificate:** a `cluster-<start date>.crt` and `.key` pair, self-signed and
//!   valid for server and client (the node trusts the certificate itself, without a CA; the host name
//!   is not verified). The name carries the start date, so successive generations do not
//!   overwrite each other.
//! - **Root token:** 256 random bits in unpadded base64 (43 characters), in the
//!   `root-token` file.
//!
//! Nothing is overwritten: if the file already exists, the command fails and the operator decides.

use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rcgen::{CertificateParams, DnType, ExtendedKeyUsagePurpose, KeyPair};
use time::OffsetDateTime;

/// Slack on the `notBefore` for clock differences between nodes, when the start is "now".
const CLOCK_SKEW: Duration = Duration::from_secs(5 * 60);
/// Default validity of the first `taba init` certificate: five years.
pub const DEFAULT_DAYS: u64 = 1825;
const ROOT_TOKEN_FILE: &str = "root-token";
const TLS_DIR: &str = "tls";

/// What `cert generate` created.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CertFiles {
    pub certificate: PathBuf,
    pub key: PathBuf,
}

/// What `init` created.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitFiles {
    pub certificate: CertFiles,
    pub root_token: PathBuf,
}

/// A random 256-bit secret, in unpadded base64 (43 characters): the root token.
pub fn generate_root_token() -> String {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).expect("the system has no source of random numbers");
    URL_SAFE_NO_PAD.encode(bytes)
}

fn already_exists(path: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::AlreadyExists,
        format!(
            "`{}` already exists; delete it or choose another folder (nothing is overwritten)",
            path.display()
        ),
    )
}

/// Creates the **new** file with the content, only for the owner when the system allows (the keys and
/// the root token are secrets). Fails if it already exists.
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

/// Generates the `cluster-<start YYYYMMDD>.crt` and `.key` pair in `dir`, self-signed, valid for
/// `days` days from the start. The start is `now + starts_in`; without `starts_in`, it is `now` minus
/// a few minutes of slack for clock differences.
pub fn generate_cert(
    dir: &Path,
    now: OffsetDateTime,
    days: u64,
    starts_in: Option<Duration>,
) -> io::Result<CertFiles> {
    if days == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "--days must be greater than zero",
        ));
    }
    let skew = time::Duration::try_from(CLOCK_SKEW).map_err(io::Error::other)?;
    let not_before = match starts_in {
        Some(wait) => now + time::Duration::try_from(wait).map_err(io::Error::other)?,
        None => now - skew,
    };
    let days = i64::try_from(days)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "--days too large"))?;
    let not_after = not_before
        .checked_add(time::Duration::days(days))
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "--days too large"))?;

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
    // Checks both before writing either: it does not leave a `.crt` without a `.key`.
    for path in [&files.certificate, &files.key] {
        if path.exists() {
            return Err(already_exists(path));
        }
    }
    write_new(&files.key, key.serialize_pem().as_bytes())?;
    write_new(&files.certificate, certificate.pem().as_bytes())?;
    Ok(files)
}

/// `taba init --out <folder>`: creates the secrets folder, with the first certificate in `tls/` and the
/// root token in `root-token`.
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
