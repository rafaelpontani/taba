//! The SNS Query protocol decoder. The first line is the query string and the rest is the body.
#![no_main]

use libfuzzer_sys::fuzz_target;
use taba_node::gateway::sns::protocol;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let (query, body) = text.split_once('\n').unwrap_or((text, ""));
    let _ = protocol::decode(query, body);
});
