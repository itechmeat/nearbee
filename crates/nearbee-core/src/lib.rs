//! Nearbee core: domain types, the `Market` and `Runner` seams, the result
//! manifest, the hire journal types and configuration loading.
//!
//! The core depends on no adapter. Adapters (`nearbee-apb`,
//! `nearbee-market`) implement the traits defined here.

pub mod attachments;
pub mod catalog;
pub mod clock;
pub mod config;
pub mod cost;
pub mod credentials;
pub mod evidence;
pub mod intake;
pub mod journal;
pub mod listing;
pub mod lock;
pub mod manifest;
pub mod market;
pub mod redact;
pub mod retention;
pub mod runner;
pub mod secret;
pub mod serve;
pub mod signature;
pub mod statement;
pub mod tiers;

pub use market::Market;
pub use runner::Runner;
pub use secret::Secret;
