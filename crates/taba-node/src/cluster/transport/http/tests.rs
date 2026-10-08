use super::*;

#[test]
fn the_own_address_is_the_interface_that_reaches_the_entry_address() {
    // Para um destino local, a interface é a de loopback (nada é enviado).
    let entry = PeerAddr::new("127.0.0.1", 9);
    assert_eq!(discover_address(&entry).unwrap(), "127.0.0.1");
    let unresolved = PeerAddr::new("nome.que.nao.existe.invalid", 9);
    assert!(discover_address(&unresolved).is_err());
}

#[test]
fn an_ipv6_host_is_bracketed_in_the_url() {
    assert_eq!(authority(&PeerAddr::new("10.0.0.5", 8081)), "10.0.0.5:8081");
    assert_eq!(authority(&PeerAddr::new("::1", 8081)), "[::1]:8081");
}
