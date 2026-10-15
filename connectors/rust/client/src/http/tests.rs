use super::*;

fn parse(bytes: &[u8]) -> Result<Response, ReadError> {
    read_response(&mut io::BufReader::new(bytes))
}

/// UC: UC-CNT-026
#[test]
fn encodes_a_request_with_host_length_and_close() {
    let req = Request {
        method: "POST".into(),
        target: "/topics".into(),
        headers: vec![("Content-Type".into(), "application/json".into())],
        body: b"{\"a\":1}".to_vec(),
    };
    let text = String::from_utf8(encode_request("taba:8080", &req)).unwrap();
    assert_eq!(
        text,
        "POST /topics HTTP/1.1\r\nHost: taba:8080\r\nContent-Type: application/json\r\nContent-Length: 7\r\nConnection: close\r\n\r\n{\"a\":1}"
    );
}

/// UC: UC-CNT-026
#[test]
fn a_get_without_body_has_no_content_length_and_post_without_body_has_zero() {
    let get = Request {
        method: "GET".into(),
        target: "/".into(),
        headers: vec![],
        body: vec![],
    };
    assert!(!String::from_utf8(encode_request("h", &get))
        .unwrap()
        .contains("Content-Length"));
    let post = Request {
        method: "POST".into(),
        ..get
    };
    assert!(String::from_utf8(encode_request("h", &post))
        .unwrap()
        .contains("Content-Length: 0\r\n"));
}

/// UC: UC-CNT-026
#[test]
fn reads_a_response_with_content_length() {
    let r = parse(b"HTTP/1.1 201 Created\r\nContent-Length: 5\r\nX-Request-Id: abc\r\nLocation: /topics/x\r\n\r\nhello-extra").unwrap();
    assert_eq!(r.status, 201);
    assert_eq!(r.body, b"hello");
    assert_eq!(r.header("x-request-id"), Some("abc"));
    assert_eq!(r.header("LOCATION"), Some("/topics/x"));
}

/// UC: UC-CNT-026
#[test]
fn reads_a_chunked_response_with_extensions_and_trailers() {
    let r = parse(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5;ext=1\r\nhello\r\n6\r\n world\r\n0\r\nTrailer: x\r\n\r\n").unwrap();
    assert_eq!(r.body, b"hello world");
}

/// UC: UC-CNT-026
#[test]
fn reads_until_close_when_there_is_no_length_and_skips_100_continue() {
    let r = parse(b"HTTP/1.1 100 Continue\r\n\r\nHTTP/1.1 200 OK\r\n\r\nrest of the body").unwrap();
    assert_eq!(r.status, 200);
    assert_eq!(r.body, b"rest of the body");
}

/// UC: UC-CNT-026
#[test]
fn no_content_has_no_body() {
    let r = parse(b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n").unwrap();
    assert_eq!((r.status, r.body.len()), (204, 0));
}

/// UC: UC-CNT-013
#[test]
fn tells_apart_a_connection_closed_before_any_byte_and_after_some() {
    match parse(b"") {
        Err(ReadError::Io {
            received: false, ..
        }) => {}
        other => panic!("{other:?}"),
    }
    match parse(b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\nabc") {
        Err(ReadError::Io { received: true, .. }) => {}
        other => panic!("{other:?}"),
    }
}

/// UC: UC-CNT-026
#[test]
fn rejects_malformed_responses() {
    for bytes in [
        &b"garbage\r\n\r\n"[..],
        b"HTTP/1.1 abc OK\r\n\r\n",
        b"HTTP/1.1 200 OK\r\nsem-dois-pontos\r\n\r\n",
        b"HTTP/1.1 200 OK\r\nContent-Length: x\r\n\r\n",
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nzz\r\n",
    ] {
        assert!(
            matches!(parse(bytes), Err(ReadError::Malformed(_))),
            "{bytes:?}"
        );
    }
}

#[test]
fn parses_urls_with_default_ports_ipv6_and_prefix() {
    let u = Url::parse("http://127.0.0.1:8080").unwrap();
    assert_eq!(
        (u.https, u.host.as_str(), u.port, u.base_path.as_str()),
        (false, "127.0.0.1", 8080, "")
    );
    let u = Url::parse("https://taba.example.com/api/").unwrap();
    assert_eq!((u.https, u.port, u.base_path.as_str()), (true, 443, "/api"));
    assert_eq!(u.host_header(), "taba.example.com");
    let u = Url::parse("http://[::1]:9090").unwrap();
    assert_eq!((u.host.as_str(), u.port), ("::1", 9090));
    assert_eq!(u.host_header(), "[::1]:9090");
    assert_eq!(Url::parse("http://h").unwrap().port, 80);
    assert_eq!(u.with_port(1).origin(), "http://[::1]:1");
}

#[test]
fn rejects_bad_urls() {
    for url in [
        "",
        "taba:8080",
        "ftp://h",
        "http://",
        "http://:80",
        "http://h:0",
        "http://h:99999",
        "http://u@h",
        "http://[::1",
    ] {
        assert!(Url::parse(url).is_err(), "{url}");
    }
}

/// UC: UC-CNT-026
#[test]
fn builds_a_target_with_encoded_query() {
    let q = vec![
        ("name".to_owned(), "a b&c".to_owned()),
        ("tag.k".to_owned(), "é".to_owned()),
    ];
    assert_eq!(
        build_target("", "/topics", &q),
        "/topics?name=a%20b%26c&tag.k=%C3%A9"
    );
    assert_eq!(build_target("/api", "/topics", &[]), "/api/topics");
}
