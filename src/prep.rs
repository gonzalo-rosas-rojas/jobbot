//! The interview drill. The bank and SM-2 are pure (`jobbot_domain::prep`) and the progress store
//! is in the database crate (`jobbot_store::prep`); both are re-exported here so every
//! `crate::prep::...` path keeps working.

pub use jobbot_domain::prep::{BankError, Question, load_questions, sm2};
pub use jobbot_store::prep::{GapRow, PrepError, PrepStats, PrepStore, TopicProgress};
