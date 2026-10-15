//! Members, ring, protocol between nodes, channel, voting, freezing, synchronization and drain (doc 04, 05
//! and 13). For now: the node state and the local drain, the member table with the join of a
//! new node in two phases, the protocol (the types of calls between nodes) and the `Transport`, which
//! only has the in-memory version, the tests' (doc 18, section 17.5).

pub mod backup;
pub mod channel;
pub mod drain;
pub mod health;
pub mod members;
pub mod protocol;
pub mod recovery;
pub mod replication;
pub mod service;
pub mod state;
pub mod sync;
pub mod tls;
pub mod transport;
pub mod volatile;
pub mod vote;
