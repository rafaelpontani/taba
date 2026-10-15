use ::time::macros::datetime;
use x509_parser::prelude::*;

use super::*;

const NOW: OffsetDateTime = datetime!(2026-10-06 12:00:00 UTC);

fn certificate_of(files: &CertFiles) -> (Vec<u8>, String) {
    let pem = std::fs::read_to_string(&files.certificate).unwrap();
    let (_, parsed) = x509_parser::pem::parse_x509_pem(pem.as_bytes()).unwrap();
    (parsed.contents, pem)
}

/// UC: UC-SEG-020
#[test]
fn a_certificate_is_self_signed_and_valid_for_server_and_client() {
    let dir = tempfile::tempdir().unwrap();
    let files =
        generate_cert(dir.path(), NOW, 365, Some(Duration::from_secs(14 * 86_400))).unwrap();
    let (der, _) = certificate_of(&files);
    let (_, cert) = X509Certificate::from_der(&der).unwrap();

    // Start in the future (`--starts-in`) and validity counted from it.
    let starts = cert.validity().not_before.to_datetime();
    let ends = cert.validity().not_after.to_datetime();
    assert_eq!(starts, datetime!(2026-10-20 12:00:00 UTC));
    assert_eq!(ends, datetime!(2027-10-20 12:00:00 UTC));
    assert_eq!(cert.issuer(), cert.subject(), "autoassinado");

    let usage = cert
        .extended_key_usage()
        .unwrap()
        .expect("has extended key usage")
        .value;
    assert!(usage.server_auth && usage.client_auth);
    assert!(
        cert.subject_alternative_name().unwrap().is_none(),
        "the host name is not verified: there is no SAN"
    );
}

/// UC: UC-SEG-020
#[test]
fn the_name_carries_the_start_date_and_the_key_is_a_pem_private_key() {
    let dir = tempfile::tempdir().unwrap();
    let files = generate_cert(dir.path(), NOW, 30, Some(Duration::from_secs(86_400))).unwrap();
    assert_eq!(
        files.certificate.file_name().unwrap(),
        "cluster-20261007.crt"
    );
    assert_eq!(files.key.file_name().unwrap(), "cluster-20261007.key");
    let key = std::fs::read_to_string(&files.key).unwrap();
    assert!(key.starts_with("-----BEGIN PRIVATE KEY-----"), "{key}");
    let (_, pem) = certificate_of(&files);
    assert!(pem.starts_with("-----BEGIN CERTIFICATE-----"));
}

/// UC: UC-SEG-020
#[test]
fn without_starts_in_it_starts_a_few_minutes_ago() {
    let dir = tempfile::tempdir().unwrap();
    let files = generate_cert(dir.path(), NOW, 10, None).unwrap();
    let (der, _) = certificate_of(&files);
    let (_, cert) = X509Certificate::from_der(&der).unwrap();
    assert_eq!(
        cert.validity().not_before.to_datetime(),
        datetime!(2026-10-06 11:55:00 UTC)
    );
    assert_eq!(
        files.certificate.file_name().unwrap(),
        "cluster-20261006.crt"
    );
}

/// UC: UC-SEG-020
#[test]
fn nothing_is_overwritten_and_no_half_pair_is_left() {
    let dir = tempfile::tempdir().unwrap();
    let first = generate_cert(dir.path(), NOW, 10, None).unwrap();
    let before = std::fs::read(&first.certificate).unwrap();
    let error = generate_cert(dir.path(), NOW, 10, None).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
    assert_eq!(std::fs::read(&first.certificate).unwrap(), before);

    // Only the `.key` exists: refuses without creating the `.crt`.
    let other = tempfile::tempdir().unwrap();
    std::fs::write(other.path().join("cluster-20261006.key"), "x").unwrap();
    assert!(generate_cert(other.path(), NOW, 10, None).is_err());
    assert!(!other.path().join("cluster-20261006.crt").exists());

    // Different start dates give different names: a rotation does not overwrite.
    let next = generate_cert(dir.path(), NOW, 10, Some(Duration::from_secs(14 * 86_400))).unwrap();
    assert_ne!(next.certificate, first.certificate);
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 4);
}

#[test]
fn zero_days_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let error = generate_cert(dir.path(), NOW, 0, None).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
}

/// UC: UC-SEG-019, UC-AUT-005
#[test]
fn init_creates_the_certificate_in_tls_and_a_root_token_of_at_least_32_characters() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("secrets");
    let files = init(&out, NOW, DEFAULT_DAYS).unwrap();
    assert_eq!(files.root_token, out.join("root-token"));
    assert!(files.certificate.certificate.starts_with(out.join("tls")));
    assert!(files.certificate.key.is_file());

    let token = std::fs::read_to_string(&files.root_token).unwrap();
    assert!(token.ends_with('\n'));
    assert!(
        token.trim().len() >= 32,
        "the root token has at least 32 characters"
    );
    assert_eq!(token.trim().len(), 43);

    let (der, _) = certificate_of(&files.certificate);
    let (_, cert) = X509Certificate::from_der(&der).unwrap();
    let days = (cert.validity().not_after.to_datetime() - cert.validity().not_before.to_datetime())
        .whole_days();
    assert_eq!(days, DEFAULT_DAYS as i64);
}

/// UC: UC-SEG-020
#[test]
fn init_never_overwrites_an_existing_root_token() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("secrets");
    init(&out, NOW, 10).unwrap();
    let token = std::fs::read_to_string(out.join("root-token")).unwrap();

    let error = init(&out, NOW, 10).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
    assert_eq!(
        std::fs::read_to_string(out.join("root-token")).unwrap(),
        token
    );
}

/// UC: UC-AUT-005
#[test]
fn generated_root_tokens_are_random_and_long_enough() {
    let a = generate_root_token();
    let b = generate_root_token();
    assert_ne!(a, b);
    assert_eq!(a.len(), 43);
    assert!(
        a.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    );
}

#[cfg(unix)]
#[test]
fn the_secrets_are_readable_only_by_the_owner() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let files = init(&dir.path().join("s"), NOW, 10).unwrap();
    for path in [&files.root_token, &files.certificate.key] {
        let mode = std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "{}", path.display());
    }
}
