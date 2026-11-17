//! The SQS JSON protocol decoder (the body of `application/x-amz-json-1.0`), which reads the bytes of any client.
#![no_main]

use libfuzzer_sys::fuzz_target;
use taba_node::gateway::sqs::protocol_json;

fuzz_target!(|data: &[u8]| {
    let _ = protocol_json::decode(data);
});
