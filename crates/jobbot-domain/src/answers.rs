//! Answer bank: the answers Gonza approved for the questions application forms ask. Pure: the
//! application reads `profile/answers.toml` and hands its text to `parse_answers`.

use std::sync::LazyLock;

use regex::Regex;
use serde::Deserialize;

use crate::text::normalize;

/// One bank entry: every pattern is a normalized phrase; `value: None` means "ask Gonza";
/// `alternatives` are other option texts to try when the field is a drop-down and `value` is not
/// among its options.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnswerEntry {
    pub patterns: Vec<String>,
    #[serde(default)]
    pub value: Option<String>,
    #[serde(default)]
    pub alternatives: Vec<String>,
}

impl AnswerEntry {
    pub fn new(patterns: &[&str], value: Option<&str>) -> Self {
        AnswerEntry {
            patterns: patterns.iter().map(|p| p.to_string()).collect(),
            value: value.map(str::to_string),
            alternatives: Vec::new(),
        }
    }

    pub fn with_alternatives(mut self, alternatives: &[&str]) -> Self {
        self.alternatives = alternatives.iter().map(|a| a.to_string()).collect();
        self
    }

    /// The value followed by the alternatives, trimmed and without blanks. Empty when there is
    /// no usable value: an entry that only lists alternatives still means "ask Gonza".
    pub fn candidates(&self) -> Vec<String> {
        let Some(value) = self
            .value
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            return Vec::new();
        };
        std::iter::once(value.to_string())
            .chain(
                self.alternatives
                    .iter()
                    .map(|alternative| alternative.trim().to_string())
                    .filter(|alternative| !alternative.is_empty()),
            )
            .collect()
    }
}

#[derive(Debug, Clone, Default)]
pub struct AnswerBank {
    pub entries: Vec<AnswerEntry>,
    /// Phrases that allow a required checkbox to be ticked (`consent_checkboxes` in the file).
    pub consent_phrases: Vec<String>,
}

/// Labels that ask about the right to work, a visa, sponsorship or citizenship. For a job in a place
/// Gonza does not live in (Australia) the contractor's answers are false: "No sponsorship" for a role
/// that needs him on site, and "authorized to work" for a place where he holds no visa. A label that
/// never names the country ("... in the country this role is located") cannot be told apart by its
/// words, so the job decides, not the label.
const WORK_RIGHTS_LABELS: &str = "authorized to work|authorised to work|legally authorized|legally authorised|work authorization|work authorisation|eligible to work|eligibility to work|right to work|legally entitled|legally permitted|work permit|work rights|working rights|visa|sponsorship|sponsor|sponsored|immigration|citizenship|citizen|permanent resident";

/// "Am I allowed to work there" labels, whose true answer for Australia is "No".
const AUSTRALIA_AUTHORIZED: &str = "authorized to work|authorised to work|legally authorized|legally authorised|eligible to work|eligibility to work|right to work|rights to work|legally entitled|legally permitted|entitled to work|permitted to work|allowed to work|work rights|working rights";

const SPONSORSHIP: &str = "sponsorship|sponsor|sponsored";

/// Words that turn a question around: "Can you work there WITHOUT sponsorship?" is also a "No".
const NEGATED_ASK: &str = "without|do not require|don't require|does not require|not require|not need|don't need|doesn't need|no need|no sponsorship|not requiring|not be required";

/// Labels about the country he lives in keep the bank's own answers (he is authorized in Chile and
/// needs no sponsorship there): the Australian answers never take them. "Remain in your current
/// location" (GitLab, 2026-10-23) is one; "remain in Australia" is not, so the bare "remain in" is
/// not a marker.
const ASKS_ABOUT_RESIDENCE: &str = "!reside|residence|live in|living in|currently live|currently located|currently based|country you are in|country you are currently|current location|remain in your";

/// What Gonza asks for a job in Australia whose posting names no pay, by the level its title names
/// (2026-11-03, his "apuntando a las responsabilidades del cargo pero al rango mínimo o medio
/// bajo": the bottom or lower middle of the Sydney market for that level, plus the superannuation
/// every employer pays on top). Until then every level asked A$100,000, under the market for a
/// senior role. This one is the plain title ("Software Engineer").
pub const AUSTRALIA_DEFAULT_ASK_AUD: u32 = 120_000;

/// A senior title ("Senior", "Sr").
pub const AUSTRALIA_SENIOR_ASK_AUD: u32 = 145_000;

/// A lead, staff, principal, architect or head title.
pub const AUSTRALIA_LEAD_ASK_AUD: u32 = 170_000;

/// A graduate, junior, entry-level or intern title.
pub const AUSTRALIA_JUNIOR_ASK_AUD: u32 = 85_000;

static LEAD_TITLE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(lead|staff|principal|principle|architect|head)\b").expect("valid regex")
});
static SENIOR_TITLE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(senior|sr)\b").expect("valid regex"));
static JUNIOR_TITLE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(graduate|junior|jr|intern|internship|entry)\b").expect("valid regex")
});

/// The ask for a title when the posting names no pay.
fn australian_ask_for_level(title: &str) -> u32 {
    if JUNIOR_TITLE.is_match(title) {
        AUSTRALIA_JUNIOR_ASK_AUD
    } else if LEAD_TITLE.is_match(title) {
        AUSTRALIA_LEAD_ASK_AUD
    } else if SENIOR_TITLE.is_match(title) {
        AUSTRALIA_SENIOR_ASK_AUD
    } else {
        AUSTRALIA_DEFAULT_ASK_AUD
    }
}

/// The ask never goes below this: an employer that sponsors a 482 visa must pay at least the Core
/// Skills Income Threshold (A$79,423), so a lower ask would make the sponsorship impossible.
pub const AUSTRALIA_SPONSORSHIP_FLOOR_AUD: u32 = 80_000;

/// The lowest yearly ask for a job in Australia: Gonza's floor for living in Sydney without
/// hardship (decided 2026-11-10), above the sponsorship floor.
pub const AUSTRALIA_MIN_ASK_AUD: u32 = 85_000;
const _: () = assert!(AUSTRALIA_MIN_ASK_AUD >= AUSTRALIA_SPONSORSHIP_FLOOR_AUD);

/// A dollar amount between 50,000 and 500,000 as postings write it: `$190,000`, `AUD 190,000`,
/// `A$150k`, `$150 k`.
static AUD_AMOUNT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:a\$|aud\s?\$?|\$)\s?(\d{2,3})(?:,(\d{3})|\s?(k))\b").expect("valid regex")
});

/// The yearly AUD ask for a job in Australia: the lowest dollar amount the posting names between
/// 50,000 and 500,000 (the bottom of its range) that is not a bonus, allowance or company money
/// (`is_extra_money`), or the ask for the level of `title` when it names
/// none, and never less than `AUSTRALIA_MIN_ASK_AUD`. A figure that runs on
/// (`$100,000,000`) is not pay.
pub fn australian_salary_ask(title: &str, posting: &str) -> u32 {
    AUD_AMOUNT
        .captures_iter(posting)
        .filter(|caps| {
            let end = caps.get(0).map_or(0, |whole| whole.end());
            let rest = &posting[end..];
            !(rest.starts_with(',') && rest[1..].starts_with(|c: char| c.is_ascii_digit()))
        })
        .filter(|caps| {
            let Some(whole) = caps.get(0) else {
                return false;
            };
            let own = whole.as_str().to_lowercase();
            own.starts_with("a$")
                || own.starts_with("aud")
                || !labelled_foreign(&posting[..whole.start()])
        })
        .filter(|caps| {
            caps.get(0)
                .is_some_and(|whole| !is_extra_money(posting, whole.start(), whole.end()))
        })
        .filter_map(|caps| {
            let head: u32 = caps.get(1)?.as_str().parse().ok()?;
            let tail: u32 = match caps.get(2) {
                Some(tail) => tail.as_str().parse().ok()?,
                None => 0,
            };
            Some(head * 1000 + tail)
        })
        .filter(|amount| (50_000..=500_000).contains(amount))
        .min()
        .unwrap_or_else(|| australian_ask_for_level(title))
        .max(AUSTRALIA_MIN_ASK_AUD)
}

/// Words that make a dollar figure something other than the salary: pay on top of it, money the
/// company raised or spends.
const NOT_SALARY_WORDS: [&str; 14] = [
    "bonus",
    "sign-on",
    "sign on",
    "signing",
    "relocation",
    "allowance",
    "stipend",
    "equity",
    "grant",
    "raised",
    "funding",
    "revenue",
    "valuation",
    "referral",
];

/// Whether the figure at `start..end` of `posting` is a bonus, an allowance or company money
/// rather than pay: one of `NOT_SALARY_WORDS` in its sentence up to 30 characters before it, or
/// right after it ("$75k sign-on"). A word further after it ("plus bonus") does not count.
fn is_extra_money(posting: &str, start: usize, end: usize) -> bool {
    let before = posting[..start].to_lowercase();
    let sentence = before
        .rfind('\n')
        .into_iter()
        .chain(before.rfind(['.', '!', '?']).filter(|at| {
            before[at + 1..].starts_with(char::is_whitespace) || at + 1 == before.len()
        }))
        .max()
        .map_or(0, |at| at + 1);
    let skip = before[sentence..].chars().count().saturating_sub(30);
    let near: String = before[sentence..].chars().skip(skip).collect();
    let after = posting[end..].trim_start().to_lowercase();
    NOT_SALARY_WORDS
        .iter()
        .any(|word| near.contains(word) || after.starts_with(word))
}

/// Whether the pay figure that follows `before` belongs to another country: `US$`, or a US label
/// ("United States", "USD") named after the last Australian one ("Australia", "AUD", "A$").
/// Postings that hire in several countries print one pay table per country.
fn labelled_foreign(before: &str) -> bool {
    let before = before.to_lowercase();
    let trimmed = before.trim_end();
    if let Some(head) = trimmed.strip_suffix("us")
        && !head.ends_with(|c: char| c.is_alphabetic())
    {
        return true;
    }
    let last = |labels: &[&str]| labels.iter().filter_map(|label| before.rfind(label)).max();
    match (
        last(&["united states", "usd", "us$", "u.s."]),
        last(&["australia", "aud", "a$"]),
    ) {
        (Some(foreign), Some(local)) => foreign > local,
        (Some(_), None) => true,
        _ => false,
    }
}

/// `100000` as `100,000`.
fn thousands(amount: u32) -> String {
    let digits = amount.to_string();
    let mut out = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

/// Names of a pay EXPECTATION (never the current pay).
const PAY_EXPECTATION: &str = "salary expectation|expected salary|desired salary|salary requirement|compensation expectation|expected compensation|desired compensation|compensation requirement|expected pay|desired pay|pay expectation|pay requirement|target salary|target compensation|expected base salary|desired base salary|salary you are looking for|salary are you looking for|salary you are seeking|expected remuneration|desired remuneration|remuneration expectation|expected package|desired package|salary package expectation|expected annual|desired annual|annual salary expectation|annual expectation|expected yearly|desired yearly";

/// Labels about pay already earned, an hourly or day rate, or a month: never the yearly ask.
const NOT_THE_YEARLY_ASK: &str = "!current salary|current annual|current monthly|current base|current pay|current compensation|current package|current remuneration|current total|previous|last salary|last drawn|salary history|hourly|per hour|day rate|daily rate|monthly|per month|a month";

/// The one sentence a visa-status question of a job in Australia gets: the facts Gonza approved.
pub const AUSTRALIA_VISA_STATUS: &str = "Chilean citizen with no Australian work rights yet; I would need employer visa sponsorship. I am willing to relocate to Australia.";

/// Working from the employer's office, as job forms name it.
const ON_SITE: &str = "in office|in-office|in the office|from the office|from our office|in person|in-person|onsite|on-site|on site|hybrid";

/// A question about willingness, not about where he is today.
const WILLING: &str = "willing|able to work|open to|happy to|excited|comfortable|available|commit|ok with|okay with|prepared to";

/// Where he lives today, how far the office is, or which office: facts, not willingness.
const NOT_WILLINGNESS: &str = "!commute|commuting|currently|located|live in|living|reside|distance|how many days|which office|preferred office|which location";

impl AnswerBank {
    /// A copy of the bank for a job in Australia where "are you willing to work in the office,
    /// in person or hybrid" is "Yes": the job would mean moving there, and on-site or remote both
    /// suit (2026-10-21). Questions about where the candidate lives today or how far they are from
    /// the office keep the bank's own answer.
    pub fn with_australian_on_site_answers(&self) -> AnswerBank {
        let willing = format!("{ON_SITE}&{WILLING}&{NOT_WILLINGNESS}");
        let mut entries = vec![
            AnswerEntry::new(&[willing.as_str()], Some("Yes"))
                .with_alternatives(&["Yes, I am", "Yes, I'm open to it"]),
        ];
        entries.extend(self.entries.iter().cloned());
        AnswerBank {
            entries,
            consent_phrases: self.consent_phrases.clone(),
        }
    }

    /// A copy of the bank for a job in Australia whose pay ask is `ask_aud` a year (see
    /// `australian_salary_ask`): a pay-expectation label gets "AUD 100,000 per year plus
    /// superannuation", one that asks for a bare yearly number gets `100000`, one in thousands gets
    /// `100`. The current pay, an hourly or day rate and a monthly figure stay unanswered (the
    /// bank's USD answers would be the wrong currency, so they never reach those labels either).
    pub fn with_australian_salary(&self, ask_aud: u32) -> AnswerBank {
        let text = format!("AUD {} per year plus superannuation", thousands(ask_aud));
        let current_expectation =
            "current salary expectation|current compensation expectation|current pay expectation";
        let thousands_label = format!(
            "thousand|000s|1000s|in 000|k aud|aud k|in k|k$&{PAY_EXPECTATION}&{NOT_THE_YEARLY_ASK}"
        );
        let yearly_number = format!(
            "{PAY_EXPECTATION}&annual|annually|yearly|per year|a year|per annum&{NOT_THE_YEARLY_ASK}"
        );
        let expectation = format!("{PAY_EXPECTATION}&{NOT_THE_YEARLY_ASK}");
        let in_aud = format!(
            "aud|australian dollar&salary|compensation|remuneration|package|pay&{NOT_THE_YEARLY_ASK}"
        );
        // Everything else about pay: the USD answers further down must not reach it.
        let rest = "salary|compensation|remuneration|pay expectation|expected pay|desired pay|aud";
        // A yes/no question about pay is not a figure: it stays with him, as in the file.
        let yes_or_no = "meet|meets|satisfy|satisfies|acceptable|comfortable|okay with|ok with|fine with|in line with|aligned with|align with|within the&salary|compensation|pay";
        let mut entries = vec![
            AnswerEntry::new(&[yes_or_no], None),
            AnswerEntry::new(&[current_expectation], Some(text.as_str())),
            AnswerEntry::new(
                &[thousands_label.as_str()],
                Some(&(ask_aud / 1000).to_string()),
            ),
            AnswerEntry::new(&[yearly_number.as_str()], Some(&ask_aud.to_string())),
            AnswerEntry::new(
                &[expectation.as_str(), in_aud.as_str()],
                Some(text.as_str()),
            ),
            AnswerEntry::new(&[rest], None),
        ];
        entries.extend(self.entries.iter().cloned());
        AnswerBank {
            entries,
            consent_phrases: self.consent_phrases.clone(),
        }
    }

    /// A copy of the bank for a job in Australia, where Gonza has no work rights today (decided on
    /// 2026-10-22: the facts): "authorized to work" and "citizen or
    /// permanent resident" are "No", needing sponsorship or a visa is "Yes", and a visa-status
    /// question gets one honest sentence. A question that asks both at once, names Chile, or is
    /// about anything else to do with visas (a visa subclass, "have you ever been sponsored") is left
    /// to him. A label about the country he resides in keeps the bank's own answer. Everything else
    /// answers as before.
    pub fn with_australian_work_rights_answers(&self) -> AnswerBank {
        let residence = format!("&{ASKS_ABOUT_RESIDENCE}");
        let chile = format!("chile|chilean&{WORK_RIGHTS_LABELS}");
        let negated = format!("{AUSTRALIA_AUTHORIZED}|{SPONSORSHIP}&{NEGATED_ASK}{residence}");
        let both = format!("{AUSTRALIA_AUTHORIZED}&{SPONSORSHIP}{residence}");
        let no_rights = format!("{AUSTRALIA_AUTHORIZED}{residence}");
        let no_citizen = format!("citizen|permanent resident{residence}");
        let no_visa = format!(
            "valid visa|valid work visa|hold a visa|hold a work visa|have a visa|have a work visa|have a valid visa{residence}"
        );
        let needs_sponsor = format!("require|need|requiring|needing&{SPONSORSHIP}{residence}");
        let needs_visa = format!(
            "require a visa|require visa|need a visa|need visa|require a work visa|need a work visa|visa support|visa assistance|immigration support|immigration assistance{residence}"
        );
        // Whatever still names a visa, sponsorship or citizenship is left to him, except the
        // residence labels and the citizenship FIELD (the bank answers "Chile" there).
        let rest = format!(
            "{WORK_RIGHTS_LABELS}{residence}&!country of citizenship|country of nationality|nationality"
        );
        // "In what countries do you have the unrestricted right to work?" (Elastic, 2026-10-21) asks
        // for a list, not a yes or no: his only one is Chile, where he is a citizen.
        let which_countries = format!(
            "in what countries|in which countries|what countries|which countries|in what country|in which country|which country|what country&{AUSTRALIA_AUTHORIZED}"
        );
        let mut entries = vec![
            AnswerEntry::new(&[chile.as_str()], None),
            AnswerEntry::new(&[which_countries.as_str()], Some("Chile")),
            AnswerEntry::new(
                &[
                    "visa status|visa type|type of visa|current visa|immigration status|work authorization status|work authorisation status|work rights status|visa situation&!sponsor|sponsorship|sponsored|require|need",
                ],
                Some(AUSTRALIA_VISA_STATUS),
            ),
            AnswerEntry::new(&[negated.as_str()], Some("No")),
            AnswerEntry::new(&[both.as_str()], None),
            AnswerEntry::new(
                &[no_rights.as_str(), no_citizen.as_str(), no_visa.as_str()],
                Some("No"),
            ),
            AnswerEntry::new(&[needs_sponsor.as_str(), needs_visa.as_str()], Some("Yes"))
                .with_alternatives(&[
                    "Yes, I will require sponsorship",
                    "Yes, I require sponsorship",
                    "Yes, now and in the future",
                ]),
            AnswerEntry::new(&[rest.as_str()], None),
        ];
        entries.extend(self.entries.iter().cloned());
        AnswerBank {
            entries,
            consent_phrases: self.consent_phrases.clone(),
        }
    }

    /// The first entry (in file order) with a pattern that appears as whole words in the label.
    /// A pattern that starts with `=` must match the whole label instead (`=name` takes "Name *"
    /// but not "Company name"), for the generic labels that would otherwise catch too much.
    /// Otherwise a pattern is one or more terms joined by `&` (every term must match), a term is
    /// one or more phrases joined by `|` (any phrase may match), and a phrase that ends with `$`
    /// must be the end of the label (`works in the country you reside in$` takes "Do you work in
    /// the country you reside in?" but not "... reside in and in the US?"). A term that starts
    /// with `!` must NOT match (`visa&!reside` takes "Do you need a visa?" but not "... a visa in
    /// the country you reside in?"); a pattern needs at least one term without the `!`.
    pub fn lookup(&self, label: &str) -> Option<&AnswerEntry> {
        // `normalize` drops "co" as a company suffix, which would turn "Co-applicant name" into
        // "applicant name": gluing a "co-" prefix first keeps it one word.
        let text = normalize(&label.to_lowercase().replace("co-", "co"));
        self.entries.iter().find(|entry| {
            entry
                .patterns
                .iter()
                .any(|pattern| pattern_matches(&text, pattern))
        })
    }

    /// True when a checkbox label says one of the consent phrases Gonza approved and NOTHING else
    /// that matters. Two layers, both must pass. First the deny list (`CONSENT_NEVER`): a box that
    /// also accepts arbitration, terms and conditions, marketing or a background check is never
    /// ticked. Then a whitelist: take the approved phrase out of the label and every word that is
    /// left must be a filler word (`CONSENT_FILLER`: "I have read and agree to the ..."), a word of
    /// a URL, or a company possessive ("Cloudflare's"). A word nobody listed means the box asks
    /// for something the bank never saw ("... and to AI-assisted screening", "... for this and
    /// other vacancies"), so it is not ticked and the form goes to Gonza.
    pub fn is_consent(&self, label: &str) -> bool {
        let text = normalize(&without_urls(label));
        let asks_for_more = CONSENT_NEVER
            .iter()
            .any(|word| contains_phrase(&text, &normalize(word)));
        !asks_for_more
            && self
                .consent_phrases
                .iter()
                .any(|phrase| says_only(&text, &normalize(phrase)))
    }
}

/// Words that add no commitment to a privacy consent box: the verbs of reading and agreeing, the
/// glue words, and the nouns of the privacy text itself. Anything else (an unknown noun, a verb of
/// sharing, storing, screening, signing, receiving) keeps the box unticked.
const CONSENT_FILLER: &[&str] = &[
    "i",
    "have",
    "has",
    "read",
    "and",
    "or",
    "understand",
    "acknowledge",
    "acknowledged",
    "agree",
    "accept",
    "consent",
    "to",
    "the",
    "of",
    "our",
    "my",
    "your",
    "you",
    "we",
    "a",
    "an",
    "as",
    "that",
    "this",
    "is",
    "are",
    "be",
    "being",
    "will",
    "by",
    "with",
    "in",
    "on",
    "for",
    "its",
    "their",
    "personal",
    "data",
    "information",
    "processed",
    "processing",
    "collected",
    "collection",
    "use",
    "used",
    "described",
    "applicable",
    "hereby",
    "candidate",
    "applicant",
    "recruitment",
    "application",
    "job",
    "careers",
    "hiring",
    "process",
    "purposes",
    "purpose",
    "regarding",
    "about",
    "please",
    "review",
    "checking",
    "clicking",
    "submitting",
    "box",
    "checkbox",
    "policy",
    "privacy",
    "notice",
    "statement",
    // "Point of Data Transfer: Acknowledge" is how Greenhouse captions a lone checkbox.
    "point",
];

/// Words that end in a possessive `s` in a label ("it's", "that's") and are not a company name.
const NOT_A_COMPANY: &[&str] = &[
    "it", "that", "there", "what", "who", "where", "here", "let", "he", "she", "how", "when", "why",
];

/// The label with every URL token removed: a link's words say nothing about what the box accepts.
fn without_urls(label: &str) -> String {
    label
        .split_whitespace()
        .filter(|token| {
            let token = token.trim_start_matches(['(', '[', '<', '{', '"', '\'']);
            let lowered = token.to_ascii_lowercase();
            let is_url = lowered.starts_with("http://")
                || lowered.starts_with("https://")
                || lowered.starts_with("www.")
                || lowered.contains("://")
                || (token.contains('.') && token.contains('/'));
            !is_url
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// True when the normalized `text` contains `phrase` and, once every occurrence of the phrase is
/// taken out, only consent filler words are left (see `AnswerBank::is_consent`).
fn says_only(text: &str, phrase: &str) -> bool {
    let phrase_words: Vec<&str> = phrase.split_whitespace().collect();
    let words: Vec<&str> = text.split_whitespace().collect();
    let n = phrase_words.len();
    if n == 0 {
        return false;
    }
    let (last, head) = phrase_words.split_last().expect("n > 0");
    let mut taken = vec![false; words.len()];
    let mut found = false;
    let mut i = 0;
    while i + n <= words.len() {
        let here = &words[i..i + n];
        let last_word = here[n - 1];
        // The phrase's last word may carry a plural `s`, as in `contains_phrase`.
        if &here[..n - 1] == head
            && (last_word == *last || last_word.strip_suffix('s') == Some(last))
        {
            taken[i..i + n].fill(true);
            found = true;
            i += n;
        } else {
            i += 1;
        }
    }
    if !found {
        return false;
    }
    let rest: Vec<&str> = words
        .iter()
        .zip(&taken)
        .filter(|(_, taken)| !**taken)
        .map(|(word, _)| *word)
        .collect();
    let mut k = 0;
    while k < rest.len() {
        let word = rest[k];
        if rest.get(k + 1) == Some(&"s") && !NOT_A_COMPANY.contains(&word) {
            k += 2; // "Cloudflare's": a company possessive
            continue;
        }
        let plural_of_filler = word.len() >= 4
            && word
                .strip_suffix('s')
                .is_some_and(|stem| CONSENT_FILLER.contains(&stem));
        if !CONSENT_FILLER.contains(&word) && !plural_of_filler {
            return false;
        }
        k += 1;
    }
    true
}

/// Phrases that mean a consent box is about more than the privacy consent Gonza authorized. One of
/// them anywhere in the label keeps the box unticked even when it also says "privacy policy"
/// (nothing legal is accepted, no background check is consented to). This
/// is a safety rule, so it lives here and not in the editable answers file; a box it blocks goes
/// to Gonza.
const CONSENT_NEVER: &[&str] = &[
    "arbitration",
    "terms and conditions",
    "terms of use",
    "terms of service",
    "terms of employment",
    "agreement",
    "contract",
    "waive",
    "waiver",
    "binding",
    "non compete",
    "noncompete",
    "confidential",
    "background",
    "marketing",
    "newsletter",
    "promotional",
    "alert",
    "sms",
    "text message",
    "contacted",
    "contact me",
    "future",
    "other opportunities",
    "other roles",
    "other positions",
    "talent community",
    "talent network",
    "talent pool",
    "third party",
    "third parties",
    "sell",
    "retain",
    "retaining",
    "retention",
];

/// Whether one bank pattern matches an already normalized label (see `AnswerBank::lookup`). A
/// term that is empty after normalizing never matches, so a stray `&` or `|` can only make a
/// pattern narrower, never wider.
fn pattern_matches(text: &str, pattern: &str) -> bool {
    if let Some(exact) = pattern.strip_prefix('=') {
        return text == normalize(exact);
    }
    let terms = || pattern.split('&').map(str::trim);
    // A pattern of only `!` terms would match every label that lacks them: it needs a plain term.
    terms().any(|term| !term.starts_with('!'))
        && terms().all(|term| {
            let (excluded, term) = match term.strip_prefix('!') {
                Some(rest) => (true, rest),
                None => (false, term),
            };
            let found = term
                .split('|')
                .any(|phrase| match phrase.trim().strip_suffix('$') {
                    Some(end) => ends_with_phrase(text, &normalize(end)),
                    None => contains_phrase(text, &normalize(phrase)),
                });
            found != excluded
        })
}

/// True when `text` is `phrase`, or ends with `phrase` as whole words. Both are already normalized.
fn ends_with_phrase(text: &str, phrase: &str) -> bool {
    !phrase.is_empty()
        && text.ends_with(phrase)
        && text[..text.len() - phrase.len()]
            .chars()
            .next_back()
            .is_none_or(|c| !c.is_ascii_alphanumeric())
}

/// True when `phrase` occurs in `text` delimited by non-alphanumerics, with an optional plural
/// `s` after it. Both strings are already normalized (lowercase ASCII letters, digits, spaces).
fn contains_phrase(text: &str, phrase: &str) -> bool {
    if phrase.is_empty() {
        return false;
    }
    let ends_word = |rest: &str| {
        rest.chars()
            .next()
            .is_none_or(|c| !c.is_ascii_alphanumeric())
    };
    text.match_indices(phrase).any(|(start, _)| {
        let starts_word = text[..start]
            .chars()
            .next_back()
            .is_none_or(|c| !c.is_ascii_alphanumeric());
        let after = &text[start + phrase.len()..];
        starts_word && (ends_word(after) || after.strip_prefix('s').is_some_and(ends_word))
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawBank {
    #[serde(default)]
    consent_checkboxes: Vec<String>,
    #[serde(default)]
    entry: Vec<AnswerEntry>,
}

/// Parses the text of `profile/answers.toml`: an optional `consent_checkboxes` list and `[[entry]]`
/// tables with `patterns`, an optional `value` and optional `alternatives`. Unknown keys are errors.
pub fn parse_answers(text: &str) -> Result<AnswerBank, toml::de::Error> {
    let raw: RawBank = toml::from_str(text)?;
    Ok(AnswerBank {
        entries: raw.entry,
        consent_phrases: raw.consent_checkboxes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_consent_phrase_matches_only_its_own_words_and_their_plural() {
        let bank = AnswerBank {
            entries: vec![],
            consent_phrases: vec!["privacy policy".into()],
        };
        assert!(bank.is_consent("I have read the privacy policy"));
        assert!(!bank.is_consent("I have read the privacy notice"));
        // A plural of a filler word ("candidates") is filler too.
        assert!(bank.is_consent("I have read the privacy policy for candidates"));
    }

    #[test]
    fn the_australian_ask_keeps_every_digit_of_the_posted_amount() {
        assert_eq!(
            australian_salary_ask("Software Engineer", "AUD 123,456 - 150,000"),
            123_456
        );
    }

    #[test]
    fn a_bonus_or_allowance_is_not_the_salary_to_ask() {
        for (posting, expected) in [
            (
                "Salary A$140,000 - A$170,000. Sign-on bonus A$60,000.",
                140_000,
            ),
            ("Salary A$140,000 - A$170,000 plus bonus", 140_000),
            ("A$150k base, $75k sign-on", 150_000),
            (
                "Relocation allowance of A$60,000. Salary A$120,000 - A$140,000",
                120_000,
            ),
            (
                "A$60,000 relocation allowance. Salary A$120,000 - A$140,000",
                120_000,
            ),
            (
                "Signing bonus: A$70,000\nSalary A$130,000 - A$150,000",
                130_000,
            ),
            // "Budget" names the pay as often as anything else, so it is not one of the words.
            ("Salary budget A$130,000 - A$150,000", 130_000),
            ("The budget for this role is A$130,000 - A$150,000", 130_000),
        ] {
            assert_eq!(
                australian_salary_ask("Software Engineer", posting),
                expected,
                "{posting}"
            );
        }
    }

    #[test]
    fn a_posting_whose_only_figures_are_not_pay_asks_for_the_level() {
        let unpriced = australian_salary_ask("Software Engineer", "We are hiring.");
        assert_eq!(
            australian_salary_ask("Software Engineer", "We raised A$200,000 in grants"),
            unpriced
        );
    }

    #[test]
    fn thousands_are_grouped_from_the_right() {
        assert_eq!(thousands(85_000), "85,000");
        assert_eq!(thousands(123_456), "123,456");
        assert_eq!(thousands(1_234_567), "1,234,567");
    }

    #[test]
    fn a_label_loses_its_links_and_keeps_words_with_a_dot_or_a_slash() {
        assert_eq!(
            without_urls(
                "Read www.c.example and/or U.S. terms at https://a.example/x or ftp://host"
            ),
            "Read and/or U.S. terms at or"
        );
    }

    #[test]
    fn a_term_with_a_bang_must_not_match_and_a_pattern_needs_a_plain_term() {
        assert!(pattern_matches("do you need a visa", "visa&!reside"));
        assert!(!pattern_matches(
            "do you need a visa in the country you reside in",
            "visa&!reside"
        ));
        assert!(!pattern_matches(
            "do you need a visa in the country you reside in",
            "visa&!reside|residence"
        ));
        assert!(
            !pattern_matches("anything at all", "!reside"),
            "only `!` terms would match every label that lacks them"
        );
        assert!(!pattern_matches("anything at all", "!"));
    }
}
