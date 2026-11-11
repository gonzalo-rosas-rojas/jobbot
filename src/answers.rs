//! Answer bank: the code is pure and lives in the domain crate (`jobbot_domain::answers`); the
//! application reads `profile/answers.toml` (`jobbot_app::profile`). The tests that pin the
//! shipped bank, Gonza's own answers, are in jobbot-app's `tests/use_cases/answers.rs`.

pub use jobbot_app::profile::load_answers;
pub use jobbot_domain::answers::*;
