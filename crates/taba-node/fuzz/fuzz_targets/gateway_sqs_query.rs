//! The SQS Query protocol decoder (form of the query string and of the body), which reads the bytes of any client
//! before the signature is checked. The first line is the query string and the rest is the body.
#![no_main]

use libfuzzer_sys::fuzz_target;
use taba_node::gateway::sqs::protocol_query;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let (query, body) = text.split_once('\n').unwrap_or((text, ""));
    let _ = protocol_query::decode(query, body);
});
