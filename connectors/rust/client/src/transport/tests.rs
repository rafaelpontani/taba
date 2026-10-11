use super::*;
use std::net::TcpListener;
use std::thread;

fn serve_once(reply: &'static [u8]) -> (u16, thread::JoinHandle<Vec<u8>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = thread::spawn(move || {
        let (mut sock, _) = listener.accept().unwrap();
        let mut got = Vec::new();
        let mut buf = [0u8; 1024];
        // Lê até o fim dos cabeçalhos e do corpo de 2 bytes que os testes mandam.
        loop {
            let n = sock.read(&mut buf).unwrap();
            got.extend_from_slice(&buf[..n]);
            if n == 0
                || got.windows(4).any(|w| w == b"\r\n\r\n")
                    && (got.ends_with(b"\r\n\r\n") || got.ends_with(b"{}"))
            {
                break;
            }
        }
        sock.write_all(reply).unwrap();
        got
    });
    (port, handle)
}

fn timeouts() -> Timeouts {
    Timeouts {
        connect: Duration::from_secs(2),
        total: Duration::from_secs(5),
    }
}

/// UC: UC-CNT-019
#[test]
fn sends_http_1_1_over_a_plain_socket_and_reads_the_reply() {
    let (port, server) = serve_once(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok");
    let url = Url::parse(&format!("http://127.0.0.1:{port}")).unwrap();
    let req = Request {
        method: "GET".into(),
        target: "/health".into(),
        headers: vec![],
        body: vec![],
    };
    let res = TcpTransport::new(None)
        .send(&url, &req, &timeouts())
        .unwrap();
    assert_eq!((res.status, res.body.as_slice()), (200, &b"ok"[..]));
    let got = String::from_utf8(server.join().unwrap()).unwrap();
    assert!(
        got.starts_with("GET /health HTTP/1.1\r\nHost: 127.0.0.1:"),
        "{got}"
    );
    assert!(got.contains("Connection: close"));
}

/// UC: UC-CNT-013
#[test]
fn a_refused_connection_is_a_failure_before_sending() {
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let url = Url::parse(&format!("http://127.0.0.1:{port}")).unwrap();
    let req = Request {
        method: "GET".into(),
        target: "/".into(),
        headers: vec![],
        body: vec![],
    };
    let err = TcpTransport::new(None)
        .send(&url, &req, &timeouts())
        .unwrap_err();
    // No Windows a recusa só aparece depois das retentativas do sistema, e vira o tempo de conexão;
    // as duas coisas são "não enviou" e o conector as repete do mesmo jeito.
    assert!(
        matches!(
            err,
            TransportError::Connect(_) | TransportError::Timeout { sent: false }
        ),
        "{err:?}"
    );
}

/// UC: UC-CNT-009, UC-CNT-013
#[test]
fn a_silent_server_times_out_after_the_request_was_sent() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let hold = thread::spawn(move || {
        let (sock, _) = listener.accept().unwrap();
        thread::sleep(Duration::from_millis(600));
        drop(sock);
    });
    let url = Url::parse(&format!("http://127.0.0.1:{port}")).unwrap();
    let req = Request {
        method: "GET".into(),
        target: "/".into(),
        headers: vec![],
        body: vec![],
    };
    let short = Timeouts {
        connect: Duration::from_secs(2),
        total: Duration::from_millis(150),
    };
    let err = TcpTransport::new(None)
        .send(&url, &req, &short)
        .unwrap_err();
    assert_eq!(err, TransportError::Timeout { sent: true });
    hold.join().unwrap();
}

/// UC: UC-CNT-013
#[test]
fn a_connection_dropped_mid_response_is_an_io_failure() {
    let (port, server) = serve_once(b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\nab");
    let url = Url::parse(&format!("http://127.0.0.1:{port}")).unwrap();
    let req = Request {
        method: "POST".into(),
        target: "/x".into(),
        headers: vec![],
        body: b"{}".to_vec(),
    };
    let err = TcpTransport::new(None)
        .send(&url, &req, &timeouts())
        .unwrap_err();
    assert!(matches!(err, TransportError::Io(_)), "{err:?}");
    server.join().unwrap();
}

/// UC: UC-CNT-030
#[test]
fn tls_config_accepts_a_pem_ca_and_rejects_files_without_certificates() {
    assert!(build_tls_config(None).is_ok());
    assert!(build_tls_config(Some(b"nada de PEM aqui")).is_err());
    assert!(build_tls_config(Some(
        b"-----BEGIN CERTIFICATE-----\n!!!\n-----END CERTIFICATE-----\n"
    ))
    .is_err());
}
