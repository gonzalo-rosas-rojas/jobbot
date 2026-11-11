//! Rules loaded from `profile/*.toml`. The types and the parsing live in the pure domain crate
//! (`jobbot_domain::rules`), the loaders in `jobbot_app::profile`; both are re-exported here.

pub use jobbot_app::profile::{load_criteria, load_exclusions};
pub use jobbot_domain::rules::*;
