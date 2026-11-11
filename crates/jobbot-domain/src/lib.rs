//! The pure core of jobbot: the job model, the application state machine, text helpers, the
//! navigation policy, the profile rules, scoring, exclusion, the outbound-text filters and the
//! interview bank with its SM-2 scheduling, with no I/O.
//!
//! Nothing here touches the file system, the network, a database, a browser or a clock. What
//! this crate may depend on is written down, and enforced, in `tests/architecture.rs` of the
//! application crate.

pub mod answers;
pub mod exclusion;
pub mod fixtures;
pub mod form;
pub mod gate;
pub mod handoff;
pub mod models;
pub mod navigation;
pub mod outbound;
pub mod prep;
pub mod privacy;
pub mod questions;
pub mod region;
pub mod rules;
pub mod scoring;
#[cfg(test)]
mod shipped;
pub mod state;
pub mod text;
pub mod untrusted;
pub mod window;
