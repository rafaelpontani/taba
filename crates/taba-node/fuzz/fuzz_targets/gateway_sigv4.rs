//! The AWS Signature Version 4 readers: the `Authorization` header, the `X-Amz-Date` and the percent-decoding of
//! the canonical request. They run on every gateway request, before the caller is known.
#![no_main]

use http::{HeaderMap, HeaderValue, header::AUTHORIZATION};
use libfuzzer_sys::fuzz_target;
use taba_node::gateway::aws::sigv4;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let _ = sigv4::parse_amz_date(text);
    let _ = sigv4::percent_decode(text);
    if let Ok(value) = HeaderValue::from_str(text) {
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, value);
        let _ = sigv4::parse(&headers);
    }
});
