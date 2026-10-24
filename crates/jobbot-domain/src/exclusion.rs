//! Exclusion engine: decides whether a posting belongs to an entity or category
//! the user must never apply to (`profile/exclusions.toml`).

use crate::models::Job;
use crate::rules::Exclusions;
use crate::text::{as_read, contains_word_lc, fold_look_alikes, host_of, normalize};

/// Outcome of `check_exclusion`. `reason` looks like `domain:harborlight-shipping.com`,
/// `name:Tidewater Systems` or `category:freight forwarding`.
#[derive(Debug, Clone, PartialEq)]
pub struct ExclusionResult {
    pub excluded: bool,
    pub reason: Option<String>,
}

impl ExclusionResult {
    fn excluded(reason: String) -> Self {
        ExclusionResult {
            excluded: true,
            reason: Some(reason),
        }
    }

    fn allowed() -> Self {
        ExclusionResult {
            excluded: false,
            reason: None,
        }
    }
}

/// Checks, in order, the apply and posting URL hosts (domains), the company name and
/// the title and description (names), and finally the category phrases. Empty fields
/// never match anything.
pub fn check_exclusion(job: &Job, rules: &Exclusions) -> ExclusionResult {
    let hosts: Vec<String> = [host_of(&job.url), host_of(&job.apply_url)]
        .into_iter()
        .filter(|host| !host.is_empty())
        .collect();
    for domain in &rules.domains {
        let domain = domain.trim().to_lowercase();
        if domain.is_empty() {
            continue;
        }
        let subdomain_suffix = format!(".{domain}");
        if hosts
            .iter()
            .any(|host| *host == domain || host.ends_with(&subdomain_suffix))
        {
            return ExclusionResult::excluded(format!("domain:{domain}"));
        }
    }

    // Both sides are read with look-alike letters folded, so a name or phrase spelled with
    // Cyrillic or Greek letters that look Latin still matches.
    let folded = |text: &str| fold_look_alikes(&as_read(text));
    let company = normalize(&folded(&job.company));
    let body = normalize(&folded(&format!("{} {}", job.title, job.description)));
    for name in &rules.names {
        let wanted = normalize(&folded(name));
        if !wanted.is_empty()
            && (contains_word_lc(&company, &wanted) || contains_word_lc(&body, &wanted))
        {
            return ExclusionResult::excluded(format!("name:{name}"));
        }
    }

    let text = folded(&format!(
        "{}\n{}\n{}",
        job.title, job.company, job.description
    ))
    .to_lowercase();
    for category in &rules.categories {
        if contains_word_lc(&text, &folded(category).to_lowercase()) {
            return ExclusionResult::excluded(format!("category:{category}"));
        }
    }
    ExclusionResult::allowed()
}

#[cfg(test)]
mod tests {
    use std::sync::LazyLock;

    use super::*;
    use crate::fixtures::{make_job, make_job_with};
    use crate::rules::Exclusions;
    use crate::shipped;

    static RULES: LazyLock<Exclusions> = LazyLock::new(shipped::exclusions);

    #[test]
    fn group_companies_excluded_by_name() {
        for company in [
            "Harbor Light Shipping and Logistics Center LLC",
            "Tidewater Systems Inc.",
            "Brightwave Technology",
            "Coastline Freight Inc.",
            "Anchor Point Consultants, LLC",
            "Anchor & Pier Point Consultants",
        ] {
            let job = make_job_with(|j| j.company = company.into());
            let result = check_exclusion(&job, &RULES);
            assert!(result.excluded, "{company} should be excluded");
            assert!(
                result
                    .reason
                    .as_deref()
                    .is_some_and(|r| r.starts_with("name:")),
                "{company}: {:?}",
                result.reason
            );
        }
    }

    #[test]
    fn excluded_by_apply_domain_even_with_other_company_name() {
        let job = make_job_with(|j| {
            j.company = "Staffing Partners".into();
            j.apply_url = "https://careers.harborlight-shipping.com/apply/9".into();
        });
        let result = check_exclusion(&job, &RULES);
        assert!(result.excluded);
        assert_eq!(
            result.reason.as_deref(),
            Some("domain:harborlight-shipping.com")
        );
    }

    #[test]
    fn domain_match_requires_a_label_boundary() {
        let job =
            make_job_with(|j| j.apply_url = "https://notharborlight-shipping.com/apply".into());
        assert!(!check_exclusion(&job, &RULES).excluded);
    }

    #[test]
    fn excluded_when_group_named_only_in_description() {
        let job = make_job_with(|j| {
            j.company = "Recruiting Agency".into();
            j.description = "Our client Tidewater Systems is hiring an engineer.".into();
        });
        let result = check_exclusion(&job, &RULES);
        assert!(result.excluded);
        assert_eq!(result.reason.as_deref(), Some("name:Tidewater Systems"));
    }

    #[test]
    fn category_freight_forwarding_excluded() {
        let job = make_job_with(|j| {
            j.company = "Harborwise".into();
            j.description = "We run remote fleet tracking for freight forwarding companies.".into();
        });
        let result = check_exclusion(&job, &RULES);
        assert!(result.excluded);
        assert!(
            result
                .reason
                .as_deref()
                .is_some_and(|r| r.starts_with("category:"))
        );
    }

    #[test]
    fn category_fleet_tracking_excluded_without_the_word_freight_forwarding() {
        let job = make_job_with(|j| {
            j.description = "We build Remote Fleet Tracking software.".into();
        });
        let result = check_exclusion(&job, &RULES);
        assert_eq!(
            result.reason.as_deref(),
            Some("category:remote fleet tracking")
        );
    }

    #[test]
    fn a_name_or_category_spelled_with_look_alike_letters_is_still_excluded() {
        // "Tidewater Systems" with a Cyrillic "Т", "е" and "Ѕ".
        let job = make_job_with(|j| j.company = "\u{0422}id\u{0435}water \u{0405}ystems".into());
        let result = check_exclusion(&job, &RULES);
        assert_eq!(result.reason.as_deref(), Some("name:Tidewater Systems"));
        // "freight forwarding" with a Cyrillic "е" and "о", hidden in the description.
        let job = make_job_with(|j| {
            j.company = "Acme".into();
            j.description = "We run fr\u{0435}ight f\u{043E}rwarding for retailers.".into();
        });
        let result = check_exclusion(&job, &RULES);
        assert_eq!(
            result.reason.as_deref(),
            Some("category:freight forwarding")
        );
    }

    #[test]
    fn unrelated_company_not_excluded() {
        assert!(!check_exclusion(&make_job(), &RULES).excluded);
    }

    #[test]
    fn a_company_sharing_a_word_with_an_excluded_name_is_not_excluded() {
        let job = make_job_with(|j| {
            j.company = "Harbor Foods".into();
            j.description = "Grocery delivery software.".into();
        });
        assert!(!check_exclusion(&job, &RULES).excluded);
    }

    #[test]
    fn empty_fields_do_not_crash() {
        let job = make_job_with(|j| {
            j.company.clear();
            j.title.clear();
            j.description.clear();
            j.url.clear();
            j.apply_url.clear();
        });
        let result = check_exclusion(&job, &RULES);
        assert!(!result.excluded);
        assert_eq!(result.reason, None);
    }

    #[test]
    fn empty_rule_entries_never_match() {
        let rules = Exclusions {
            names: vec!["".into(), "LLC".into()],
            domains: vec!["".into()],
            categories: vec![],
        };
        assert!(!check_exclusion(&make_job(), &rules).excluded);
        let blank = make_job_with(|j| {
            j.company.clear();
            j.apply_url.clear();
        });
        assert!(!check_exclusion(&blank, &rules).excluded);
    }
}
