//! Salary parsing and the 0-100 job score (spec section 4).

use std::collections::BTreeMap;
use std::sync::LazyLock;

use regex::Regex;

use crate::models::Job;
use crate::rules::Criteria;
use crate::text::contains_word_lc;

/// `$18-$22`, `$120,000 - $160,000`, `$257K - $335K`, `$120 - $160k`.
static RANGE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\$\s?([0-9]{1,3}(?:,[0-9]{3})+|[0-9]+(?:\.[0-9]+)?)\s?(k)?\s*(?:-|–|—|to)\s*\$?\s?([0-9]{1,3}(?:,[0-9]{3})+|[0-9]+(?:\.[0-9]+)?)\s?(k)?",
    )
    .unwrap()
});

static HOURLY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)/\s?h(?:ou)?r|per hour|hourly|/h\b").unwrap());

/// Reads the first pay range in `text` as `(min, max)` US dollars per month: hourly
/// rates times 160, annual amounts divided by 12, monthly amounts as they are. A range
/// that is not clearly pay (`$5 - $10 million`) is ignored.
pub fn parse_salary_monthly_usd(text: &str) -> (Option<f64>, Option<f64>) {
    for caps in RANGE.captures_iter(text) {
        let (Some(low_text), Some(high_text)) = (caps.get(1), caps.get(3)) else {
            continue;
        };
        let parse = |s: &str| s.replace(',', "").parse::<f64>().ok();
        let (Some(mut low), Some(mut high)) = (parse(low_text.as_str()), parse(high_text.as_str()))
        else {
            continue;
        };
        let mut low_k = caps.get(2).is_some();
        let high_k = caps.get(4).is_some();
        // "$120 - $160k": the k applies to both ends.
        if high_k && !low_k && low < 1000.0 {
            low_k = true;
        }
        if low_k {
            low *= 1000.0;
        }
        if high_k {
            high *= 1000.0;
        }
        if low > high {
            std::mem::swap(&mut low, &mut high);
        }
        let end = caps.get(0).map_or(0, |whole| whole.end());
        let tail: String = text[end..].chars().take(24).collect();
        if high < 300.0 {
            if HOURLY.is_match(&tail) {
                return (Some(low * 160.0), Some(high * 160.0));
            }
            continue;
        }
        if high >= 20_000.0 {
            return (Some(low / 12.0), Some(high / 12.0));
        }
        if high >= 1000.0 {
            return (Some(low), Some(high));
        }
    }
    (None, None)
}

/// Case-insensitive whole-word match that also accepts a plural: `"LLMs"` contains
/// `"llm"`, `"best"` does not contain `"est"`.
pub fn contains_term(text: &str, term: &str) -> bool {
    contains_term_lc(&text.to_lowercase(), &term.to_lowercase())
}

fn contains_term_lc(text_lc: &str, term_lc: &str) -> bool {
    contains_word_lc(text_lc, term_lc) || contains_word_lc(text_lc, &format!("{term_lc}s"))
}

/// How many of `terms` appear in the lowercase text.
fn hits(text_lc: &str, terms: &[String]) -> usize {
    terms
        .iter()
        .filter(|term| contains_term_lc(text_lc, &term.to_lowercase()))
        .count()
}

fn any_hit(text_lc: &str, terms: &[String]) -> bool {
    hits(text_lc, terms) > 0
}

fn round1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

fn unit(x: f64) -> f64 {
    x.clamp(0.0, 1.0)
}

/// Points available inside the fit score: title 12, domain 8, seniority 6, skills 4.
const FIT_POINTS: f64 = 30.0;

#[derive(Debug, Clone, PartialEq)]
pub struct ScoreResult {
    pub total: f64,
    /// Keys: fit, compensation, stability, learning, interview_style, other.
    pub detail: BTreeMap<String, f64>,
    pub below_floor: bool,
}

/// Fit as a fraction of `FIT_POINTS`. It rewards the kind of work (the title tier and
/// domain phrases such as applied AI or full-stack) and the seniority of the role.
/// Technologies Gonza has used add a few points, but a posting that asks for a
/// language he does not use loses nothing: no point is ever taken for a missing skill.
fn fit_fraction(title_lc: &str, text_lc: &str, c: &Criteria) -> f64 {
    let title_points = if any_hit(title_lc, &c.titles.tier1) {
        12.0
    } else if any_hit(title_lc, &c.titles.tier2) {
        8.0
    } else if any_hit(title_lc, &c.titles.tier3) {
        6.0
    } else if contains_term_lc(title_lc, "engineer") || contains_term_lc(title_lc, "developer") {
        4.0
    } else {
        0.0
    };
    let domain_points = (2.0 * hits(text_lc, &c.signals.domain) as f64).min(8.0);
    let skill_points = (hits(text_lc, &c.skills) as f64).min(4.0);

    let relevance = title_points + domain_points + skill_points;
    // Seniority never makes an unrelated role look relevant, and a junior title earns none.
    let seniority_points = if relevance == 0.0 || any_hit(title_lc, &c.signals.seniority_neg) {
        0.0
    } else if any_hit(title_lc, &c.signals.seniority_pos) {
        6.0
    } else if any_hit(text_lc, &c.signals.seniority_pos) {
        3.0
    } else {
        2.0
    };
    (relevance + seniority_points) / FIT_POINTS
}

fn compensation_fraction(reference: Option<f64>, below_floor: bool, c: &Criteria) -> f64 {
    let low_target = c.target_monthly_usd.0;
    match reference {
        None => 0.4,
        Some(_) if below_floor => 0.0,
        Some(pay) if pay >= low_target => 1.0,
        Some(pay) => 0.33 + 0.67 * (pay - c.floor_monthly_usd) / (low_target - c.floor_monthly_usd),
    }
}

pub fn score_job(job: &Job, c: &Criteria) -> ScoreResult {
    let title_lc = job.title.to_lowercase();
    let text_lc = format!("{}\n{}\n{}", job.title, job.location, job.description).to_lowercase();
    let sig = &c.signals;

    let fit = fit_fraction(&title_lc, &text_lc, c);

    // A zero or negative figure means "not published".
    let published = |pay: Option<f64>| pay.filter(|value| *value > 0.0);
    let reference =
        published(job.salary_max_monthly_usd).or_else(|| published(job.salary_min_monthly_usd));
    let below_floor = reference.is_some_and(|pay| pay < c.floor_monthly_usd);
    let compensation = compensation_fraction(reference, below_floor, c);

    let capped = |terms: &[String], cap: usize| hits(&text_lc, terms).min(cap) as f64;
    let stability = unit(
        (8.0 + 4.0 * capped(&sig.stability_pos, 3) - 4.0 * capped(&sig.stability_neg, 2)) / 20.0,
    );
    let learning = unit((5.0 + 2.0 * capped(&sig.learning, 5)) / 15.0);
    let interview = unit(
        (5.0 + 2.0 * capped(&sig.interview_pos, 3) - 2.0 * capped(&sig.interview_neg, 3)) / 10.0,
    );
    let other = unit((3.0 + 3.0 * capped(&sig.other_pos, 2) + 2.0 * capped(&sig.china, 1)) / 10.0);

    let w = &c.weights;
    let parts = [
        ("fit", w.fit, fit),
        ("compensation", w.compensation, compensation),
        ("stability", w.stability, stability),
        ("learning", w.learning, learning),
        ("interview_style", w.interview_style, interview),
        ("other", w.other, other),
    ];
    let detail: BTreeMap<String, f64> = parts
        .iter()
        .map(|(name, weight, fraction)| (name.to_string(), round1(weight * fraction)))
        .collect();
    let total = round1(detail.values().sum());
    ScoreResult {
        total,
        detail,
        below_floor,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::LazyLock;

    use super::*;
    use crate::fixtures::{make_job, make_job_with};
    use crate::models::Job;
    use crate::rules::Criteria;
    use crate::shipped;

    fn close(actual: Option<f64>, expected: f64, tolerance: f64) {
        let actual = actual.expect("expected a value");
        assert!(
            (actual - expected).abs() <= tolerance,
            "{actual} is not {expected}"
        );
    }

    #[test]
    fn hourly_range_matches_original_posting() {
        assert_eq!(
            parse_salary_monthly_usd("Pay: $18–$22/hr"),
            (Some(2880.0), Some(3520.0))
        );
    }

    #[test]
    fn hourly_range_written_per_hour() {
        assert_eq!(
            parse_salary_monthly_usd("$25 - $30 per hour"),
            (Some(4000.0), Some(4800.0))
        );
    }

    #[test]
    fn annual_range_with_commas() {
        let (low, high) = parse_salary_monthly_usd("Salary $120,000 - $160,000 per year");
        close(low, 10_000.0, 1e-6);
        close(high, 13_333.33, 0.01);
    }

    #[test]
    fn annual_range_with_k_suffix() {
        let (low, high) = parse_salary_monthly_usd("$257K - $335K • Offers Equity");
        close(low, 21_416.67, 0.01);
        close(high, 27_916.67, 0.01);
    }

    #[test]
    fn k_applies_to_both_ends() {
        let (low, high) = parse_salary_monthly_usd("$120 - $160k");
        close(low, 10_000.0, 1e-6);
        close(high, 13_333.33, 0.01);
    }

    #[test]
    fn monthly_range_is_kept() {
        assert_eq!(
            parse_salary_monthly_usd("$4,000 - $6,000 per month"),
            (Some(4000.0), Some(6000.0))
        );
    }

    #[test]
    fn non_pay_range_is_ignored() {
        assert_eq!(
            parse_salary_monthly_usd("We raised $5 - $10 million"),
            (None, None)
        );
    }

    #[test]
    fn a_later_pay_range_wins_over_an_earlier_non_pay_one() {
        let text = "Backed by $5 - $10 million in funding. Pay: $4,500 - $6,000 monthly.";
        assert_eq!(parse_salary_monthly_usd(text), (Some(4500.0), Some(6000.0)));
    }

    #[test]
    fn no_range() {
        assert_eq!(parse_salary_monthly_usd("Competitive salary"), (None, None));
        assert_eq!(parse_salary_monthly_usd(""), (None, None));
    }

    #[test]
    fn range_at_the_end_of_multibyte_text_does_not_panic() {
        let text = format!("Sueldo: $18 - $22 {}", "ñandú ".repeat(10));
        assert_eq!(parse_salary_monthly_usd(&text), (None, None));
    }

    static CRITERIA: LazyLock<Criteria> = LazyLock::new(shipped::criteria);

    fn strong() -> Job {
        make_job_with(|j| {
            j.title = "Senior AI Engineer (LLM agents)".into();
            j.description = "Build agentic LLM applications as a full-stack engineer with \
                python, typescript, react, rag and mcp. Full-time position with mentorship \
                and a take-home exercise."
                .into();
            j.location = "Remote - Latin America".into();
            j.salary_min_monthly_usd = Some(5000.0);
            j.salary_max_monthly_usd = Some(7000.0);
        })
    }

    fn fit_of(job: &Job) -> f64 {
        score_job(job, &CRITERIA).detail["fit"]
    }

    #[test]
    fn strong_job_scores_high() {
        let r = score_job(&strong(), &CRITERIA);
        assert_eq!(r.total, 75.0);
        assert_eq!(r.detail["fit"], 28.0);
        assert_eq!(r.detail["compensation"], 15.0);
        assert_eq!(r.detail["stability"], 12.0);
        assert_eq!(r.detail["learning"], 7.0);
        assert_eq!(r.detail["interview_style"], 7.0);
        assert_eq!(r.detail["other"], 6.0);
        assert!(!r.below_floor);
        assert!(r.total >= CRITERIA.apply_threshold);
    }

    #[test]
    fn below_floor_flagged_and_zero_compensation() {
        let job = make_job_with(|j| {
            *j = strong();
            j.salary_min_monthly_usd = Some(1500.0);
            j.salary_max_monthly_usd = Some(2200.0);
        });
        let r = score_job(&job, &CRITERIA);
        assert!(r.below_floor);
        assert_eq!(r.detail["compensation"], 0.0);
    }

    #[test]
    fn unknown_salary_is_neutral_not_a_rejection() {
        let job = make_job_with(|j| {
            *j = strong();
            j.salary_min_monthly_usd = None;
            j.salary_max_monthly_usd = None;
        });
        let r = score_job(&job, &CRITERIA);
        assert!(!r.below_floor);
        assert_eq!(r.detail["compensation"], 6.0);
    }

    #[test]
    fn zero_salary_counts_as_unknown() {
        let job = make_job_with(|j| {
            j.salary_min_monthly_usd = Some(0.0);
            j.salary_max_monthly_usd = Some(0.0);
        });
        let r = score_job(&job, &CRITERIA);
        assert!(!r.below_floor);
        assert_eq!(r.detail["compensation"], 6.0);
    }

    #[test]
    fn pay_between_floor_and_target_scales_up() {
        // Halfway between the shipped floor (3000) and the low end of the target (5000).
        let job = make_job_with(|j| j.salary_max_monthly_usd = Some(4000.0));
        assert_eq!(score_job(&job, &CRITERIA).detail["compensation"], 10.0);
    }

    #[test]
    fn irrelevant_title_has_zero_fit() {
        let job = make_job_with(|j| {
            j.title = "Account Executive".into();
            j.description = "sales".into();
            j.location = "Remote".into();
        });
        assert_eq!(fit_of(&job), 0.0);
        let senior = make_job_with(|j| {
            j.title = "Senior Account Executive".into();
            j.description = "sales".into();
            j.location = "Remote".into();
        });
        assert_eq!(fit_of(&senior), 0.0);
    }

    #[test]
    fn language_mismatch_is_not_penalized() {
        let with_description = |text: &str| {
            make_job_with(|j| {
                j.title = "Senior Full-Stack Engineer".into();
                j.description = text.into();
            })
        };
        let baseline = fit_of(&with_description("Build web products."));
        let foreign = fit_of(&with_description(
            "Build web products. Requires strong Java, Kotlin and Rust experience.",
        ));
        let familiar = fit_of(&with_description(
            "Build web products. Requires strong Python experience.",
        ));
        assert_eq!(
            foreign, baseline,
            "asking for other languages must not lower fit"
        );
        assert!(familiar > baseline, "skills Gonza has still add a little");
        assert!(
            baseline >= 0.6 * 30.0,
            "a senior full-stack role is a good fit in any language"
        );
    }

    #[test]
    fn seniority_changes_fit() {
        let titled = |title: &str| make_job_with(|j| j.title = title.into());
        let junior = fit_of(&titled("Junior AI Engineer"));
        let unspecified = fit_of(&titled("AI Engineer"));
        let senior = fit_of(&titled("Senior AI Engineer"));
        assert!(
            junior < unspecified && unspecified < senior,
            "{junior} {unspecified} {senior}"
        );
    }

    #[test]
    fn leetcode_lowers_interview_style() {
        let style = |text: &str| {
            let job = make_job_with(|j| j.description = text.into());
            score_job(&job, &CRITERIA).detail["interview_style"]
        };
        assert_eq!(style("python"), 5.0);
        assert_eq!(style("python and leetcode"), 3.0);
    }

    #[test]
    fn contains_term_boundaries_and_plural() {
        assert!(contains_term("We use LLMs daily", "llm"));
        assert!(contains_term("We use Next.js daily", "next.js"));
        assert!(contains_term("the llms.", "LLM"));
        assert!(!contains_term("the best team", "est"));
        assert!(!contains_term("anything", ""));
    }

    #[test]
    fn detail_has_the_six_keys_and_adds_up() {
        let r = score_job(&make_job(), &CRITERIA);
        let keys: Vec<&str> = r.detail.keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            [
                "compensation",
                "fit",
                "interview_style",
                "learning",
                "other",
                "stability"
            ]
        );
        let sum: f64 = r.detail.values().sum();
        assert!((r.total - sum).abs() < 0.11, "{} vs {sum}", r.total);
    }

    #[test]
    fn empty_fields_do_not_crash() {
        let blank = make_job_with(|j| {
            j.title.clear();
            j.description.clear();
            j.location.clear();
        });
        let r = score_job(&blank, &CRITERIA);
        assert!(r.total.is_finite());
        assert_eq!(r.detail["fit"], 0.0);
        let default = score_job(&Job::default(), &CRITERIA);
        assert!(default.total.is_finite() && !default.below_floor);
    }
}
