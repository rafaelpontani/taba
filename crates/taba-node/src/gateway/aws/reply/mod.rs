//! The HTTP answer of an AWS port (doc 39): status, content type and the `x-amzn-requestid` header, shared by SQS and SNS.

use axum::body::Body;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::Response;

/// Builds an answer. `extra` are more headers (`retry-after`, `x-amzn-query-error`).
pub fn build(
    status: StatusCode,
    content_type: &'static str,
    body: Vec<u8>,
    request_id: &str,
    extra: &[(&'static str, String)],
) -> Response {
    let mut response = Response::new(Body::from(body));
    *response.status_mut() = status;
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    if let Ok(value) = HeaderValue::from_str(request_id) {
        headers.insert("x-amzn-requestid", value);
    }
    for (name, value) in extra {
        if let Ok(value) = HeaderValue::from_str(value) {
            headers.insert(*name, value);
        }
    }
    response
}

#[cfg(test)]
mod tests;
