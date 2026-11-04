use super::*;

/// UC: UC-GTW-025
#[test]
fn the_answer_carries_the_status_the_content_type_the_request_id_and_the_extra_headers() {
    let response = build(
        StatusCode::SERVICE_UNAVAILABLE,
        "text/xml",
        b"<x/>".to_vec(),
        "req-1",
        &[("retry-after", "3".to_owned())],
    );
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let headers = response.headers();
    assert_eq!(headers[header::CONTENT_TYPE], "text/xml");
    assert_eq!(headers["x-amzn-requestid"], "req-1");
    assert_eq!(headers["retry-after"], "3");
}
