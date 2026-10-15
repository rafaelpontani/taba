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

/// A self-signed pair in `dir` that starts at `starts_in` (from now) and is valid for `days`.
fn pair(dir: &Path, starts_in: Option<Duration>, days: u64) -> CertificateDer<'static> {
    let files = generate_cert(dir, OffsetDateTime::now_utc(), days, starts_in).unwrap();
    der_of(&files.certificate)
}

/// A pair issued by a CA: the CA goes to `ca_dir` only as `.crt` (to trust) and the node's pair
/// to `node_dir`.
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
    // Starts only in two weeks: it is already accepted (the start date does not count for trust).
    let future = pair(dir.path(), Some(Duration::from_secs(14 * 86_400)), 30);
    let set = load(dir.path()).unwrap();
    assert!(set.trusts(&future, &[], now()));

    // Any other certificate is not accepted.
    let other_dir = tempfile::tempdir().unwrap();
    let stranger = pair(other_dir.path(), None, 30);
    assert!(!set.trusts(&stranger, &[], now()));
}

/// UC: UC-SEG-009, UC-SEG-016
#[test]
fn an_expired_certificate_is_neither_trusted_nor_presented() {
    let dir = tempfile::tempdir().unwrap();
    // Generated 10 days ago, valid for 1: expired 9 days ago.
    let files = generate_cert(
        dir.path(),
        OffsetDateTime::now_utc() - time::Duration::days(10),
        1,
        None,
    )
    .unwrap();
    let expired = der_of(&files.certificate);
    // One more good pair, so the folder is valid.
    let good = pair(dir.path(), Some(Duration::from_secs(3600)), 30);
    let set = load(dir.path()).unwrap();
    assert!(
        !set.trusts(&expired, &[], now()),
        "the expired one is not accepted"
    );
    assert!(set.trusts(&good, &[], now()));
    // Does not present the expired one nor the one that has not started yet.
    assert!(set.presented(now()).is_none());
    // In two hours the good one becomes valid.
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
    // The name carries the start date: the new pair starts on another day.
    let newer = pair(dir.path(), Some(Duration::from_secs(2 * 86_400)), 30);
    let set = load(dir.path()).unwrap();
    assert_eq!(set.names().len(), 2);
    assert_eq!(set.presented(now()).unwrap().der.as_ref(), old.as_ref());
    assert_eq!(
        set.presented(now() + 3 * 86_400).unwrap().der.as_ref(),
        newer.as_ref(),
        "after the start, the newest"
    );
}

/// UC: UC-SEG-012
#[test]
fn a_crt_without_key_only_serves_to_trust_and_a_key_without_crt_is_incomplete() {
    let dir = tempfile::tempdir().unwrap();
    let mine = pair(dir.path(), None, 30);
    // A colleague's certificate: only the .crt.
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
        "only presents what has a .key"
    );

    // A .key without the .crt of the same name is an incomplete pair.
    std::fs::write(dir.path().join("alone.key"), "x").unwrap();
    let error = load(dir.path()).err().unwrap();
    assert!(error.contains("incomplete pair"), "{error}");
}

/// UC: UC-SEG-008
#[test]
fn a_folder_without_a_valid_pair_does_not_open() {
    let empty = tempfile::tempdir().unwrap();
    assert!(TlsStore::open(empty.path()).is_err());

    // A .crt without a .key is not enough.
    let only_crt = tempfile::tempdir().unwrap();
    let others = tempfile::tempdir().unwrap();
    pair(others.path(), None, 30);
    for entry in std::fs::read_dir(others.path()).unwrap().flatten() {
        if entry.path().extension().is_some_and(|e| e == "crt") {
            std::fs::copy(entry.path(), only_crt.path().join("a.crt")).unwrap();
        }
    }
    assert!(TlsStore::open(only_crt.path()).is_err());

    // A .key that is not the .crt's.
    let wrong = tempfile::tempdir().unwrap();
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let fa = generate_cert(a.path(), OffsetDateTime::now_utc(), 30, None).unwrap();
    let fb = generate_cert(b.path(), OffsetDateTime::now_utc(), 30, None).unwrap();
    std::fs::copy(&fa.certificate, wrong.path().join("p.crt")).unwrap();
    std::fs::copy(&fb.key, wrong.path().join("p.key")).unwrap();
    let error = TlsStore::open(wrong.path()).err().unwrap();
    assert!(error.contains("is not the key"), "{error}");
}

/// UC: UC-SEG-013
#[test]
fn a_reload_with_an_invalid_folder_keeps_the_previous_set_and_a_valid_one_replaces_it() {
    let dir = tempfile::tempdir().unwrap();
    let first = pair(dir.path(), None, 30);
    let store = TlsStore::open(dir.path()).unwrap();
    assert_eq!(store.presented_der().unwrap(), first.as_ref());

    // Unreadable file: the reload is refused and the previous set applies.
    std::fs::write(dir.path().join("junk.crt"), "this is not PEM").unwrap();
    assert!(store.reload().is_err());
    assert_eq!(store.presented_der().unwrap(), first.as_ref());

    // Folder valid again, with one more pair (starts later): the reload applies.
    std::fs::remove_file(dir.path().join("junk.crt")).unwrap();
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
    // This node trusts the CA and presents its pair.
    let mine = tempfile::tempdir().unwrap();
    pair(mine.path(), None, 30);
    std::fs::copy(ca_dir.path().join("ca.crt"), mine.path().join("ca.crt")).unwrap();
    let set = load(mine.path()).unwrap();
    assert!(set.trusts(&issued, &[], now()), "issued by the folder's CA");

    // Without the CA, the same certificate is not accepted.
    let without = tempfile::tempdir().unwrap();
    pair(without.path(), None, 30);
    assert!(!load(without.path()).unwrap().trusts(&issued, &[], now()));
}
