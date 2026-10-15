use super::*;

#[test]
fn the_own_address_is_the_interface_that_reaches_the_entry_address() {
    // For a local destination, the interface is the loopback one (nothing is sent).
    let entry = PeerAddr::new("127.0.0.1", 9);
    assert_eq!(discover_address(&entry).unwrap(), "127.0.0.1");
    let unresolved = PeerAddr::new("name.that.does.not.exist.invalid", 9);
    assert!(discover_address(&unresolved).is_err());
}

#[test]
fn an_ipv6_host_is_bracketed_in_the_url() {
    assert_eq!(authority(&PeerAddr::new("10.0.0.5", 8081)), "10.0.0.5:8081");
    assert_eq!(authority(&PeerAddr::new("::1", 8081)), "[::1]:8081");
}

/// A minimal HTTP/1.1 server that counts the accepted connections and leaves each one open (keep-alive).
async fn counting_server() -> (u16, Arc<std::sync::atomic::AtomicUsize>) {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let accepted = Arc::new(AtomicUsize::new(0));
    let counter = accepted.clone();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            counter.fetch_add(1, Ordering::SeqCst);
            tokio::spawn(async move {
                let mut buf = vec![0u8; 4096];
                while let Ok(n) = socket.read(&mut buf).await {
                    if n == 0 {
                        break;
                    }
                    let reply = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\n\r\n{}";
                    if socket.write_all(reply.as_bytes()).await.is_err() {
                        break;
                    }
                }
            });
        }
    });
    (port, accepted)
}

/// UC: UC-ING-028
#[tokio::test]
async fn every_node_listing_opens_a_new_connection_so_the_balancer_can_pick_another_node() {
    use std::sync::atomic::Ordering;

    let (port, accepted) = counting_server().await;
    let transport = HttpTransport::plain().unwrap();
    let to = PeerAddr::new("127.0.0.1", port);
    for _ in 0..3 {
        let _ = transport.call(&to, Request::ListNodes).await;
    }
    assert_eq!(
        accepted.load(Ordering::SeqCst),
        3,
        "one connection per listing"
    );
}
