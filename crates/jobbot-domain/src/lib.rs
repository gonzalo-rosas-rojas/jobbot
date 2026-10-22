//! The pure core of jobbot: the job model, the application state machine, text helpers, the
//! navigation policy, the profile rules, scoring, exclusion and the outbound-text filters, with
//! no I/O.
//!
//! Nothing here touches the file system, the network, a database, a browser or a clock. What
//! this crate may depend on is written down, and enforced, in `tests/architecture.rs` of the
//! application crate.

pub mod exclusion;
pub mod fixtures;
pub mod models;
pub mod navigation;
pub mod outbound;
pub mod rules;
pub mod scoring;
#[cfg(test)]
mod shipped;
pub mod state;
pub mod text;
pub mod window;
