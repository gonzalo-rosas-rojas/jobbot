//! Rules loaded from `profile/*.toml`. The types and the parsing live in the pure domain crate
//! (`jobbot_domain::rules`) and are re-exported here; this file only reads the files.

use std::path::Path;

use anyhow::Context;

pub use jobbot_domain::rules::*;

pub fn load_exclusions(path: &Path) -> anyhow::Result<Exclusions> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    parse_exclusions(&text).with_context(|| format!("in {}", path.display()))
}

pub fn load_criteria(path: &Path) -> anyhow::Result<Criteria> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    parse_criteria(&text).with_context(|| format!("in {}", path.display()))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::testing::real_paths;

    #[test]
    fn real_exclusions_cover_the_listed_entities() {
        let ex = load_exclusions(&real_paths().profile_dir.join("exclusions.toml")).unwrap();
        let names: Vec<String> = ex.names.iter().map(|n| n.to_lowercase()).collect();
        for required in [
            "harbor light shipping and logistics center",
            "tidewater systems",
            "brightwave technology",
            "coastline freight",
            "anchor point consultants",
            "anchor & pier point consultants",
        ] {
            assert!(names.iter().any(|n| n == required), "missing {required}");
        }
        assert!(ex.domains.contains(&"harborlight-shipping.com".to_string()));
        assert!(ex.categories.contains(&"freight forwarding".to_string()));
    }

    #[test]
    fn real_criteria_values() {
        let c = load_criteria(&real_paths().profile_dir.join("criteria.toml")).unwrap();
        assert_eq!(c.floor_monthly_usd, 3000.0);
        assert_eq!(c.target_monthly_usd, (5000.0, 7000.0));
        assert_eq!(c.apply_threshold, 60.0);
        assert_eq!(c.daily_cap, 25);
        assert_eq!(c.daily_cap_until_reviewed, 10);
        assert_eq!(c.company_cooldown_days, 90);
        assert_eq!(c.first_n_reviewed, 5);
        assert_eq!(c.warmup_days, 14);
        assert_eq!(c.review_dry_runs_per_run, 5);
        assert_eq!(c.max_posting_age_days, 45);
        assert_eq!(c.weights.total(), 100.0);
        assert_eq!(c.weights.fit, 30.0);
        assert!(!c.skills.is_empty());
        assert!(!c.titles.tier1.is_empty());
        assert!(!c.signals.domain.is_empty());
    }

    #[test]
    fn missing_file_reports_its_path() {
        let err = load_criteria(Path::new("no/such/criteria.toml")).unwrap_err();
        assert!(err.to_string().contains("criteria.toml"), "{err}");
    }
}
