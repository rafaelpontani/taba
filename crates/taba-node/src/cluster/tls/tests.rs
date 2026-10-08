use std::path::Path;
use std::time::Duration;

use rcgen::{BasicConstraints, CertificateParams, DnType, IsCa, Issuer, KeyPair};
use time::OffsetDateTime;

use super::*;
use crate::secrets::generate_cert;

fn provider() -> CryptoProvider {
    rustls::crypto::ring::default_provider()
}

fn der_of(path: &Path) -> CertificateDer<'static> {
    CertificateDer::from_pem_file(path).unwrap()
}

/// Um par autoassinado em `dir` que começa em `starts_in` (a partir de agora) e vale `days`.
fn pair(dir: &Path, starts_in: Option<Duration>, days: u64) -> CertificateDer<'static> {
    let files = generate_cert(dir, OffsetDateTime::now_utc(), days, starts_in).unwrap();
    der_of(&files.certificate)
}

/// Um par emitido por uma CA: a CA vai para `ca_dir` só como `.crt` (para confiar) e o par do nó
/// para `node_dir`.
fn issued_pair(ca_dir: &Path, node_dir: &Path) -> CertificateDer<'static> {
    let ca_key = KeyPair::generate().unwrap();
    let mut ca = CertificateParams::new(Vec::<String>::new()).unwrap();
    ca.distinguished_name
        .push(DnType::CommonName, "ca-do-cluster");
    ca.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    let ca_cert = ca.self_signed(&ca_key).unwrap();
    std::fs::write(ca_dir.join("ca.crt"), ca_cert.pem()).unwrap();

    let node_key = KeyPair::generate().unwrap();
    let mut node = CertificateParams::new(Vec::<String>::new()).unwrap();
    node.distinguished_name.push(DnType::CommonName, "no-1");
    let issuer = Issuer::new(ca, ca_key);
    let node_cert = node.signed_by(&node_key, &issuer).unwrap();
    std::fs::write(node_dir.join("no-1.crt"), node_cert.pem()).unwrap();
    std::fs::write(node_dir.join("no-1.key"), node_key.serialize_pem()).unwrap();
    node_cert.der().clone()
}

fn load(dir: &Path) -> Result<CertSet, String> {
    CertSet::load(dir, &provider())
}

fn now() -> i64 {
    unix_now()
}

/// UC: UC-SEG-009
#[test]
fn it_trusts_an_equal_certificate_and_ignores_the_start_date() {
    let dir = tempfile::tempdir().unwrap();
    // Começa só daqui a duas semanas: já é aceito (a data de início não conta para a confiança).
    let future = pair(dir.path(), Some(Duration::from_secs(14 * 86_400)), 30);
    let set = load(dir.path()).unwrap();
    assert!(set.trusts(&future, &[], now()));

    // Outro certificado qualquer não é aceito.
    let other_dir = tempfile::tempdir().unwrap();
    let stranger = pair(other_dir.path(), None, 30);
    assert!(!set.trusts(&stranger, &[], now()));
}

/// UC: UC-SEG-009, UC-SEG-016
#[test]
fn an_expired_certificate_is_neither_trusted_nor_presented() {
    let dir = tempfile::tempdir().unwrap();
    // Gerado há 10 dias, válido por 1: expirou há 9.
    let files = generate_cert(
        dir.path(),
        OffsetDateTime::now_utc() - time::Duration::days(10),
        1,
        None,
    )
    .unwrap();
    let expired = der_of(&files.certificate);
    // Mais um par bom, para a pasta ser válida.
    let good = pair(dir.path(), Some(Duration::from_secs(3600)), 30);
    let set = load(dir.path()).unwrap();
    assert!(!set.trusts(&expired, &[], now()), "o expirado não é aceito");
    assert!(set.trusts(&good, &[], now()));
    // Não apresenta o expirado nem o que ainda não começou.
    assert!(set.presented(now()).is_none());
    // Daqui a duas horas o bom começa a valer.
    assert_eq!(
        set.presented(now() + 7200).unwrap().der.as_ref(),
        good.as_ref()
    );
}

/// UC: UC-SEG-010
#[test]
fn it_presents_the_newest_pair_that_has_already_started() {
    let dir = tempfile::tempdir().unwrap();
    let old = pair(dir.path(), None, 30);
    // O nome traz a data de início: o par novo começa em outro dia.
    let newer = pair(dir.path(), Some(Duration::from_secs(2 * 86_400)), 30);
    let set = load(dir.path()).unwrap();
    assert_eq!(set.names().len(), 2);
    assert_eq!(set.presented(now()).unwrap().der.as_ref(), old.as_ref());
    assert_eq!(
        set.presented(now() + 3 * 86_400).unwrap().der.as_ref(),
        newer.as_ref(),
        "depois do início, o mais novo"
    );
}

/// UC: UC-SEG-012
#[test]
fn a_crt_without_key_only_serves_to_trust_and_a_key_without_crt_is_incomplete() {
    let dir = tempfile::tempdir().unwrap();
    let mine = pair(dir.path(), None, 30);
    // O certificado de um colega: só o .crt.
    let others = tempfile::tempdir().unwrap();
    let theirs = pair(others.path(), None, 30);
    let their_crt = std::fs::read_dir(others.path())
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|e| e == "crt"))
        .unwrap();
    std::fs::copy(&their_crt, dir.path().join("colega.crt")).unwrap();

    let set = load(dir.path()).unwrap();
    assert!(set.trusts(&theirs, &[], now()), "confia no .crt avulso");
    assert_eq!(
        set.presented(now()).unwrap().der.as_ref(),
        mine.as_ref(),
        "só apresenta o que tem .key"
    );

    // Um .key sem o .crt de mesmo nome é par incompleto.
    std::fs::write(dir.path().join("sozinho.key"), "x").unwrap();
    let error = load(dir.path()).err().unwrap();
    assert!(error.contains("par incompleto"), "{error}");
}

/// UC: UC-SEG-008
#[test]
fn a_folder_without_a_valid_pair_does_not_open() {
    let empty = tempfile::tempdir().unwrap();
    assert!(TlsStore::open(empty.path()).is_err());

    // Um .crt sem .key não basta.
    let only_crt = tempfile::tempdir().unwrap();
    let others = tempfile::tempdir().unwrap();
    pair(others.path(), None, 30);
    for entry in std::fs::read_dir(others.path()).unwrap().flatten() {
        if entry.path().extension().is_some_and(|e| e == "crt") {
            std::fs::copy(entry.path(), only_crt.path().join("a.crt")).unwrap();
        }
    }
    assert!(TlsStore::open(only_crt.path()).is_err());

    // Um .key que não é o do .crt.
    let wrong = tempfile::tempdir().unwrap();
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let fa = generate_cert(a.path(), OffsetDateTime::now_utc(), 30, None).unwrap();
    let fb = generate_cert(b.path(), OffsetDateTime::now_utc(), 30, None).unwrap();
    std::fs::copy(&fa.certificate, wrong.path().join("p.crt")).unwrap();
    std::fs::copy(&fb.key, wrong.path().join("p.key")).unwrap();
    let error = TlsStore::open(wrong.path()).err().unwrap();
    assert!(error.contains("não é a chave"), "{error}");
}

/// UC: UC-SEG-013
#[test]
fn a_reload_with_an_invalid_folder_keeps_the_previous_set_and_a_valid_one_replaces_it() {
    let dir = tempfile::tempdir().unwrap();
    let first = pair(dir.path(), None, 30);
    let store = TlsStore::open(dir.path()).unwrap();
    assert_eq!(store.presented_der().unwrap(), first.as_ref());

    // Arquivo ilegível: a recarga é recusada e vale o conjunto anterior.
    std::fs::write(dir.path().join("lixo.crt"), "isto não é PEM").unwrap();
    assert!(store.reload().is_err());
    assert_eq!(store.presented_der().unwrap(), first.as_ref());

    // Pasta de novo válida, com um par a mais (começa mais tarde): a recarga vale.
    std::fs::remove_file(dir.path().join("lixo.crt")).unwrap();
    pair(dir.path(), Some(Duration::from_secs(2 * 86_400)), 30);
    store.reload().unwrap();
    assert_eq!(store.presented_der().unwrap(), first.as_ref());
    assert!(store.expires_in().unwrap() > 0);
}

/// UC: UC-SEG-017
#[test]
fn a_ca_in_the_folder_makes_the_certificates_it_issued_accepted() {
    let ca_dir = tempfile::tempdir().unwrap();
    let node_dir = tempfile::tempdir().unwrap();
    let issued = issued_pair(ca_dir.path(), node_dir.path());
    // Este nó confia na CA e apresenta o seu par.
    let mine = tempfile::tempdir().unwrap();
    pair(mine.path(), None, 30);
    std::fs::copy(ca_dir.path().join("ca.crt"), mine.path().join("ca.crt")).unwrap();
    let set = load(mine.path()).unwrap();
    assert!(set.trusts(&issued, &[], now()), "emitido pela CA da pasta");

    // Sem a CA, o mesmo certificado não é aceito.
    let without = tempfile::tempdir().unwrap();
    pair(without.path(), None, 30);
    assert!(!load(without.path()).unwrap().trusts(&issued, &[], now()));
}
