//! Domain types and pure rules, with no I/O: clock, IDs, errors and validations.
//!
//! It knows no other module of the node (doc 18, section 17.3).

pub mod clock;
pub mod content;
pub mod error;
pub mod id;
pub mod pagination;
pub mod ring;
pub mod time;
pub mod token;
pub mod topic;
pub mod validation;
