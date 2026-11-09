//! Nearbee core: domain types, the `Market` and `Runner` seams, the result
//! manifest, the hire journal types and configuration loading.
//!
//! The core depends on no adapter. Adapters (`nearbee-apb`,
//! `nearbee-market`) implement the traits defined here.

pub mod config;
pub mod journal;
pub mod manifest;
pub mod market;
pub mod redact;
pub mod runner;
pub mod secret;

pub use market::Market;
pub use runner::Runner;
pub use secret::Secret;
