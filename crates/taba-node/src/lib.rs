//! The Taba node. The `taba` binary (`main.rs`) is only the command line and the signals;
//! everything else lives here so the integration tests can bring real nodes up.
//!
//! The direction of dependencies is `core ← storage ← engine ← cluster ← http ← node`
//! (doc 18, section 17.3): each module knows only those on its left. `config` sits
//! beside them: it reads the file and the environment and depends only on `core`.

pub mod cli;
pub mod cluster;
pub mod config;
pub mod core;
pub mod engine;
pub mod http;
pub mod node;
pub mod secrets;
pub mod storage;
pub mod telemetry;
