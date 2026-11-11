//! The pure core of form filling: the outcome types of a submission, the planner that decides
//! from the answer bank what every scanned field gets, how a drop-down option is matched, and how
//! the page text is read (walls, confirmations, validation errors). The browser crate drives the
//! page with this; the question review reads answers through the same rules.

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;
use serde::Deserialize;

use crate::answers::{AnswerBank, AnswerEntry};
use crate::text::normalize;

/// How far a filler goes with a form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubmitMode {
    /// Fill and take a screenshot; never click submit.
    DryRun,
    /// Fill, click submit and wait for the page to confirm.
    Live,
    /// Fill in a visible browser and wait for Gonza to click submit (and to solve any captcha).
    Assist,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubmitStatus {
    DryRun,
    Submitted,
    NeedsGonza,
}

#[derive(Debug, Clone)]
pub struct SubmitResult {
    pub status: SubmitStatus,
    pub reason: Option<String>,
    pub screenshot: Option<PathBuf>,
}

static RESUME_LABEL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)r[eé]sum[eé]|\bcv\b|curr[ií]culum").expect("valid resume regex")
});

/// One form control as reported by the page scan (`FIELD_JS`). For a radio button, a button of a
/// Yes/No toggle (`tag` is `button`, `kind` `radio`) or a checkbox of a group, `label` is the
/// question of its group and `option` its own text ("Yes", "No, I do not need sponsorship");
/// `name` identifies the group (the shared `name` attribute, or a key made up by the scan when the
/// page names every checkbox differently). A checkbox that stands alone keeps its own text as
/// `label` and has no `option`. `checked` says whether that radio button, checkbox or toggle button
/// is selected when the page is scanned. `kind` is the `type` attribute, except `combobox`
/// (react-select, typeahead) and `date` (date pickers drawn as text inputs).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct FieldInfo {
    pub idx: usize,
    pub tag: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub label: String,
    pub required: bool,
    #[serde(default)]
    pub option: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub checked: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FillValue {
    File(PathBuf),
    /// Option texts to try in order: the bank value first, then its alternatives.
    Select(Vec<String>),
    Text(String),
    /// Tick this checkbox or radio button.
    Check,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FillAction {
    pub idx: usize,
    pub label: String,
    pub value: FillValue,
}

/// What to do to the page, and which required questions have no approved answer.
#[derive(Debug, Default, PartialEq)]
pub struct FillPlan {
    pub actions: Vec<FillAction>,
    pub missing: Vec<String>,
    /// Options that already show the answer the bank gives (a button pressed, a box ticked): they
    /// need no action, because a second click may undo them, and are not left for Gonza.
    pub kept: Vec<usize>,
    /// Boxes the page ticked that sign Gonza up for something he never agreed to (marketing, SMS,
    /// arbitration...). The filler never unticks, so each one is reported in `missing` and flagged
    /// on the page for him to untick.
    pub to_untick: Vec<usize>,
}

impl FillPlan {
    fn note_missing(&mut self, label: &str) {
        if !self.missing.iter().any(|known| known == label) {
            self.missing.push(label.to_string());
        }
    }

    /// Reports a box the page ticked and the allow list would never tick: `idx` is the box,
    /// `label` its question and `option` its own text (empty for a box that stands alone).
    fn note_pre_ticked(&mut self, idx: usize, label: &str, option: &str) {
        let shown = if option.is_empty() {
            label.to_string()
        } else {
            format!("{label}: {option}")
        };
        self.note_missing(&format!("{shown} (casilla ya marcada; desmárcala tú)"));
        self.to_untick.push(idx);
    }

    /// Answers one question made of several options (radio buttons, toggle buttons, a checkbox
    /// group) from the bank: the options the answer names are chosen, an option that is already
    /// selected is only recorded as kept. When the bank names none of them the question is missing
    /// if any option is required, and left alone otherwise.
    fn answer_group(
        &mut self,
        bank: &AnswerBank,
        label: &str,
        members: &[&FieldInfo],
        several: bool,
    ) {
        let wanted = bank
            .lookup(label)
            .map(AnswerEntry::candidates)
            .unwrap_or_default();
        let picked = picked_options(&wanted, members, several);
        if picked.is_empty() {
            if members.iter().any(|member| member.required) {
                let fallback = if several { "checkbox" } else { "radio" };
                self.note_missing(label_or(label, fallback));
            }
            return;
        }
        for member in picked {
            if member.checked {
                self.kept.push(member.idx);
            } else {
                self.actions.push(FillAction {
                    idx: member.idx,
                    label: label.to_string(),
                    value: FillValue::Check,
                });
            }
        }
    }
}

/// The options of a group that the bank's answer names, by the rules of `options_named`.
fn picked_options<'a>(
    wanted: &[String],
    members: &[&'a FieldInfo],
    several: bool,
) -> Vec<&'a FieldInfo> {
    let texts: Vec<String> = members.iter().map(|member| member.option.clone()).collect();
    options_named(&texts, wanted, several)
        .into_iter()
        .map(|index| members[index])
        .collect()
}

/// `label`, or `fallback` when the field has none (how a missing answer is named).
pub fn label_or<'a>(label: &'a str, fallback: &'a str) -> &'a str {
    if label.is_empty() { fallback } else { label }
}

/// Decides, without touching a browser, how every field is handled: resume inputs get the CV,
/// text fields get the bank value, selects get the value and its alternatives, a radio group (or a
/// Yes/No pair of buttons) is answered when the bank value names one of its options, a "select all
/// that apply" checkbox group gets every option the bank value lists (never an option that says
/// "Not Hispanic or Latino"), and a checkbox that stands alone is ticked only when it is required,
/// its text matches a consent phrase and names no Hispanic or Latino answer. A date or number box that cannot take the answer's text,
/// and a control of a kind the planner does not know, count as unanswered. A textarea, or a
/// one-line box under a long label, is answered only by an entry written for its whole label (an
/// `=` pattern), never by a keyword inside a paragraph. A single-choice group that offers Hispanic
/// or Latino is answered only by a candidate that names it, and a bare Yes or No never picks between several options that merely
/// start with it. A box the page already ticked that signs Gonza up for more than the privacy
/// consent (marketing, SMS, arbitration...) is reported as missing and listed in `to_untick`: the
/// filler never unticks. Every required field without an answer (including other file inputs) is
/// reported as missing, once per label.
pub fn plan_fields(fields: &[FieldInfo], bank: &AnswerBank, cv_pdf: &Path) -> FillPlan {
    let mut plan = FillPlan::default();
    for field in fields {
        let label = field.label.trim();
        match field.kind.as_str() {
            "file" => {
                if RESUME_LABEL.is_match(label) {
                    plan.actions.push(FillAction {
                        idx: field.idx,
                        label: label.to_string(),
                        value: FillValue::File(cv_pdf.to_path_buf()),
                    });
                } else if field.required {
                    plan.note_missing(label_or(label, "archivo"));
                }
            }
            "checkbox" => {
                let members = choice_group(fields, field);
                if members.len() >= 2 {
                    // "Select all that apply": one decision for the whole group, taken at its
                    // first option. The question says nothing about consent, so the allow list
                    // plays no part: only an answer the bank gives ticks anything.
                    if members.first().map(|first| first.idx) == Some(field.idx) {
                        plan.answer_group(bank, label, &members, true);
                    }
                    // An option the page already ticked goes out as it is: when it signs him up
                    // for something, he has to see it.
                    if field.checked && signs_him_up_for_more(&field.option) {
                        plan.note_pre_ticked(field.idx, label, &field.option);
                    }
                    continue;
                }
                if field.checked && signs_him_up_for_more(label) {
                    plan.note_pre_ticked(field.idx, label, "");
                    continue;
                }
                if !field.required {
                    continue; // newsletters and the like are never ticked
                }
                // A box whose text names Hispanic or Latino answers the question left to Gonza,
                // whatever the page captions it (a "privacy policy" caption over "Not Hispanic or
                // Latino" was found by the fuzz body).
                if mentions_hispanic(label) || mentions_hispanic(&field.option) {
                    plan.note_missing(label_or(label, "checkbox"));
                    continue;
                }
                if bank.is_consent(label) {
                    if field.checked {
                        plan.kept.push(field.idx);
                    } else {
                        plan.actions.push(FillAction {
                            idx: field.idx,
                            label: label.to_string(),
                            value: FillValue::Check,
                        });
                    }
                } else {
                    plan.note_missing(label_or(label, "checkbox"));
                }
            }
            "radio" => {
                // One decision per group, taken when its first button comes up.
                let members = choice_group(fields, field);
                if members.first().map(|first| first.idx) == Some(field.idx) {
                    plan.answer_group(bank, label, &members, false);
                }
            }
            _ => {
                let essay = is_essay_box(&field.tag, &field.kind, label);
                let candidates = answer_entry(bank, label, essay)
                    .map(AnswerEntry::candidates)
                    .unwrap_or_default();
                let Some(first) = candidates.first().cloned() else {
                    if field.required {
                        plan.note_missing(label_or(label, "campo sin etiqueta"));
                    }
                    continue;
                };
                // A date box, a number box or a control of a kind this planner does not know cannot
                // take just any text: the question stays open (missing when required) instead of
                // typing a word the page will reject.
                if !takes_text(&field.kind, &first) {
                    if field.required {
                        plan.note_missing(label_or(label, "campo sin etiqueta"));
                    }
                    continue;
                }
                // Greenhouse's drop-downs are inputs, not selects: a one-line input keeps every
                // option text to try (a plain text box then takes the first), a textarea cannot be
                // a drop-down.
                let value =
                    if field.tag == "select" || (field.tag == "input" && candidates.len() > 1) {
                        FillValue::Select(candidates)
                    } else {
                        FillValue::Text(first)
                    };
                plan.actions.push(FillAction {
                    idx: field.idx,
                    label: label.to_string(),
                    value,
                });
            }
        }
    }
    plan
}

/// The options that make up one question, in page order: the radio buttons (or toggle buttons) or
/// the checkboxes of the group `field` belongs to. A checkbox with no `option` stands alone, and so
/// does any other control: each is a group of its own.
pub fn choice_group<'a>(fields: &'a [FieldInfo], field: &'a FieldInfo) -> Vec<&'a FieldInfo> {
    let grouped = match field.kind.as_str() {
        "radio" => true,
        "checkbox" => !field.option.is_empty(),
        _ => false,
    };
    if grouped {
        fields
            .iter()
            .filter(|other| same_group(field, other))
            .collect()
    } else {
        vec![field]
    }
}

/// Options of one group share a `name`; unnamed ones are grouped by their question.
fn same_group(a: &FieldInfo, b: &FieldInfo) -> bool {
    if b.kind != a.kind || (a.kind == "checkbox" && b.option.is_empty()) {
        return false;
    }
    if a.name.is_empty() {
        b.label.trim() == a.label.trim()
    } else {
        b.name == a.name
    }
}

/// Lower-case words only, so punctuation and spacing do not matter: "No, I don't." -> "no i don t".
fn words(text: &str) -> String {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// A radio option matches the wanted answer when its text is the answer or starts with it as a
/// whole word: "No" matches "No, I do not require sponsorship" but not "Not sure".
fn option_matches(option: &str, wanted: &str) -> bool {
    let (option, wanted) = (words(option), words(wanted));
    !wanted.is_empty() && (option == wanted || option.starts_with(&format!("{wanted} ")))
}

/// A bare yes or no ("No", "Sí"): the one answer whose prefix says nothing about what follows it,
/// because "No, but I would relocate" and "No, and I would not" both start with "No" and each adds
/// a claim of its own.
fn is_bare_yes_no(wanted_words: &str) -> bool {
    matches!(wanted_words, "yes" | "no" | "si" | "sí")
}

/// The index of the option of a drop-down list for `wanted`: the exact one, else the first that
/// starts with it as a whole word, else (only for a place such as "Santiago, Chile", whose parts
/// a city list spreads over "Santiago, Metropolitana, Chile") the first that has all its words.
/// A bare yes or no takes a prefix match only when exactly one option starts with it: with
/// several, none is chosen (the exact "No" is always preferred). Never an option that says
/// "(Not Hispanic or Latino)", and no loose match for a plain answer: "Computer Engineering" is
/// not "Computer Science and Engineering".
pub fn best_option(options: &[String], wanted: &str) -> Option<usize> {
    let wanted_words = words(wanted);
    if wanted_words.is_empty() {
        return None;
    }
    let allowed = |option: &&String| !claims_not_hispanic(option);
    let candidates = || {
        options
            .iter()
            .enumerate()
            .filter(|(_, option)| allowed(option))
    };
    if let Some((index, _)) = candidates().find(|(_, option)| words(option) == wanted_words) {
        return Some(index);
    }
    let prefixed: Vec<usize> = candidates()
        .filter(|(_, option)| option_matches(option, wanted))
        .map(|(index, _)| index)
        .collect();
    match prefixed.as_slice() {
        [] => {}
        [only] => return Some(*only),
        [first, ..] if !is_bare_yes_no(&wanted_words) => return Some(*first),
        _ => return None,
    }
    if wanted.contains(',') {
        let parts: Vec<&str> = wanted_words.split(' ').collect();
        return candidates()
            .find(|(_, option)| {
                let have = words(option);
                let have: Vec<&str> = have.split(' ').collect();
                parts.iter().all(|part| have.contains(part))
            })
            .map(|(index, _)| index);
    }
    None
}

/// Why no option of a list is picked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoOption {
    /// None of the candidates names an option of the list.
    NoMatch,
    /// An option exists, but the list offers Hispanic or Latino and no candidate names it: any
    /// other pick from a single-choice list that does would answer that question for him.
    Reserved,
}

impl std::fmt::Display for NoOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            NoOption::NoMatch => "no option of the drop-down matches",
            NoOption::Reserved => "the list offers Hispanic or Latino: that answer is Gonza's",
        })
    }
}

impl std::error::Error for NoOption {}

/// The option of a single-choice list that the first candidate naming one decides. A list that
/// offers Hispanic or Latino takes one answer for race and ethnicity together ("White" there
/// excludes "Hispanic or Latino"), and Gonza is Hispanic or Latino: only a candidate that names it
/// picks anything, and never an option that says "Not". Any other candidate there is his to answer.
pub fn pick_one(options: &[String], candidates: &[String]) -> Result<usize, NoOption> {
    let named = candidates
        .iter()
        .find_map(|candidate| best_option(options, candidate));
    if options.iter().any(|option| mentions_hispanic(option)) {
        return candidates
            .iter()
            .filter(|candidate| mentions_hispanic(candidate))
            .find_map(|candidate| best_option(options, candidate))
            .ok_or(if named.is_some() {
                NoOption::Reserved
            } else {
                NoOption::NoMatch
            });
    }
    named.ok_or(NoOption::NoMatch)
}

/// The options of a "select all that apply" list named by the first candidate that names any: a
/// candidate is a list separated by `;` ("Python; Rust"), each item resolved like a single answer.
/// Ticking White alone says nothing about a Hispanic option beside it, so that stays allowed.
fn pick_many(options: &[String], candidates: &[String]) -> Vec<usize> {
    for candidate in candidates {
        let mut named: Vec<usize> = candidate
            .split(';')
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .filter_map(|item| best_option(options, item))
            .collect();
        named.sort_unstable();
        named.dedup();
        if !named.is_empty() {
            return named;
        }
    }
    Vec::new()
}

/// The indexes of the options of a list that the bank's answer names: the first candidate (the
/// value, then each alternative) that names any option decides. A choice between options
/// (`several` false) takes one option, by the rules of `pick_one`; a "select all that apply" list
/// takes every option the candidate lists. Shared by the planner and the question review, so both
/// read an answer the same way.
pub fn options_named(options: &[String], candidates: &[String], several: bool) -> Vec<usize> {
    if several {
        pick_many(options, candidates)
    } else {
        pick_one(options, candidates).ok().into_iter().collect()
    }
}

/// Labels of this many words or more are questions with several parts.
const LONG_LABEL_WORDS: usize = 12;

/// True for a free-text control whose question is an essay: a textarea, or a one-line box under a
/// long label. A drop-down, a Yes/No toggle and a checkbox group can only take one of their own
/// options, so a long label cannot make them carry extra claims.
pub fn is_essay_box(tag: &str, kind: &str, label: &str) -> bool {
    if tag == "select" || kind == "combobox" {
        return false;
    }
    tag == "textarea" || label.split_whitespace().count() >= LONG_LABEL_WORDS
}

/// The bank entry for `label`. For an essay box only an entry written for that whole label (an
/// `=` pattern) answers: a keyword inside a paragraph ("salary") would put a one-line answer under
/// a question that asks for much more. Shared by the planner and the question review.
pub fn answer_entry<'a>(bank: &'a AnswerBank, label: &str, essay: bool) -> Option<&'a AnswerEntry> {
    let entry = bank.lookup(label)?;
    (!essay || written_for(entry, label)).then_some(entry)
}

/// True when one of the entry's `=` patterns is exactly this label.
fn written_for(entry: &AnswerEntry, label: &str) -> bool {
    let text = normalize(label);
    entry.patterns.iter().any(|pattern| {
        pattern
            .strip_prefix('=')
            .is_some_and(|exact| normalize(exact) == text)
    })
}

/// True for a checkbox text that signs Gonza up for more than the privacy consent he allowed
/// (marketing, SMS, arbitration, a talent network...): the consent deny list of the answer bank.
/// Asked of the bank itself, with the label as its own allowed phrase, so the list stays in one
/// place: a label the bank refuses even then hits that list.
fn signs_him_up_for_more(label: &str) -> bool {
    if normalize(label).is_empty() {
        return false;
    }
    let only_this = AnswerBank {
        consent_phrases: vec![label.to_string()],
        ..AnswerBank::default()
    };
    !only_this.is_consent(label)
}

/// The text the page scan gives a checkbox that is alone in a group with a caption: "caption:
/// option", the caption without its trailing star. The question review builds it the same way.
pub fn single_checkbox_label(question: &str, option: &str) -> String {
    let caption = question.trim_end_matches(|c: char| c == '*' || c.is_whitespace());
    format!("{caption}: {option}")
}

/// The index of the option that is exactly `wanted`, if the list already shows it.
pub fn exact_option(options: &[String], wanted: &str) -> Option<usize> {
    let wanted_words = words(wanted);
    options
        .iter()
        .position(|option| words(option) == wanted_words && !claims_not_hispanic(option))
}

/// What to type into a drop-down to find `candidate`: for a place ("Santiago, Chile") the first
/// part, because a city list finds nothing for the whole text with its comma.
pub fn search_text(candidate: &str) -> &str {
    match candidate.split(',').next().map(str::trim) {
        Some(first) if !first.is_empty() => first,
        _ => candidate,
    }
}

fn is_hispanic_word(token: &str) -> bool {
    matches!(token, "hispanic" | "latino" | "latina" | "latinx")
}

/// True when the text names Hispanic or Latino in any way ("Hispanic/Latino", "Latinx", "Not
/// Hispanic or Latino"). A list with such an option asks the question Gonza keeps for himself.
pub fn mentions_hispanic(text: &str) -> bool {
    words(text).split(' ').any(is_hispanic_word)
}

/// True for an option such as "White (Not Hispanic or Latino)": it answers the Hispanic or Latino
/// question as well, and Gonza keeps that one for himself, so it is never picked for him. A
/// negation (not, non, no, than, excluding, except, without) within three words of Hispanic or
/// Latino covers "not of Hispanic origin", "Non-Latino", "I am not of Latino descent", "Other than
/// Hispanic or Latino" and "White except Hispanic"; refusing too much only leaves the question for
/// him.
pub fn claims_not_hispanic(option: &str) -> bool {
    let option = words(option);
    let tokens: Vec<&str> = option.split(' ').collect();
    tokens.iter().enumerate().any(|(i, token)| {
        matches!(
            *token,
            "not" | "non" | "no" | "than" | "excluding" | "excluded" | "except" | "without"
        ) && tokens[i + 1..]
            .iter()
            .take(3)
            .any(|next| is_hispanic_word(next))
    })
}

/// True when `value` can be typed into a control of this kind: the text-like boxes (and the
/// drop-downs, which are typed into to find their option) take free text, a number box only a
/// number. A date or time box takes no free text, and neither does a kind this planner does not
/// know (a color picker, a password box, whatever a later scan invents): for those the question
/// stays open. A list of the kinds that do take text, not of the ones that do not, so a new kind
/// is safe until someone decides what to type into it.
fn takes_text(kind: &str, value: &str) -> bool {
    match kind {
        "" | "text" | "search" | "email" | "tel" | "url" | "combobox" => true,
        "number" | "range" => value.trim().parse::<f64>().is_ok(),
        _ => false,
    }
}

const CAPTCHA_PHRASES: &[&str] = &[
    "verification code",
    "código de verificación",
    "security code",
    "i'm not a robot",
    "verify you are human",
    "verify you're human",
];
const LOGIN_PHRASES: &[&str] = &[
    "sign in to apply",
    "log in to apply",
    "login to apply",
    "create an account to apply",
    "inicia sesión para",
    "iniciar sesión para",
];
const BLOCKED_PHRASES: &[&str] = &[
    "access denied",
    "unusual traffic",
    "are you a robot",
    "request blocked",
    "you have been blocked",
    "just a moment",
];

/// Reason (`captcha`, `login_required`, `blocked_page`) when the visible page text shows a wall
/// that must never be bypassed; `None` for an ordinary application page.
pub fn blocker_in_text(text: &str) -> Option<&'static str> {
    let lower = text.to_lowercase().replace('\u{2019}', "'");
    let has = |phrases: &[&str]| phrases.iter().any(|phrase| lower.contains(phrase));
    if has(CAPTCHA_PHRASES) {
        Some("captcha")
    } else if has(LOGIN_PHRASES) {
        Some("login_required")
    } else if has(BLOCKED_PHRASES) {
        Some("blocked_page")
    } else {
        None
    }
}

/// Text that only a confirmation page shows (lower case; compared against the page text).
const SUCCESS_PHRASES: &[&str] = &[
    "thank you for applying",
    "thanks for applying",
    "thank you for your application",
    "application has been submitted",
    "application submitted",
    "application was received",
    "application has been received",
    "we have received your application",
    "we've received your application",
    "received your application",
    "successfully submitted",
    "gracias por postular",
    "postulación enviada",
    "postulación recibida",
    "hemos recibido tu postulación",
    "recibimos tu postulación",
];

/// Fragments of the path of the thank-you pages the hosted boards redirect to.
const CONFIRMATION_URL_TOKENS: &[&str] = &[
    "thank",
    "confirmation",
    "success",
    "submitted",
    "applied",
    "gracias",
];

/// What the page looks like at one instant: where it is, what it says and which validation
/// error it shows. Taken before the click and again while waiting for the confirmation, so that
/// only what *changed* counts.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PageSnapshot {
    pub url: String,
    pub text: String,
    pub errors: String,
}

/// How many characters before a success phrase are searched for a denial.
const DENIAL_REACH: usize = 15;

/// A denial word (or the end of "haven't") standing on its own, as English and Spanish pages write
/// them just before a success phrase: "we have not received", "unable to", "aún no".
static DENIAL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?:^|\W)(?:not|no|never|unable to|could not|aún no|todavía no)(?:\W|$)|n't(?:\W|$)",
    )
    .expect("valid denial regex")
});

/// True when `phrase` stands in `text` (both lower case) at least once without a denial in the
/// `DENIAL_REACH` characters before it: "we have not received your application" confirms nothing.
fn affirmed(text: &str, phrase: &str) -> bool {
    text.match_indices(phrase).any(|(at, _)| {
        let before: Vec<char> = text[..at].chars().rev().take(DENIAL_REACH).collect();
        let window: String = before.into_iter().rev().collect();
        !DENIAL.is_match(&window)
    })
}

/// The path of an address, lower case: no scheme, host, query or fragment. A thank-you token in the
/// query (`?applied=false`, `?msg=not-submitted`) or in the host says nothing about the page.
fn path_of(url: &str) -> String {
    let url = url.to_lowercase();
    let rest = url.split_once("://").map_or(url.as_str(), |(_, rest)| rest);
    let rest = rest.split(['?', '#']).next().unwrap_or_default();
    // After the host (the first slash) when there was a scheme; the whole text otherwise.
    match (url.contains("://"), rest.find('/')) {
        (true, Some(slash)) => rest[slash..].to_string(),
        (true, None) => String::new(),
        (false, _) => rest.to_string(),
    }
}

/// True when the page gained a confirmation signal: a thank-you token that was not in the address's
/// path before, or a success phrase that was not in the text before and is not denied where it
/// stands. A phrase that was already on the page (a sidebar saying "Thanks for applying!") proves
/// nothing, and neither does "we have not received your application".
pub fn newly_confirmed(before: &PageSnapshot, after: &PageSnapshot) -> bool {
    let (was, now) = (path_of(&before.url), path_of(&after.url));
    let token_appeared = CONFIRMATION_URL_TOKENS
        .iter()
        .any(|token| now.contains(token) && !was.contains(token));
    let (was, now) = (
        before.text.to_lowercase().replace('\u{2019}', "'"),
        after.text.to_lowercase().replace('\u{2019}', "'"),
    );
    token_appeared
        || SUCCESS_PHRASES
            .iter()
            .any(|phrase| affirmed(&now, phrase) && !affirmed(&was, phrase))
}

/// The validation message that appeared after the click; one that was already showing is not
/// a reaction to it.
pub fn newly_rejected(before: &PageSnapshot, after: &PageSnapshot) -> Option<String> {
    (!after.errors.is_empty() && after.errors != before.errors).then(|| after.errors.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn option(kind: &str, name: &str, label: &str, option: &str) -> FieldInfo {
        FieldInfo {
            idx: 0,
            tag: "input".into(),
            kind: kind.into(),
            label: label.into(),
            required: true,
            option: option.into(),
            name: name.into(),
            checked: false,
        }
    }

    #[test]
    fn a_group_is_one_kind_one_name_or_one_question() {
        let radio = option("radio", "g", "Q", "A");
        assert!(same_group(&radio, &option("radio", "g", "Other", "B")));
        assert!(!same_group(&radio, &option("checkbox", "g", "Q", "B")));
        assert!(!same_group(
            &option("checkbox", "g", "Q", "A"),
            &option("checkbox", "g", "Q", "")
        ));
        let unnamed = option("radio", "", "Q1", "A");
        assert!(same_group(&unnamed, &option("radio", "", " Q1 ", "B")));
        assert!(!same_group(&unnamed, &option("radio", "", "Q2", "B")));
    }

    #[test]
    fn the_reason_no_option_was_picked_is_spelled_out() {
        assert!(NoOption::NoMatch.to_string().contains("no option"));
        assert!(
            NoOption::Reserved
                .to_string()
                .contains("Hispanic or Latino")
        );
    }

    #[test]
    fn a_lone_checkbox_label_drops_the_star_of_its_caption() {
        assert_eq!(single_checkbox_label("I agree *", "Yes"), "I agree: Yes");
    }

    #[test]
    fn the_exact_option_is_matched_by_its_words_and_never_claims_not_hispanic() {
        let options: Vec<String> = ["Yes", "No, I don't", "White (Not Hispanic or Latino)"]
            .map(String::from)
            .to_vec();
        assert_eq!(exact_option(&options, "yes"), Some(0));
        assert_eq!(exact_option(&options, "No I don't"), Some(1));
        assert_eq!(exact_option(&options, "Maybe"), None);
        assert_eq!(
            exact_option(&options, "White (Not Hispanic or Latino)"),
            None
        );
    }

    #[test]
    fn a_place_is_searched_by_its_first_part_unless_that_part_is_empty() {
        assert_eq!(search_text("Santiago, Chile"), "Santiago");
        assert_eq!(search_text(", Chile"), ", Chile");
    }

    fn snapshot(url: &str, text: &str) -> PageSnapshot {
        PageSnapshot {
            url: url.into(),
            text: text.into(),
            errors: String::new(),
        }
    }

    #[test]
    fn a_success_phrase_that_is_denied_does_not_confirm() {
        let before = snapshot("https://x.test/apply", "Apply for Senior AI Engineer.");
        for denied in [
            "We have not received your application yet.",
            "We haven't received your application.",
            "We haven\u{2019}t received your application.",
            "Sorry, we have never received your application.",
            "We could not say your application was received.",
            "Unable to say application was received.",
            "Aún no hemos recibido tu postulación.",
            "Todavía no recibimos tu postulación.",
        ] {
            assert!(
                !newly_confirmed(&before, &snapshot(&before.url, denied)),
                "{denied}"
            );
        }
        // The same words in an affirmative sentence still confirm, as do the plain phrases.
        for confirmed in [
            "We have received your application.",
            "Thank you for applying",
            "Hemos recibido tu postulación",
            "Note: we know that your application has been submitted.",
        ] {
            assert!(
                newly_confirmed(&before, &snapshot(&before.url, confirmed)),
                "{confirmed}"
            );
        }
    }

    #[test]
    fn a_denied_phrase_that_gives_way_to_an_affirmative_one_is_news() {
        let before = snapshot(
            "https://x.test/apply",
            "We have not received your application yet.",
        );
        let after = snapshot(
            "https://x.test/apply",
            "We have received your application. Thank you!",
        );
        assert!(newly_confirmed(&before, &after));
    }

    #[test]
    fn the_address_confirms_only_by_its_path() {
        let before = snapshot("https://x.test/apply", "form");
        let moved = |url: &str| newly_confirmed(&before, &snapshot(url, "form"));
        assert!(!moved("https://x.test/apply?applied=false"));
        assert!(!moved("https://x.test/apply?msg=not-submitted"));
        assert!(!moved("https://x.test/apply#success"));
        assert!(!moved("https://success.x.test/apply"));
        assert!(moved("https://x.test/apply/thank-you"));
        assert!(moved("https://x.test/APPLY/Confirmation?id=3"));
        assert!(moved("https://x.test/jobs/1/submitted#top"));
    }

    #[test]
    fn a_negation_reaches_three_words_past_itself() {
        assert!(claims_not_hispanic("White, not of any Hispanic origin"));
        assert!(!claims_not_hispanic(
            "White, not of any other Hispanic origin"
        ));
    }
}
