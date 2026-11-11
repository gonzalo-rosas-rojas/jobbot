//! Repository chores that are not part of jobbot itself.
//!
//! `export` builds a public copy of one commit: only the paths of a whitelist, the example
//! profile in place of the real one, every file scanned for private terms, secrets and e-mail
//! addresses, and, when nothing is found, a fresh repository with a single commit. Nothing is
//! pushed: publishing stays a person's decision.

pub mod export;
pub mod scan;
pub mod terms;
