//! Tipos de domínio e regras puras, sem I/O: relógio, IDs, erros e validações.
//!
//! Não conhece nenhum outro módulo do nó (doc 18, seção 17.3).

pub mod clock;
pub mod error;
pub mod id;
pub mod pagination;
pub mod time;
pub mod topic;
pub mod validation;
