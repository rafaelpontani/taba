//! Conector Rust do Taba (doc 19): as nove operações da API de cliente, a repetição de chamadas do
//! contrato comum e um HTTP/1.1 próprio sobre `std::net::TcpStream`, com o `rustls` para o TLS.

// O erro carrega o corpo da resposta e só existe no caminho de falha, então seu tamanho não pesa.
#![allow(clippy::result_large_err)]

pub mod base64;
pub mod client;
pub mod http;
pub mod json;
#[doc(hidden)]
pub mod testing;
pub mod transport;

pub use client::{
    Call, Client, Config, ConsumeError, ConsumeOptions, Delivery, Error, HandlerError, Message,
    Nullable, Page, PublishOptions, PublishResponse, Reply, Topic, TopicRequest, TopicUpdate,
    VisibilityResponse, CONTRACT_VERSION,
};
