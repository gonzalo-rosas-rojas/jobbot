//! `jobbot`: local job-application automation.

// The pure core lives in its own crate; these paths keep working for everything that imports
// `crate::models`, `crate::text`, `crate::scoring` and the rest.
pub use jobbot_domain::{exclusion, models, navigation, outbound, scoring, text};

pub mod actions;
pub mod answers;
pub mod claude;
pub mod cli;
pub mod cv;
pub mod digest;
pub mod discovery;
pub mod dossier;
pub mod form;
pub mod freelance;
pub mod gate;
pub mod handoff;
pub mod inbox;
pub mod open_check;
pub mod panel;
pub mod paths;
pub mod pipeline;
pub mod prep;
pub mod question_review;
pub mod questions;
pub mod results;
pub mod rules;
pub mod sources;
pub mod testing;
pub mod today;
pub mod tracker;
pub mod window;
