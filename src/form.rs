//! Application form filling: a pure planning core plus the Playwright code that drives the page.

use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::Duration;

use anyhow::Context;
use playwright_rs::{
    BrowserContext, GotoOptions, Page, Playwright, ScreenshotOptions, SelectOption, SelectOptions,
    WaitUntil,
};
use regex::Regex;
use serde::Deserialize;

use crate::answers::{AnswerBank, AnswerEntry};
use crate::gate::{is_listing_only, is_manual_only};
use crate::handoff::is_handoff;
use crate::models::Job;
use crate::navigation::{self, NavPolicy};
use crate::text::{normalize, safe_key};

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

/// Fills the application form of `job` and, according to `mode`, submits it or waits for Gonza
/// to. Anything it cannot answer or must not bypass comes back as `NeedsGonza` with a reason;
/// `Submitted` means the page confirmed the submission.
#[async_trait::async_trait]
pub trait FormFiller: Send + Sync {
    async fn submit(
        &self,
        job: &Job,
        cv_pdf: &Path,
        bank: &AnswerBank,
        mode: SubmitMode,
        shots_dir: &Path,
    ) -> anyhow::Result<SubmitResult>;
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
pub(crate) struct FieldInfo {
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
pub(crate) enum FillValue {
    File(PathBuf),
    /// Option texts to try in order: the bank value first, then its alternatives.
    Select(Vec<String>),
    Text(String),
    /// Tick this checkbox or radio button.
    Check,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FillAction {
    pub idx: usize,
    pub label: String,
    pub value: FillValue,
}

/// What to do to the page, and which required questions have no approved answer.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct FillPlan {
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

fn label_or<'a>(label: &'a str, fallback: &'a str) -> &'a str {
    if label.is_empty() { fallback } else { label }
}

/// Decides, without touching a browser, how every field is handled: resume inputs get the CV,
/// text fields get the bank value, selects get the value and its alternatives, a radio group (or a
/// Yes/No pair of buttons) is answered when the bank value names one of its options, a "select all
/// that apply" checkbox group gets every option the bank value lists (never an option that says
/// "Not Hispanic or Latino"), and a checkbox that stands alone is ticked only when it is required
/// and its text matches a consent phrase. A date or number box that cannot take the answer's text,
/// and a control of a kind the planner does not know, count as unanswered. A textarea, or a
/// one-line box under a long label, is answered only by an entry written for its whole label (an
/// `=` pattern), never by a keyword inside a paragraph. A single-choice group that offers Hispanic
/// or Latino is answered only by a candidate that names it, and a bare Yes or No never picks between several options that merely
/// start with it. A box the page already ticked that signs Gonza up for more than the privacy
/// consent (marketing, SMS, arbitration...) is reported as missing and listed in `to_untick`: the
/// filler never unticks. Every required field without an answer (including other file inputs) is
/// reported as missing, once per label.
pub(crate) fn plan_fields(fields: &[FieldInfo], bank: &AnswerBank, cv_pdf: &Path) -> FillPlan {
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
fn choice_group<'a>(fields: &'a [FieldInfo], field: &'a FieldInfo) -> Vec<&'a FieldInfo> {
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
pub(crate) fn best_option(options: &[String], wanted: &str) -> Option<usize> {
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
pub(crate) enum NoOption {
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
/// excludes "Hispanic or Latino"), and that answer is the user's alone: only a candidate naming it
/// picks anything, and never an option that says "Not". Any other candidate there is the user's
/// to answer.
pub(crate) fn pick_one(options: &[String], candidates: &[String]) -> Result<usize, NoOption> {
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
pub(crate) fn options_named(
    options: &[String],
    candidates: &[String],
    several: bool,
) -> Vec<usize> {
    if several {
        pick_many(options, candidates)
    } else {
        pick_one(options, candidates).ok().into_iter().collect()
    }
}

/// True when a failed fill only means "this list has no option for the answer" and the field is
/// optional: the field is skipped and left as it was. A required field keeps failing loudly, and
/// so does anything that is not a missing option (a page that did not answer).
fn is_skippable(err: &anyhow::Error, required: bool) -> bool {
    !required && err.downcast_ref::<NoOption>().is_some()
}

/// A drop-down list is settled once it shows options and stopped changing for a moment. An empty
/// list never is: a typeahead that answers late shows nothing at first, and that is not "no
/// options". The wait for an empty list ends only with `OPTIONS_MAX_MS`.
fn menu_settled(listed: usize, quiet: Duration) -> bool {
    listed > 0 && quiet >= Duration::from_millis(OPTIONS_SETTLE_MS)
}

/// Labels of this many words or more are questions with several parts.
const LONG_LABEL_WORDS: usize = 12;

/// True for a free-text control whose question is an essay: a textarea, or a one-line box under a
/// long label. A drop-down, a Yes/No toggle and a checkbox group can only take one of their own
/// options, so a long label cannot make them carry extra claims.
pub(crate) fn is_essay_box(tag: &str, kind: &str, label: &str) -> bool {
    if tag == "select" || kind == "combobox" {
        return false;
    }
    tag == "textarea" || label.split_whitespace().count() >= LONG_LABEL_WORDS
}

/// The bank entry for `label`. For an essay box only an entry written for that whole label (an
/// `=` pattern) answers: a keyword inside a paragraph ("salary") would put a one-line answer under
/// a question that asks for much more. Shared by the planner and the question review.
pub(crate) fn answer_entry<'a>(
    bank: &'a AnswerBank,
    label: &str,
    essay: bool,
) -> Option<&'a AnswerEntry> {
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
pub(crate) fn single_checkbox_label(question: &str, option: &str) -> String {
    let caption = question.trim_end_matches(|c: char| c == '*' || c.is_whitespace());
    format!("{caption}: {option}")
}

/// The index of the option that is exactly `wanted`, if the list already shows it.
fn exact_option(options: &[String], wanted: &str) -> Option<usize> {
    let wanted_words = words(wanted);
    options
        .iter()
        .position(|option| words(option) == wanted_words && !claims_not_hispanic(option))
}

/// What to type into a drop-down to find `candidate`: for a place ("Santiago, Chile") the first
/// part, because a city list finds nothing for the whole text with its comma.
fn search_text(candidate: &str) -> &str {
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
pub(crate) fn mentions_hispanic(text: &str) -> bool {
    words(text).split(' ').any(is_hispanic_word)
}

/// True for an option such as "White (Not Hispanic or Latino)": it answers the Hispanic or Latino
/// question as well, and Gonza keeps that one for himself, so it is never picked for him. A
/// negation (not, non, no, than, excluding, except, without) within three words of Hispanic or
/// Latino covers "not of Hispanic origin", "Non-Latino", "I am not of Latino descent", "Other than
/// Hispanic or Latino" and "White except Hispanic"; refusing too much only leaves the question for
/// him.
pub(crate) fn claims_not_hispanic(option: &str) -> bool {
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
pub(crate) fn blocker_in_text(text: &str) -> Option<&'static str> {
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

/// Address fragments of the thank-you pages the hosted boards redirect to.
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
pub(crate) struct PageSnapshot {
    pub url: String,
    pub text: String,
    pub errors: String,
}

/// True when the page gained a confirmation signal: a thank-you token that was not in the address
/// before, or a success phrase that was not in the text before. A phrase that was already on the
/// page (a sidebar saying "Thanks for applying!") proves nothing.
pub(crate) fn newly_confirmed(before: &PageSnapshot, after: &PageSnapshot) -> bool {
    fn appeared(signals: &[&str], was: &str, now: &str) -> bool {
        signals
            .iter()
            .any(|signal| now.contains(signal) && !was.contains(signal))
    }
    appeared(
        CONFIRMATION_URL_TOKENS,
        &before.url.to_lowercase(),
        &after.url.to_lowercase(),
    ) || appeared(
        SUCCESS_PHRASES,
        &before.text.to_lowercase(),
        &after.text.to_lowercase(),
    )
}

/// The validation message that appeared after the click; one that was already showing is not
/// a reaction to it.
pub(crate) fn newly_rejected(before: &PageSnapshot, after: &PageSnapshot) -> Option<String> {
    (!after.errors.is_empty() && after.errors != before.errors).then(|| after.errors.clone())
}

/// Default timeout for every page action. A missing field fails in 10 s, not in Playwright's 30 s.
const ACTION_TIMEOUT_MS: f64 = 10_000.0;
const SELECT_TIMEOUT_MS: f64 = 3_000.0;
/// How long a hosted board gets to render its form after `DOMContentLoaded`.
const FORM_WAIT_MS: u64 = 10_000;
/// How long the page gets to react after the submit click.
const CONFIRM_WAIT_MS: u64 = 8_000;
/// How long Gonza has to review and click in Assist mode.
const ASSIST_TIMEOUT_MS: u64 = 600_000;
const POLL_MS: u64 = 500;
/// A captcha wall: a visible reCAPTCHA or hCaptcha frame or widget. The invisible-reCAPTCHA badge
/// that Greenhouse shows on every form (`size=invisible` in the frame address) is not a wall; the
/// challenge frame that may pop up after a click has no `size=invisible`, so it still is.
const CAPTCHA_WALL_SELECTOR: &str = r#"iframe[src*="recaptcha"]:not([src*="size=invisible"]):visible, iframe[src*="hcaptcha"]:not([src*="size=invisible"]):visible, .g-recaptcha:not([data-size="invisible"]):visible, .h-captcha:not([data-size="invisible"]):visible"#;
const PASSWORD_SELECTOR: &str = r#"input[type="password"]:visible"#;
const SUBMIT_SELECTOR: &str = r#"[data-jb-submit="1"]"#;

/// Lists the form controls of the page, tagging each one with `data-jb="<idx>"` so the Rust side
/// can address it again. The script lives in `form/field_scan.js` (its header explains what each
/// field reports and how labels, groups and the required flag are worked out).
const FIELD_JS: &str = include_str!("form/field_scan.js");

/// The validation message the page is showing: the first visible alert text, or a generic note
/// when a field is only flagged with `aria-invalid`. Empty when there is none.
const REJECTION_JS: &str = r#"() => {
  const visible = (el) => el.getClientRects().length > 0;
  const alerts = Array.from(document.querySelectorAll('[role="alert"], .error-message, .field-error, .invalid-feedback'))
    .filter(visible)
    .map((el) => (el.innerText || '').replace(/\s+/g, ' ').trim())
    .filter(Boolean);
  if (alerts.length) return alerts[0];
  const invalid = Array.from(document.querySelectorAll('[aria-invalid="true"]')).filter(visible);
  return invalid.length ? 'el formulario marcó un error de validación' : '';
}"#;

/// Tags the submit button of the form (inside the first tagged field's form when there is one)
/// with `data-jb-submit="1"`; returns false when there is no visible button.
const TAG_SUBMIT_JS: &str = r#"() => {
  const re = /submit|apply|enviar|postular/i;
  document.querySelectorAll('[data-jb-submit]').forEach((el) => el.removeAttribute('data-jb-submit'));
  const first = document.querySelector('[data-jb]');
  const scope = (first && first.closest('form')) || document;
  const text = (b) => b.innerText || b.value || b.getAttribute('aria-label') || '';
  const candidates = Array.from(scope.querySelectorAll('button, input[type="submit"], input[type="button"], [role="button"]'))
    .filter((b) => b.offsetParent !== null && re.test(text(b)));
  const hit = candidates.find((b) => (b.getAttribute('type') || '').toLowerCase() === 'submit') || candidates[0];
  if (!hit) return false;
  hit.setAttribute('data-jb-submit', '1');
  return true;
}"#;

/// Reason when the page the browser is on now belongs to a platform the program never fills in
/// (the gate only sees the address from the job posting; a redirect link such as RemoteOK's
/// `/l/<id>` can end on LinkedIn). Asked after the navigation and again once the form appears,
/// because a script may send the browser on after `goto` has returned.
fn landing_wall(page: &Page, mode: SubmitMode) -> Option<&'static str> {
    wall_for(&page.url(), mode)
}

/// Why a page at `url` is off limits for `mode`, from its address alone. Manual-only and
/// listing-only pages stop every mode. A handoff host (Ashby flags the bot's Chromium whoever
/// clicks) stops everything but a dry run, which only looks: the pipeline hands those forms off by
/// the posting's own link, and this catches the ones a redirect sent there.
fn wall_for(url: &str, mode: SubmitMode) -> Option<&'static str> {
    if is_manual_only(url) {
        Some("manual_only_platform")
    } else if is_listing_only(url) {
        Some("listing_only")
    } else if mode != SubmitMode::DryRun && is_handoff(url) {
        Some("handoff_host")
    } else {
        None
    }
}

/// The hand-back for a posting whose link the browser must not open, or `None` when it may go on.
/// Asked before the browser moves; the reason starts with `unsafe_url:`.
fn refuse_unsafe_link(job: &Job, nav: NavPolicy) -> Option<SubmitResult> {
    navigation::check(&job.apply_url, nav)
        .err()
        .map(|refusal| needs_gonza(format!("unsafe_url: {refusal}"), None))
}

/// Reason when the page the browser is on now is not one it may fill in: a redirect or a script can
/// take the browser from the posting's link to a local or private host. Nothing has been typed yet
/// when this is asked. (The browser may already have issued the GET to that host; stopping the
/// request itself needs request interception, which belongs to the extraction of the browser crate.)
fn landing_refusal(page: &Page, nav: NavPolicy) -> Option<String> {
    navigation::check(&page.url(), nav).err().map(|refusal| {
        format!("unsafe_url: the browser landed on a page it may not fill ({refusal})")
    })
}

fn needs_gonza(reason: impl Into<String>, screenshot: Option<PathBuf>) -> SubmitResult {
    SubmitResult {
        status: SubmitStatus::NeedsGonza,
        reason: Some(reason.into()),
        screenshot,
    }
}

/// First line of an error: Playwright errors carry a long call log after it.
fn first_line(err: &impl std::fmt::Display) -> String {
    err.to_string()
        .lines()
        .next()
        .unwrap_or_default()
        .to_string()
}

async fn take_shot(page: &Page, path: &Path) -> anyhow::Result<()> {
    page.screenshot_to_file(path, ScreenshotOptions::builder().full_page(true).build())
        .await?;
    Ok(())
}

/// Looks for a wall that must not be bypassed: a visible captcha widget or challenge, a visible
/// password field or a page whose text announces a verification code, a login or a bot block.
async fn detect_blocker(page: &Page) -> anyhow::Result<Option<&'static str>> {
    if page.locator(CAPTCHA_WALL_SELECTOR).count().await? > 0 {
        return Ok(Some("captcha"));
    }
    if page.locator(PASSWORD_SELECTOR).count().await? > 0 {
        return Ok(Some("login_required"));
    }
    let text = page.locator("body").inner_text().await.unwrap_or_default();
    Ok(blocker_in_text(&text))
}

/// Performs one planned action on the control tagged `data-jb`. A choice (`Select`) or a text
/// answer goes to whatever the control turns out to be: a native `<select>` takes the first
/// option label that exists, a custom drop-down (an input with `role="combobox"`, Greenhouse's
/// react-select) is opened and an option is clicked, and a plain input is simply filled.
async fn apply_action(page: &Page, action: &FillAction) -> anyhow::Result<()> {
    let locator = page.locator(format!(r#"[data-jb="{}"]"#, action.idx));
    match &action.value {
        FillValue::File(path) => locator.set_input_files(path, None).await?,
        FillValue::Select(candidates) => choose(page, &locator, candidates).await?,
        FillValue::Text(text) => choose(page, &locator, std::slice::from_ref(text)).await?,
        FillValue::Check => select_option(&locator).await?,
    }
    Ok(())
}

/// True for the toggle buttons Ashby draws its Yes/No questions with.
const IS_BUTTON_JS: &str = r#"(el) => el.tagName === 'BUTTON'"#;
const PRESSED_JS: &str = r#"(el) => el.getAttribute('aria-pressed') === 'true'"#;
/// How long a toggle button gets to show it is pressed after the click (React redraws it a moment
/// later).
const PRESS_SETTLE_MS: u64 = 1_000;

/// Selects the option behind `locator`. A radio button or a checkbox is checked. A toggle button
/// is clicked unless it is already pressed (a second click may release it again) and must show as
/// pressed afterwards: a click the page ignored is a failure, not a filled field.
async fn select_option(locator: &playwright_rs::Locator) -> anyhow::Result<()> {
    let is_button: bool = locator.evaluate(IS_BUTTON_JS, None::<()>).await?;
    if !is_button {
        locator.check(None).await?;
        return Ok(());
    }
    let pressed = || async { locator.evaluate::<bool, ()>(PRESSED_JS, None).await };
    if pressed().await? {
        return Ok(());
    }
    locator.click(None).await?;
    let deadline = tokio::time::Instant::now() + Duration::from_millis(PRESS_SETTLE_MS);
    while !pressed().await? {
        if tokio::time::Instant::now() >= deadline {
            anyhow::bail!("el botón no quedó seleccionado");
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Ok(())
}

/// What kind of control `locator` is: `select`, `combobox` or `text`.
const CONTROL_KIND_JS: &str = r#"(el) => el.tagName === 'SELECT' ? 'select' : (el.getAttribute('role') === 'combobox' ? 'combobox' : 'text')"#;

/// The visible options of the open menu of a combobox: the listbox the input points at with
/// `aria-controls` when it does (the page may also hold hidden lists, such as a phone country
/// picker), any visible `role="option"` otherwise.
const VISIBLE_OPTIONS_JS: &str = r#"(controls) => {
  const root = controls ? document.getElementById(controls) : null;
  return Array.from((root || document).querySelectorAll('[role="option"]'))
    .filter((o) => o.getClientRects().length > 0)
    .map((o) => (o.innerText || o.textContent || '').replace(/\s+/g, ' ').trim());
}"#;

/// Tags the `index`-th visible option (same scope as `VISIBLE_OPTIONS_JS`) with `data-jb-opt`.
const TAG_OPTION_JS: &str = r#"(arg) => {
  document.querySelectorAll('[data-jb-opt]').forEach((el) => el.removeAttribute('data-jb-opt'));
  const root = arg.controls ? document.getElementById(arg.controls) : null;
  const options = Array.from((root || document).querySelectorAll('[role="option"]'))
    .filter((o) => o.getClientRects().length > 0);
  const hit = options[arg.index];
  if (!hit) return false;
  hit.setAttribute('data-jb-opt', '1');
  return true;
}"#;

/// How long a drop-down list may keep changing (a school list fetched while typing) before the
/// filler decides from what it shows, and the most it waits in all.
const OPTIONS_SETTLE_MS: u64 = 900;
const OPTIONS_MAX_MS: u64 = 4_000;

/// The option labels of a native `<select>`, in order (the label attribute, else the text: what
/// Playwright matches by label).
const SELECT_OPTIONS_JS: &str = r#"(el) => Array.from(el.options).map((o) => (o.label || o.text || '').replace(/\s+/g, ' ').trim())"#;

/// How long a combobox that was just opened gets to show its whole list before it is typed into.
const PEEK_MS: u64 = 400;

/// Gives `candidates` (the bank value, then its alternatives) to the control in `locator`.
async fn choose(
    page: &Page,
    locator: &playwright_rs::Locator,
    candidates: &[String],
) -> anyhow::Result<()> {
    let kind: String = locator.evaluate(CONTROL_KIND_JS, None::<()>).await?;
    match kind.as_str() {
        "select" => {
            // The same matching as every other list (case, punctuation and place rules, the bare
            // yes/no rule, the Hispanic rule), then the option it names is selected by position:
            // "Job board" finds the page's "Job Board".
            let labels: Vec<String> = locator.evaluate(SELECT_OPTIONS_JS, None::<()>).await?;
            let index = pick_one(&labels, candidates).map_err(anyhow::Error::new)?;
            let options = SelectOptions::builder().timeout(SELECT_TIMEOUT_MS).build();
            locator
                .select_option(SelectOption::Index(index), options)
                .await?;
            Ok(())
        }
        "combobox" => pick_option(page, locator, candidates).await,
        _ => match candidates.first() {
            Some(first) => Ok(locator.fill(first, None).await?),
            None => Ok(()),
        },
    }
}

/// The visible options of a combobox that has just been opened: waits up to `PEEK_MS` for the list
/// to show something (a typeahead shows nothing until typed into, and then this is empty).
async fn peek_options(page: &Page, controls: &str) -> anyhow::Result<Vec<String>> {
    let deadline = tokio::time::Instant::now() + Duration::from_millis(PEEK_MS);
    loop {
        let options: Vec<String> = page.evaluate(VISIBLE_OPTIONS_JS, Some(&controls)).await?;
        if !options.is_empty() || tokio::time::Instant::now() >= deadline {
            return Ok(options);
        }
        tokio::time::sleep(Duration::from_millis(POLL_MS / 8)).await;
    }
}

/// Opens a custom drop-down, types each candidate in turn to filter (or load) its list and clicks
/// the option that is the candidate (or starts with it as a whole word), never one that claims
/// "(Not Hispanic or Latino)". The whole list is read before anything is typed (typing filters it
/// down): a list that offers Hispanic or Latino is never answered, however it was filtered.
/// Errors with `NoOption` when no candidate matches any option or the list is reserved; the
/// control is then left closed and empty.
async fn pick_option(
    page: &Page,
    locator: &playwright_rs::Locator,
    candidates: &[String],
) -> anyhow::Result<()> {
    for (round, candidate) in candidates.iter().enumerate() {
        locator.click(None).await?;
        if round == 0 {
            let opened = locator
                .get_attribute("aria-controls")
                .await?
                .unwrap_or_default();
            let whole = peek_options(page, &opened).await?;
            if !whole.is_empty() && pick_one(&whole, candidates) == Err(NoOption::Reserved) {
                locator.press("Escape", None).await?;
                return Err(NoOption::Reserved.into());
            }
        }
        locator
            .press_sequentially(search_text(candidate), None)
            .await?;
        let controls = locator
            .get_attribute("aria-controls")
            .await?
            .unwrap_or_default();
        if let Some(index) = settled_match(page, &controls, candidate).await? {
            let arg = serde_json::json!({ "controls": controls, "index": index });
            let tagged: bool = page.evaluate(TAG_OPTION_JS, Some(&arg)).await?;
            if tagged {
                page.locator(r#"[data-jb-opt="1"]"#).click(None).await?;
                return Ok(());
            }
        }
        // Nothing for this candidate: clear what was typed and close the list before the next.
        locator.fill("", None).await?;
        locator.press("Escape", None).await?;
    }
    Err(NoOption::NoMatch.into())
}

/// Waits for the menu to settle (an exact match ends the wait at once) and returns the index of
/// the option for `wanted`: an exact one if there is one, otherwise the first that starts with it.
/// An empty list keeps being waited for up to `OPTIONS_MAX_MS` (see `menu_settled`).
async fn settled_match(page: &Page, controls: &str, wanted: &str) -> anyhow::Result<Option<usize>> {
    let start = tokio::time::Instant::now();
    let mut last: Vec<String> = Vec::new();
    let mut last_change = start;
    loop {
        let options: Vec<String> = page.evaluate(VISIBLE_OPTIONS_JS, Some(&controls)).await?;
        if options != last {
            last = options;
            last_change = tokio::time::Instant::now();
        }
        let exact = exact_option(&last, wanted);
        if exact.is_some() {
            return Ok(exact);
        }
        let now = tokio::time::Instant::now();
        if menu_settled(last.len(), now.duration_since(last_change))
            || now.duration_since(start) >= Duration::from_millis(OPTIONS_MAX_MS)
        {
            return Ok(best_option(&last, wanted));
        }
        tokio::time::sleep(Duration::from_millis(POLL_MS / 3)).await;
    }
}

/// What a skipped optional field says on the terminal.
fn skip_note(err: &anyhow::Error) -> &'static str {
    match err.downcast_ref::<NoOption>() {
        Some(NoOption::Reserved) => "la lista ofrece Hispanic o Latino: esa respuesta es tuya",
        _ => "ninguna opción de la lista coincide con la respuesta",
    }
}

fn failure_reason(action: &FillAction, err: &anyhow::Error) -> String {
    if err.downcast_ref::<NoOption>() == Some(&NoOption::Reserved) {
        return format!(
            "pregunta reservada para ti (la lista ofrece Hispanic o Latino): {}",
            action.label
        );
    }
    match &action.value {
        FillValue::Select(candidates) => {
            format!(
                "opción no encontrada: {} = {}",
                action.label,
                candidates.join(" | ")
            )
        }
        _ => format!("no se pudo completar {}: {}", action.label, first_line(err)),
    }
}

/// The `data-jb` indexes of the controls a run leaves for Gonza, in page order: every required
/// control the plan has no answer for (one option per radio or checkbox group, the first), every
/// pre-ticked box he has to untick and every control whose fill failed. When the plan misses
/// nothing, only the failed ones.
fn controls_left(fields: &[FieldInfo], plan: &FillPlan, failed: &[usize]) -> Vec<usize> {
    let answered = |idx: usize| {
        plan.actions.iter().any(|action| action.idx == idx) || plan.kept.contains(&idx)
    };
    let mut left = failed.to_vec();
    left.extend(&plan.to_untick);
    if !plan.missing.is_empty() {
        for field in fields {
            let group = choice_group(fields, field);
            let unanswered = if group.len() >= 2 || field.kind == "radio" {
                group.first().is_some_and(|first| first.idx == field.idx)
                    && group.iter().any(|member| member.required)
                    && !group.iter().any(|member| answered(member.idx))
            } else {
                field.required && !answered(field.idx)
            };
            if unanswered {
                left.push(field.idx);
            }
        }
    }
    left.sort_unstable();
    left.dedup();
    left
}

/// What `FLAG_JS` marks: for each control left for Gonza, the `data-jb` numbers of the whole
/// question it belongs to, itself first (it is the one outlined). The mark goes away as soon as any
/// of them is chosen, so it must watch every button of a radio group, not only the outlined one.
fn flag_groups(fields: &[FieldInfo], left: &[usize]) -> Vec<Vec<usize>> {
    left.iter()
        .map(|&idx| {
            let mut group = vec![idx];
            if let Some(field) = fields.iter().find(|field| field.idx == idx) {
                group.extend(
                    choice_group(fields, field)
                        .iter()
                        .map(|member| member.idx)
                        .filter(|&member| member != idx),
                );
            }
            group
        })
        .collect()
}

/// Why a form that was filled as far as it could be still needs Gonza: the required answers the
/// bank does not have, then the fields that would not take their value. `None` when nothing is left.
fn leftover_reason(missing: &[String], failures: &[String]) -> Option<String> {
    let missing =
        (!missing.is_empty()).then(|| format!("faltan respuestas: {}", missing.join("; ")));
    let failures = (!failures.is_empty()).then(|| failures.join("; "));
    match (missing, failures) {
        (Some(missing), Some(failures)) => Some(format!("{missing} — {failures}")),
        (missing, failures) => missing.or(failures),
    }
}

/// `reason`, plus what was still pending for Gonza when the program stopped waiting for him.
fn with_pending(reason: String, leftover: Option<&str>) -> String {
    match leftover {
        Some(left) => format!("{reason}. Quedaba pendiente: {left}"),
        None => reason,
    }
}

/// The reason recorded when Gonza never confirmed an assisted form.
fn assist_timeout_reason(minutes: u64, leftover: Option<&str>) -> String {
    with_pending(
        format!("assist_timeout: sin confirmación en {minutes} minutos"),
        leftover,
    )
}

/// The reason recorded when Gonza closed the tab of an assisted form: the way to skip a form
/// without answering it. If he had sent it first, `confirm` records that.
fn assist_closed_reason(leftover: Option<&str>) -> String {
    with_pending(
        "assist_closed: cerraste la pestaña sin que la página confirmara; si sí la enviaste, `jobbot confirm KEY` la marca"
            .to_string(),
        leftover,
    )
}

/// Page text for the terminal, without control characters (a page cannot print escape codes).
fn plain(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).collect()
}

/// Marks the controls Gonza has to finish: a red outline, a `data-jb-todo` attribute and the first
/// one scrolled into view. The script gets only `data-jb` numbers, never text from the page, and
/// writes no markup. Each entry is one question: the control to outline first, then the other
/// options of the same question (the other buttons of a radio group). The mark goes away once any
/// of them has a value; a toggle button is watched through its `aria-pressed`, because the page
/// updates it after the click event has passed. A box listed in `untick` is flagged the other way
/// round: the page ticked it, and the mark goes away once he unticks that box.
const FLAG_JS: &str = r#"({ groups, untick }) => {
  const filled = (el) => {
    if (el.tagName === 'BUTTON') return el.getAttribute('aria-pressed') === 'true';
    const type = (el.getAttribute('type') || '').toLowerCase();
    if (type === 'checkbox' || type === 'radio') return el.checked;
    if (type === 'file') return el.files.length > 0;
    return el.value.trim() !== '';
  };
  let first = null;
  groups.forEach((indices) => {
    const members = indices
      .map((idx) => document.querySelector('[data-jb="' + idx + '"]'))
      .filter(Boolean);
    const el = members[0];
    if (!el) return;
    // A hidden control (a styled file picker) shows no outline: mark what Gonza can see.
    const box = el.getClientRects().length > 0 ? el : (el.closest('label') || el.parentElement || el);
    box.style.outline = '3px solid #d93025';
    box.style.outlineOffset = '2px';
    box.setAttribute('data-jb-todo', '1');
    first = first || box;
    const unticking = untick.includes(indices[0]);
    const clear = () => {
      if (unticking ? el.checked : !members.some(filled)) return;
      box.style.outline = '';
      box.style.outlineOffset = '';
      box.removeAttribute('data-jb-todo');
    };
    members.forEach((member) => {
      ['input', 'change'].forEach((event) => member.addEventListener(event, clear));
      if (member.tagName === 'BUTTON') {
        new MutationObserver(clear).observe(member, { attributes: true, attributeFilter: ['aria-pressed'] });
      }
    });
  });
  if (first) first.scrollIntoView({ block: 'center' });
  return first !== null;
}"#;

/// Best effort: a page that rejects the script, or has already re-rendered the form, just goes
/// without marks. `groups` as `flag_groups` builds them; `untick` are the pre-ticked boxes among
/// them (`FillPlan::to_untick`).
async fn flag_for_gonza(page: &Page, groups: &[Vec<usize>], untick: &[usize]) {
    if groups.is_empty() {
        return;
    }
    let arg = serde_json::json!({ "groups": groups, "untick": untick });
    let _: Result<bool, _> = page.evaluate(FLAG_JS, Some(&arg)).await;
}

/// Scans the form, waiting up to `FORM_WAIT_MS` for controls to appear: Greenhouse's and Ashby's
/// hosted boards render the form from JavaScript after `DOMContentLoaded`, so an immediate scan
/// would report `no_form_found` on a page that is still loading.
async fn scan_fields(page: &Page) -> anyhow::Result<Vec<FieldInfo>> {
    let deadline = tokio::time::Instant::now() + Duration::from_millis(FORM_WAIT_MS);
    loop {
        let fields: Vec<FieldInfo> = page
            .evaluate(FIELD_JS, None::<&()>)
            .await
            .context("scanning the form fields")?;
        if !fields.is_empty() || tokio::time::Instant::now() >= deadline {
            return Ok(fields);
        }
        tokio::time::sleep(Duration::from_millis(POLL_MS)).await;
    }
}

async fn snapshot(page: &Page) -> PageSnapshot {
    // Each read may fail while the page navigates; an empty value then means "no signal yet".
    let errors: String = page
        .evaluate(REJECTION_JS, None::<&()>)
        .await
        .unwrap_or_default();
    PageSnapshot {
        url: page
            .evaluate_value("location.href")
            .await
            .unwrap_or_default(),
        text: page.locator("body").inner_text().await.unwrap_or_default(),
        errors,
    }
}

/// What the page did after the submit click (or, in Assist mode, after Gonza's click).
#[derive(Debug, PartialEq)]
enum Confirmation {
    Confirmed,
    Rejected(String),
    Blocked(&'static str),
    Silent,
    /// The tab was closed while waiting (in Assist mode, Gonza skipping the form).
    Closed,
}

/// Polls the page for up to `wait_ms`: a new thank-you address or phrase confirms; in `Live` mode
/// a new validation error or a wall ends the wait early. `Silent` means nothing happened in time.
/// In `Assist` mode only the confirmation counts, because Gonza may be fixing fields or solving a
/// captcha in the meantime.
async fn await_confirmation(
    page: &Page,
    before: &PageSnapshot,
    mode: SubmitMode,
    wait_ms: u64,
) -> Confirmation {
    let deadline = tokio::time::Instant::now() + Duration::from_millis(wait_ms);
    loop {
        if page.is_closed() {
            return Confirmation::Closed;
        }
        let after = snapshot(page).await;
        if newly_confirmed(before, &after) {
            return Confirmation::Confirmed;
        }
        if mode == SubmitMode::Live {
            if let Some(message) = newly_rejected(before, &after) {
                return Confirmation::Rejected(message);
            }
            if let Ok(Some(reason)) = detect_blocker(page).await {
                return Confirmation::Blocked(reason);
            }
        }
        if tokio::time::Instant::now() >= deadline {
            return Confirmation::Silent;
        }
        tokio::time::sleep(Duration::from_millis(POLL_MS)).await;
    }
}

/// Opens the application page in `page`, fills what the bank can answer and acts according to
/// `mode`:
/// * `DryRun` fills and stops after the screenshot. A required question the bank cannot answer is
///   left untouched and flagged, the rest of the form is still filled, and the result is
///   `NeedsGonza` with the reasons: the screenshot shows what is done and what is left.
/// * `Live` clicks submit and waits for the page to confirm, but never while a required answer
///   is missing or a field would not take its value: it stops before typing anything (a missing
///   answer) or before the click (a failed field).
/// * `Assist` fills what it can, flags what it cannot (a required question, a field that failed)
///   and leaves both the answers and the click to Gonza in the visible browser, then waits for
///   the same confirmation as `Live`.
///
/// Problems with one page (captcha, login, a missing answer, an HTTP error, a click the page did
/// not confirm) come back as `NeedsGonza`; only failures that make the whole run unsafe return `Err`.
pub(crate) async fn fill_application(
    page: &Page,
    job: &Job,
    cv_pdf: &Path,
    bank: &AnswerBank,
    mode: SubmitMode,
    shots_dir: &Path,
    nav: NavPolicy,
) -> anyhow::Result<SubmitResult> {
    if let Some(reason) = wall_for(&job.apply_url, mode)
        && reason != "listing_only"
    {
        // Before any navigation. A listing-only link may still be followed: it can lead to a form.
        return Ok(needs_gonza(reason, None));
    }
    if let Some(refused) = refuse_unsafe_link(job, nav) {
        // Before any navigation: a link the browser must not follow goes back to Gonza.
        return Ok(refused);
    }
    tokio::fs::create_dir_all(shots_dir)
        .await
        .with_context(|| format!("creating {}", shots_dir.display()))?;
    let shot = shots_dir.join(format!("{}.png", safe_key(&job.key())));
    page.set_default_timeout(ACTION_TIMEOUT_MS).await;

    let options = GotoOptions::new().wait_until(WaitUntil::DomContentLoaded);
    let response = match page.goto(&job.apply_url, options).await {
        Ok(response) => response,
        Err(err) => {
            return Ok(needs_gonza(
                format!("page_error: {}", first_line(&err)),
                None,
            ));
        }
    };
    if let Some(response) = response
        && response.status() >= 400
    {
        take_shot(page, &shot).await?;
        return Ok(needs_gonza(
            format!("page_error: HTTP {}", response.status()),
            Some(shot),
        ));
    }
    if let Some(reason) = landing_refusal(page, nav) {
        take_shot(page, &shot).await?;
        return Ok(needs_gonza(reason, Some(shot)));
    }
    if let Some(reason) = landing_wall(page, mode) {
        take_shot(page, &shot).await?;
        return Ok(needs_gonza(reason, Some(shot)));
    }
    if let Some(reason) = detect_blocker(page).await? {
        take_shot(page, &shot).await?;
        return Ok(needs_gonza(reason, Some(shot)));
    }

    let fields = scan_fields(page).await?;
    if let Some(reason) = landing_refusal(page, nav) {
        take_shot(page, &shot).await?;
        return Ok(needs_gonza(reason, Some(shot)));
    }
    if let Some(reason) = landing_wall(page, mode) {
        take_shot(page, &shot).await?;
        return Ok(needs_gonza(reason, Some(shot)));
    }
    if fields.is_empty() {
        take_shot(page, &shot).await?;
        return Ok(needs_gonza("no_form_found", Some(shot)));
    }
    let plan = plan_fields(&fields, bank, cv_pdf);
    // Live is the only mode that clicks on its own, and it never goes on with a required answer
    // missing: it stops before typing a word into a form it will not send.
    if mode == SubmitMode::Live
        && let Some(reason) = leftover_reason(&plan.missing, &[])
    {
        take_shot(page, &shot).await?;
        return Ok(needs_gonza(reason, Some(shot)));
    }
    // A form with nowhere to attach the CV is not an application Gonza wants sent, or approved
    // from a dry run that looked fine: Live would click a button on a page where nothing was
    // filled (a header search box, a newsletter field). In Assist mode Gonza sees the page himself.
    let takes_the_cv = plan
        .actions
        .iter()
        .any(|action| matches!(action.value, FillValue::File(_)));
    if mode != SubmitMode::Assist && !takes_the_cv {
        take_shot(page, &shot).await?;
        return Ok(needs_gonza(
            "sin campo de CV: el formulario no tiene dónde adjuntar el CV",
            Some(shot),
        ));
    }
    // Fill everything the plan can answer. One field that fails (an option the form does not
    // have, a control that refuses the value) must not hide the others, so every action is tried
    // and the failures are reported together. Required questions with no answer stay untouched.
    // An OPTIONAL field whose list has no option for the answer is not a failure: it is skipped and
    // stays as the page had it, like an optional radio group with no matching option.
    let mut failures = Vec::new();
    let mut failed = Vec::new();
    for action in &plan.actions {
        if let Err(err) = apply_action(page, action).await {
            let required = fields
                .iter()
                .any(|field| field.idx == action.idx && field.required);
            if is_skippable(&err, required) {
                eprintln!(
                    "[omitido] {}: {}; es opcional y queda como estaba",
                    plain(&action.label),
                    skip_note(&err)
                );
                continue;
            }
            failures.push(failure_reason(action, &err));
            failed.push(action.idx);
        }
    }
    // What is left for Gonza is marked on the page, so it shows in the screenshot and, in Assist
    // mode, in the browser he is looking at.
    let left = controls_left(&fields, &plan, &failed);
    flag_for_gonza(page, &flag_groups(&fields, &left), &plan.to_untick).await;
    let leftover = leftover_reason(&plan.missing, &failures);

    take_shot(page, &shot).await?;
    // A dry run or a live run with something left over stops here, before any click. In Assist
    // mode the leftover is Gonza's to finish in the browser.
    if mode != SubmitMode::Assist
        && let Some(reason) = &leftover
    {
        return Ok(needs_gonza(reason.clone(), Some(shot)));
    }
    if mode == SubmitMode::DryRun {
        return Ok(SubmitResult {
            status: SubmitStatus::DryRun,
            reason: None,
            screenshot: Some(shot),
        });
    }
    // Nothing that clicks, or hands the page to Gonza to click, starts on a page the address now
    // closes: a script may have sent the browser on while the form was being filled.
    if let Some(reason) = landing_wall(page, mode) {
        return Ok(needs_gonza(reason, Some(shot)));
    }

    let before = snapshot(page).await;
    let outcome = if mode == SubmitMode::Assist {
        let waiting = format!(
            "Espero la confirmación hasta {} minutos.",
            ASSIST_TIMEOUT_MS / 60_000
        );
        match &leftover {
            Some(left) => eprintln!(
                "[assist] {} — {}: formulario llenado hasta donde pude. Te quedó por completar (marcado en rojo en la página): {}. \
                 Complétalo, resuelve el captcha si aparece y envía tú. {waiting}",
                job.company,
                job.title,
                plain(left)
            ),
            None => eprintln!(
                "[assist] {} — {}: formulario listo en el navegador. Revisa, resuelve el captcha si aparece y envía tú. {waiting}",
                job.company, job.title
            ),
        }
        await_confirmation(page, &before, mode, ASSIST_TIMEOUT_MS).await
    } else {
        // A widget injected while the form was being filled still stops a real submission.
        if let Some(reason) = detect_blocker(page).await? {
            return Ok(needs_gonza(reason, Some(shot)));
        }
        let has_button: bool = page.evaluate(TAG_SUBMIT_JS, None::<&()>).await?;
        if !has_button {
            return Ok(needs_gonza("no_submit_button", Some(shot)));
        }
        if let Err(err) = page.locator(SUBMIT_SELECTOR).click(None).await {
            return Ok(needs_gonza(
                format!("no se pudo enviar: {}", first_line(&err)),
                Some(shot),
            ));
        }
        // From here the form may have been sent: nothing below may turn the result into an error,
        // or the pipeline would not record it and could send the form again on the next run.
        await_confirmation(page, &before, mode, CONFIRM_WAIT_MS).await
    };

    let after = shots_dir.join(format!("{}-after.png", safe_key(&job.key())));
    let screenshot = take_shot(page, &after)
        .await
        .ok()
        .map(|()| after)
        .or(Some(shot));
    Ok(match outcome {
        Confirmation::Confirmed => SubmitResult {
            status: SubmitStatus::Submitted,
            reason: None,
            screenshot,
        },
        // The click happened but the page did not confirm: Gonza looks at the "-after" screenshot
        // and either fixes and resends (`jobbot resolve`) or records it as sent (`jobbot confirm`).
        Confirmation::Rejected(message) => {
            needs_gonza(format!("unconfirmed_submit: {message}"), screenshot)
        }
        Confirmation::Blocked(reason) => {
            needs_gonza(format!("unconfirmed_submit: {reason}"), screenshot)
        }
        Confirmation::Silent if mode == SubmitMode::Assist => needs_gonza(
            assist_timeout_reason(ASSIST_TIMEOUT_MS / 60_000, leftover.as_deref()),
            screenshot,
        ),
        Confirmation::Closed if mode == SubmitMode::Assist => {
            needs_gonza(assist_closed_reason(leftover.as_deref()), screenshot)
        }
        // A tab that closes after the program's own click is as silent as a page that said nothing.
        Confirmation::Silent | Confirmation::Closed => {
            needs_gonza("unconfirmed_submit: sin confirmación visible", screenshot)
        }
    })
}

/// Fills application forms in a persistent Chromium profile (cookies and logins survive runs).
pub struct BrowserFiller {
    // Field order matters: the context is dropped before the driver that owns it.
    context: BrowserContext,
    _playwright: Playwright,
    /// Which addresses the browser may open; strict outside the Chromium tests.
    nav: NavPolicy,
}

impl BrowserFiller {
    /// Starts the driver and Chromium with `profile_dir` as the user data directory.
    /// `headed` shows the browser window (the CLI's `--headed`).
    pub async fn launch(profile_dir: &Path, headed: bool) -> anyhow::Result<BrowserFiller> {
        tokio::fs::create_dir_all(profile_dir)
            .await
            .with_context(|| format!("creating {}", profile_dir.display()))?;
        let playwright = Playwright::launch()
            .await
            .context("starting the Playwright driver")?;
        let context = playwright
            .chromium()
            .launch_persistent_context_with_options(
                profile_dir.to_string_lossy().into_owned(),
                crate::launch::persistent_options(headed),
            )
            .await
            .context("launching Chromium with the persistent profile")?;
        Ok(BrowserFiller {
            context,
            _playwright: playwright,
            nav: NavPolicy::STRICT,
        })
    }

    /// Closes the browser; call it once the run is over.
    pub async fn close(self) -> anyhow::Result<()> {
        self.context.close().await?;
        Ok(())
    }
}

#[async_trait::async_trait]
impl FormFiller for BrowserFiller {
    async fn submit(
        &self,
        job: &Job,
        cv_pdf: &Path,
        bank: &AnswerBank,
        mode: SubmitMode,
        shots_dir: &Path,
    ) -> anyhow::Result<SubmitResult> {
        let page = self.context.new_page().await?;
        let result = fill_application(&page, job, cv_pdf, bank, mode, shots_dir, self.nav).await;
        let _ = page.close().await;
        result
    }
}

#[cfg(test)]
mod scan_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::answers::AnswerEntry;

    fn field(idx: usize, tag: &str, kind: &str, label: &str, required: bool) -> FieldInfo {
        FieldInfo {
            idx,
            tag: tag.into(),
            kind: kind.into(),
            label: label.into(),
            required,
            option: String::new(),
            name: String::new(),
            checked: false,
        }
    }

    /// One radio button of the group `name`: `label` is the question, `option` its own text.
    fn radio(idx: usize, name: &str, label: &str, option: &str, required: bool) -> FieldInfo {
        FieldInfo {
            option: option.into(),
            name: name.into(),
            ..field(idx, "input", "radio", label, required)
        }
    }

    /// One checkbox of the group `name`: `label` is the question, `option` its own text.
    fn checkbox(idx: usize, name: &str, label: &str, option: &str, required: bool) -> FieldInfo {
        FieldInfo {
            option: option.into(),
            name: name.into(),
            ..field(idx, "input", "checkbox", label, required)
        }
    }

    /// A group of checkboxes that share the question `label` and a group name made from `first`,
    /// one per option text, numbered from `first`.
    fn checkbox_group(
        first: usize,
        label: &str,
        options: &[&str],
        required: bool,
    ) -> Vec<FieldInfo> {
        options
            .iter()
            .enumerate()
            .map(|(i, option)| {
                checkbox(
                    first + i,
                    &format!("group-{first}"),
                    label,
                    option,
                    required,
                )
            })
            .collect()
    }

    fn bank() -> AnswerBank {
        AnswerBank {
            entries: vec![
                AnswerEntry::new(&["first name"], Some("Jane")),
                AnswerEntry::new(&["email"], None),
                AnswerEntry::new(&["country"], Some("Chile")),
                AnswerEntry::new(&["phone"], Some("   ")),
                AnswerEntry::new(&["how did you hear"], Some("Company careers page"))
                    .with_alternatives(&["Company website", "Other"]),
                AnswerEntry::new(&["visa sponsorship", "require sponsorship"], Some("No")),
            ],
            consent_phrases: vec![],
        }
    }

    fn snap(url: &str, text: &str) -> PageSnapshot {
        PageSnapshot {
            url: url.into(),
            text: text.into(),
            errors: String::new(),
        }
    }

    #[test]
    fn plan_fills_text_and_select_fields_from_the_bank() {
        let fields = [
            field(0, "input", "text", "First name *", true),
            field(1, "select", "", "Country", false),
        ];
        let plan = plan_fields(&fields, &bank(), Path::new("cv.pdf"));
        assert!(plan.missing.is_empty());
        assert_eq!(
            plan.actions,
            vec![
                FillAction {
                    idx: 0,
                    label: "First name *".into(),
                    value: FillValue::Text("Jane".into())
                },
                FillAction {
                    idx: 1,
                    label: "Country".into(),
                    value: FillValue::Select(vec!["Chile".into()])
                },
            ]
        );
    }

    #[test]
    fn select_candidates_include_alternatives() {
        let fields = [field(0, "select", "", "How did you hear about us?", true)];
        let plan = plan_fields(&fields, &bank(), Path::new("cv.pdf"));
        assert_eq!(
            plan.actions[0].value,
            FillValue::Select(vec![
                "Company careers page".into(),
                "Company website".into(),
                "Other".into()
            ])
        );
        // A textarea is an essay box: a keyword pattern ("how did you hear") does not answer it,
        // only an entry written for its whole label does, and then it gets only the value:
        // alternatives are option texts, not free text. (A one-line input may be a custom
        // drop-down and keeps them: see `a_text_input_with_alternatives_may_be_a_custom_dropdown`.)
        let fields = [field(0, "textarea", "", "How did you hear about us?", true)];
        let plan = plan_fields(&fields, &bank(), Path::new("cv.pdf"));
        assert!(plan.actions.is_empty());
        assert_eq!(plan.missing, vec!["How did you hear about us?"]);
        let exact = AnswerBank {
            entries: vec![
                AnswerEntry::new(
                    &["=how did you hear about us"],
                    Some("Company careers page"),
                )
                .with_alternatives(&["Company website", "Other"]),
            ],
            consent_phrases: vec![],
        };
        let plan = plan_fields(&fields, &exact, Path::new("cv.pdf"));
        assert_eq!(
            plan.actions[0].value,
            FillValue::Text("Company careers page".into())
        );
    }

    #[test]
    fn consent_checkbox_is_ticked_only_when_allowed() {
        let fields = [
            field(
                0,
                "input",
                "checkbox",
                "I have read and accept the Privacy Policy *",
                true,
            ),
            field(1, "input", "checkbox", "Send me job alerts", false),
        ];
        // The shipped default: nothing is allowed, so the required checkbox is a missing answer.
        let plan = plan_fields(&fields, &bank(), Path::new("cv.pdf"));
        assert_eq!(
            plan.missing,
            vec!["I have read and accept the Privacy Policy *"]
        );
        assert!(plan.actions.is_empty());
        // With Gonza's phrase the required checkbox is ticked; the optional one is left alone.
        let allowed = AnswerBank {
            consent_phrases: vec!["privacy policy".into()],
            ..bank()
        };
        let plan = plan_fields(&fields, &allowed, Path::new("cv.pdf"));
        assert!(plan.missing.is_empty());
        assert_eq!(
            plan.actions,
            vec![FillAction {
                idx: 0,
                label: "I have read and accept the Privacy Policy *".into(),
                value: FillValue::Check,
            }]
        );
    }

    #[test]
    fn radio_group_is_answered_from_the_bank() {
        let fields = [
            radio(
                0,
                "sponsor",
                "Do you require visa sponsorship? *",
                "Yes",
                true,
            ),
            radio(
                1,
                "sponsor",
                "Do you require visa sponsorship? *",
                "No",
                true,
            ),
            radio(2, "relocate", "Willing to relocate?", "Yes", false),
            radio(3, "relocate", "Willing to relocate?", "No", false),
            radio(
                4,
                "sponsor2",
                "Will you now or in the future require sponsorship?",
                "Yes, I will need sponsorship",
                true,
            ),
            radio(
                5,
                "sponsor2",
                "Will you now or in the future require sponsorship?",
                "No, I will not need sponsorship",
                true,
            ),
        ];
        let plan = plan_fields(&fields, &bank(), Path::new("cv.pdf"));
        assert!(plan.missing.is_empty());
        // One button per answered group ("No" also matches "No, ..."); the optional group with no
        // bank answer is skipped.
        assert_eq!(
            plan.actions,
            vec![
                FillAction {
                    idx: 1,
                    label: "Do you require visa sponsorship? *".into(),
                    value: FillValue::Check
                },
                FillAction {
                    idx: 5,
                    label: "Will you now or in the future require sponsorship?".into(),
                    value: FillValue::Check,
                },
            ]
        );
    }

    #[test]
    fn an_option_that_already_shows_the_answer_is_not_clicked_again() {
        // A pressed toggle button released by a second click would undo the answer, and a box the
        // page already ticked needs nothing: the question is answered either way.
        let option = |idx: usize, text: &str, checked: bool| FieldInfo {
            checked,
            ..radio(
                idx,
                "sponsor",
                "Do you require visa sponsorship? *",
                text,
                true,
            )
        };
        let unanswered = field(2, "input", "text", "Favorite color *", true);
        let fields = [option(0, "Yes", false), option(1, "No", true), unanswered];
        let plan = plan_fields(&fields, &bank(), Path::new("cv.pdf"));
        assert!(plan.actions.is_empty(), "{:?}", plan.actions);
        assert_eq!(plan.missing, vec!["Favorite color *"]);
        // Only the open question is left for Gonza, not the group that already says "No".
        assert_eq!(controls_left(&fields, &plan, &[]), vec![2]);
        // Another option selected: the bank's answer is clicked.
        let fields = [option(0, "Yes", true), option(1, "No", false)];
        let plan = plan_fields(&fields, &bank(), Path::new("cv.pdf"));
        assert_eq!(plan.actions.len(), 1);
        assert_eq!(plan.actions[0].idx, 1);
    }

    #[test]
    fn confirmation_requires_a_new_signal() {
        let before = snap(
            "https://x.test/applied-ai/apply",
            "Apply for Senior AI Engineer. Submit application.",
        );
        // A thank-you phrase that was not on the page before the click confirms.
        assert!(newly_confirmed(
            &before,
            &snap(
                &before.url,
                "Thank you for applying! We received your application."
            )
        ));
        // So does a navigation to a thank-you page, whatever its text says.
        assert!(newly_confirmed(
            &before,
            &snap("https://x.test/applied-ai/apply/confirmation", "Done")
        ));
        // The same page, a navigation without a confirmation token, or a token that was already in
        // the address ("applied" above) are not confirmations.
        assert!(!newly_confirmed(&before, &before));
        assert!(!newly_confirmed(
            &before,
            &snap("https://x.test/applied-ai/apply?step=2", &before.text)
        ));
        assert!(!newly_confirmed(
            &before,
            &snap("https://x.test/applied-ai/apply?error=1", &before.text)
        ));
        // A phrase that was there before the click (a sidebar "Thanks for applying!") is not news.
        let sidebar = snap(
            &before.url,
            "Thanks for applying to Acme! Fill in the form below.",
        );
        assert!(!newly_confirmed(&sidebar, &sidebar));
    }

    #[test]
    fn rejection_counts_only_when_it_is_new() {
        let before = snap("https://x.test/apply", "form");
        let mut after = before.clone();
        after.errors = "Phone number is invalid.".into();
        assert_eq!(
            newly_rejected(&before, &after).as_deref(),
            Some("Phone number is invalid.")
        );
        assert_eq!(newly_rejected(&before, &before), None);
        // An error that was already showing before the click is not a reaction to it.
        assert_eq!(newly_rejected(&after, &after), None);
    }

    #[test]
    fn plan_uploads_the_cv_only_to_resume_inputs() {
        let fields = [
            field(0, "input", "file", "Resume/CV *", true),
            field(1, "input", "file", "Cover letter", false),
            field(2, "input", "file", "Currículum", false),
        ];
        let plan = plan_fields(&fields, &bank(), Path::new("cv.pdf"));
        assert!(plan.missing.is_empty());
        let uploaded: Vec<usize> = plan.actions.iter().map(|a| a.idx).collect();
        assert_eq!(uploaded, vec![0, 2]);
        assert_eq!(
            plan.actions[0].value,
            FillValue::File(PathBuf::from("cv.pdf"))
        );
    }

    #[test]
    fn required_file_input_that_is_not_a_resume_is_missing() {
        let fields = [field(0, "input", "file", "Portfolio PDF", true)];
        let plan = plan_fields(&fields, &bank(), Path::new("cv.pdf"));
        assert_eq!(plan.missing, vec!["Portfolio PDF"]);
        assert!(plan.actions.is_empty());
    }

    #[test]
    fn required_field_without_a_value_is_missing_and_optional_one_is_skipped() {
        let fields = [
            field(0, "input", "email", "Email *", true),
            field(1, "input", "text", "Favorite color", true),
            field(2, "input", "text", "Nickname", false),
            field(3, "input", "text", "", true),
        ];
        let plan = plan_fields(&fields, &bank(), Path::new("cv.pdf"));
        assert_eq!(
            plan.missing,
            vec!["Email *", "Favorite color", "campo sin etiqueta"]
        );
        assert!(plan.actions.is_empty());
    }

    #[test]
    fn blank_bank_value_counts_as_missing() {
        let fields = [field(0, "input", "tel", "Phone", true)];
        let plan = plan_fields(&fields, &bank(), Path::new("cv.pdf"));
        assert_eq!(plan.missing, vec!["Phone"]);
    }

    fn pay_and_start_bank() -> AnswerBank {
        AnswerBank {
            entries: vec![
                AnswerEntry::new(&["start"], Some("Immediately")),
                AnswerEntry::new(&["salary"], Some("USD 6,000 per month")),
                AnswerEntry::new(&["annual"], Some("72000")),
            ],
            consent_phrases: vec![],
        }
    }

    #[test]
    fn a_date_input_never_gets_the_word_immediately() {
        // A date input rejects free text: typing "Immediately" would leave it empty or throw
        // halfway through the form. The question is Gonza's, with a real date.
        let fields = [
            field(0, "input", "date", "Earliest start date *", true),
            field(1, "input", "month", "Start", false),
            field(2, "input", "datetime-local", "Start", true),
            field(3, "input", "text", "Start", true),
        ];
        let plan = plan_fields(&fields, &pay_and_start_bank(), Path::new("cv.pdf"));
        assert_eq!(plan.missing, vec!["Earliest start date *", "Start"]);
        assert_eq!(
            plan.actions,
            vec![FillAction {
                idx: 3,
                label: "Start".into(),
                value: FillValue::Text("Immediately".into())
            }]
        );
    }

    #[test]
    fn a_number_input_only_gets_a_number() {
        let fields = [
            field(0, "input", "number", "Salary", true),
            field(1, "input", "number", "Annual", true),
            field(2, "input", "number", "Salary", false),
        ];
        let plan = plan_fields(&fields, &pay_and_start_bank(), Path::new("cv.pdf"));
        // The monthly text cannot go in a numeric box (required: Gonza answers; optional: skipped);
        // the annual figure can.
        assert_eq!(plan.missing, vec!["Salary"]);
        assert_eq!(
            plan.actions,
            vec![FillAction {
                idx: 1,
                label: "Annual".into(),
                value: FillValue::Text("72000".into())
            }]
        );
    }

    #[test]
    fn no_date_or_time_box_ever_gets_free_text() {
        let fields = |required: bool| {
            ["date", "month", "week", "time", "datetime-local"]
                .iter()
                .enumerate()
                .map(|(idx, kind)| field(idx, "input", kind, "Start", required))
                .collect::<Vec<_>>()
        };
        // Required: the question is Gonza's. Optional: left alone. Never "Immediately".
        let plan = plan_fields(&fields(true), &pay_and_start_bank(), Path::new("cv.pdf"));
        assert!(plan.actions.is_empty(), "{:?}", plan.actions);
        assert_eq!(plan.missing, vec!["Start"]);
        let plan = plan_fields(&fields(false), &pay_and_start_bank(), Path::new("cv.pdf"));
        assert!(plan.actions.is_empty(), "{:?}", plan.actions);
        assert!(plan.missing.is_empty());
    }

    #[test]
    fn a_date_picker_stays_unanswered_whatever_text_the_scan_gives_it() {
        // Ashby's start date is a text input drawn as a date picker. Its label used to be the
        // placeholder ("Pick date..."), now it is the question; the shipped bank answers that
        // question with "Immediately", which a date picker cannot take, so Gonza picks the day.
        let path = crate::paths::Paths::new(env!("CARGO_MANIFEST_DIR"))
            .root
            .join("profile")
            .join("answers.toml");
        let shipped = crate::answers::load_answers(&path).unwrap();
        for label in ["When can you start a new role?", "Pick date...", ""] {
            let fields = [field(0, "input", "date", label, true)];
            let plan = plan_fields(&fields, &shipped, Path::new("cv.pdf"));
            assert!(plan.actions.is_empty(), "{label:?}: {:?}", plan.actions);
            assert_eq!(
                plan.missing,
                vec![label_or(label, "campo sin etiqueta")],
                "{label:?}"
            );
        }
    }

    #[test]
    fn a_control_of_an_unknown_kind_is_never_typed_into_and_is_reported() {
        // A kind this planner does not know (a color picker, a password box, something a later
        // version of the scan invents) must not be skipped quietly or fed text: a required one is
        // reported whatever the bank says, an optional one is left untouched.
        let fields = [
            field(0, "input", "color", "Brand color *", true),
            field(1, "input", "hologram", "Mystery *", true),
            field(2, "input", "password", "Choose a password *", true),
            field(3, "input", "hologram", "Another mystery", false),
        ];
        let bank = AnswerBank {
            entries: vec![
                AnswerEntry::new(&["brand color"], Some("Blue")),
                AnswerEntry::new(&["mystery"], Some("42")),
                AnswerEntry::new(&["password"], Some("hunter2")),
            ],
            consent_phrases: vec![],
        };
        for bank in [&bank, &AnswerBank::default()] {
            let plan = plan_fields(&fields, bank, Path::new("cv.pdf"));
            assert!(plan.actions.is_empty(), "{:?}", plan.actions);
            assert_eq!(
                plan.missing,
                vec!["Brand color *", "Mystery *", "Choose a password *"]
            );
        }
        // The kinds that do take text are still filled.
        let known: Vec<FieldInfo> = ["", "text", "search", "email", "tel", "url", "combobox"]
            .iter()
            .enumerate()
            .map(|(idx, kind)| field(idx, "input", kind, "Mystery *", true))
            .collect();
        let plan = plan_fields(&known, &bank, Path::new("cv.pdf"));
        assert_eq!(plan.actions.len(), known.len());
        assert!(plan.missing.is_empty());
    }

    #[test]
    fn a_page_the_browser_lands_on_can_close_the_door_by_its_address() {
        let ashby = "https://jobs.ashbyhq.com/acme/0b1c2d3e/application";
        // Ashby flags the bot's Chromium whoever clicks: only a dry run may look at such a form.
        assert_eq!(wall_for(ashby, SubmitMode::DryRun), None);
        assert_eq!(wall_for(ashby, SubmitMode::Assist), Some("handoff_host"));
        assert_eq!(wall_for(ashby, SubmitMode::Live), Some("handoff_host"));
        // Manual-only and listing-only pages stop every mode, as before.
        for mode in [SubmitMode::DryRun, SubmitMode::Assist, SubmitMode::Live] {
            assert_eq!(
                wall_for("https://www.linkedin.com/jobs/view/1", mode),
                Some("manual_only_platform")
            );
            assert_eq!(
                wall_for("https://remotive.com/remote-jobs/x-1", mode),
                Some("listing_only")
            );
            assert_eq!(
                wall_for("https://boards.greenhouse.io/acme/jobs/1", mode),
                None
            );
            assert_eq!(wall_for("https://notashbyhq.com/x", mode), None);
        }
    }

    #[test]
    fn every_spelling_of_not_hispanic_is_refused() {
        for option in [
            "Not Hispanic or Latino",
            "White (Not Hispanic or Latino)",
            "White, non-Hispanic",
            "Non-Hispanic White",
            "No, I am not Hispanic or Latino",
            "Not of Hispanic or Latino origin",
            "White (not of Hispanic origin)",
            "Not Latino",
            "Non-Latino White",
            "I am not of Latino descent",
        ] {
            assert!(claims_not_hispanic(option), "{option}");
        }
        for option in [
            "Hispanic or Latino",
            "Hispanic",
            "White",
            "Decline to self-identify",
        ] {
            assert!(!claims_not_hispanic(option), "{option}");
        }
    }

    fn race_bank() -> AnswerBank {
        AnswerBank {
            entries: vec![
                AnswerEntry::new(&["=race"], Some("White")).with_alternatives(&["Caucasian"]),
            ],
            consent_phrases: vec![],
        }
    }

    #[test]
    fn an_option_that_says_not_hispanic_is_never_picked_for_gonza() {
        // Ashby's race list has no plain "White": its option is "White (Not Hispanic or Latino)",
        // which also answers the Hispanic/Latino question Gonza keeps for himself.
        let group = |required: bool| {
            [
                radio(0, "race", "Race", "Hispanic or Latino", required),
                radio(
                    1,
                    "race",
                    "Race",
                    "White (Not Hispanic or Latino)",
                    required,
                ),
                radio(
                    2,
                    "race",
                    "Race",
                    "Asian (Not Hispanic or Latino)",
                    required,
                ),
                radio(3, "race", "Race", "Decline to self-identify", required),
            ]
        };
        let optional = plan_fields(&group(false), &race_bank(), Path::new("cv.pdf"));
        assert!(optional.actions.is_empty());
        assert!(optional.missing.is_empty());
        let required = plan_fields(&group(true), &race_bank(), Path::new("cv.pdf"));
        assert!(required.actions.is_empty());
        assert_eq!(required.missing, vec!["Race"]);
        // A plain "White" option beside a separate "Hispanic or Latino" one (Greenhouse) is no
        // longer picked: in a single-choice list the two exclude each other, so "White" would
        // answer the Hispanic question too. Required: Gonza's. Optional: left alone.
        let plain = |required: bool| {
            [
                radio(0, "race", "Race", "Hispanic or Latino", required),
                radio(1, "race", "Race", "White", required),
            ]
        };
        let plan = plan_fields(&plain(true), &race_bank(), Path::new("cv.pdf"));
        assert!(plan.actions.is_empty(), "{:?}", plan.actions);
        assert_eq!(plan.missing, vec!["Race"]);
        let plan = plan_fields(&plain(false), &race_bank(), Path::new("cv.pdf"));
        assert!(plan.actions.is_empty() && plan.missing.is_empty());
    }

    fn checks(plan: &FillPlan) -> Vec<usize> {
        plan.actions
            .iter()
            .filter(|action| action.value == FillValue::Check)
            .map(|action| action.idx)
            .collect()
    }

    #[test]
    fn a_bank_that_answers_hispanic_or_latino_picks_it_wherever_the_list_offers_it() {
        // A list of one option that offers "Hispanic or Latino" takes that option (never
        // "White (Not Hispanic or Latino)"); a mark-all list takes every option the answer names; a
        // plain Yes/No question about it takes Yes. The bank is a made-up one: the example profile
        // answers no self-identification question.
        let bank = AnswerBank {
            entries: vec![
                AnswerEntry::new(
                    &["=race", "race ethnicity", "my race"],
                    Some("Asian; Hispanic or Latino"),
                )
                .with_alternatives(&[
                    "Asian",
                    "Hispanic or Latino",
                    "Hispanic/Latino",
                ]),
                AnswerEntry::new(&["hispanic", "latino"], Some("Yes"))
                    .with_alternatives(&["Hispanic or Latino", "Hispanic/Latino"]),
            ],
            consent_phrases: vec![],
        };
        let ashby = [
            radio(0, "race", "Race", "Hispanic or Latino", true),
            radio(1, "race", "Race", "White (Not Hispanic or Latino)", true),
            radio(2, "race", "Race", "Asian (Not Hispanic or Latino)", true),
            radio(3, "race", "Race", "Decline to self-identify", true),
        ];
        let plan = plan_fields(&ashby, &bank, Path::new("cv.pdf"));
        assert_eq!(checks(&plan), vec![0]);
        assert!(plan.missing.is_empty(), "{:?}", plan.missing);

        let greenhouse = [
            radio(0, "race", "Race", "White", true),
            radio(1, "race", "Race", "Hispanic or Latino", true),
            radio(2, "race", "Race", "Asian", true),
        ];
        let plan = plan_fields(&greenhouse, &bank, Path::new("cv.pdf"));
        assert_eq!(checks(&plan), vec![1]);

        let mark_all = checkbox_group(
            0,
            "I identify my race/ethnicity as (mark all that apply):*",
            &["Asian", "Hispanic or Latino", "White"],
            true,
        );
        let plan = plan_fields(&mark_all, &bank, Path::new("cv.pdf"));
        assert_eq!(checks(&plan), vec![0, 1]);

        let separate = [
            radio(0, "hl", "Are you Hispanic/Latino?", "Yes", true),
            radio(1, "hl", "Are you Hispanic/Latino?", "No", true),
            radio(2, "hl", "Are you Hispanic/Latino?", "Decline", true),
        ];
        let plan = plan_fields(&separate, &bank, Path::new("cv.pdf"));
        assert_eq!(checks(&plan), vec![0]);
    }

    fn texts(options: &[&str]) -> Vec<String> {
        options.iter().map(|option| option.to_string()).collect()
    }

    #[test]
    fn a_drop_down_option_is_the_exact_one_then_the_prefix_never_a_loose_match() {
        let list = texts(&["Chile Islands", "Chile +56", "Chile"]);
        // The exact option wins even when a longer one comes first; without it the first option
        // that starts with the answer as a whole word is the one.
        assert_eq!(best_option(&list, "Chile"), Some(2));
        assert_eq!(best_option(&list[..2], "Chile"), Some(0));
        assert_eq!(
            best_option(&texts(&["Not sure", "No, thanks"]), "No"),
            Some(1)
        );
        // No answer without words in common, and no loose match for a plain answer: "Computer
        // Engineering" is not "Computer Science and Engineering".
        assert_eq!(best_option(&texts(&["Asian", "Other"]), "White"), None);
        assert_eq!(
            best_option(
                &texts(&["Computer Science and Engineering"]),
                "Computer Engineering"
            ),
            None
        );
        // Never an option that says "(Not Hispanic or Latino)".
        let race = texts(&["White (Not Hispanic or Latino)", "White"]);
        assert_eq!(best_option(&race, "White"), Some(1));
        assert_eq!(best_option(&race[..1], "White"), None);
    }

    #[test]
    fn a_place_matches_an_option_that_names_all_its_parts() {
        // The city list answers "Santiago" with full place names; the commas of the bank's
        // "Santiago, Chile" find nothing when typed, so only its first part is searched.
        let places = texts(&[
            "Santiago de Cali, Valle del Cauca, Colombia",
            "Santiago, Dominican Republic",
            "Santiago, Metropolitana, Chile",
        ]);
        assert_eq!(search_text("Santiago, Chile"), "Santiago");
        assert_eq!(best_option(&places, "Santiago, Chile"), Some(2));
        assert_eq!(best_option(&places[..2], "Santiago, Chile"), None);
        assert_eq!(search_text("Engineer's Degree"), "Engineer's Degree");
    }

    #[test]
    fn a_text_input_with_alternatives_may_be_a_custom_dropdown() {
        // Greenhouse's drop-downs are inputs, not selects: the filler needs every option text to
        // try. A plain text box (and a textarea) takes the first one only.
        let bank = AnswerBank {
            entries: vec![
                AnswerEntry::new(&["=school"], Some("Example Professional Institute"))
                    .with_alternatives(&["Example Institute", "Other"]),
            ],
            consent_phrases: vec![],
        };
        let fields = [
            field(0, "input", "text", "School*", true),
            field(1, "textarea", "", "School", false),
        ];
        let plan = plan_fields(&fields, &bank, Path::new("cv.pdf"));
        assert_eq!(
            plan.actions[0].value,
            FillValue::Select(vec![
                "Example Professional Institute".into(),
                "Example Institute".into(),
                "Other".into()
            ])
        );
        assert_eq!(
            plan.actions[1].value,
            FillValue::Text("Example Professional Institute".into())
        );
    }

    #[test]
    fn required_checkbox_and_radio_group_are_missing_once() {
        let fields = [
            field(
                0,
                "input",
                "checkbox",
                "I agree to the privacy policy",
                true,
            ),
            radio(1, "remote", "Open to remote work?", "Yes", true),
            radio(2, "remote", "Open to remote work?", "No", true),
            field(3, "input", "checkbox", "Subscribe to the newsletter", false),
        ];
        let plan = plan_fields(&fields, &bank(), Path::new("cv.pdf"));
        assert_eq!(
            plan.missing,
            vec!["I agree to the privacy policy", "Open to remote work?"]
        );
        assert!(plan.actions.is_empty());
    }

    fn languages_bank(value: Option<&str>) -> AnswerBank {
        AnswerBank {
            entries: vec![AnswerEntry::new(&["which languages"], value)],
            consent_phrases: vec![],
        }
    }

    const LANGUAGES: &str = "Which languages do you use? (select all that apply) *";

    #[test]
    fn a_consent_phrase_in_the_question_of_a_group_ticks_none_of_its_options() {
        // The question mentions the privacy notice, the options are channels: ticking "every
        // option because the question sounds like consent" would sign Gonza up for all of them.
        let question = "Review our privacy notice, then choose how we may reach you *";
        let fields = checkbox_group(0, question, &["Email", "SMS", "Phone"], true);
        let allowed = AnswerBank {
            consent_phrases: vec!["privacy notice".into()],
            ..bank()
        };
        let plan = plan_fields(&fields, &allowed, Path::new("cv.pdf"));
        assert!(plan.actions.is_empty(), "{:?}", plan.actions);
        assert_eq!(plan.missing, vec![question]);
    }

    #[test]
    fn a_checkbox_group_ticks_every_option_the_bank_lists() {
        let fields = checkbox_group(0, LANGUAGES, &["Python", "Rust", "Go"], true);
        let plan = plan_fields(
            &fields,
            &languages_bank(Some("Python; Rust")),
            Path::new("cv.pdf"),
        );
        assert!(plan.missing.is_empty(), "{:?}", plan.missing);
        let ticked: Vec<usize> = plan.actions.iter().map(|action| action.idx).collect();
        assert_eq!(ticked, vec![0, 1]);
        assert!(
            plan.actions
                .iter()
                .all(|action| action.value == FillValue::Check && action.label == LANGUAGES)
        );
    }

    #[test]
    fn a_checkbox_group_takes_the_first_candidate_the_form_has() {
        // Like a drop-down: the value, then each alternative; the first one that names an option
        // wins and nothing else is ticked.
        let bank = AnswerBank {
            entries: vec![
                AnswerEntry::new(&["how did you hear"], Some("Company careers page"))
                    .with_alternatives(&["Company website", "Other"]),
            ],
            consent_phrases: vec![],
        };
        let question = "How did you hear about us? (select all that apply) *";
        let fields = checkbox_group(0, question, &["LinkedIn", "Glassdoor", "Other"], true);
        let plan = plan_fields(&fields, &bank, Path::new("cv.pdf"));
        assert_eq!(plan.actions.len(), 1);
        assert_eq!(plan.actions[0].idx, 2);
        // None of the candidates is an option here: a required group is Gonza's.
        let fields = checkbox_group(0, question, &["LinkedIn", "Glassdoor"], true);
        let plan = plan_fields(&fields, &bank, Path::new("cv.pdf"));
        assert!(plan.actions.is_empty());
        assert_eq!(plan.missing, vec![question]);
    }

    #[test]
    fn a_checkbox_group_without_an_approved_answer_stays_unticked() {
        // No entry, an entry without a value and one with a blank value all mean "ask Gonza":
        // required, the question is reported once; optional, it is simply left alone.
        let banks = [
            AnswerBank::default(),
            languages_bank(None),
            languages_bank(Some("   ")),
        ];
        for bank in &banks {
            let required = checkbox_group(0, LANGUAGES, &["Python", "Rust", "Go"], true);
            let plan = plan_fields(&required, bank, Path::new("cv.pdf"));
            assert!(plan.actions.is_empty());
            assert_eq!(plan.missing, vec![LANGUAGES]);
            let optional = checkbox_group(0, LANGUAGES, &["Python", "Rust", "Go"], false);
            let plan = plan_fields(&optional, bank, Path::new("cv.pdf"));
            assert!(plan.actions.is_empty());
            assert!(plan.missing.is_empty());
        }
    }

    #[test]
    fn an_optional_checkbox_group_is_ticked_when_the_bank_answers_it() {
        // Like an optional radio group: an explicit answer is applied, required or not.
        let fields = checkbox_group(0, "Which languages do you use?", &["Python", "Go"], false);
        let plan = plan_fields(&fields, &languages_bank(Some("Go")), Path::new("cv.pdf"));
        assert_eq!(plan.actions.len(), 1);
        assert_eq!(plan.actions[0].idx, 1);
        assert!(plan.missing.is_empty());
    }

    #[test]
    fn a_checkbox_group_never_ticks_a_not_hispanic_option() {
        let options = [
            "White (Not Hispanic or Latino)",
            "Hispanic or Latino",
            "Decline to self-identify",
        ];
        let question = "Race or ethnicity (select all that apply) *";
        let fields = checkbox_group(0, question, &options, true);
        let bank = AnswerBank {
            entries: vec![AnswerEntry::new(&["race or ethnicity"], Some("White"))],
            consent_phrases: vec![],
        };
        let plan = plan_fields(&fields, &bank, Path::new("cv.pdf"));
        assert!(plan.actions.is_empty(), "{:?}", plan.actions);
        assert_eq!(plan.missing, vec![question]);
    }

    #[test]
    fn an_option_that_is_already_ticked_is_kept_and_the_others_are_ticked() {
        let mut fields = checkbox_group(0, LANGUAGES, &["Python", "Rust", "Go"], true);
        fields[0].checked = true;
        let plan = plan_fields(
            &fields,
            &languages_bank(Some("Python; Rust")),
            Path::new("cv.pdf"),
        );
        assert_eq!(plan.kept, vec![0]);
        let ticked: Vec<usize> = plan.actions.iter().map(|action| action.idx).collect();
        assert_eq!(ticked, vec![1]);
    }

    #[test]
    fn an_open_checkbox_group_is_left_to_gonza_once_on_its_first_option() {
        let mut fields = checkbox_group(0, LANGUAGES, &["Python", "Rust", "Go"], true);
        fields.extend(checkbox_group(
            3,
            "Which tools? (select all that apply) *",
            &["Git", "Make"],
            true,
        ));
        // The languages are answered (so they are not flagged), the tools are open.
        let bank = AnswerBank {
            entries: vec![AnswerEntry::new(&["which languages"], Some("Go"))],
            consent_phrases: vec![],
        };
        let plan = plan_fields(&fields, &bank, Path::new("cv.pdf"));
        assert_eq!(plan.missing, vec!["Which tools? (select all that apply) *"]);
        let left = controls_left(&fields, &plan, &[]);
        assert_eq!(left, vec![3]);
        // The mark is watched through every option of the group, so ticking any of them clears it.
        assert_eq!(flag_groups(&fields, &left), vec![vec![3, 4]]);
    }

    #[test]
    fn a_lone_checkbox_with_a_caption_is_still_judged_as_a_consent_box() {
        // One option: not a group. The scan labels it "question: option" and the allow list
        // decides, exactly as before checkbox groups were decided as a whole.
        let alone = [checkbox(
            0,
            "ack[]",
            "Point of Data Transfer: Acknowledge",
            "Acknowledge",
            true,
        )];
        let allowed = AnswerBank {
            consent_phrases: vec!["data transfer".into()],
            ..AnswerBank::default()
        };
        let plan = plan_fields(&alone, &allowed, Path::new("cv.pdf"));
        assert_eq!(plan.actions.len(), 1);
        assert!(plan.missing.is_empty());
    }

    #[test]
    fn blocker_text_detects_codes_login_and_bot_walls() {
        assert_eq!(
            blocker_in_text("We sent a Verification Code to your email"),
            Some("captcha")
        );
        assert_eq!(
            blocker_in_text("Please confirm: I\u{2019}m not a robot"),
            Some("captcha")
        );
        assert_eq!(
            blocker_in_text("Sign in to apply for this job"),
            Some("login_required")
        );
        assert_eq!(
            blocker_in_text("Access denied. Unusual traffic detected."),
            Some("blocked_page")
        );
    }

    #[test]
    fn blocker_text_ignores_ordinary_application_pages() {
        let text = "Apply for Senior AI Engineer. First name. Last name. Email. Resume. Submit application.";
        assert_eq!(blocker_in_text(text), None);
        assert_eq!(blocker_in_text(""), None);
    }

    #[test]
    fn controls_left_are_the_required_ones_without_an_answer() {
        let fields = [
            field(0, "input", "text", "First name *", true), // answered
            field(1, "input", "email", "Email *", true),     // required, no value in the bank
            field(2, "input", "text", "Nickname", false),    // optional: never flagged
            radio(3, "remote", "Open to remote work?", "Yes", true),
            radio(4, "remote", "Open to remote work?", "No", true),
            radio(
                5,
                "sponsor",
                "Do you require visa sponsorship? *",
                "Yes",
                true,
            ),
            radio(
                6,
                "sponsor",
                "Do you require visa sponsorship? *",
                "No",
                true,
            ),
            field(
                7,
                "input",
                "checkbox",
                "I agree to the privacy policy",
                true,
            ),
            field(8, "input", "file", "Portfolio PDF", true),
            field(9, "input", "file", "Resume/CV *", true), // gets the CV
        ];
        let plan = plan_fields(&fields, &bank(), Path::new("cv.pdf"));
        // The unanswered radio group is flagged once, on its first button; the answered one
        // (sponsorship) and the optional field are left out.
        assert_eq!(controls_left(&fields, &plan, &[]), vec![1, 3, 7, 8]);
        // A control whose fill failed is left for Gonza too, once, in page order.
        assert_eq!(controls_left(&fields, &plan, &[0, 7]), vec![0, 1, 3, 7, 8]);
    }

    #[test]
    fn a_flagged_radio_group_is_watched_through_all_its_buttons() {
        let fields = [
            field(0, "input", "email", "Email *", true),
            radio(1, "remote", "Open to remote work?", "Yes", true),
            radio(2, "remote", "Open to remote work?", "No", true),
        ];
        // The control to outline leads its group; the mark goes away when any button is chosen.
        assert_eq!(flag_groups(&fields, &[0, 1]), vec![vec![0], vec![1, 2]]);
        // A button whose fill failed leads the group it belongs to.
        assert_eq!(flag_groups(&fields, &[2]), vec![vec![2, 1]]);
    }

    #[test]
    fn without_missing_answers_only_failed_controls_are_left() {
        let fields = [field(0, "input", "text", "First name *", true)];
        let plan = plan_fields(&fields, &bank(), Path::new("cv.pdf"));
        assert!(plan.missing.is_empty());
        assert_eq!(controls_left(&fields, &plan, &[]), Vec::<usize>::new());
        assert_eq!(controls_left(&fields, &plan, &[0]), vec![0]);
    }

    #[test]
    fn leftover_reason_lists_missing_answers_first_and_failures_after() {
        let missing = vec!["Email *".to_string(), "Why this role? *".to_string()];
        let failures = vec![
            "opción no encontrada: Country = Atlantis".to_string(),
            "no se pudo completar Phone: timeout".to_string(),
        ];
        assert_eq!(leftover_reason(&[], &[]), None);
        assert_eq!(
            leftover_reason(&missing, &[]).as_deref(),
            Some("faltan respuestas: Email *; Why this role? *")
        );
        assert_eq!(
            leftover_reason(&[], &failures[..1]).as_deref(),
            Some("opción no encontrada: Country = Atlantis")
        );
        assert_eq!(
            leftover_reason(&missing, &failures).as_deref(),
            Some(
                "faltan respuestas: Email *; Why this role? * — opción no encontrada: Country = Atlantis; no se pudo completar Phone: timeout"
            )
        );
    }

    #[test]
    fn assist_timeout_says_what_was_still_pending() {
        assert_eq!(
            assist_timeout_reason(10, None),
            "assist_timeout: sin confirmación en 10 minutos"
        );
        assert_eq!(
            assist_timeout_reason(10, Some("faltan respuestas: Email *")),
            "assist_timeout: sin confirmación en 10 minutos. Quedaba pendiente: faltan respuestas: Email *"
        );
    }

    #[test]
    fn an_unsafe_link_is_handed_back_before_any_navigation() {
        for url in [
            "https://169.254.169.254/latest/meta-data/",
            "file:///C:/Windows/win.ini",
            "javascript:alert(1)",
            "https://localhost:8080/",
            "/apply/1",
            "",
        ] {
            let job = make_job_with(|j| j.apply_url = url.to_string());
            let result = refuse_unsafe_link(&job, NavPolicy::STRICT)
                .unwrap_or_else(|| panic!("{url:?} was allowed"));
            assert_eq!(result.status, SubmitStatus::NeedsGonza, "{url:?}");
            assert!(
                result.reason.as_deref().unwrap().starts_with("unsafe_url:"),
                "{url:?}: {:?}",
                result.reason
            );
            assert!(result.screenshot.is_none(), "{url:?}");
        }
        let fine =
            make_job_with(|j| j.apply_url = "https://boards.greenhouse.io/acme/jobs/1".into());
        assert!(refuse_unsafe_link(&fine, NavPolicy::STRICT).is_none());
    }

    #[test]
    fn assist_closed_points_to_confirm_and_says_what_was_pending() {
        let base = "assist_closed: cerraste la pestaña sin que la página confirmara; si sí la enviaste, `jobbot confirm KEY` la marca";
        assert_eq!(assist_closed_reason(None), base);
        assert_eq!(
            assist_closed_reason(Some("faltan respuestas: Email *")),
            format!("{base}. Quedaba pendiente: faltan respuestas: Email *")
        );
    }

    // ---- Review fixes, pure planning ----

    const ESSAYS: [&str; 3] = [
        "Describe a project you are proud of and include your salary expectations",
        "Tell us why you are a good fit. Please also include your salary expectations and notice period.",
        "Additional Information (please include your location, notice period and salary expectations)",
    ];

    #[test]
    fn an_essay_box_never_gets_a_one_line_value_found_by_a_keyword() {
        // "salary" is in each question, but each one asks for far more than a salary: the bank's
        // "USD 6,000 per month" would answer only a corner of it. Required: Gonza answers.
        // Optional: left alone.
        for required in [true, false] {
            for label in ESSAYS {
                let fields = [field(0, "textarea", "", label, required)];
                let plan = plan_fields(&fields, &pay_and_start_bank(), Path::new("cv.pdf"));
                assert!(plan.actions.is_empty(), "{label}: {:?}", plan.actions);
                let want: Vec<&str> = if required { vec![label] } else { vec![] };
                assert_eq!(plan.missing, want, "{label}");
            }
        }
    }

    #[test]
    fn the_shipped_bank_answers_a_salary_text_box_by_its_whole_label_and_no_essay() {
        let bank = crate::answers::load_answers(
            &crate::paths::Paths::new(env!("CARGO_MANIFEST_DIR"))
                .profile_dir
                .join("answers.toml"),
        )
        .unwrap();
        for label in [
            "Salary expectations",
            "Compensation Expectations*",
            "What are your salary expectations?",
            "Expected salary",
            "Desired salary",
            "Salary requirements",
        ] {
            let fields = [field(0, "textarea", "", label, true)];
            let plan = plan_fields(&fields, &bank, Path::new("cv.pdf"));
            assert!(plan.missing.is_empty(), "{label}: {:?}", plan.missing);
            assert_eq!(
                plan.actions[0].value,
                FillValue::Text("USD 6,000 per month".into()),
                "{label}"
            );
        }
        for label in ESSAYS {
            let fields = [field(0, "textarea", "", label, true)];
            let plan = plan_fields(&fields, &bank, Path::new("cv.pdf"));
            assert!(plan.actions.is_empty(), "{label}: {:?}", plan.actions);
        }
    }

    #[test]
    fn an_essay_box_is_answered_by_an_entry_written_for_its_whole_label() {
        let bank = AnswerBank {
            entries: vec![AnswerEntry::new(
                &["=compensation expectations"],
                Some("USD 6,000 per month"),
            )],
            consent_phrases: vec![],
        };
        let fields = [field(0, "textarea", "", "Compensation Expectations*", true)];
        let plan = plan_fields(&fields, &bank, Path::new("cv.pdf"));
        assert!(plan.missing.is_empty(), "{:?}", plan.missing);
        assert_eq!(
            plan.actions[0].value,
            FillValue::Text("USD 6,000 per month".into())
        );
        // The exact pattern does not stretch to a longer question that merely starts the same way.
        let longer = [field(
            0,
            "textarea",
            "",
            "Compensation Expectations and notice period",
            true,
        )];
        let plan = plan_fields(&longer, &bank, Path::new("cv.pdf"));
        assert!(plan.actions.is_empty());
        assert_eq!(
            plan.missing,
            vec!["Compensation Expectations and notice period"]
        );
    }

    #[test]
    fn a_long_label_is_answered_in_a_one_line_box_only_by_its_exact_entry() {
        let twelve = ESSAYS[0];
        assert_eq!(twelve.split_whitespace().count(), 12);
        let short = "What are your salary expectations for this role in USD?";
        for label in [twelve, ESSAYS[1]] {
            let fields = [field(0, "input", "text", label, true)];
            let plan = plan_fields(&fields, &pay_and_start_bank(), Path::new("cv.pdf"));
            assert!(plan.actions.is_empty(), "{label}: {:?}", plan.actions);
            assert_eq!(plan.missing, vec![label], "{label}");
        }
        let fields = [field(0, "input", "text", short, true)];
        let plan = plan_fields(&fields, &pay_and_start_bank(), Path::new("cv.pdf"));
        assert_eq!(plan.actions.len(), 1, "a short question is still answered");
        // A drop-down or a Yes/No group can only take one of its own options, so a long label
        // cannot make it carry extra claims: those are still answered.
        let long_yes_no =
            "Are you legally authorized to work in the country where you are applying today?";
        let bank = AnswerBank {
            entries: vec![AnswerEntry::new(&["legally authorized"], Some("Yes"))],
            consent_phrases: vec![],
        };
        let fields = [
            field(0, "select", "", long_yes_no, true),
            field(1, "input", "combobox", long_yes_no, true),
            radio(2, "auth", long_yes_no, "Yes", true),
            radio(3, "auth", long_yes_no, "No", true),
        ];
        let plan = plan_fields(&fields, &bank, Path::new("cv.pdf"));
        let answered: Vec<usize> = plan.actions.iter().map(|action| action.idx).collect();
        assert_eq!(answered, vec![0, 1, 2]);
    }

    fn relocation_options() -> Vec<String> {
        texts(&[
            "Yes",
            "No, but I am willing to relocate to the US",
            "No, and I am not willing to relocate",
        ])
    }

    #[test]
    fn a_bare_yes_no_takes_a_qualified_option_only_when_it_is_the_only_one() {
        let options = relocation_options();
        assert_eq!(best_option(&options, "Yes"), Some(0));
        // Two options start with "No": the one Gonza would have to be speaking for is not guessed.
        assert_eq!(best_option(&options, "No"), None);
        // The exact option comes first wherever it is.
        let with_exact = texts(&["No, but I am willing to relocate", "No", "No, and I am not"]);
        assert_eq!(best_option(&with_exact, "No"), Some(1));
        // One qualified option is the only reading of the answer.
        let sponsor = texts(&["Yes, I need sponsorship", "No, I do not need sponsorship"]);
        assert_eq!(best_option(&sponsor, "No"), Some(1));
        assert_eq!(best_option(&sponsor, "Yes"), Some(0));
        // A longer answer is not a bare yes/no: the first prefix still wins.
        let thanks = texts(&["No, thanks a lot", "No, thanks anyway"]);
        assert_eq!(best_option(&thanks, "No, thanks"), Some(0));
    }

    #[test]
    fn a_radio_group_does_not_guess_between_two_no_options() {
        let bank = AnswerBank {
            entries: vec![AnswerEntry::new(
                &["work location in the united states"],
                Some("No"),
            )],
            consent_phrases: vec![],
        };
        let question = "Is your work location in the United States";
        for required in [true, false] {
            let fields: Vec<FieldInfo> = relocation_options()
                .iter()
                .enumerate()
                .map(|(i, option)| radio(i, "us", question, option, required))
                .collect();
            let plan = plan_fields(&fields, &bank, Path::new("cv.pdf"));
            assert!(plan.actions.is_empty(), "{:?}", plan.actions);
            let want: Vec<&str> = if required { vec![question] } else { vec![] };
            assert_eq!(plan.missing, want);
        }
        // With a plain "No" in the group, that exact option is clicked.
        let fields = [
            radio(0, "us", question, "Yes", true),
            radio(1, "us", question, "No, but I would relocate", true),
            radio(2, "us", question, "No", true),
        ];
        let plan = plan_fields(&fields, &bank, Path::new("cv.pdf"));
        assert_eq!(plan.actions.len(), 1);
        assert_eq!(plan.actions[0].idx, 2);
    }

    #[test]
    fn a_checkbox_group_does_not_guess_between_two_no_options_either() {
        let bank = AnswerBank {
            entries: vec![AnswerEntry::new(&["work location"], Some("No"))],
            consent_phrases: vec![],
        };
        let fields = checkbox_group(
            0,
            "Is your work location in the United States? (select one)",
            &[
                "Yes",
                "No, but I am willing to relocate",
                "No, and I am not willing to",
            ],
            true,
        );
        let plan = plan_fields(&fields, &bank, Path::new("cv.pdf"));
        assert!(plan.actions.is_empty(), "{:?}", plan.actions);
        assert_eq!(plan.missing.len(), 1);
    }

    fn race_options(options: &[&str], required: bool) -> Vec<FieldInfo> {
        options
            .iter()
            .enumerate()
            .map(|(i, option)| radio(i, "race", "Race", option, required))
            .collect()
    }

    #[test]
    fn a_single_choice_race_list_with_a_hispanic_option_is_never_answered() {
        // In such a list "White" excludes "Hispanic or Latino": picking it answers the Hispanic
        // question, which Gonza keeps for himself. Whatever the exact wording of the options.
        let lists: [&[&str]; 3] = [
            &["Hispanic or Latino", "White", "Asian"],
            &["White", "Black or African American", "Hispanic/Latino"],
            &["White", "Latinx", "Decline to self-identify"],
        ];
        for list in lists {
            let required =
                plan_fields(&race_options(list, true), &race_bank(), Path::new("cv.pdf"));
            assert!(
                required.actions.is_empty(),
                "{list:?}: {:?}",
                required.actions
            );
            assert_eq!(required.missing, vec!["Race"], "{list:?}");
            let optional = plan_fields(
                &race_options(list, false),
                &race_bank(),
                Path::new("cv.pdf"),
            );
            assert!(optional.actions.is_empty(), "{list:?}");
            assert!(optional.missing.is_empty(), "{list:?}");
        }
        // A race list that does not mention Hispanic or Latino at all is answered as before.
        let plain = race_options(&["White", "Black or African American", "Asian"], true);
        let plan = plan_fields(&plain, &race_bank(), Path::new("cv.pdf"));
        assert_eq!(plan.actions.len(), 1);
        assert_eq!(plan.actions[0].idx, 0);
    }

    #[test]
    fn a_mark_all_that_apply_race_group_still_ticks_only_white() {
        // Several boxes may be ticked: White alone says nothing about the Hispanic option beside it.
        let question = "I identify my race/ethnicity as (mark all that apply): *";
        let fields = checkbox_group(
            0,
            question,
            &[
                "Asian",
                "Hispanic or Latino",
                "White",
                "Decline to self-identify",
            ],
            true,
        );
        let bank = AnswerBank {
            entries: vec![AnswerEntry::new(&["race/ethnicity"], Some("White"))],
            consent_phrases: vec![],
        };
        let plan = plan_fields(&fields, &bank, Path::new("cv.pdf"));
        assert!(plan.missing.is_empty(), "{:?}", plan.missing);
        let ticked: Vec<usize> = plan.actions.iter().map(|action| action.idx).collect();
        assert_eq!(ticked, vec![2]);
    }

    #[test]
    fn a_drop_down_list_is_never_answered_when_it_offers_hispanic_or_latino() {
        let with = texts(&["White", "Asian", "Hispanic or Latino"]);
        let without = texts(&["White", "Asian"]);
        let race = texts(&["White"]);
        assert_eq!(options_named(&with, &race, false), Vec::<usize>::new());
        assert_eq!(options_named(&without, &race, false), vec![0]);
        // A multi-select list takes the answer whatever else it offers.
        assert_eq!(options_named(&with, &race, true), vec![0]);
    }

    #[test]
    fn every_spelling_of_not_hispanic_is_refused_more() {
        for option in [
            "White (Other than Hispanic or Latino)",
            "White, excluding Hispanic or Latino",
            "White (No Hispanic origin)",
            "White except Hispanic",
            "White without Hispanic or Latino origin",
            "Caucasian (other than Latinx)",
            "Black or African American (Excluding Latina)",
        ] {
            assert!(claims_not_hispanic(option), "{option}");
        }
        for option in [
            "Hispanic or Latino",
            "White",
            "Other",
            "Other (please specify)",
            "No answer",
            "I do not wish to answer",
            "Prefer not to say",
            "Hispanic or Latino (any race)",
        ] {
            assert!(!claims_not_hispanic(option), "{option}");
        }
    }

    #[test]
    fn a_pre_ticked_marketing_box_is_reported_instead_of_going_out() {
        let ticked = |idx: usize, label: &str, required: bool| FieldInfo {
            checked: true,
            ..field(idx, "input", "checkbox", label, required)
        };
        let fields = [
            ticked(0, "Yes, text me job alerts by SMS", false),
            ticked(1, "Subscribe to our newsletter", false),
            ticked(2, "I agree to binding arbitration *", true),
            // Already ticked and a consent Gonza allowed: nothing to report.
            ticked(3, "I have read and accept the Privacy Policy *", true),
            // Not ticked: an optional newsletter box is simply left alone.
            field(4, "input", "checkbox", "Send me marketing emails", false),
            // Ticked, but not about anything that signs him up: not reported.
            ticked(5, "Remember me on this device", false),
        ];
        let allowed = AnswerBank {
            consent_phrases: vec!["privacy policy".into()],
            ..bank()
        };
        let plan = plan_fields(&fields, &allowed, Path::new("cv.pdf"));
        let reported: Vec<&str> = plan
            .missing
            .iter()
            .map(|label| label.split(" (").next().unwrap())
            .collect();
        assert_eq!(
            reported,
            vec![
                "Yes, text me job alerts by SMS",
                "Subscribe to our newsletter",
                "I agree to binding arbitration *"
            ]
        );
        // It is flagged red on the page (the box is optional, so only this makes it show), and
        // nothing was planned for it: the filler never unticks.
        assert_eq!(controls_left(&fields, &plan, &[]), vec![0, 1, 2]);
        assert!(plan.actions.is_empty(), "{:?}", plan.actions);
        assert_eq!(plan.kept, vec![3]);
    }

    #[test]
    fn a_pre_ticked_option_of_a_group_that_signs_him_up_is_reported() {
        let mut fields = checkbox_group(
            0,
            "How may we contact you? (select all that apply)",
            &["Email", "SMS", "Phone"],
            false,
        );
        fields[1].checked = true;
        let plan = plan_fields(&fields, &AnswerBank::default(), Path::new("cv.pdf"));
        assert_eq!(plan.missing.len(), 1, "{:?}", plan.missing);
        assert!(plan.missing[0].starts_with("How may we contact you?"));
        assert_eq!(controls_left(&fields, &plan, &[]), vec![1]);
        // The same group with nothing ticked reports nothing.
        let untouched = checkbox_group(
            0,
            "How may we contact you? (select all that apply)",
            &["Email", "SMS", "Phone"],
            false,
        );
        let plan = plan_fields(&untouched, &AnswerBank::default(), Path::new("cv.pdf"));
        assert!(plan.missing.is_empty());
    }

    #[test]
    fn an_empty_drop_down_list_is_never_settled_before_the_most_it_waits() {
        let quiet = |ms: u64| Duration::from_millis(ms);
        // A typeahead that answers late shows nothing for a while: that is not "no options".
        assert!(!menu_settled(0, quiet(OPTIONS_SETTLE_MS + 100)));
        assert!(!menu_settled(0, quiet(OPTIONS_MAX_MS)));
        // A list with options is settled once it stopped changing for a moment.
        assert!(!menu_settled(3, quiet(OPTIONS_SETTLE_MS - 100)));
        assert!(menu_settled(3, quiet(OPTIONS_SETTLE_MS)));
    }

    #[test]
    fn a_failed_pick_of_an_optional_field_is_a_skip_and_a_required_one_a_failure() {
        let no_match = anyhow::Error::new(NoOption::NoMatch);
        let reserved = anyhow::Error::new(NoOption::Reserved);
        let broken = anyhow::anyhow!("Timeout 10000ms exceeded");
        assert!(is_skippable(&no_match, false));
        assert!(is_skippable(&reserved, false));
        assert!(!is_skippable(&no_match, true));
        assert!(!is_skippable(&reserved, true));
        // A page that did not answer is a failure whatever the field.
        assert!(!is_skippable(&broken, false));
        assert!(!is_skippable(&broken, true));
    }

    // ---- Browser tests: `cargo test --lib form:: -- --ignored --test-threads=1` ----

    use playwright_rs::Browser;
    use wiremock::matchers::method;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use crate::paths::Paths;
    use crate::testing::make_job_with;

    fn fixture_url(variant: &str) -> String {
        let path = Paths::new(env!("CARGO_MANIFEST_DIR"))
            .root
            .join("tests")
            .join("fixtures")
            .join("ats_form.html");
        format!(
            "file:///{}?variant={variant}",
            path.display().to_string().replace('\\', "/")
        )
    }

    fn full_bank() -> AnswerBank {
        AnswerBank {
            entries: vec![
                AnswerEntry::new(&["first name"], Some("Jane")),
                AnswerEntry::new(&["last name"], Some("Doe Garcia")),
                AnswerEntry::new(&["email"], Some("search@example.com")),
                AnswerEntry::new(&["phone"], Some("+56 9 0000 0000")),
                AnswerEntry::new(&["country"], Some("Chile")),
                AnswerEntry::new(&["how did you hear"], Some("Company careers page"))
                    .with_alternatives(&["Company website", "Other"]),
                AnswerEntry::new(&["visa sponsorship", "require sponsorship"], Some("No")),
            ],
            consent_phrases: vec![],
        }
    }

    /// One headless browser page plus a temp folder holding a fake CV and the screenshots.
    struct Session {
        _playwright: Playwright,
        browser: Browser,
        page: Page,
        dir: tempfile::TempDir,
    }

    impl Session {
        async fn start() -> Session {
            let playwright = Playwright::launch().await.unwrap();
            let browser = playwright
                .chromium()
                .launch_with_options(crate::launch::launch_options())
                .await
                .unwrap();
            let page = browser.new_page().await.unwrap();
            Session {
                _playwright: playwright,
                browser,
                page,
                dir: tempfile::tempdir().unwrap(),
            }
        }

        fn cv(&self) -> PathBuf {
            let path = self.dir.path().join("cv.pdf");
            std::fs::write(&path, b"%PDF-1.4 fake").unwrap();
            path
        }

        fn shots(&self) -> PathBuf {
            self.dir.path().join("shots")
        }

        async fn fill_url(&self, url: &str, bank: &AnswerBank, mode: SubmitMode) -> SubmitResult {
            let job = make_job_with(|j| j.apply_url = url.to_string());
            fill_application(
                &self.page,
                &job,
                &self.cv(),
                bank,
                mode,
                &self.shots(),
                NavPolicy::LOCAL_TESTS,
            )
            .await
            .unwrap()
        }

        async fn fill(&self, variant: &str, bank: &AnswerBank, mode: SubmitMode) -> SubmitResult {
            self.fill_url(&fixture_url(variant), bank, mode).await
        }

        async fn eval(&self, expression: &str) -> String {
            self.page.evaluate_value(expression).await.unwrap()
        }

        async fn value(&self, selector: &str) -> String {
            self.page.locator(selector).input_value(None).await.unwrap()
        }

        async fn stop(self) {
            self.browser.close().await.unwrap();
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "needs chromium"]
    async fn dry_run_fills_and_does_not_submit() {
        let session = Session::start().await;
        let result = session
            .fill("default", &full_bank(), SubmitMode::DryRun)
            .await;
        assert_eq!(result.status, SubmitStatus::DryRun);
        assert_eq!(result.reason, None);
        assert!(result.screenshot.unwrap().exists());
        assert_eq!(session.value("#first_name").await, "Jane");
        assert_eq!(session.value("#email").await, "search@example.com");
        assert_eq!(session.value("#country").await, "cl");
        let uploaded = session
            .page
            .locator("#filename")
            .text_content()
            .await
            .unwrap()
            .unwrap_or_default();
        assert!(
            uploaded.contains("cv.pdf"),
            "uploaded file label was: {uploaded}"
        );
        assert_eq!(
            session.eval("String(Boolean(window.submitted))").await,
            "false"
        );
        session.stop().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "needs chromium"]
    async fn live_run_waits_for_the_button_clicks_it_and_saves_the_after_screenshot() {
        let session = Session::start().await;
        let result = session
            .fill("default", &full_bank(), SubmitMode::Live)
            .await;
        assert_eq!(result.status, SubmitStatus::Submitted);
        let shot = result.screenshot.unwrap();
        assert!(shot.exists());
        assert!(
            shot.file_name()
                .unwrap()
                .to_string_lossy()
                .ends_with("-after.png")
        );
        assert_eq!(
            session.eval("String(Boolean(window.submitted))").await,
            "true"
        );
        let confirm = session
            .page
            .locator("#confirm")
            .text_content()
            .await
            .unwrap()
            .unwrap_or_default();
        assert!(confirm.contains("received"), "confirmation was: {confirm}");
        session.stop().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "needs chromium"]
    async fn missing_required_answer_goes_to_gonza_without_submitting() {
        let session = Session::start().await;
        let mut bank = full_bank();
        bank.entries[2] = AnswerEntry::new(&["email"], None);
        let result = session.fill("default", &bank, SubmitMode::Live).await;
        assert_eq!(result.status, SubmitStatus::NeedsGonza);
        assert!(
            result
                .reason
                .unwrap()
                .starts_with("faltan respuestas: Email")
        );
        assert!(result.screenshot.unwrap().exists());
        assert_eq!(
            session.eval("String(Boolean(window.submitted))").await,
            "false"
        );
        session.stop().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "needs chromium"]
    async fn unknown_required_question_goes_to_gonza() {
        let session = Session::start().await;
        let result = session
            .fill("extra_required", &full_bank(), SubmitMode::Live)
            .await;
        assert_eq!(result.status, SubmitStatus::NeedsGonza);
        let reason = result.reason.unwrap();
        assert!(
            reason.contains("Favorite programming paradigm"),
            "reason was: {reason}"
        );
        assert_eq!(
            session.eval("String(Boolean(window.submitted))").await,
            "false"
        );
        session.stop().await;
    }

    /// Which controls carry the "left for Gonza" flag, by element id.
    async fn flagged_ids(session: &Session) -> String {
        session
            .eval("Array.from(document.querySelectorAll('[data-jb-todo]')).map((el) => el.id).join(',')")
            .await
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "needs chromium"]
    async fn dry_run_with_a_missing_answer_fills_the_rest_and_flags_what_is_left() {
        let session = Session::start().await;
        let result = session
            .fill("extra_required", &full_bank(), SubmitMode::DryRun)
            .await;
        assert_eq!(result.status, SubmitStatus::NeedsGonza);
        let reason = result.reason.unwrap();
        assert!(
            reason.starts_with("faltan respuestas: Favorite programming paradigm"),
            "reason was: {reason}"
        );
        // The screenshot is of the partly filled form, not of an empty one.
        assert!(result.screenshot.unwrap().exists());
        assert_eq!(session.value("#first_name").await, "Jane");
        assert_eq!(session.value("#email").await, "search@example.com");
        assert_eq!(session.value("#country").await, "cl");
        let uploaded = session
            .page
            .locator("#filename")
            .text_content()
            .await
            .unwrap()
            .unwrap_or_default();
        assert!(uploaded.contains("cv.pdf"), "uploaded was: {uploaded}");
        // What only Gonza can answer is untouched, flagged, and nothing was sent.
        assert_eq!(session.value("#paradigm").await, "");
        assert_eq!(flagged_ids(&session).await, "paradigm");
        assert_eq!(session.eval("String(window.clicks || 0)").await, "0");
        assert_eq!(
            session.eval("String(Boolean(window.submitted))").await,
            "false"
        );
        session.stop().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "needs chromium"]
    async fn a_field_that_fails_does_not_hide_the_ones_after_it() {
        let session = Session::start().await;
        let mut bank = full_bank();
        // The form has no "Atlantis" option, the country is a required question here and it comes
        // before the email and the CV.
        bank.entries[4] = AnswerEntry::new(&["country"], Some("Atlantis"));
        for mode in [SubmitMode::DryRun, SubmitMode::Live] {
            let result = session.fill("required_country", &bank, mode).await;
            assert_eq!(result.status, SubmitStatus::NeedsGonza, "{mode:?}");
            assert_eq!(
                result.reason.as_deref(),
                Some("opción no encontrada: Country * = Atlantis"),
                "{mode:?}"
            );
            assert!(result.screenshot.unwrap().exists(), "{mode:?}");
            assert_eq!(
                session.value("#email").await,
                "search@example.com",
                "{mode:?}"
            );
            let uploaded = session
                .page
                .locator("#filename")
                .text_content()
                .await
                .unwrap()
                .unwrap_or_default();
            assert!(uploaded.contains("cv.pdf"), "{mode:?}: {uploaded}");
            assert_eq!(flagged_ids(&session).await, "country", "{mode:?}");
            assert_eq!(
                session.eval("String(window.clicks || 0)").await,
                "0",
                "{mode:?}"
            );
        }
        session.stop().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "needs chromium"]
    async fn live_never_clicks_submit_while_a_required_answer_is_missing() {
        let session = Session::start().await;
        let mut no_email = full_bank();
        no_email.entries[2] = AnswerEntry::new(&["email"], None);
        for (variant, bank) in [
            ("extra_required", full_bank()),
            ("consent", full_bank()),
            ("default", no_email),
        ] {
            let result = session.fill(variant, &bank, SubmitMode::Live).await;
            assert_eq!(result.status, SubmitStatus::NeedsGonza, "{variant}");
            assert!(
                result
                    .reason
                    .as_deref()
                    .unwrap_or("")
                    .starts_with("faltan respuestas: "),
                "{variant}: {:?}",
                result.reason
            );
            assert!(result.screenshot.unwrap().exists(), "{variant}");
            // The button was never even clicked, so native validation had nothing to block, and a
            // form that will not be sent is not typed into either.
            assert_eq!(
                session.eval("String(window.clicks || 0)").await,
                "0",
                "{variant}"
            );
            assert_eq!(
                session.eval("String(Boolean(window.submitted))").await,
                "false",
                "{variant}"
            );
            assert_eq!(session.value("#first_name").await, "", "{variant}");
        }
        session.stop().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "needs chromium"]
    async fn assist_leaves_the_unanswerable_question_to_gonza_and_waits_for_his_click() {
        let session = Session::start().await;
        // The fixture plays Gonza: it answers the essay question and clicks submit by itself.
        let result = session
            .fill("assist_finishes", &full_bank(), SubmitMode::Assist)
            .await;
        assert_eq!(
            result.status,
            SubmitStatus::Submitted,
            "{:?}",
            result.reason
        );
        // What the form looked like when Gonza took over: the question untouched, flagged and
        // scrolled into view; flagged no more once he answered it.
        assert_eq!(session.eval("window.review.essayBefore").await, "");
        assert_eq!(session.eval("String(window.review.flagged)").await, "1");
        assert_eq!(
            session.eval("String(window.review.essayFlagged)").await,
            "true"
        );
        assert_eq!(
            session.eval("String(window.review.essayInView)").await,
            "true"
        );
        assert_eq!(
            session
                .eval("String(window.review.flaggedAfterAnswer)")
                .await,
            "0"
        );
        // Everything the bank could answer was filled, his answer is his own, and he clicked.
        assert_eq!(session.value("#first_name").await, "Jane");
        assert_eq!(session.value("#email").await, "search@example.com");
        assert_eq!(session.value("#essay").await, "Because I build agents.");
        assert_eq!(
            session.eval("String(Boolean(window.submitted))").await,
            "true"
        );
        session.stop().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "needs chromium"]
    async fn closing_the_tab_in_assist_mode_skips_the_form() {
        let session = Session::start().await;
        // Gonza decides not to answer this one and closes the tab.
        let page = session.page.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(3)).await;
            let _ = page.close().await;
        });
        // Without the skip the program would wait the full ten minutes for a click.
        let result = tokio::time::timeout(
            Duration::from_secs(60),
            session.fill("extra_required", &full_bank(), SubmitMode::Assist),
        )
        .await
        .expect("a closed tab ends the wait");
        assert_eq!(result.status, SubmitStatus::NeedsGonza);
        let reason = result.reason.unwrap();
        assert!(
            reason.starts_with("assist_closed: ")
                && reason.contains("faltan respuestas: Favorite programming paradigm"),
            "reason was: {reason}"
        );
        session.stop().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "needs chromium"]
    async fn assist_keeps_the_browser_open_when_a_field_fails() {
        let session = Session::start().await;
        let mut bank = full_bank();
        bank.entries[4] = AnswerEntry::new(&["country"], Some("Atlantis"));
        // The fixture clicks submit by itself 2.5 s after the CV upload; the country (a required
        // question here) fails first, and the old flow gave up right there, before the CV was even
        // attached.
        let result = session
            .fill("auto_submit_required_country", &bank, SubmitMode::Assist)
            .await;
        assert_eq!(
            result.status,
            SubmitStatus::Submitted,
            "{:?}",
            result.reason
        );
        assert_eq!(
            session.eval("String(Boolean(window.submitted))").await,
            "true"
        );
        session.stop().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "needs chromium"]
    async fn captcha_widgets_and_verification_codes_go_to_gonza() {
        let session = Session::start().await;
        for variant in ["recaptcha", "hcaptcha", "code"] {
            let result = session.fill(variant, &full_bank(), SubmitMode::Live).await;
            assert_eq!(result.status, SubmitStatus::NeedsGonza, "{variant}");
            assert_eq!(result.reason.as_deref(), Some("captcha"), "{variant}");
            assert!(result.screenshot.unwrap().exists(), "{variant}");
        }
        session.stop().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "needs chromium"]
    async fn captcha_that_appears_while_filling_stops_the_live_submit() {
        let session = Session::start().await;
        let result = session
            .fill("captcha_after_fill", &full_bank(), SubmitMode::Live)
            .await;
        assert_eq!(result.status, SubmitStatus::NeedsGonza);
        assert_eq!(result.reason.as_deref(), Some("captcha"));
        assert_eq!(
            session.eval("String(Boolean(window.submitted))").await,
            "false"
        );
        session.stop().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "needs chromium"]
    async fn login_walls_blocked_pages_and_missing_forms_go_to_gonza() {
        let session = Session::start().await;
        for (variant, reason) in [
            ("login", "login_required"),
            ("blocked", "blocked_page"),
            ("no_form", "no_form_found"),
        ] {
            let result = session.fill(variant, &full_bank(), SubmitMode::Live).await;
            assert_eq!(result.status, SubmitStatus::NeedsGonza, "{variant}");
            assert_eq!(result.reason.as_deref(), Some(reason), "{variant}");
        }
        session.stop().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "needs chromium"]
    async fn manual_only_host_is_never_opened() {
        let session = Session::start().await;
        let result = session
            .fill_url(
                "https://www.linkedin.com/jobs/view/1",
                &full_bank(),
                SubmitMode::Live,
            )
            .await;
        assert_eq!(result.status, SubmitStatus::NeedsGonza);
        assert_eq!(result.reason.as_deref(), Some("manual_only_platform"));
        assert!(result.screenshot.is_none());
        assert_eq!(session.eval("location.href").await, "about:blank");
        session.stop().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "needs chromium"]
    async fn http_error_page_goes_to_gonza() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(403).set_body_string("Access denied"))
            .mount(&server)
            .await;
        let session = Session::start().await;
        let result = session
            .fill_url(
                &format!("{}/apply", server.uri()),
                &full_bank(),
                SubmitMode::Live,
            )
            .await;
        assert_eq!(result.status, SubmitStatus::NeedsGonza);
        assert_eq!(result.reason.as_deref(), Some("page_error: HTTP 403"));
        session.stop().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "needs chromium"]
    async fn browser_filler_runs_a_dry_run_end_to_end() {
        let dir = tempfile::tempdir().unwrap();
        let cv = dir.path().join("cv.pdf");
        std::fs::write(&cv, b"%PDF-1.4 fake").unwrap();
        let mut filler = BrowserFiller::launch(&dir.path().join("profile"), false)
            .await
            .unwrap();
        filler.nav = NavPolicy::LOCAL_TESTS; // the fixtures are served from this machine
        let job = make_job_with(|j| j.apply_url = fixture_url("default"));
        let result = filler
            .submit(
                &job,
                &cv,
                &full_bank(),
                SubmitMode::DryRun,
                &dir.path().join("shots"),
            )
            .await
            .unwrap();
        assert_eq!(result.status, SubmitStatus::DryRun);
        assert!(result.screenshot.unwrap().exists());
        filler.close().await.unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "needs chromium"]
    async fn spa_form_that_renders_late_is_filled() {
        let session = Session::start().await;
        let result = session.fill("spa", &full_bank(), SubmitMode::DryRun).await;
        assert_eq!(result.status, SubmitStatus::DryRun, "{:?}", result.reason);
        assert_eq!(session.value("#first_name").await, "Jane");
        session.stop().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "needs chromium"]
    async fn invisible_recaptcha_badge_is_not_a_wall() {
        let session = Session::start().await;
        let result = session
            .fill("invisible_captcha", &full_bank(), SubmitMode::DryRun)
            .await;
        assert_eq!(result.status, SubmitStatus::DryRun, "{:?}", result.reason);
        session.stop().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "needs chromium"]
    async fn radio_groups_are_answered_from_the_bank() {
        let session = Session::start().await;
        for (variant, name) in [("radio", "sponsor"), ("radio_lever", "sponsor2")] {
            let result = session
                .fill(variant, &full_bank(), SubmitMode::DryRun)
                .await;
            assert_eq!(
                result.status,
                SubmitStatus::DryRun,
                "{variant}: {:?}",
                result.reason
            );
            let checked = session
                .eval(&format!(
                    "document.querySelector('input[name={name}]:checked').value"
                ))
                .await;
            assert_eq!(checked, "no", "{variant}");
        }
        session.stop().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "needs chromium"]
    async fn consent_checkbox_follows_the_allow_list() {
        let session = Session::start().await;
        // Shipped default: no phrase is allowed, so the required checkbox is a missing answer.
        let result = session
            .fill("consent", &full_bank(), SubmitMode::DryRun)
            .await;
        assert_eq!(result.status, SubmitStatus::NeedsGonza);
        assert_eq!(
            result.reason.as_deref(),
            Some("faltan respuestas: I have read and accept the Privacy Policy *")
        );
        // The rest of the form is filled and the checkbox is left for Gonza to tick himself.
        assert_eq!(session.value("#first_name").await, "Jane");
        assert_eq!(
            session
                .eval("String(document.getElementById('privacy').checked)")
                .await,
            "false"
        );
        assert_eq!(flagged_ids(&session).await, "privacy");
        // With Gonza's phrase it is ticked.
        let allowed = AnswerBank {
            consent_phrases: vec!["privacy policy".to_string()],
            ..full_bank()
        };
        let result = session.fill("consent", &allowed, SubmitMode::DryRun).await;
        assert_eq!(result.status, SubmitStatus::DryRun, "{:?}", result.reason);
        assert_eq!(
            session
                .eval("String(document.getElementById('privacy').checked)")
                .await,
            "true"
        );
        session.stop().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "needs chromium"]
    async fn select_falls_back_to_an_alternative_option() {
        let session = Session::start().await;
        let result = session
            .fill("select_alt", &full_bank(), SubmitMode::DryRun)
            .await;
        assert_eq!(result.status, SubmitStatus::DryRun, "{:?}", result.reason);
        // "Company careers page" is not an option on this form; the first alternative is.
        assert_eq!(session.value("#source").await, "web");
        session.stop().await;
    }

    /// The text a custom drop-down (tests/fixtures/ats_dropdowns.html) shows as its value.
    async fn dropdown_value(session: &Session, id: &str) -> String {
        session
            .eval(&format!(
                r#"(() => {{ const v = document.getElementById('{id}').closest('.select').querySelector('.select__single-value'); return v ? v.textContent : '(nothing)'; }})()"#
            ))
            .await
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "needs chromium"]
    async fn custom_dropdowns_are_picked_by_option_text_not_typed_into() {
        let session = Session::start().await;
        let path = Paths::new(env!("CARGO_MANIFEST_DIR"))
            .root
            .join("tests")
            .join("fixtures")
            .join("ats_dropdowns.html");
        let url = format!("file:///{}", path.display().to_string().replace('\\', "/"));
        session.page.goto(&url, None).await.unwrap();
        let bank = AnswerBank {
            entries: vec![
                AnswerEntry::new(&["work location in the united states"], Some("No")),
                AnswerEntry::new(&["=country"], Some("Chile")),
                AnswerEntry::new(&["=location city"], Some("Santiago, Chile")),
                AnswerEntry::new(&["=school"], Some("Example Professional Institute"))
                    .with_alternatives(&["Example Institute", "Example", "Other"]),
                AnswerEntry::new(&["=race"], Some("White")).with_alternatives(&["Caucasian"]),
                AnswerEntry::new(&["=ethnicity"], Some("White")),
                AnswerEntry::new(&["how did you hear"], Some("Company careers page"))
                    .with_alternatives(&["Other"]),
            ],
            consent_phrases: vec![],
        };
        let fields = scan_fields(&session.page).await.unwrap();
        let plan = plan_fields(&fields, &bank, &session.cv());
        assert!(plan.missing.is_empty(), "{:?}", plan.missing);
        let mut failed = Vec::new();
        for action in &plan.actions {
            if let Err(err) = apply_action(&session.page, action).await {
                failed.push(action.label.clone());
                eprintln!("{} failed: {}", action.label, first_line(&err));
            }
        }
        // A yes/no list takes the exact option; "Chile" wins over "Chile Islands"; the school list
        // is fetched while typing, so the filler waits for it and falls back to "Other", and "Other"
        // wins over "Otherton College".
        assert_eq!(dropdown_value(&session, "q_us").await, "No");
        assert_eq!(dropdown_value(&session, "country").await, "Chile");
        assert_eq!(dropdown_value(&session, "school").await, "Other");
        // The place list is searched with "Santiago" and the option that also says Chile is picked.
        assert_eq!(
            dropdown_value(&session, "city").await,
            "Santiago, Metropolitana, Chile"
        );
        // Both race lists also offer "Hispanic or Latino", so in neither would "White" be only a
        // race: picking it answers the question Gonza keeps for himself. Nothing is picked and the
        // fields are reported as failed (they are required), so the application goes to Gonza.
        assert_eq!(dropdown_value(&session, "race").await, "(nothing)");
        assert_eq!(dropdown_value(&session, "ethnicity").await, "(nothing)");
        assert_eq!(failed, vec!["Race*", "Ethnicity*"]);
        // A plain text input still takes the first candidate typed.
        assert_eq!(session.value("#source").await, "Company careers page");
        session.stop().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "needs chromium"]
    async fn validation_error_after_click_is_unconfirmed() {
        let session = Session::start().await;
        let result = session
            .fill("rejects", &full_bank(), SubmitMode::Live)
            .await;
        assert_eq!(result.status, SubmitStatus::NeedsGonza);
        let reason = result.reason.unwrap();
        assert!(
            reason.starts_with("unconfirmed_submit: Phone number is invalid"),
            "reason was: {reason}"
        );
        let shot = result.screenshot.unwrap();
        assert!(
            shot.file_name()
                .unwrap()
                .to_string_lossy()
                .ends_with("-after.png")
        );
        assert_eq!(
            session.eval("String(Boolean(window.submitted))").await,
            "false"
        );
        session.stop().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "needs chromium"]
    async fn silent_submit_is_unconfirmed() {
        let session = Session::start().await;
        let result = session.fill("silent", &full_bank(), SubmitMode::Live).await;
        assert_eq!(result.status, SubmitStatus::NeedsGonza);
        assert_eq!(
            result.reason.as_deref(),
            Some("unconfirmed_submit: sin confirmación visible")
        );
        session.stop().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "needs chromium"]
    async fn redirect_to_a_thanks_url_confirms() {
        let session = Session::start().await;
        let result = session
            .fill("redirect", &full_bank(), SubmitMode::Live)
            .await;
        assert_eq!(
            result.status,
            SubmitStatus::Submitted,
            "{:?}",
            result.reason
        );
        assert!(
            session
                .eval("location.href")
                .await
                .ends_with("variant=thanks")
        );
        session.stop().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "needs chromium"]
    async fn assist_mode_waits_for_the_human_click() {
        let session = Session::start().await;
        // The fixture clicks submit by itself 2.5 s after the CV upload, standing in for Gonza.
        let result = session
            .fill("auto_submit", &full_bank(), SubmitMode::Assist)
            .await;
        assert_eq!(
            result.status,
            SubmitStatus::Submitted,
            "{:?}",
            result.reason
        );
        assert_eq!(
            session.eval("String(Boolean(window.submitted))").await,
            "true"
        );
        session.stop().await;
    }

    /// Manual check against real forms (Step 18b). `JOBBOT_REAL_URLS` holds comma-separated
    /// application URLs; each one is filled in a visible browser as a dry run and the outcome is
    /// printed. Without the variable the test only says it was skipped, so `-- --ignored` stays green.
    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "needs chromium and JOBBOT_REAL_URLS"]
    async fn real_sites_dry_run() {
        let Ok(urls) = std::env::var("JOBBOT_REAL_URLS") else {
            eprintln!("real_sites_dry_run: JOBBOT_REAL_URLS is not set, nothing to check");
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let cv = dir.path().join("cv.pdf");
        std::fs::write(&cv, b"%PDF-1.4 fake").unwrap();
        let shots = Paths::new(env!("CARGO_MANIFEST_DIR"))
            .root
            .join("target")
            .join("real-shots");
        let filler = BrowserFiller::launch(&dir.path().join("profile"), true)
            .await
            .unwrap();
        for url in urls.split(',').map(str::trim).filter(|url| !url.is_empty()) {
            let job = make_job_with(|j| j.apply_url = url.to_string());
            let result = filler
                .submit(&job, &cv, &full_bank(), SubmitMode::DryRun, &shots)
                .await
                .unwrap();
            eprintln!(
                "{url}\n  status: {:?}\n  reason: {}\n  screenshot: {}",
                result.status,
                result.reason.as_deref().unwrap_or("-"),
                result
                    .screenshot
                    .as_deref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default()
            );
            assert_ne!(
                result.status,
                SubmitStatus::Submitted,
                "a dry run must never submit"
            );
        }
        filler.close().await.unwrap();
    }

    // ---- Final review fixes (browser tests) ----

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "needs chromium"]
    async fn a_page_that_sends_the_browser_to_a_manual_only_platform_is_never_filled() {
        use playwright_rs::FulfillOptions;

        let playwright = Playwright::launch().await.unwrap();
        let browser = playwright
            .chromium()
            .launch_with_options(crate::launch::launch_options())
            .await
            .unwrap();
        let page = browser.new_page().await.unwrap();

        // Nothing in this test may reach the internet. The catch-all route is registered first (so
        // it has the lowest priority): it lets the local server through and aborts everything else.
        page.route("**/*", |route| async move {
            if route.request().url().starts_with("http://127.0.0.1") {
                route.continue_(None).await
            } else {
                route.abort(None).await
            }
        })
        .await
        .unwrap();
        // LinkedIn is a stub served by the test.
        page.route("https://www.linkedin.com/**", |route| async move {
            let form = r#"<form><label for="fn">First name *</label><input id="fn" name="fn" required>
                <label for="cv">Resume/CV *</label><input id="cv" type="file" required>
                <button type="submit">Submit application</button></form>"#;
            route
                .fulfill(
                    FulfillOptions::builder()
                        .status(200)
                        .content_type("text/html")
                        .body_string(form)
                        .build(),
                )
                .await
        })
        .await
        .unwrap();

        // The first address looks harmless (a job board's own link) and sends the browser on to
        // LinkedIn, like RemoteOK's `/l/<id>` redirect links do.
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                "<script>location.href = 'https://www.linkedin.com/jobs/view/1'</script>",
                "text/html",
            ))
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().unwrap();
        let cv = dir.path().join("cv.pdf");
        std::fs::write(&cv, b"%PDF-1.4 fake").unwrap();
        let job = make_job_with(|j| j.apply_url = format!("{}/l/1", server.uri()));
        let result = fill_application(
            &page,
            &job,
            &cv,
            &full_bank(),
            SubmitMode::DryRun,
            &dir.path().join("shots"),
            NavPolicy::LOCAL_TESTS,
        )
        .await
        .unwrap();
        assert_eq!(
            result.status,
            SubmitStatus::NeedsGonza,
            "{:?}",
            result.reason
        );
        assert_eq!(result.reason.as_deref(), Some("manual_only_platform"));
        let typed = page.locator("#fn").input_value(None).await.unwrap();
        assert_eq!(typed, "", "nothing is typed into the page it landed on");
        browser.close().await.unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "needs chromium"]
    async fn a_link_to_a_private_address_is_never_opened() {
        let session = Session::start().await;
        let job =
            make_job_with(|j| j.apply_url = "https://169.254.169.254/latest/meta-data/".into());
        let result = fill_application(
            &session.page,
            &job,
            &session.cv(),
            &full_bank(),
            SubmitMode::Live,
            &session.shots(),
            NavPolicy::STRICT,
        )
        .await
        .unwrap();
        assert_eq!(
            result.status,
            SubmitStatus::NeedsGonza,
            "{:?}",
            result.reason
        );
        assert!(result.reason.as_deref().unwrap().starts_with("unsafe_url:"));
        assert_eq!(session.page.url(), "about:blank", "the browser never moved");
        session.stop().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "needs chromium"]
    async fn a_redirect_to_a_private_address_is_never_filled() {
        use playwright_rs::FulfillOptions;

        let session = Session::start().await;
        // Nothing here may reach the network: the catch-all (registered first, lowest priority)
        // lets the local server through and aborts everything else.
        session
            .page
            .route("**/*", |route| async move {
                if route.request().url().starts_with("http://127.0.0.1") {
                    route.continue_(None).await
                } else {
                    route.abort(None).await
                }
            })
            .await
            .unwrap();
        // The private address answers with a form that would be filled if the guard did not exist.
        session
            .page
            .route("https://169.254.169.254/**", |route| async move {
                let form = r#"<form><label for="fn">First name *</label><input id="fn" name="fn" required>
                    <label for="cv">Resume/CV *</label><input id="cv" type="file" required>
                    <button type="submit">Submit application</button></form>"#;
                route
                    .fulfill(
                        FulfillOptions::builder()
                            .status(200)
                            .content_type("text/html")
                            .body_string(form)
                            .build(),
                    )
                    .await
            })
            .await
            .unwrap();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                "<script>location.href = 'https://169.254.169.254/latest/meta-data/'</script>",
                "text/html",
            ))
            .mount(&server)
            .await;
        let job = make_job_with(|j| j.apply_url = format!("{}/l/1", server.uri()));
        let result = fill_application(
            &session.page,
            &job,
            &session.cv(),
            &full_bank(),
            SubmitMode::DryRun,
            &session.shots(),
            NavPolicy::LOCAL_TESTS,
        )
        .await
        .unwrap();
        assert_eq!(
            result.status,
            SubmitStatus::NeedsGonza,
            "{:?}",
            result.reason
        );
        assert!(
            result.reason.as_deref().unwrap().starts_with("unsafe_url:"),
            "{:?}",
            result.reason
        );
        let typed = session.page.locator("#fn").input_value(None).await.unwrap();
        assert_eq!(typed, "", "nothing is typed into the page it landed on");
        session.stop().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "needs chromium"]
    async fn a_page_that_sends_the_browser_to_ashby_is_never_assisted_or_sent() {
        use playwright_rs::FulfillOptions;

        let playwright = Playwright::launch().await.unwrap();
        let browser = playwright
            .chromium()
            .launch_with_options(crate::launch::launch_options())
            .await
            .unwrap();
        let page = browser.new_page().await.unwrap();
        page.route("**/*", |route| async move {
            if route.request().url().starts_with("http://127.0.0.1") {
                route.continue_(None).await
            } else {
                route.abort(None).await
            }
        })
        .await
        .unwrap();
        // Ashby is a stub served by the test: nothing here reaches the internet.
        page.route("https://jobs.ashbyhq.com/**", |route| async move {
            let form = r#"<form><label for="fn">First name *</label><input id="fn" name="fn" required>
                <label for="cv">Resume/CV *</label><input id="cv" type="file" required>
                <button type="submit">Submit application</button></form>"#;
            route
                .fulfill(
                    FulfillOptions::builder()
                        .status(200)
                        .content_type("text/html")
                        .body_string(form)
                        .build(),
                )
                .await
        })
        .await
        .unwrap();
        // A harmless-looking board link that sends the browser on to an Ashby form.
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                "<script>location.href = 'https://jobs.ashbyhq.com/acme/0b1c2d3e/application'</script>",
                "text/html",
            ))
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().unwrap();
        let cv = dir.path().join("cv.pdf");
        std::fs::write(&cv, b"%PDF-1.4 fake").unwrap();
        let job = make_job_with(|j| j.apply_url = format!("{}/l/1", server.uri()));
        for mode in [SubmitMode::Live, SubmitMode::Assist] {
            let result = fill_application(
                &page,
                &job,
                &cv,
                &full_bank(),
                mode,
                &dir.path().join("shots"),
                NavPolicy::LOCAL_TESTS,
            )
            .await
            .unwrap();
            assert_eq!(
                result.status,
                SubmitStatus::NeedsGonza,
                "{mode:?}: {:?}",
                result.reason
            );
            assert_eq!(result.reason.as_deref(), Some("handoff_host"), "{mode:?}");
            let typed = page.locator("#fn").input_value(None).await.unwrap();
            assert_eq!(typed, "", "{mode:?}: nothing is typed into the Ashby form");
        }
        browser.close().await.unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "needs chromium"]
    async fn a_form_with_nowhere_to_attach_the_cv_is_neither_filled_nor_clicked() {
        let session = Session::start().await;
        for mode in [SubmitMode::DryRun, SubmitMode::Live] {
            let result = session.fill("no_cv_field", &full_bank(), mode).await;
            assert_eq!(
                result.status,
                SubmitStatus::NeedsGonza,
                "{mode:?}: {:?}",
                result.reason
            );
            assert!(
                result
                    .reason
                    .as_deref()
                    .unwrap_or("")
                    .starts_with("sin campo de CV"),
                "{mode:?}: {:?}",
                result.reason
            );
            assert_eq!(
                session.eval("String(Boolean(window.submitted))").await,
                "false",
                "{mode:?}"
            );
            assert_eq!(
                session.value("#first_name").await,
                "",
                "{mode:?}: nothing was typed"
            );
        }
        session.stop().await;
    }

    // ---- Review fixes (browser tests) ----

    /// The drop-down fixture with the set of questions `set` (see tests/fixtures/ats_dropdowns.html).
    fn dropdowns_url(set: &str) -> String {
        let path = Paths::new(env!("CARGO_MANIFEST_DIR"))
            .root
            .join("tests")
            .join("fixtures")
            .join("ats_dropdowns.html");
        format!(
            "file:///{}?set={set}",
            path.display().to_string().replace('\\', "/")
        )
    }

    fn dropdowns_bank() -> AnswerBank {
        AnswerBank {
            entries: vec![
                AnswerEntry::new(&["how did you hear"], Some("Company careers page")),
                AnswerEntry::new(
                    &["relocation"],
                    Some("Prefer remote; open to discussing it"),
                ),
                AnswerEntry::new(&["=race ethnicity"], Some("White")),
                AnswerEntry::new(&["work location in the united states"], Some("No")),
                AnswerEntry::new(&["=graduate school"], Some("Slow University")),
            ],
            consent_phrases: vec![],
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "needs chromium"]
    async fn an_optional_dropdown_the_answer_does_not_fit_is_skipped_not_failed() {
        let session = Session::start().await;
        // Prose for a Yes/No list, a race whose only White says "(Not Hispanic or Latino)", and a
        // bare "No" between two options that both start with it: all optional, none of them a
        // reason to hold the whole application.
        let result = session
            .fill_url(
                &dropdowns_url("optional"),
                &dropdowns_bank(),
                SubmitMode::DryRun,
            )
            .await;
        assert_eq!(result.status, SubmitStatus::DryRun, "{:?}", result.reason);
        assert_eq!(result.reason, None);
        for id in ["q_reloc", "q_race_opt", "q_us_opt"] {
            assert_eq!(dropdown_value(&session, id).await, "(nothing)", "{id}");
        }
        assert_eq!(flagged_ids(&session).await, "");
        session.stop().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "needs chromium"]
    async fn a_required_dropdown_the_answer_does_not_fit_still_fails_loudly() {
        let session = Session::start().await;
        let result = session
            .fill_url(
                &dropdowns_url("required_prose"),
                &dropdowns_bank(),
                SubmitMode::DryRun,
            )
            .await;
        assert_eq!(result.status, SubmitStatus::NeedsGonza);
        let reason = result.reason.unwrap();
        assert!(
            reason.contains("Are you open to relocation")
                && reason.contains("no option of the drop-down matches"),
            "reason was: {reason}"
        );
        assert_eq!(dropdown_value(&session, "q_reloc_req").await, "(nothing)");
        assert_eq!(flagged_ids(&session).await, "q_reloc_req");
        session.stop().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "needs chromium"]
    async fn a_required_race_list_that_offers_hispanic_goes_to_gonza_with_its_own_reason() {
        let session = Session::start().await;
        let bank = AnswerBank {
            entries: vec![AnswerEntry::new(&["=race"], Some("White"))],
            consent_phrases: vec![],
        };
        let result = session
            .fill_url(&dropdowns_url("base"), &bank, SubmitMode::DryRun)
            .await;
        assert_eq!(result.status, SubmitStatus::NeedsGonza);
        let reason = result.reason.unwrap();
        assert!(
            reason.contains("pregunta reservada para ti")
                && reason.contains("Hispanic o Latino")
                && reason.contains("Race"),
            "reason was: {reason}"
        );
        assert_eq!(dropdown_value(&session, "race").await, "(nothing)");
        session.stop().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "needs chromium"]
    async fn a_typeahead_that_answers_late_is_waited_for() {
        let session = Session::start().await;
        session
            .page
            .goto(&dropdowns_url("slow"), None)
            .await
            .unwrap();
        let fields = scan_fields(&session.page).await.unwrap();
        let plan = plan_fields(&fields, &dropdowns_bank(), &session.cv());
        let grad: Vec<&FillAction> = plan
            .actions
            .iter()
            .filter(|action| action.label.starts_with("Graduate school"))
            .collect();
        assert_eq!(grad.len(), 1, "{:?}", plan.actions);
        // The list shows nothing for 1.5 s after the last key, longer than the 0.9 s a list may
        // go on changing: an empty list is not "no options".
        apply_action(&session.page, grad[0]).await.unwrap();
        assert_eq!(dropdown_value(&session, "grad").await, "Slow University");
        session.stop().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "needs chromium"]
    async fn a_native_select_matches_its_option_like_every_other_list() {
        let session = Session::start().await;
        // "Job board" in the bank, "Job Board" on the page: the exact, case-sensitive label of the
        // old select rule found nothing here, while a drop-down or a radio group would have.
        let bank = AnswerBank {
            entries: vec![
                AnswerEntry::new(&["how did you hear"], Some("Job board"))
                    .with_alternatives(&["Other"]),
                AnswerEntry::new(&["first name"], Some("Jane")),
                AnswerEntry::new(&["last name"], Some("Doe Garcia")),
                AnswerEntry::new(&["email"], Some("search@example.com")),
            ],
            consent_phrases: vec![],
        };
        let result = session.fill("select_case", &bank, SubmitMode::DryRun).await;
        assert_eq!(result.status, SubmitStatus::DryRun, "{:?}", result.reason);
        assert_eq!(session.value("#source").await, "board");
        session.stop().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "needs chromium"]
    async fn an_optional_select_with_no_matching_option_is_skipped_and_the_form_goes_on() {
        let session = Session::start().await;
        let mut bank = full_bank();
        bank.entries[4] = AnswerEntry::new(&["country"], Some("Atlantis"));
        // The country is optional here: Atlantis is no option, so it stays empty and the rest of
        // the form is filled and sent.
        let result = session.fill("default", &bank, SubmitMode::Live).await;
        assert_eq!(
            result.status,
            SubmitStatus::Submitted,
            "{:?}",
            result.reason
        );
        assert_eq!(session.value("#country").await, "");
        assert_eq!(session.value("#email").await, "search@example.com");
        session.stop().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "needs chromium"]
    async fn a_pre_ticked_marketing_box_stops_live_and_is_flagged_until_he_unticks_it() {
        let session = Session::start().await;
        for mode in [SubmitMode::DryRun, SubmitMode::Live] {
            let result = session.fill("prechecked", &full_bank(), mode).await;
            assert_eq!(result.status, SubmitStatus::NeedsGonza, "{mode:?}");
            let reason = result.reason.unwrap();
            assert!(
                reason.starts_with("faltan respuestas: ")
                    && reason.contains("marketing emails and SMS alerts")
                    && reason.contains("casilla ya marcada"),
                "{mode:?}: {reason}"
            );
            // Nothing was clicked, nothing sent, and the filler did not untick it.
            assert_eq!(
                session.eval("String(window.clicks || 0)").await,
                "0",
                "{mode:?}"
            );
            assert_eq!(
                session
                    .eval("String(document.getElementById('marketing').checked)")
                    .await,
                "true",
                "{mode:?}"
            );
        }
        // Flagged on the page, and the mark goes away once he unticks the box himself.
        let result = session
            .fill("prechecked", &full_bank(), SubmitMode::DryRun)
            .await;
        assert_eq!(result.status, SubmitStatus::NeedsGonza);
        assert_eq!(flagged_ids(&session).await, "marketing");
        session
            .page
            .locator("#marketing")
            .uncheck(None)
            .await
            .unwrap();
        assert_eq!(flagged_ids(&session).await, "");
        session.stop().await;
    }
}
