//! Made-up profile files for tests that need rules but must not read the user's own `profile/`.
//! Every value is invented and generic; a test that depends on one says so by importing it, and a
//! test that needs a different number builds its own text (see `with_line`). The pay numbers are
//! the ones of the example `criteria.toml` the public repository ships, on purpose: nobody's own
//! floor or target belongs in a test.

use jobbot_domain::rules::{Criteria, Exclusions, parse_criteria, parse_exclusions};
use jobbot_domain::window::{Schedule, parse_schedule};

/// A valid `criteria.toml`: a floor, a target, a threshold, caps that all differ from each other
/// and from the defaults (so a test that reads one back can tell which setting it got), and just
/// enough titles, signals and skills for an AI-engineer posting to score above the threshold.
pub const SAMPLE_CRITERIA: &str = r#"
floor_monthly_usd = 3000
target_monthly_usd = [5000, 7000]
apply_threshold = 60
daily_cap = 5
daily_cap_until_reviewed = 2
company_cooldown_days = 30
first_n_reviewed = 4
max_posting_age_days = 40
skills = ["python", "typescript", "react", "rag", "mcp"]

[weights]
fit = 40
compensation = 20
stability = 10
learning = 10
interview_style = 10
other = 10

[titles]
tier1 = ["ai engineer", "llm engineer", "applied ai"]
tier2 = ["full-stack engineer", "software engineer"]
tier3 = ["backend engineer"]

[signals]
domain = ["llm", "agent", "agentic", "applied ai", "generative ai", "rag", "mcp", "full-stack"]
seniority_pos = ["senior", "staff", "lead"]
seniority_neg = ["junior", "intern"]
stability_pos = ["full-time", "permanent", "equity"]
stability_neg = ["contract", "temporary"]
learning = ["mentorship", "open source", "engineering culture"]
interview_pos = ["take-home", "pair programming"]
interview_neg = ["whiteboard", "leetcode"]
other_pos = ["remote", "async"]
"#;

/// A valid `exclusions.toml` with one made-up company and one made-up domain, no categories.
pub const SAMPLE_EXCLUSIONS: &str = r#"
names = ["Blocked Example Corp"]
domains = ["blocked-example.test"]
"#;

/// A valid `schedule.toml`: busy from 13:00 to 14:00 UTC every day.
pub const SAMPLE_SCHEDULE: &str = "[[busy]]\nzone = \"UTC\"\nstart = \"13:00\"\nend = \"14:00\"\n\
    days = [\"mon\", \"tue\", \"wed\", \"thu\", \"fri\", \"sat\", \"sun\"]\n";

/// `SAMPLE_CRITERIA`, parsed.
pub fn sample_criteria() -> Criteria {
    parse_criteria(SAMPLE_CRITERIA).expect("the sample criteria are valid")
}

/// `SAMPLE_EXCLUSIONS`, parsed.
pub fn sample_exclusions() -> Exclusions {
    parse_exclusions(SAMPLE_EXCLUSIONS).expect("the sample exclusions are valid")
}

/// The exclusion list the repository ships, read from the real `profile/` folder. Only for the
/// tests whose assertion names an entry of that list (a company or a category it holds); any
/// other test should use `sample_exclusions`, so that what it needs is written in the test.
pub fn shipped_exclusions() -> Exclusions {
    let path = crate::real_paths().profile_dir.join("exclusions.toml");
    // The error is dropped on purpose: a parse error quotes the lines of the file.
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("the shipped exclusions.toml can be read"));
    parse_exclusions(&text).unwrap_or_else(|_| panic!("the shipped exclusions.toml is valid"))
}

/// `SAMPLE_SCHEDULE`, parsed.
pub fn sample_schedule() -> Schedule {
    parse_schedule(SAMPLE_SCHEDULE).expect("the sample schedule is valid")
}

/// `text` with the line that starts with `key` replaced by `replacement`. Panics when no line
/// starts with `key`, so a test cannot silently change nothing.
pub fn with_line(text: &str, key: &str, replacement: &str) -> String {
    let mut found = false;
    let lines: Vec<String> = text
        .lines()
        .map(|line| {
            if line.starts_with(key) {
                found = true;
                replacement.to_string()
            } else {
                line.to_string()
            }
        })
        .collect();
    assert!(found, "no line starts with {key:?}");
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_samples_parse() {
        assert_eq!(sample_criteria().apply_threshold, 60.0);
        assert_eq!(sample_exclusions().names, ["Blocked Example Corp"]);
        assert_eq!(sample_schedule().busy.len(), 1);
    }

    #[test]
    fn with_line_replaces_only_the_line_that_starts_with_the_key() {
        let text = with_line(SAMPLE_CRITERIA, "first_n_reviewed", "first_n_reviewed = 9");
        assert_eq!(parse_criteria(&text).unwrap().first_n_reviewed, 9);
        assert_eq!(parse_criteria(&text).unwrap().daily_cap, 5);
    }

    #[test]
    #[should_panic(expected = "no line starts with")]
    fn with_line_refuses_a_key_that_is_not_there() {
        with_line(SAMPLE_CRITERIA, "not_a_key", "not_a_key = 1");
    }
}
