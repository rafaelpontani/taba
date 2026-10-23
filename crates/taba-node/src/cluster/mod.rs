//! Members, ring, protocol between nodes, channel, voting, freezing, synchronization and drain (doc 04, 05
//! and 13). The `Transport` has two implementations: HTTP with mutual TLS (`transport/http`), used in
//! production, and the in-memory network of the tests (`transport/memory`; doc 18, section 17.5).

pub mod backup;
pub mod channel;
pub mod drain;
pub mod health;
pub mod members;
pub mod protocol;
pub mod recovery;
pub mod replication;
pub mod restore;
pub mod service;
pub mod state;
pub mod sync;
pub mod tls;
pub mod transport;
pub mod upgrade;
pub mod volatile;
pub mod vote;
