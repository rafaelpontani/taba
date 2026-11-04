use axum::http::{HeaderMap, Method, Request};

use super::*;

const NOW: &str = "20261104T120000Z";

fn credentials() -> Credentials {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("credentials.json");
    std::fs::write(&path, r#"{"KEY1":{"secret":"secret-1","token":"tb_a.b"}}"#).expect("write");
    // The file is read when it is opened; the folder can go away after that.
    Credentials::open(&path).expect("open")
}

fn signed_request(service: &str, secret: &str) -> (Parts, Vec<u8>) {
    let body = b"Action=ListTopics".to_vec();
    let mut headers = HeaderMap::new();
    headers.insert("host", "gw:9325".parse().unwrap());
    headers.insert("x-amz-date", NOW.parse().unwrap());
    headers.insert(
        "content-type",
        "application/x-www-form-urlencoded".parse().unwrap(),
    );
    let authorization = sigv4::sign(
        "KEY1",
        secret,
        "eu-west-1",
        service,
        NOW,
        &Signed {
            method: "POST",
            path: "/",
            query: "",
            headers: &headers,
            body: &body,
        },
        &["content-type", "host", "x-amz-date"],
    );
    headers.insert("authorization", authorization.parse().unwrap());
    let mut request = Request::builder().method(Method::POST).uri("/");
    *request.headers_mut().unwrap() = headers;
    let (parts, ()) = request.body(()).unwrap().into_parts();
    (parts, body)
}

fn now_ms() -> u64 {
    sigv4::parse_amz_date(NOW).expect("date")
}

/// UC: UC-GTW-025
#[test]
fn a_call_signed_for_the_service_of_the_port_gets_the_token_of_its_key() {
    let (parts, body) = signed_request("sns", "secret-1");
    let identity =
        authenticate("sns", &credentials(), true, now_ms(), &parts, &body).expect("signed for sns");
    assert_eq!(identity.token.as_deref(), Some("tb_a.b"));
    assert_eq!(identity.region, "eu-west-1");
}

/// UC: UC-GTW-025
#[test]
fn a_call_signed_for_another_service_is_refused() {
    let (parts, body) = signed_request("sqs", "secret-1");
    let error = authenticate("sns", &credentials(), true, now_ms(), &parts, &body)
        .err()
        .expect("an SQS signature at the SNS port");
    assert_eq!(error.json_code, "IncompleteSignature");
    assert!(error.message.contains("sns"), "{}", error.message);
}

/// UC: UC-GTW-025
#[test]
fn a_wrong_secret_an_unknown_key_and_a_missing_signature_are_refused() {
    let (parts, body) = signed_request("sns", "another-secret");
    let wrong = authenticate("sns", &credentials(), true, now_ms(), &parts, &body)
        .err()
        .expect("wrong secret");
    assert_eq!(wrong.json_code, "SignatureDoesNotMatch");

    let (parts, body) = signed_request("sns", "secret-1");
    let unknown = authenticate("sns", &Credentials::none(), true, now_ms(), &parts, &body)
        .err()
        .expect("unknown key");
    assert_eq!(unknown.json_code, "InvalidClientTokenId");

    let (bare, ()) = Request::builder().uri("/").body(()).unwrap().into_parts();
    let missing = authenticate("sns", &credentials(), true, now_ms(), &bare, b"")
        .err()
        .expect("no signature");
    assert_eq!(missing.json_code, "MissingAuthenticationToken");
}

/// UC: UC-GTW-025
#[test]
fn with_the_authentication_off_nobody_is_checked_and_the_region_is_the_signed_one_or_the_default() {
    let (parts, body) = signed_request("sns", "any");
    let signed = authenticate("sns", &Credentials::none(), false, now_ms(), &parts, &body)
        .expect("not checked");
    assert!(signed.token.is_none());
    assert_eq!(signed.region, "eu-west-1");

    let (bare, ()) = Request::builder().uri("/").body(()).unwrap().into_parts();
    let unsigned = authenticate("sns", &Credentials::none(), false, now_ms(), &bare, b"")
        .expect("not checked");
    assert_eq!(unsigned.region, naming::DEFAULT_REGION);
}
