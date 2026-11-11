//! `jobbot`: local job-application automation.

// The pure core lives in its own crate; these paths keep working for everything that imports
// `crate::models`, `crate::text`, `crate::scoring` and the rest.
pub use jobbot_domain::{exclusion, models, navigation, outbound, privacy, scoring, text};
// The SQLite storage is its own crate too; `crate::tracker` keeps naming it.
pub use jobbot_store::tracker;
// The job boards and their HTTP neighbours (is a posting still open, what a form asks).
pub use jobbot_sources::{http, open_check, questions, sources};

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
pub mod panel;
pub mod paths;
pub mod pipeline;
pub mod prep;
pub mod question_review;
pub mod results;
pub mod rules;
// The test support is a dev-dependency; the unit tests keep reading it as `crate::testing`.
#[cfg(test)]
use jobbot_testkit as testing;
pub mod today;
pub mod window;
