//! jobbot's storage: one SQLite database for the job search (`tracker`) and one for the interview
//! drill's progress (`prep`).
//!
//! This crate is an adapter: it knows SQLite and the domain types, and nothing about job boards,
//! Claude, the browser or the panel (`tests/architecture.rs` of the application enforces that).

pub mod prep;
mod repos;
pub mod tracker;
