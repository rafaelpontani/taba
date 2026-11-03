//! The Taba Rust connector (doc 19): the nine operations of the client API, the common contract's call retries
//! and its own HTTP/1.1 over `std::net::TcpStream`, with `rustls` for TLS.

// The error carries the response body and only exists on the failure path, so its size does not matter.
#![allow(clippy::result_large_err)]

pub mod base64;
pub mod client;
pub mod http;
pub mod json;
#[doc(hidden)]
pub mod testing;
pub mod transport;

pub use client::{
    BatchMessage, BatchResult, Call, Client, Config, ConsumeError, ConsumeOptions, Delivery, Error,
    HandlerError, Message, Nullable, Page, PublishOptions, PublishResponse, Reply, Topic,
    TopicRequest, TopicUpdate, VisibilityResponse, CONTRACT_VERSION,
};
