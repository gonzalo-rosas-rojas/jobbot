//! Exclusion engine: decides whether a posting belongs to an entity or category
//! Gonza must never apply to.

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
    excluded_by_domain(job, rules)
        .or_else(|| excluded_by_name(job, rules))
        .or_else(|| excluded_by_category(job, rules))
        .map_or_else(ExclusionResult::allowed, ExclusionResult::excluded)
}

/// Both sides are read with look-alike letters folded, so a name or phrase spelled with
/// Cyrillic or Greek letters that look Latin still matches.
fn folded(text: &str) -> String {
    fold_look_alikes(&as_read(text))
}

/// `domain:<d>` when the apply or posting URL is on an excluded domain or one of its subdomains.
fn excluded_by_domain(job: &Job, rules: &Exclusions) -> Option<String> {
    let hosts: Vec<String> = [host_of(&job.url), host_of(&job.apply_url)]
        .into_iter()
        .filter(|host| !host.is_empty())
        .collect();
    rules.domains.iter().find_map(|domain| {
        let domain = domain.trim().to_lowercase();
        let subdomain_suffix = format!(".{domain}");
        let on_domain = !domain.is_empty()
            && hosts
                .iter()
                .any(|host| *host == domain || host.ends_with(&subdomain_suffix));
        on_domain.then(|| format!("domain:{domain}"))
    })
}

/// `name:<n>` when an excluded name is the company or appears in the title or description.
fn excluded_by_name(job: &Job, rules: &Exclusions) -> Option<String> {
    let company = normalize(&folded(&job.company));
    let body = normalize(&folded(&format!("{} {}", job.title, job.description)));
    rules.names.iter().find_map(|name| {
        let wanted = normalize(&folded(name));
        let named = !wanted.is_empty()
            && (contains_word_lc(&company, &wanted) || contains_word_lc(&body, &wanted));
        named.then(|| format!("name:{name}"))
    })
}

/// `category:<c>` when an excluded category phrase appears in the title, company or description.
fn excluded_by_category(job: &Job, rules: &Exclusions) -> Option<String> {
    let text = folded(&format!(
        "{}\n{}\n{}",
        job.title, job.company, job.description
    ))
    .to_lowercase();
    rules
        .categories
        .iter()
        .find(|category| contains_word_lc(&text, &folded(category).to_lowercase()))
        .map(|category| format!("category:{category}"))
}

#[cfg(test)]
mod tests {
    use std::sync::LazyLock;

    use super::*;
    use crate::fixtures::{make_job, make_job_with};
    use crate::rules::Exclusions;
    use crate::shipped;

    static RULES: LazyLock<Exclusions> = LazyLock::new(shipped::exclusions);

    /// A small rule set of one made-up name and one category, to test the matching itself.
    fn blocked_rules() -> Exclusions {
        Exclusions {
            names: vec!["Blocked Industries".into()],
            domains: vec![],
            categories: vec!["gambling".into()],
        }
    }

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
            j.company = "Wavecrest".into();
            j.description = "We run remote fleet tracking for freight forwarding desks.".into();
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
    fn category_fleet_tracking_excluded_without_the_words_freight_forwarding() {
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
    fn unrelated_company_not_excluded() {
        assert!(!check_exclusion(&make_job(), &RULES).excluded);
    }

    #[test]
    fn harbor_lighthouse_museum_is_not_excluded() {
        let job = make_job_with(|j| {
            j.company = "Harbor Lighthouse Museum".into();
            j.description = "Museum IT.".into();
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
    fn a_name_or_category_split_by_an_invisible_character_or_written_wide_is_still_excluded() {
        let rules = blocked_rules();
        for company in ["Blo\u{200B}cked Industries", "Ｂｌｏｃｋｅｄ Industries"] {
            let job = make_job_with(|j| j.company = company.into());
            assert!(check_exclusion(&job, &rules).excluded, "{company:?}");
        }
        for word in ["gam\u{00AD}bling", "ｇａｍｂｌｉｎｇ"] {
            let job = make_job_with(|j| j.description = format!("A {word} platform."));
            assert!(check_exclusion(&job, &rules).excluded, "{word:?}");
        }
    }

    #[test]
    fn a_name_or_category_spelled_with_look_alike_letters_is_still_excluded() {
        let rules = blocked_rules();
        // Cyrillic Ve; Cyrillic o, es and ie; Greek Beta; mathematical bold B.
        for company in [
            "\u{0412}locked Industries",
            "Bl\u{043E}\u{0441}k\u{0435}d Industries",
            "\u{0392}locked Industries",
            "\u{1D401}locked Industries",
        ] {
            let job = make_job_with(|j| j.company = company.into());
            assert!(check_exclusion(&job, &rules).excluded, "{company:?}");
        }
        let job = make_job_with(|j| j.description = "A g\u{0430}mbling platform.".into());
        assert!(check_exclusion(&job, &rules).excluded);
        // Look-alikes do not join words that differ: neither of these is the excluded name.
        for company in ["Locked Industries", "Blocker Industries"] {
            let job = make_job_with(|j| j.company = company.into());
            assert!(!check_exclusion(&job, &rules).excluded, "{company:?}");
        }
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
