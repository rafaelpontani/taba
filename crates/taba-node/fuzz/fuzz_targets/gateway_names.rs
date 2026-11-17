//! The names a gateway client sends: receipt handles, queue names and URLs, topic names and subscription ARNs.
#![no_main]

use libfuzzer_sys::fuzz_target;
use taba_node::gateway::{naming, sqs::receipt};

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let _ = receipt::decode(text);
    let _ = naming::parse(text);
    let _ = naming::parse_topic(text);
    let _ = naming::parse_subscription_arn(text);
});
