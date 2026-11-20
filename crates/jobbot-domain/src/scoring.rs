//! Salary parsing and the 0-100 job score.

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

/// The period a pay range names right after it. Between the range and the word only separators
/// that are not letters, digits, `/`, `+`, `.` or `;` may stand (spaces, commas, brackets, a line
/// break), and a currency word, itself followed by such separators: `, per year`, `(USD) per
/// year`. The word is `per year`, `a month`, `/yr`, `annually`, `p.a.`, or a bare `annual`,
/// `yearly`, `monthly` (see `period_after` for the last one). Anything else in between (`+ annual
/// bonus`, `. Annual leave`) means the word is not about the pay. `annually` and `p.a.` count
/// whatever follows them: `annually reviewed` reads as yearly, which holds a low posting for the
/// owner rather than passing it as a monthly pay.
static PERIOD_AFTER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)^[^\p{L}\p{N}/+.;]*(?:(?:usd|us\$|dollars?|gross|net)[^\p{L}\p{N}/+.;]*)*(?:(?:per|a|each|/)\s?(?P<fixed>year|yr|annum|month|mo)\b|(?P<adverb>annually\b|p\.a\.?)|(?P<bare>annual|yearly|monthly)\b)",
    )
    .unwrap()
});

/// The period a pay range is introduced with, right before it: `Annual salary: $15,000 - $19,000`,
/// `Yearly base pay range (USD) is US$15,000 - $19,000`.
static PERIOD_BEFORE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\b(annual|yearly|monthly)\s+(?:base\s+)?(?:salary|pay|compensation)(?:\s+range)?(?:\s*\([^)]{0,20}\))?(?:\s+(?:is|of|between))?[\s:]*(?:(?:usd|us)\s*)?$",
    )
    .unwrap()
});

/// How many characters before a range are read for the word that introduces it.
const PERIOD_BEFORE_CHARS: usize = 60;

/// A pay period the text states.
#[derive(Clone, Copy, PartialEq)]
enum Period {
    Yearly,
    Monthly,
}

fn period_named(word: &str) -> Option<Period> {
    match word.to_lowercase().as_str() {
        "year" | "yr" | "annum" | "annual" | "annually" | "yearly" | "p.a" | "p.a." => {
            Some(Period::Yearly)
        }
        "month" | "mo" | "monthly" => Some(Period::Monthly),
        _ => None,
    }
}

/// The period the word right after a range (`tail` is the text after it) names, or `None` when
/// there is none or it is not about the pay: a period word followed by a hyphen (`/ year-end
/// bonus`), or a bare `annual`/`yearly`/`monthly` that starts another phrase on the same line
/// (`Yearly learning budget`; one at the end of its line, `yearly` then `Requirements`, is the
/// period).
fn period_after(tail: &str) -> Option<Period> {
    let caps = PERIOD_AFTER.captures(tail)?;
    let end = caps.get(0).map_or(0, |whole| whole.end());
    let rest = &tail[end..];
    if rest.starts_with(['-', '\u{2010}', '\u{2013}', '\u{2014}']) {
        return None;
    }
    let bare = caps.name("bare");
    let same_line = rest.split(['\n', '\r']).next().unwrap_or_default();
    if bare.is_some() && same_line.trim_start().starts_with(char::is_alphanumeric) {
        return None;
    }
    let word = caps
        .name("fixed")
        .or(caps.name("adverb"))
        .or(bare)
        .map(|found| found.as_str());
    word.and_then(period_named)
}

/// The period the text says for the range that starts at `start`: the word right after it, else
/// the one that introduces it. `None` when it says none, or the word after is not about the pay
/// and nothing introduces the range (the size of the numbers decides then, as before).
fn stated_period(text: &str, start: usize, tail: &str) -> Option<Period> {
    if let Some(period) = period_after(tail) {
        return Some(period);
    }
    let before: String = {
        let mut chars: Vec<char> = text[..start]
            .chars()
            .rev()
            .take(PERIOD_BEFORE_CHARS)
            .collect();
        chars.reverse();
        chars.into_iter().collect()
    };
    PERIOD_BEFORE
        .captures(&before)
        .and_then(|caps| caps.get(1))
        .and_then(|word| period_named(word.as_str()))
}

static HOURLY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)/\s?h(?:ou)?r|per hour|hourly|/h\b").unwrap());

/// Letters written against a `$` that make it another currency: `A$`, `NZ$`, `R$`... (`US$` and
/// `USD` are US dollars and stay).
const FOREIGN_DOLLAR_SYMBOLS: [&str; 9] = ["au", "a", "nz", "ca", "c", "hk", "mx", "s", "r"];
/// Currency codes of other currencies, as whole words next to a range.
const FOREIGN_CODES: [&str; 12] = [
    "aud", "nzd", "cad", "sgd", "hkd", "mxn", "brl", "clp", "cop", "ars", "pen", "uyu",
];

/// Whether the range that starts at `start` (its `$`) and ends at `end` is in a currency other
/// than US dollars: a foreign symbol or code just before it, or a code, `pesos` or Australian
/// `super` just after it.
fn is_foreign_currency(text: &str, start: usize, end: usize) -> bool {
    let before = text[..start].to_lowercase();
    // A symbol written against the `$`. A letter before it means a longer word: "US$" and
    // "USA $" are not `S$`.
    let symbol = FOREIGN_DOLLAR_SYMBOLS.iter().any(|symbol| {
        before
            .strip_suffix(symbol)
            .is_some_and(|rest| !rest.chars().next_back().is_some_and(char::is_alphanumeric))
    });
    if symbol {
        return true;
    }
    // The 8 characters before the range, without a word cut in half by the window.
    let skip = before.chars().count().saturating_sub(8);
    let mut window = before.chars().skip(skip).collect::<String>();
    let cut_word = before
        .chars()
        .nth(skip.wrapping_sub(1))
        .is_some_and(char::is_alphanumeric);
    if cut_word {
        let keep = window
            .find(|c: char| !c.is_alphanumeric())
            .unwrap_or(window.len());
        window.drain(..keep);
    }
    if FOREIGN_CODES
        .iter()
        .any(|code| contains_word_lc(&window, code))
    {
        return true;
    }
    // The 16 characters after it, extended to the end of the word the cut lands in.
    let after = text[end..].to_lowercase();
    let mut tail: String = after.chars().take(16).collect();
    tail.extend(
        after[tail.len()..]
            .chars()
            .take_while(|c| c.is_alphanumeric()),
    );
    ["pesos", "super", "superannuation"]
        .iter()
        .chain(FOREIGN_CODES.iter())
        .any(|word| contains_word_lc(&tail, word))
}

/// A range whose top is below this is an hourly rate (when `HOURLY` follows it), not a salary.
const HOURLY_CEILING: f64 = 300.0;

/// Working hours in a month, to turn an hourly rate into a monthly one.
const HOURS_PER_MONTH: f64 = 160.0;

/// A range whose top is at least this is a yearly salary.
const ANNUAL_FLOOR: f64 = 20_000.0;

/// A range whose top is at least this (and below `ANNUAL_FLOOR`) is a monthly salary; below it
/// (and not hourly) it is not pay.
const MONTHLY_FLOOR: f64 = 1000.0;

/// How many characters after a range are read for "per hour" and the like.
const RATE_TAIL_CHARS: usize = 24;

/// Reads the first pay range in `text` as `(min, max)` US dollars per month: hourly
/// rates times `HOURS_PER_MONTH`, annual amounts divided by 12, monthly amounts as they are. A range
/// that is not clearly pay (`$5 - $10 million`, or a number too long for any pay) is ignored,
/// and so is one in another currency (`A$150k`, `$120,000 + super`, `CLP $1,500,000`): only
/// US dollars are converted, no exchange rate is ever guessed.
pub fn parse_salary_monthly_usd(text: &str) -> (Option<f64>, Option<f64>) {
    for caps in RANGE.captures_iter(text) {
        let (Some(whole), Some(low_text), Some(high_text)) =
            (caps.get(0), caps.get(1), caps.get(3))
        else {
            continue;
        };
        if is_foreign_currency(text, whole.start(), whole.end()) {
            continue;
        }
        let parse = |s: &str| {
            s.replace(',', "")
                .parse::<f64>()
                .ok()
                .filter(|amount| amount.is_finite())
        };
        let (Some(mut low), Some(mut high)) = (parse(low_text.as_str()), parse(high_text.as_str()))
        else {
            continue;
        };
        let mut low_k = caps.get(2).is_some();
        let high_k = caps.get(4).is_some();
        // "$120 - $160k": the k applies to both ends.
        if high_k && !low_k && low < MONTHLY_FLOOR {
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
        let tail: String = text[whole.end()..].chars().take(RATE_TAIL_CHARS).collect();
        if high < HOURLY_CEILING {
            if HOURLY.is_match(&tail) {
                return (Some(low * HOURS_PER_MONTH), Some(high * HOURS_PER_MONTH));
            }
            continue;
        }
        // What the text says about the period beats the size of the numbers.
        if high >= MONTHLY_FLOOR {
            match stated_period(text, whole.start(), &tail) {
                Some(Period::Yearly) => return (Some(low / 12.0), Some(high / 12.0)),
                Some(Period::Monthly) => return (Some(low), Some(high)),
                None => {}
            }
        }
        if high >= ANNUAL_FLOOR {
            return (Some(low / 12.0), Some(high / 12.0));
        }
        if high >= MONTHLY_FLOOR {
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

/// The most each part of the fit score can give.
const TITLE_MAX: f64 = 12.0;
const DOMAIN_MAX: f64 = 8.0;
const SKILL_MAX: f64 = 4.0;
const SENIORITY_MAX: f64 = 6.0;

/// What a title outside the first tier gives (the first tier gives `TITLE_MAX`).
const TITLE_TIER2_POINTS: f64 = 8.0;
const TITLE_TIER3_POINTS: f64 = 6.0;
/// A title that only says "engineer" or "developer".
const TITLE_GENERIC_POINTS: f64 = 4.0;

/// Points available inside the fit score: the sum of the parts' maxima.
const FIT_POINTS: f64 = TITLE_MAX + DOMAIN_MAX + SKILL_MAX + SENIORITY_MAX;

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
        TITLE_MAX
    } else if any_hit(title_lc, &c.titles.tier2) {
        TITLE_TIER2_POINTS
    } else if any_hit(title_lc, &c.titles.tier3) {
        TITLE_TIER3_POINTS
    } else if contains_term_lc(title_lc, "engineer") || contains_term_lc(title_lc, "developer") {
        TITLE_GENERIC_POINTS
    } else {
        0.0
    };
    let domain_points = (2.0 * hits(text_lc, &c.signals.domain) as f64).min(DOMAIN_MAX);
    let skill_points = (hits(text_lc, &c.skills) as f64).min(SKILL_MAX);

    let relevance = title_points + domain_points + skill_points;
    // Seniority never makes an unrelated role look relevant, and a junior title earns none.
    let seniority_points = if relevance == 0.0 || any_hit(title_lc, &c.signals.seniority_neg) {
        0.0
    } else if any_hit(title_lc, &c.signals.seniority_pos) {
        SENIORITY_MAX
    } else if any_hit(text_lc, &c.signals.seniority_pos) {
        3.0
    } else {
        2.0
    };
    (relevance + seniority_points) / FIT_POINTS
}

/// The compensation score of a posting that publishes no pay.
const UNKNOWN_PAY_FRACTION: f64 = 0.4;

fn compensation_fraction(reference: Option<f64>, below_floor: bool, c: &Criteria) -> f64 {
    let low_target = c.target_monthly_usd.0;
    match reference {
        None => UNKNOWN_PAY_FRACTION,
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

    #[test]
    fn a_range_in_another_currency_is_not_read_as_us_dollars() {
        for text in [
            "AUD $120,000 - $150,000",
            "$120,000 - $150,000 AUD",
            "A$120k - $150k",
            "$120,000 - $140,000 + super",
            "$120,000 - $140,000 plus superannuation",
            "CLP $1,500,000 - $2,000,000 mensual",
            "NZ$90,000 - $110,000",
            "Pay: C$100k - $120k",
            "$2,000,000 - $3,000,000 pesos",
        ] {
            assert_eq!(parse_salary_monthly_usd(text), (None, None), "{text}");
        }
    }

    #[test]
    fn a_us_dollar_range_is_still_read() {
        for text in [
            "US$120,000 - $150,000",
            "$120,000 - $150,000 USD",
            "Salary: $120,000 - $150,000",
            "Based in the USA $120,000 - $150,000",
            "Open role $120,000 - $150,000",
            "Australia: A$150,000 - A$180,000. United States: $120,000 - $150,000",
        ] {
            assert_eq!(
                parse_salary_monthly_usd(text),
                (Some(10_000.0), Some(12_500.0)),
                "{text}"
            );
        }
    }

    #[test]
    fn a_foreign_range_is_skipped_and_the_next_one_is_read() {
        assert_eq!(
            parse_salary_monthly_usd("AUD $90,000 - $100,000 or $120,000 - $150,000"),
            (Some(10_000.0), Some(12_500.0))
        );
    }

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
            parse_salary_monthly_usd("$5,200 - $7,300 per month"),
            (Some(5200.0), Some(7300.0))
        );
    }

    #[test]
    fn the_period_the_text_names_beats_the_size_of_the_numbers() {
        // 19,000 is under the size that reads as a yearly salary, but the text says per year.
        for text in [
            "$15,000 – $19,000 USD per year",
            "$15,000 - $19,000 a year",
            "$15,000 - $19,000/yr",
            "$15,000 - $19,000 annually",
            "Annual salary: $15,000 - $19,000",
        ] {
            let (low, high) = parse_salary_monthly_usd(text);
            assert_eq!(low.unwrap().round(), 1250.0, "{text}");
            assert_eq!(high.unwrap().round(), 1583.0, "{text}");
        }
        // And a monthly pay that is large stays monthly.
        assert_eq!(
            parse_salary_monthly_usd("$20,000 - $25,000 per month"),
            (Some(20_000.0), Some(25_000.0))
        );
        // A word that is not about the pay does not set its period.
        assert_eq!(
            parse_salary_monthly_usd("$5,200 - $7,300 plus an annual bonus"),
            (Some(5200.0), Some(7300.0))
        );
    }

    #[test]
    fn a_period_word_that_is_not_about_the_pay_does_not_set_its_period() {
        // Monthly by size: the word is not right after the range, or it starts another phrase.
        for text in [
            "$5,200 - $7,300 + annual bonus",
            "$5,200 – $7,300. Annual leave",
            "$5,200-$7,300\nYearly learning budget",
            "$5,200 - $7,300 yearly learning budget",
            "Annual pay review included. Salary: $5,200 - $7,300",
        ] {
            assert_eq!(
                parse_salary_monthly_usd(text),
                (Some(5200.0), Some(7300.0)),
                "{text:?}"
            );
        }
        // Yearly by size, and a month word that is not about the pay does not change it.
        assert_eq!(
            parse_salary_monthly_usd("$40,000 - $60,000. Monthly internet stipend"),
            (Some(40_000.0 / 12.0), Some(5000.0))
        );
        // The period words right after the range, or the introduction right before it, still count.
        let (low, high) = parse_salary_monthly_usd("$15,000 - $19,000 (per year)");
        assert_eq!(
            (low.unwrap().round(), high.unwrap().round()),
            (1250.0, 1583.0)
        );
        let (low, high) = parse_salary_monthly_usd("$15,000 - $19,000 annual");
        assert_eq!(
            (low.unwrap().round(), high.unwrap().round()),
            (1250.0, 1583.0)
        );
        let (low, _) = parse_salary_monthly_usd("Yearly salary:  $15,000 - $19,000");
        assert_eq!(low.unwrap().round(), 1250.0);
    }

    #[test]
    fn a_period_written_with_commas_brackets_or_an_introduction_is_still_read() {
        for text in [
            "$15,000 - $19,000, per year",
            "$15,000 - $19,000 (USD) per year",
            "$15,000 - $19,000 USD, a year",
            "$15,000 - $19,000 p.a.",
            "Annual salary is $15,000 - $19,000",
            "Annual salary range: $15,000 - $19,000",
            "Annual base salary (gross) of $15,000 - $19,000",
            "Annual salary: US$15,000 - $19,000",
            "Annual salary: USD $15,000 - $19,000",
            "$15,000 - $19,000 yearly\nRequirements",
        ] {
            let (low, high) = parse_salary_monthly_usd(text);
            assert_eq!(
                (low.map(f64::round), high.map(f64::round)),
                (Some(1250.0), Some(1583.0)),
                "{text:?}"
            );
        }
    }

    #[test]
    fn a_period_word_that_starts_another_phrase_falls_back_to_the_introduction_or_the_size() {
        for text in [
            "$5,200 - $7,300 / year-end bonus",
            "Monthly salary: $5,200 - $7,300 yearly learning budget",
            "$5,200 - $7,300\nYearly learning budget",
        ] {
            assert_eq!(
                parse_salary_monthly_usd(text),
                (Some(5200.0), Some(7300.0)),
                "{text:?}"
            );
        }
    }

    #[test]
    fn an_adverb_after_the_range_is_the_period_whatever_follows_it() {
        for text in [
            "$15,000 - $19,000 annually",
            "$15,000 - $19,000 annually.",
            "$15,000 - $19,000 annually, plus equity",
            "$15,000 - $19,000 annually\nRequirements",
            "$15,000 - $19,000 p.a., plus equity",
            "Compensation: $15,000 - $19,000 annually depending on experience",
            "$12,000 - $18,000 annually plus performance bonus",
            "$18,000 - $19,800 p.a. plus bonus",
        ] {
            let (low, high) = parse_salary_monthly_usd(text);
            let (low, high) = (low.expect(text), high.expect(text));
            assert!(
                high < 2000.0,
                "{text:?}: read as a monthly pay ({low}, {high})"
            );
        }
        let (low, high) =
            parse_salary_monthly_usd("$15,000 - $19,000 annually depending on experience");
        assert_eq!(
            (low.map(f64::round), high.map(f64::round)),
            (Some(1250.0), Some(1583.0))
        );
    }

    #[test]
    fn a_range_followed_by_super_is_ignored_whatever_its_period_word() {
        // "super" next to the range is the Australian pension: another currency, so no range.
        assert_eq!(
            parse_salary_monthly_usd("$18,000 - $19,800 p.a. plus super"),
            (None, None)
        );
    }

    #[test]
    fn annually_reviewed_reads_as_yearly_which_is_the_safe_direction() {
        // Known and accepted: the word after the range is about the review, but it still reads as
        // the period. A low posting is then held for the owner instead of being passed as a
        // monthly pay.
        let (low, high) = parse_salary_monthly_usd("$5,200 - $7,300, annually reviewed");
        assert_eq!(
            (low.map(f64::round), high.map(f64::round)),
            (Some(433.0), Some(608.0))
        );
    }

    #[test]
    fn a_period_word_inside_a_longer_word_does_not_introduce_the_range() {
        // `semiannual` is not `annual`: 5,200 - 7,300 stays monthly by size.
        assert_eq!(
            parse_salary_monthly_usd("semiannual salary $5,200 - $7,300"),
            (Some(5200.0), Some(7300.0))
        );
        let (low, _) = parse_salary_monthly_usd("Annual salary $15,000 - $19,000");
        assert_eq!(low.unwrap().round(), 1250.0);
    }

    #[test]
    fn a_number_longer_than_any_pay_is_not_read_as_a_range() {
        let huge = "9".repeat(320);
        assert_eq!(
            parse_salary_monthly_usd(&format!("$100,000 - ${huge} a year")),
            (None, None)
        );
        assert_eq!(
            parse_salary_monthly_usd(&format!("${huge} - $1 and later $5,200 - $7,300 per month")),
            (Some(5200.0), Some(7300.0))
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
    fn a_range_followed_by_multibyte_text_does_not_panic() {
        // No unit after the range: nothing to read, and the multibyte tail is walked over safely.
        let text = format!("Sueldo: $18 - $22 {}", "ñandú ".repeat(10));
        assert_eq!(parse_salary_monthly_usd(&text), (None, None));
        // With its unit the same range is read: 160 paid hours a month (the rule of the hourly
        // rates above, 18 x 160 and 22 x 160) even with multibyte text after it.
        let text = format!("Sueldo: $18 - $22/hr {}", "ñandú ".repeat(10));
        assert_eq!(
            parse_salary_monthly_usd(&text),
            (Some(18.0 * 160.0), Some(22.0 * 160.0))
        );
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
        // Halfway between the shipped floor and the low end of the target.
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
            baseline >= 0.6 * CRITERIA.weights.fit,
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
