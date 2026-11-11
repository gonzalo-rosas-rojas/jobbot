//! Execution window. The rules live in the pure domain crate
//! (`jobbot_domain::window`), the loader in `jobbot_app::profile`; both are re-exported here.

pub use jobbot_app::profile::load_schedule;
pub use jobbot_domain::window::*;
