//! Rules loaded from `profile/*.toml`: what to exclude and how to score and cap. This module holds
//! the types and the parsing; the application reads the files (`jobbot::rules::load_*`).

use serde::Deserialize;

/// Companies, domains and categories the user must never apply to or write about
/// (`profile/exclusions.toml`), for example a former employer and its group.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Exclusions {
    pub names: Vec<String>,
    pub domains: Vec<String>,
    pub categories: Vec<String>,
}

/// Score weights; they must add up to 100.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Weights {
    pub fit: f64,
    pub compensation: f64,
    pub stability: f64,
    pub learning: f64,
    pub interview_style: f64,
    pub other: f64,
}

impl Weights {
    pub fn total(&self) -> f64 {
        self.fit
            + self.compensation
            + self.stability
            + self.learning
            + self.interview_style
            + self.other
    }
}

/// Title phrases per tier: tier 1 is the target role, tier 2 a stretch, tier 3 a fallback.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TitleTiers {
    pub tier1: Vec<String>,
    pub tier2: Vec<String>,
    pub tier3: Vec<String>,
}

/// Phrases searched in the posting text. Each list feeds one part of the score.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Signals {
    /// Kind of work: applied AI, full-stack, architecture (fit).
    pub domain: Vec<String>,
    /// Seniority wording that matches Gonza's level (fit).
    pub seniority_pos: Vec<String>,
    /// Seniority wording below Gonza's level (fit).
    pub seniority_neg: Vec<String>,
    pub stability_pos: Vec<String>,
    pub stability_neg: Vec<String>,
    pub learning: Vec<String>,
    pub interview_pos: Vec<String>,
    pub interview_neg: Vec<String>,
    pub other_pos: Vec<String>,
    pub china: Vec<String>,
}

/// Scoring and gating settings.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Criteria {
    pub floor_monthly_usd: f64,
    pub target_monthly_usd: (f64, f64),
    pub apply_threshold: f64,
    /// Submissions per day once the first reviewed ones are approved.
    pub daily_cap: u32,
    /// Submissions per day until `first_n_reviewed` are approved.
    pub daily_cap_until_reviewed: u32,
    pub company_cooldown_days: i64,
    pub first_n_reviewed: u32,
    #[serde(default = "default_warmup_days")]
    pub warmup_days: i64,
    #[serde(default)]
    pub priority_companies: Vec<String>,
    /// New dry runs per run while the first submissions are still waiting for review.
    #[serde(default = "default_review_dry_runs_per_run")]
    pub review_dry_runs_per_run: u32,
    /// Postings older than this are withdrawn instead of submitted: they are probably filled, and
    /// an application to one would also burn the 90-day company cooldown.
    #[serde(default = "default_max_posting_age_days")]
    pub max_posting_age_days: i64,
    /// Places besides Chile where the user would take a job, on site or remote (the example
    /// profile lists Australia). Lowercase words, read as whole words ("australia", "sydney").
    /// Empty means Chile and the open regions only.
    #[serde(default)]
    pub extra_open_places: Vec<String>,
    /// `apply_threshold` for a posting in one of `extra_open_places`. `None` keeps the normal
    /// threshold. Such a posting is also never held back for being under the pay floor: the floor
    /// is a US-dollar figure, and pay in those places is usually stated in another currency.
    #[serde(default)]
    pub extra_open_apply_threshold: Option<f64>,
    /// Words (whole words, lowercase) one of which a title must hold to earn the extra threshold,
    /// so that sales, legal or marketing postings in those places do not earn it. A title without
    /// one still goes through the normal threshold. Empty: every title.
    #[serde(default)]
    pub extra_open_title_words: Vec<String>,
    pub weights: Weights,
    /// Technologies Gonza has used. They only add to fit, never subtract.
    #[serde(default)]
    pub skills: Vec<String>,
    #[serde(default)]
    pub titles: TitleTiers,
    #[serde(default)]
    pub signals: Signals,
}

fn default_warmup_days() -> i64 {
    14
}

fn default_review_dry_runs_per_run() -> u32 {
    5
}

fn default_max_posting_age_days() -> i64 {
    45
}

/// Why a rules file was refused. The messages are what Gonza reads after a wrong edit.
#[derive(Debug, thiserror::Error)]
pub enum RulesError {
    // The TOML message is part of ours (not a `#[source]`): printing the whole chain would say it twice.
    #[error("invalid exclusions: {0}")]
    Exclusions(toml::de::Error),
    #[error("invalid criteria: {0}")]
    Criteria(toml::de::Error),
    #[error("weights must sum to 100, got {0}")]
    WeightsDoNotSum(f64),
    #[error("target_monthly_usd must be an ascending range above floor_monthly_usd")]
    TargetBelowFloor,
    #[error("{field} must be a finite number")]
    NotFinite { field: &'static str },
    #[error("{field} must not be negative")]
    NegativeWeight { field: &'static str },
}

/// Parses the text of `exclusions.toml`. Domains are lowercased.
pub fn parse_exclusions(text: &str) -> Result<Exclusions, RulesError> {
    let mut rules: Exclusions = toml::from_str(text).map_err(RulesError::Exclusions)?;
    rules.domains = rules
        .domains
        .iter()
        .map(|d| d.trim().to_lowercase())
        .collect();
    Ok(rules)
}

/// TOML accepts `nan` and `inf` as floats, and every comparison with NaN is false: a NaN floor
/// would let every salary pass the floor check, a NaN weight would slip past the sum check. A
/// negative weight can make the sum 100 while the score leaves the 0 to 100 range.
fn check_numbers(criteria: &Criteria) -> Result<(), RulesError> {
    let weights = &criteria.weights;
    let weight_fields = [
        ("weights.fit", weights.fit),
        ("weights.compensation", weights.compensation),
        ("weights.stability", weights.stability),
        ("weights.learning", weights.learning),
        ("weights.interview_style", weights.interview_style),
        ("weights.other", weights.other),
    ];
    let (target_low, target_high) = criteria.target_monthly_usd;
    let other_fields = [
        ("floor_monthly_usd", criteria.floor_monthly_usd),
        ("target_monthly_usd", target_low),
        ("target_monthly_usd", target_high),
        ("apply_threshold", criteria.apply_threshold),
    ];
    let extra_threshold = criteria
        .extra_open_apply_threshold
        .map(|value| ("extra_open_apply_threshold", value));
    for (field, value) in weight_fields
        .into_iter()
        .chain(other_fields)
        .chain(extra_threshold)
    {
        if !value.is_finite() {
            return Err(RulesError::NotFinite { field });
        }
    }
    for (field, value) in weight_fields {
        if value < 0.0 {
            return Err(RulesError::NegativeWeight { field });
        }
    }
    Ok(())
}

/// Parses the text of `criteria.toml` and checks it is coherent.
pub fn parse_criteria(text: &str) -> Result<Criteria, RulesError> {
    let criteria: Criteria = toml::from_str(text).map_err(RulesError::Criteria)?;
    check_numbers(&criteria)?;
    let total = criteria.weights.total();
    if (total - 100.0).abs() > 1e-9 {
        return Err(RulesError::WeightsDoNotSum(total));
    }
    let (low, high) = criteria.target_monthly_usd;
    if low <= criteria.floor_monthly_usd || high < low {
        return Err(RulesError::TargetBelowFloor);
    }
    Ok(criteria)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL: &str = r#"
floor_monthly_usd = 1
target_monthly_usd = [2, 3]
apply_threshold = 1
daily_cap = 1
daily_cap_until_reviewed = 1
company_cooldown_days = 1
first_n_reviewed = 1

[weights]
fit = 50
compensation = 10
stability = 10
learning = 10
interview_style = 10
other = 10
"#;

    #[test]
    fn minimal_criteria_gets_defaults() {
        let c = parse_criteria(MINIMAL).unwrap();
        assert_eq!(c.warmup_days, 14);
        assert_eq!((c.review_dry_runs_per_run, c.max_posting_age_days), (5, 45));
        assert!(c.priority_companies.is_empty() && c.skills.is_empty());
        assert!(c.extra_open_places.is_empty());
        assert_eq!(c.extra_open_apply_threshold, None);
        assert!(c.extra_open_title_words.is_empty());
    }

    #[test]
    fn the_extra_threshold_is_read_and_must_be_finite() {
        let text = format!("extra_open_apply_threshold = 25\n{MINIMAL}");
        assert_eq!(
            parse_criteria(&text).unwrap().extra_open_apply_threshold,
            Some(25.0)
        );
        let text = format!("extra_open_apply_threshold = nan\n{MINIMAL}");
        let err = parse_criteria(&text).unwrap_err();
        assert!(
            err.to_string().contains("extra_open_apply_threshold"),
            "{err}"
        );
    }

    #[test]
    fn extra_open_places_are_read_from_the_file() {
        let text = format!("extra_open_places = [\"australia\", \"sydney\"]\n{MINIMAL}");
        let c = parse_criteria(&text).unwrap();
        assert_eq!(c.extra_open_places, ["australia", "sydney"]);
    }

    #[test]
    fn a_number_that_is_not_finite_is_refused_wherever_it_sits() {
        // TOML accepts `nan` and `inf`. A NaN floor would let every salary pass the floor check.
        for (from, to, field) in [
            (
                "floor_monthly_usd = 1",
                "floor_monthly_usd = nan",
                "floor_monthly_usd",
            ),
            ("[2, 3]", "[2, inf]", "target_monthly_usd"),
            (
                "apply_threshold = 1",
                "apply_threshold = nan",
                "apply_threshold",
            ),
            ("fit = 50", "fit = nan", "weights.fit"),
            ("other = 10", "other = inf", "weights.other"),
        ] {
            let err = parse_criteria(&MINIMAL.replace(from, to)).unwrap_err();
            assert!(
                matches!(err, RulesError::NotFinite { field: f } if f == field),
                "{field}: {err}"
            );
        }
    }

    #[test]
    fn a_negative_weight_is_refused_even_when_the_sum_is_100() {
        let text = MINIMAL
            .replace("fit = 50", "fit = 150")
            .replace("other = 10", "other = -90");
        let err = parse_criteria(&text).unwrap_err();
        assert!(
            matches!(
                err,
                RulesError::NegativeWeight {
                    field: "weights.other"
                }
            ),
            "{err}"
        );
    }

    #[test]
    fn weights_must_sum_to_100() {
        let err = parse_criteria(&MINIMAL.replace("fit = 50", "fit = 45")).unwrap_err();
        assert!(err.to_string().contains("sum to 100"), "{err}");
    }

    #[test]
    fn target_must_sit_above_the_floor() {
        let err = parse_criteria(&MINIMAL.replace("[2, 3]", "[1, 3]")).unwrap_err();
        assert!(err.to_string().contains("above floor_monthly_usd"), "{err}");
    }

    #[test]
    fn unknown_keys_are_rejected() {
        let err = parse_criteria(&format!("daily_capp = 3\n{MINIMAL}")).unwrap_err();
        assert!(format!("{err:#}").contains("daily_capp"), "{err:#}");
    }

    #[test]
    fn exclusion_domains_are_lowercased() {
        let ex = parse_exclusions("domains = [\" Harborlight-Shipping.com \"]").unwrap();
        assert_eq!(ex.domains, vec!["harborlight-shipping.com".to_string()]);
        assert!(ex.names.is_empty() && ex.categories.is_empty());
    }
}
