//! O `taba-cli`: a linha de comando do cliente do Taba (doc 19b). A biblioteca existe para os
//! testes; o programa é o `main.rs`.

// O erro da API carrega o corpo da resposta e só existe no caminho de falha, então seu tamanho não pesa.
#![allow(clippy::result_large_err)]

pub mod app;
pub mod args;
pub mod commands;
pub mod output;
pub mod profile;
pub mod settings;
pub mod system;
pub mod tui;
pub mod yaml;

pub use app::run;
