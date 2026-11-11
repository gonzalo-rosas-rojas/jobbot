//! Handoff, the pure part: which forms the bot fills but Gonza sends from his own browser (Ashby
//! flags a Playwright-driven Chromium as spam even when a human clicks Submit), the reasons that
//! say a form is his to send, and the answer sheet as data: every question with the answer the
//! bank gives, read with the same planner the filler uses. Writing the sheet is the application's.

use std::path::{Path, PathBuf};

use crate::answers::AnswerBank;
use crate::form::{FieldInfo, FillValue, SubmitResult, SubmitStatus, plan_fields};
use crate::models::Job;
use crate::text::{host_of, safe_key};

/// Hosts whose forms are filled by the bot but sent by Gonza from his own browser.
pub const HANDOFF_HOSTS: &[&str] = &["ashbyhq.com"];

/// The `needs_gonza` reason of a handed-off application always starts with this.
pub const HANDOFF_REASON_PREFIX: &str = "handoff:";

/// True when the URL's host is one of `HANDOFF_HOSTS` or a subdomain of one (same rule as the
/// gate's host lists: `notashbyhq.com` and `?ref=ashbyhq.com` do not match).
pub fn is_handoff(url: &str) -> bool {
    let host = host_of(url);
    HANDOFF_HOSTS
        .iter()
        .any(|handoff| host == *handoff || host.ends_with(&format!(".{handoff}")))
}

/// True when the form of `job` lives on a handoff host: its apply link or, like the gate's
/// manual-only check, its listing link.
pub fn is_handoff_job(job: &Job) -> bool {
    is_handoff(&job.apply_url) || is_handoff(&job.url)
}

/// True when `reason` is the `needs_gonza` reason of an application waiting for Gonza's own send.
pub fn is_handoff_reason(reason: &str) -> bool {
    reason.starts_with(HANDOFF_REASON_PREFIX)
}

/// True when `reason` says the form is Gonza's to send by hand: an Ashby handoff, or a posting
/// whose platform or listing the bot never opens (`manual_only_platform`, `listing_only`). Such a
/// row may already have gone out when he has not yet run `confirm`, so nothing withdraws it, and
/// confirming it proves nothing about the form filler.
pub fn is_sent_by_hand(reason: &str) -> bool {
    is_handoff_reason(reason)
        || reason.starts_with("manual_only_platform")
        || reason.starts_with("listing_only")
        || reason.starts_with(MANUAL_RECORD_PREFIX)
}

/// The reason `jobbot record` leaves between its two moves (`scored` to `needs_gonza` to
/// `submitted`): the form already went out by hand.
pub const MANUAL_RECORD_PREFIX: &str = "manual_record:";

/// What the bank gives for one question of the form.
#[derive(Debug, Clone, PartialEq)]
pub enum SheetAnswer {
    /// Text to type, exactly as the bank has it.
    Text(String),
    /// The option to pick in a drop-down or a group of buttons, and the others the bank accepts.
    Choice {
        pick: String,
        alternatives: Vec<String>,
    },
    /// The CV goes in this file input.
    Attach(PathBuf),
    /// A consent box the bank allows ticking.
    Tick,
    /// Required, and the bank has no answer: Gonza writes it.
    Missing,
    /// Optional and the bank has no answer: the bot leaves it blank.
    Skipped,
}

impl SheetAnswer {
    fn is_answered(&self) -> bool {
        !matches!(self, SheetAnswer::Missing | SheetAnswer::Skipped)
    }
}

/// One question of the form as the sheet shows it. A radio group or a group of checkboxes is one
/// question with several `options`.
#[derive(Debug, Clone, PartialEq)]
pub struct SheetField {
    pub label: String,
    pub required: bool,
    pub answer: SheetAnswer,
    pub options: Vec<String>,
}

impl SheetField {
    /// A required question the bank cannot answer, known only by its label.
    pub fn missing(label: &str) -> SheetField {
        SheetField {
            label: label.to_string(),
            required: true,
            answer: SheetAnswer::Missing,
            options: Vec::new(),
        }
    }
}

/// What a handoff run knows about the form it opened.
#[derive(Debug, Clone, PartialEq)]
pub enum FieldReport {
    /// Every question the form asked, in page order.
    Full(Vec<SheetField>),
    /// Only the required questions the bank could not answer: the filler could not list the rest.
    MissingOnly(Vec<SheetField>),
    /// The form could not be read (a wall, an error page, no form).
    Unreadable,
}

/// The two numbers the results log keeps about a form.
#[derive(Debug, Clone, PartialEq)]
pub struct FormTally {
    /// Questions the bank answered; `None` when the report cannot say.
    pub filled: Option<usize>,
    /// Labels of the required questions nobody answered.
    pub left: Vec<String>,
}

impl FieldReport {
    pub fn tally(&self) -> FormTally {
        let left_of = |fields: &[SheetField]| {
            fields
                .iter()
                .filter(|field| field.answer == SheetAnswer::Missing)
                .map(|field| field.label.clone())
                .collect()
        };
        match self {
            FieldReport::Full(fields) => FormTally {
                filled: Some(fields.iter().filter(|f| f.answer.is_answered()).count()),
                left: left_of(fields),
            },
            FieldReport::MissingOnly(fields) => FormTally {
                filled: None,
                left: left_of(fields),
            },
            FieldReport::Unreadable => FormTally {
                filled: None,
                left: Vec::new(),
            },
        }
    }
}

/// A radio button, or a checkbox that belongs to a group, shares its question with its siblings.
fn is_grouped(field: &FieldInfo) -> bool {
    field.kind == "radio" || (field.kind == "checkbox" && !field.option.is_empty())
}

/// What identifies the group of `field`: the shared name, or the question when there is none.
fn group_key(field: &FieldInfo) -> (&str, &str) {
    let key = if field.name.is_empty() {
        field.label.trim()
    } else {
        field.name.as_str()
    };
    (field.kind.as_str(), key)
}

fn label_of(field: &FieldInfo) -> String {
    match field.label.trim() {
        "" => "(campo sin etiqueta)".to_string(),
        label => label.to_string(),
    }
}

/// Lists every question of the scanned form with the answer the bank gives, using the same
/// planner the filler uses, so the sheet says exactly what the filler would have typed.
pub fn sheet_fields(fields: &[FieldInfo], bank: &AnswerBank, cv_pdf: &Path) -> Vec<SheetField> {
    let plan = plan_fields(fields, bank, cv_pdf);
    let action_for = |idx: usize| plan.actions.iter().find(|action| action.idx == idx);
    let mut sheet = Vec::new();
    let mut seen = Vec::new();
    for field in fields {
        if is_grouped(field) {
            let key = group_key(field);
            if seen.contains(&key) {
                continue;
            }
            seen.push(key);
            let members: Vec<&FieldInfo> = fields
                .iter()
                .filter(|member| is_grouped(member) && group_key(member) == key)
                .collect();
            let options: Vec<String> = members
                .iter()
                .map(|member| member.option.trim().to_string())
                .filter(|option| !option.is_empty())
                .collect();
            let picked: Vec<String> = members
                .iter()
                .filter(|member| {
                    action_for(member.idx).is_some() || plan.kept.contains(&member.idx)
                })
                .map(|member| member.option.trim().to_string())
                .collect();
            let required = members.iter().any(|member| member.required);
            let answer = if picked.is_empty() {
                unanswered(required)
            } else {
                SheetAnswer::Choice {
                    pick: picked.join(", "),
                    alternatives: Vec::new(),
                }
            };
            sheet.push(SheetField {
                label: label_of(field),
                required,
                answer,
                options,
            });
            continue;
        }
        let answer = match action_for(field.idx).map(|action| &action.value) {
            Some(FillValue::File(path)) => SheetAnswer::Attach(path.clone()),
            Some(FillValue::Select(candidates)) => SheetAnswer::Choice {
                pick: candidates.first().cloned().unwrap_or_default(),
                alternatives: candidates.iter().skip(1).cloned().collect(),
            },
            Some(FillValue::Text(text)) => SheetAnswer::Text(text.clone()),
            Some(FillValue::Check) => SheetAnswer::Tick,
            None => unanswered(field.required),
        };
        sheet.push(SheetField {
            label: label_of(field),
            required: field.required,
            answer,
            options: Vec::new(),
        });
    }
    sheet
}

fn unanswered(required: bool) -> SheetAnswer {
    if required {
        SheetAnswer::Missing
    } else {
        SheetAnswer::Skipped
    }
}

/// Where the sheet of an application goes: `<handoff_dir>/<key as a file name>.md`.
pub fn sheet_path(handoff_dir: &Path, job_key: &str) -> PathBuf {
    handoff_dir.join(format!("{}.md", safe_key(job_key)))
}

/// What a dry run of a handoff form found: the filler's own result (never `Submitted`) and what
/// the form asked.
#[derive(Debug, Clone)]
pub struct FormInspection {
    pub result: SubmitResult,
    pub report: FieldReport,
}

/// Reasons of a dry run that stopped with the form on screen, filled as far as it could be.
const FILLED_FORM_REASONS: &[&str] = &[
    "faltan respuestas",
    "opción no encontrada",
    "no se pudo completar",
];

/// Whether the page still shows the application form after `result`: a clean dry run, or one
/// that stopped for answers it lacks or a field that refused its value. A wall, an error page or
/// a missing form leaves something else on screen, and scanning it would list its login box.
pub fn page_shows_the_form(result: &SubmitResult) -> bool {
    match result.status {
        SubmitStatus::DryRun => true,
        SubmitStatus::NeedsGonza => result.reason.as_deref().is_some_and(|reason| {
            FILLED_FORM_REASONS
                .iter()
                .any(|prefix| reason.starts_with(prefix))
        }),
        SubmitStatus::Submitted => false,
    }
}

/// The labels listed after `faltan respuestas:` in a filler's reason, wherever that text sits
/// (an assisted form left open repeats it after "Quedaba pendiente:"). The failures that follow
/// the dash are not questions and are left out. Empty when the reason lists none.
pub fn missing_labels(reason: &str) -> Vec<String> {
    const MARKER: &str = "faltan respuestas:";
    let Some(start) = reason.find(MARKER) else {
        return Vec::new();
    };
    let listed = &reason[start + MARKER.len()..];
    let listed = listed.split(" — ").next().unwrap_or(listed);
    listed
        .split(';')
        .map(str::trim)
        .filter(|label| !label.is_empty())
        .map(str::to_string)
        .collect()
}

/// What can be said about the form from the filler's reply alone, for a filler that cannot list
/// the fields: the required questions it lacked answers for, or nothing when it never saw the form.
pub fn report_from_result(result: &SubmitResult) -> FieldReport {
    if !page_shows_the_form(result) {
        return FieldReport::Unreadable;
    }
    let labels = result
        .reason
        .as_deref()
        .map(missing_labels)
        .unwrap_or_default();
    FieldReport::MissingOnly(
        labels
            .iter()
            .map(|label| SheetField::missing(label))
            .collect(),
    )
}
