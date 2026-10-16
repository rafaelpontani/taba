//! `taba-cli`: the Taba client command line (doc 19b). The library exists for the
//! tests; the program is `main.rs`.

// The API error carries the response body and only exists on the failure path, so its size does not matter.
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

// The catalogs of the interface (doc 34, section 8): `locales/en-US.yml` is the source and the fallback.
#[cfg(feature = "tui")]
rust_i18n::i18n!("locales", fallback = "en-US");
