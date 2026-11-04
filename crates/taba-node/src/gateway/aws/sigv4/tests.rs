use super::*;
use axum::http::HeaderValue;

const SECRET: &str = "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY";

fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
    let mut map = HeaderMap::new();
    for (name, value) in pairs {
        map.append(
            axum::http::HeaderName::from_bytes(name.as_bytes()).expect("header name"),
            HeaderValue::from_str(value).expect("header value"),
        );
    }
    map
}

/// The `get-vanilla` case of the AWS Signature Version 4 test suite.
fn vanilla() -> HeaderMap {
    headers(&[
        ("host", "example.amazonaws.com"),
        ("x-amz-date", "20150830T123600Z"),
        (
            "authorization",
            "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20150830/us-east-1/service/aws4_request, SignedHeaders=host;x-amz-date, Signature=5fa00fa31553b73ebf1942676e86291e8372ff2a2260956d9b8aae1d763fbf31",
        ),
    ])
}

fn at(date: &str) -> u64 {
    parse_amz_date(date).expect("date")
}

/// UC: UC-GTW-002
#[test]
fn hmac_matches_the_rfc_4231_vector() {
    let mac = hmac_sha256(b"Jefe", b"what do ya want for nothing?");
    assert_eq!(
        hex(&mac),
        "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
    );
}

/// UC: UC-GTW-002
#[test]
fn the_signature_of_the_aws_test_suite_is_accepted() {
    let headers = vanilla();
    let parsed = parse(&headers).expect("parse");
    assert_eq!(parsed.access_key, "AKIDEXAMPLE");
    assert_eq!(parsed.region, "us-east-1");
    assert_eq!(parsed.service, "service");
    let request = Signed {
        method: "GET",
        path: "/",
        query: "",
        headers: &headers,
        body: b"",
    };
    assert_eq!(
        parsed.verify(SECRET, &request, at("20150830T123600Z")),
        Ok(())
    );
}

/// UC: UC-GTW-002
#[test]
fn a_wrong_secret_or_a_changed_request_does_not_match() {
    let headers = vanilla();
    let parsed = parse(&headers).expect("parse");
    let now = at("20150830T123600Z");
    let request = Signed {
        method: "GET",
        path: "/",
        query: "",
        headers: &headers,
        body: b"",
    };
    assert_eq!(
        parsed.verify("another-secret", &request, now),
        Err(SigError::Mismatch)
    );
    let other_path = Signed {
        path: "/other",
        ..Signed {
            method: "GET",
            path: "/",
            query: "",
            headers: &headers,
            body: b"",
        }
    };
    assert_eq!(
        parsed.verify(SECRET, &other_path, now),
        Err(SigError::Mismatch)
    );
    let other_body = Signed {
        method: "GET",
        path: "/",
        query: "",
        headers: &headers,
        body: b"changed",
    };
    assert_eq!(
        parsed.verify(SECRET, &other_body, now),
        Err(SigError::Mismatch)
    );
}

/// UC: UC-GTW-002
#[test]
fn a_signature_far_from_the_clock_is_expired() {
    let headers = vanilla();
    let parsed = parse(&headers).expect("parse");
    let request = Signed {
        method: "GET",
        path: "/",
        query: "",
        headers: &headers,
        body: b"",
    };
    let signed_at = at("20150830T123600Z");
    assert_eq!(
        parsed.verify(SECRET, &request, signed_at + MAX_SKEW_MS + 1000),
        Err(SigError::Expired)
    );
    assert_eq!(
        parsed.verify(SECRET, &request, signed_at - MAX_SKEW_MS - 1000),
        Err(SigError::Expired)
    );
    assert_eq!(
        parsed.verify(SECRET, &request, signed_at + MAX_SKEW_MS - 1000),
        Ok(())
    );
}

/// UC: UC-GTW-002
#[test]
fn what_sign_produces_verify_accepts_with_a_body_and_a_query() {
    let mut headers = headers(&[
        ("host", "gw.example:9324"),
        ("x-amz-date", "20261104T101500Z"),
        ("content-type", "application/x-www-form-urlencoded"),
    ]);
    let body = b"Action=ListQueues&Version=2012-11-05&QueueNamePrefix=a%20b";
    let request = Signed {
        method: "POST",
        path: "/000000000000/my-queue",
        query: "b=2&a=1&c=%2F+x",
        headers: &headers,
        body,
    };
    let authorization = sign(
        "AKID",
        "secret",
        "eu-west-1",
        "sqs",
        "20261104T101500Z",
        &request,
        &["content-type", "host", "x-amz-date"],
    );
    headers.insert(
        axum::http::header::AUTHORIZATION,
        HeaderValue::from_str(&authorization).expect("value"),
    );
    let parsed = parse(&headers).expect("parse");
    assert_eq!(parsed.region, "eu-west-1");
    let request = Signed {
        method: "POST",
        path: "/000000000000/my-queue",
        // The order of the pairs on the wire does not matter.
        query: "a=1&c=%2F+x&b=2",
        headers: &headers,
        body,
    };
    assert_eq!(
        parsed.verify("secret", &request, at("20261104T101500Z")),
        Ok(())
    );
}

/// UC: UC-GTW-002
#[test]
fn a_declared_payload_hash_that_is_not_the_body_is_refused() {
    let mut headers = vanilla();
    headers.insert(
        "x-amz-content-sha256",
        HeaderValue::from_static(
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        ),
    );
    let parsed = parse(&headers).expect("parse");
    let request = Signed {
        method: "GET",
        path: "/",
        query: "",
        headers: &headers,
        body: b"not empty",
    };
    assert_eq!(
        parsed.verify(SECRET, &request, at("20150830T123600Z")),
        Err(SigError::Mismatch)
    );
}

/// UC: UC-GTW-002
#[test]
fn a_missing_or_malformed_authorization_header_is_reported() {
    assert_eq!(parse(&HeaderMap::new()).unwrap_err(), SigError::Missing);
    assert_eq!(
        parse(&headers(&[("authorization", "Bearer abc")])).unwrap_err(),
        SigError::Missing
    );
    assert!(matches!(
        parse(&headers(&[(
            "authorization",
            "AWS4-HMAC-SHA256 Credential=AKID/20150830/us-east-1/sqs, SignedHeaders=host, Signature=ab"
        )]))
        .unwrap_err(),
        SigError::Malformed(_)
    ));
    assert!(matches!(
        parse(&headers(&[(
            "authorization",
            "AWS4-HMAC-SHA256 Credential=AKID/20150830/us-east-1/sqs/aws4_request, SignedHeaders=x-amz-date, Signature=ab"
        )]))
        .unwrap_err(),
        SigError::Malformed(_)
    ));
}

/// UC: UC-GTW-002
#[test]
fn the_amz_date_is_read_in_utc() {
    assert_eq!(at("19700101T000000Z"), 0);
    assert_eq!(at("20150830T123600Z"), 1_440_938_160_000);
    assert!(parse_amz_date("2015-08-30").is_none());
    assert!(parse_amz_date("20151330T123600Z").is_none());
}
