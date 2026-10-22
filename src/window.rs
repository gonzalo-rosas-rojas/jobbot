//! Execution window (spec section 3). The rules live in the pure domain crate
//! (`jobbot_domain::window`) and are re-exported here; this file only reads `profile/schedule.toml`.

use std::path::Path;

use anyhow::{Context, Result};

pub use jobbot_domain::window::*;

pub fn load_schedule(path: &Path) -> Result<Schedule> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    parse_schedule(&text).with_context(|| format!("in {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_schedule_reads_the_profile_file() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("profile")
            .join("schedule.toml");
        let s = load_schedule(&path).unwrap();
        assert_eq!(s.busy.len(), 2);
        assert!(load_schedule(&path.with_file_name("missing.toml")).is_err());
    }
}
