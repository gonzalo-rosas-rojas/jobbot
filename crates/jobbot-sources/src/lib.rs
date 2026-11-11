//! jobbot's job boards: one adapter per public job API or feed (`sources`), the check that a
//! posting is still open (`open_check`) and the reader of what an application form asks
//! (`questions`). Every GET of the sources goes through `http`: one stack of retries, resting
//! hosts and a run deadline over one client.
//!
//! This crate is an adapter: it knows HTTP and the domain types, and nothing about the database,
//! Claude, the browser or the panel (`tests/architecture.rs` of the application enforces that).

pub mod http;
pub mod open_check;
pub mod questions;
pub mod sources;
