//! Handoff: forms the bot opens and fills for Gonza but never sends.
//!
//! Ashby flags a Playwright-driven Chromium as spam even when a human clicks Submit, so an
//! application to a handoff host is never sent from the bot's browser, in any mode. The bot does
//! what a dry run does (opens the form, fills what the answer bank knows, takes a screenshot),
//! writes an answer sheet Gonza can copy from (`data/handoff/<key>.md`) and parks the application
//! in `needs_gonza` with a reason that starts with `handoff:`. Gonza sends the form from his own
//! browser and records it with the usual `jobbot confirm KEY`.

use std::path::{Path, PathBuf};

use anyhow::Context;
use async_trait::async_trait;
use playwright_rs::{Browser, Page, Playwright};

use crate::answers::AnswerBank;
use crate::form::{
    FieldInfo, FillValue, SubmitMode, SubmitResult, SubmitStatus, fill_application, plan_fields,
};
use crate::models::Job;
use crate::navigation::NavPolicy;
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
pub(crate) fn sheet_fields(
    fields: &[FieldInfo],
    bank: &AnswerBank,
    cv_pdf: &Path,
) -> Vec<SheetField> {
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

/// A path as Gonza will need it in a file picker: absolute when that can be worked out.
fn shown(path: &Path) -> String {
    std::path::absolute(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .display()
        .to_string()
}

const MISSING_TEXT: &str = "⚠ OBLIGATORIA, contéstala tú";
const SKIPPED_TEXT: &str = "(opcional, sin respuesta)";

fn answer_lines(field: &SheetField) -> String {
    let single_or_block = |text: &str| {
        if text.contains('\n') {
            format!("**Respuesta:**\n\n{text}")
        } else {
            format!("**Respuesta:** {text}")
        }
    };
    match &field.answer {
        SheetAnswer::Text(text) => single_or_block(text),
        SheetAnswer::Choice { pick, alternatives } if alternatives.is_empty() => {
            single_or_block(pick)
        }
        SheetAnswer::Choice { pick, alternatives } => single_or_block(&format!(
            "{pick} (si no está esa opción: {})",
            alternatives.join(" / ")
        )),
        SheetAnswer::Attach(path) => {
            format!("**Respuesta:** adjuntar el CV: {}", shown(path))
        }
        SheetAnswer::Tick => {
            "**Respuesta:** marcar la casilla (la autorizaste en `consent_checkboxes`)".to_string()
        }
        SheetAnswer::Missing => format!("**Respuesta:** {MISSING_TEXT}"),
        SheetAnswer::Skipped => format!("**Respuesta:** {SKIPPED_TEXT}"),
    }
}

/// The answer sheet, in Spanish: the job, where to apply, the CV and one entry per question with
/// its label, whether it is required and the bank's answer word for word. `note` is what the bot
/// saw when the form did not fill cleanly (a filler's reason).
pub fn render_sheet(
    job: &Job,
    cv_pdf: &Path,
    report: &FieldReport,
    screenshot: Option<&Path>,
    note: Option<&str>,
) -> String {
    let key = job.key();
    let mut text = format!(
        "# Hoja de respuestas: {} — {}\n\n\
         Este formulario lo envías tú desde tu navegador normal: el sitio marca como spam el Chromium del bot, \
         incluso cuando el clic lo das tú. El bot lo abrió, llenó lo que el banco de respuestas sabe y guardó una captura.\n\n\
         - **Empresa:** {}\n\
         - **Cargo:** {}\n\
         - **Enlace para postular:** {}\n\
         - **CV (PDF):** {}\n",
        job.company,
        job.title,
        job.company,
        job.title,
        job.apply_url,
        shown(cv_pdf),
    );
    if let Some(shot) = screenshot {
        text.push_str(&format!("- **Captura del bot:** {}\n", shown(shot)));
    }
    text.push_str(&format!("- **Clave:** `{key}`\n"));
    if let Some(note) = note {
        text.push_str(&format!("- **El bot vio:** {note}\n"));
    }
    text.push_str(&format!(
        "\n## Cómo enviar\n\n\
         1. Abre el enlace en tu navegador normal.\n\
         2. Adjunta el CV de arriba y copia las respuestas de abajo tal cual.\n\
         3. Lo marcado con {MISSING_TEXT} lo contestas tú: el banco de respuestas no lo sabe.\n\
         4. Envía el formulario y registra el envío con `jobbot confirm {key}`.\n\n\
         ## Preguntas del formulario\n\n"
    ));
    match report {
        FieldReport::Full(fields) => push_fields(&mut text, fields),
        FieldReport::MissingOnly(fields) => {
            text.push_str(
                "Esta es una lista incompleta: el bot solo reportó las preguntas obligatorias que no sabe contestar. \
                 El resto del formulario lo ves tú en la página.\n\n",
            );
            if fields.is_empty() {
                text.push_str("Todas las preguntas obligatorias tienen respuesta en el banco.\n");
            }
            push_fields(&mut text, fields);
        }
        FieldReport::Unreadable => text.push_str(
            "El bot no pudo leer las preguntas del formulario. Abre el enlace y llénalo tú; \
             las respuestas que sabe el bot están en `profile/answers.toml`.\n",
        ),
    }
    text
}

fn push_fields(text: &mut String, fields: &[SheetField]) {
    for (number, field) in fields.iter().enumerate() {
        let need = if field.required {
            "obligatoria"
        } else {
            "opcional"
        };
        text.push_str(&format!("### {}. {} ({need})\n\n", number + 1, field.label));
        text.push_str(&answer_lines(field));
        text.push('\n');
        if !field.options.is_empty() {
            text.push_str(&format!("**Opciones:** {}\n", field.options.join(" / ")));
        }
        text.push('\n');
    }
}

/// Writes the sheet of `job` under `handoff_dir` (created when missing) and returns its path.
pub fn write_sheet(
    handoff_dir: &Path,
    job: &Job,
    cv_pdf: &Path,
    report: &FieldReport,
    screenshot: Option<&Path>,
    note: Option<&str>,
) -> anyhow::Result<PathBuf> {
    std::fs::create_dir_all(handoff_dir)
        .with_context(|| format!("creating {}", handoff_dir.display()))?;
    let path = sheet_path(handoff_dir, &job.key());
    std::fs::write(&path, render_sheet(job, cv_pdf, report, screenshot, note))
        .with_context(|| format!("writing {}", path.display()))?;
    Ok(path)
}

/// What a dry run of a handoff form found: the filler's own result (never `Submitted`) and what
/// the form asked.
#[derive(Debug, Clone)]
pub struct FormInspection {
    pub result: SubmitResult,
    pub report: FieldReport,
}

/// Opens a form like a dry run (fills what the bank answers, takes the screenshot, never clicks
/// anything) and also lists every question the form asked, which `FormFiller` does not report.
#[async_trait]
pub trait FormInspector: Send + Sync {
    async fn inspect(
        &self,
        job: &Job,
        cv_pdf: &Path,
        bank: &AnswerBank,
        shots_dir: &Path,
    ) -> anyhow::Result<FormInspection>;
}

/// The same scan the filler runs on the page (`FieldInfo` is its output).
const FIELD_JS: &str = include_str!("form/field_scan.js");

/// Reasons of a dry run that stopped with the form on screen, filled as far as it could be.
const FILLED_FORM_REASONS: &[&str] = &[
    "faltan respuestas",
    "opción no encontrada",
    "no se pudo completar",
];

/// Whether the page still shows the application form after `result`: a clean dry run, or one
/// that stopped for answers it lacks or a field that refused its value. A wall, an error page or
/// a missing form leaves something else on screen, and scanning it would list its login box.
fn page_shows_the_form(result: &SubmitResult) -> bool {
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

/// Dry-runs the form open in `page` and scans it again: the page keeps showing the filled form
/// after a dry run, so the scan sees every question the form asked.
async fn inspect_page(
    page: &Page,
    job: &Job,
    cv_pdf: &Path,
    bank: &AnswerBank,
    shots_dir: &Path,
    nav: NavPolicy,
) -> anyhow::Result<FormInspection> {
    let result =
        fill_application(page, job, cv_pdf, bank, SubmitMode::DryRun, shots_dir, nav).await?;
    let mut report = FieldReport::Unreadable;
    if page_shows_the_form(&result) {
        let scanned: Vec<FieldInfo> = page
            .evaluate(FIELD_JS, None::<&()>)
            .await
            .context("scanning the form fields")?;
        if !scanned.is_empty() {
            report = FieldReport::Full(sheet_fields(&scanned, bank, cv_pdf));
        }
    }
    Ok(FormInspection { result, report })
}

/// A headless Chromium of its own, with no profile: it only looks at the form and never sends it.
pub struct BrowserInspector {
    // Field order matters: the browser is dropped before the driver that owns it.
    browser: Browser,
    _playwright: Playwright,
    /// Which addresses the browser may open; strict outside the Chromium tests.
    nav: NavPolicy,
}

impl BrowserInspector {
    pub async fn launch() -> anyhow::Result<BrowserInspector> {
        let playwright = Playwright::launch()
            .await
            .context("starting the Playwright driver")?;
        let browser = playwright
            .chromium()
            .launch_with_options(crate::launch::launch_options())
            .await
            .context("launching Chromium")?;
        Ok(BrowserInspector {
            browser,
            _playwright: playwright,
            nav: NavPolicy::STRICT,
        })
    }

    /// Closes the browser; call it once the run is over.
    pub async fn close(self) -> anyhow::Result<()> {
        self.browser.close().await?;
        Ok(())
    }
}

#[async_trait]
impl FormInspector for BrowserInspector {
    async fn inspect(
        &self,
        job: &Job,
        cv_pdf: &Path,
        bank: &AnswerBank,
        shots_dir: &Path,
    ) -> anyhow::Result<FormInspection> {
        let page = self.browser.new_page().await?;
        let inspection = inspect_page(&page, job, cv_pdf, bank, shots_dir, self.nav).await;
        let _ = page.close().await;
        inspection
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::*;
    use crate::answers::{AnswerBank, AnswerEntry};
    use crate::form::FieldInfo;
    use crate::testing::make_job_with;

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

    /// One button of a Yes/No toggle or one radio button of the group `name`.
    fn option(idx: usize, name: &str, label: &str, text: &str, required: bool) -> FieldInfo {
        FieldInfo {
            option: text.into(),
            name: name.into(),
            ..field(idx, "input", "radio", label, required)
        }
    }

    fn bank() -> AnswerBank {
        AnswerBank {
            entries: vec![
                AnswerEntry::new(&["name"], Some("Jane Doe")),
                AnswerEntry::new(&["email"], Some("search@example.com")),
                AnswerEntry::new(&["linkedin"], None),
                AnswerEntry::new(&["how did you hear"], Some("Company careers page"))
                    .with_alternatives(&["Company website", "Other"]),
                AnswerEntry::new(&["require sponsorship"], Some("No")),
            ],
            consent_phrases: vec!["privacy policy".into()],
        }
    }

    // ---- is_handoff ----

    #[test]
    fn ashby_is_a_handoff_host_with_its_subdomains_but_not_lookalikes() {
        assert!(is_handoff("https://jobs.ashbyhq.com/acme/123/application"));
        assert!(is_handoff("https://ashbyhq.com/"));
        assert!(is_handoff("https://JOBS.AshbyHQ.com/acme"));
        assert!(!is_handoff("https://notashbyhq.com/acme"));
        assert!(!is_handoff("https://example.com/apply?ref=ashbyhq.com"));
        assert!(!is_handoff("https://boards.greenhouse.io/acme/jobs/1"));
        assert!(!is_handoff(""));
    }

    // ---- sheet_fields ----

    #[test]
    fn every_field_comes_with_its_label_whether_it_is_required_and_the_banks_answer() {
        let fields = [
            field(0, "input", "text", "Name", true),
            field(1, "input", "text", "Email", true),
            field(2, "input", "file", "Resume", true),
            field(3, "input", "text", "LinkedIn", false),
            field(4, "textarea", "", "Why do you want to work here?", true),
            field(5, "textarea", "", "Additional Information", false),
        ];
        let sheet = sheet_fields(&fields, &bank(), Path::new("cv.pdf"));
        let summary: Vec<(&str, bool, &SheetAnswer)> = sheet
            .iter()
            .map(|f| (f.label.as_str(), f.required, &f.answer))
            .collect();
        assert_eq!(
            summary,
            vec![
                ("Name", true, &SheetAnswer::Text("Jane Doe".into())),
                (
                    "Email",
                    true,
                    &SheetAnswer::Text("search@example.com".into())
                ),
                (
                    "Resume",
                    true,
                    &SheetAnswer::Attach(PathBuf::from("cv.pdf"))
                ),
                ("LinkedIn", false, &SheetAnswer::Skipped),
                ("Why do you want to work here?", true, &SheetAnswer::Missing),
                ("Additional Information", false, &SheetAnswer::Skipped),
            ]
        );
    }

    #[test]
    fn an_option_the_dry_run_already_pressed_still_counts_as_the_answer() {
        // The inspector scans the page AFTER the dry run, so the button the filler pressed shows as
        // pressed and the planner keeps it instead of clicking again. The sheet must not call
        // that question unanswered.
        let mut no = option(1, "sponsor", "Will you require sponsorship?", "No", true);
        no.checked = true;
        let fields = [
            option(0, "sponsor", "Will you require sponsorship?", "Yes", true),
            no,
        ];
        let sheet = sheet_fields(&fields, &bank(), Path::new("cv.pdf"));
        assert_eq!(
            sheet[0].answer,
            SheetAnswer::Choice {
                pick: "No".into(),
                alternatives: vec![]
            }
        );
    }

    #[test]
    fn a_dropdown_answer_keeps_its_alternatives() {
        let fields = [field(0, "select", "", "How did you hear about us?", false)];
        let sheet = sheet_fields(&fields, &bank(), Path::new("cv.pdf"));
        assert_eq!(
            sheet[0].answer,
            SheetAnswer::Choice {
                pick: "Company careers page".into(),
                alternatives: vec!["Company website".into(), "Other".into()],
            }
        );
    }

    #[test]
    fn a_radio_group_is_one_question_with_the_option_that_answers_it() {
        let fields = [
            option(0, "sponsor", "Will you require sponsorship?", "Yes", true),
            option(1, "sponsor", "Will you require sponsorship?", "No", true),
            option(2, "years", "Years of experience", "1-2 years", true),
            option(3, "years", "Years of experience", "3-5 years", true),
        ];
        let sheet = sheet_fields(&fields, &bank(), Path::new("cv.pdf"));
        assert_eq!(sheet.len(), 2, "one entry per group, not per button");
        assert_eq!(sheet[0].label, "Will you require sponsorship?");
        assert_eq!(
            sheet[0].answer,
            SheetAnswer::Choice {
                pick: "No".into(),
                alternatives: vec![]
            }
        );
        // The bank has nothing for the second one: the options are listed for Gonza.
        assert_eq!(sheet[1].answer, SheetAnswer::Missing);
        assert!(sheet[1].required);
        assert_eq!(sheet[1].options, vec!["1-2 years", "3-5 years"]);
    }

    #[test]
    fn a_required_checkbox_is_ticked_only_when_the_bank_allows_it() {
        let fields = [
            field(0, "input", "checkbox", "I accept the Privacy Policy", true),
            field(
                1,
                "input",
                "checkbox",
                "I acknowledge the Arbitration Agreement",
                true,
            ),
            field(2, "input", "checkbox", "Send me job alerts", false),
        ];
        let sheet = sheet_fields(&fields, &bank(), Path::new("cv.pdf"));
        assert_eq!(sheet[0].answer, SheetAnswer::Tick);
        assert_eq!(sheet[1].answer, SheetAnswer::Missing);
        assert_eq!(sheet[2].answer, SheetAnswer::Skipped);
    }

    #[test]
    fn a_label_the_page_left_empty_still_gets_a_name() {
        let fields = [field(0, "input", "text", "   ", false)];
        let sheet = sheet_fields(&fields, &bank(), Path::new("cv.pdf"));
        assert_eq!(sheet[0].label, "(campo sin etiqueta)");
    }

    #[test]
    fn the_tally_counts_answered_fields_and_lists_the_required_ones_left() {
        let fields = [
            field(0, "input", "text", "Name", true),
            field(1, "input", "text", "Email", true),
            field(2, "textarea", "", "Why do you want to work here?", true),
            field(3, "textarea", "", "Additional Information", false),
        ];
        let report = FieldReport::Full(sheet_fields(&fields, &bank(), Path::new("cv.pdf")));
        let tally = report.tally();
        assert_eq!(tally.filled, Some(2));
        assert_eq!(tally.left, vec!["Why do you want to work here?"]);
        // A report that only knows what is missing cannot say how many were filled.
        let partial = FieldReport::MissingOnly(vec![SheetField::missing("Email *")]);
        assert_eq!(partial.tally().filled, None);
        assert_eq!(partial.tally().left, vec!["Email *"]);
        assert_eq!(FieldReport::Unreadable.tally().filled, None);
        assert!(FieldReport::Unreadable.tally().left.is_empty());
    }

    // ---- the answer sheet ----

    fn sheet_text(report: &FieldReport, note: Option<&str>) -> String {
        let job = make_job_with(|j| {
            j.company = "Acme AI".into();
            j.title = "Senior AI Engineer".into();
            j.apply_url = "https://jobs.ashbyhq.com/acme/123/application".into();
        });
        render_sheet(
            &job,
            Path::new("C:/jobs/cv/test_1.pdf"),
            report,
            Some(Path::new("C:/jobs/screenshots/test_1.png")),
            note,
        )
    }

    #[test]
    fn the_sheet_has_spanish_headings_the_job_the_cv_and_every_question() {
        let fields = [
            field(0, "input", "text", "Name", true),
            field(1, "input", "file", "Resume", true),
            field(2, "textarea", "", "Why do you want to work here?", true),
            field(3, "input", "text", "LinkedIn", false),
        ];
        let report = FieldReport::Full(sheet_fields(&fields, &bank(), Path::new("cv.pdf")));
        let text = sheet_text(&report, None);
        for expected in [
            "# Hoja de respuestas: Acme AI — Senior AI Engineer",
            "https://jobs.ashbyhq.com/acme/123/application",
            "test_1.pdf",
            "test_1.png",
            "## Preguntas del formulario",
            "jobbot confirm test:1",
            "### 1. Name (obligatoria)",
            "Jane Doe",
            "### 2. Resume (obligatoria)",
            "### 3. Why do you want to work here? (obligatoria)",
            "⚠ OBLIGATORIA, contéstala tú",
            "### 4. LinkedIn (opcional)",
            "(opcional, sin respuesta)",
        ] {
            assert!(text.contains(expected), "missing {expected:?} in:\n{text}");
        }
    }

    #[test]
    fn answers_are_copied_verbatim_even_over_several_lines() {
        let bank = AnswerBank {
            entries: vec![AnswerEntry::new(
                &["=cover note"],
                Some("Line one, with *stars* and | pipes\nLine two"),
            )],
            consent_phrases: vec![],
        };
        let fields = [field(0, "textarea", "", "Cover note", false)];
        let report = FieldReport::Full(sheet_fields(&fields, &bank, Path::new("cv.pdf")));
        let text = sheet_text(&report, None);
        assert!(
            text.contains("Line one, with *stars* and | pipes\nLine two"),
            "{text}"
        );
    }

    #[test]
    fn a_partial_report_says_it_is_partial_and_an_unreadable_form_says_so() {
        let partial = FieldReport::MissingOnly(vec![SheetField::missing("Email *")]);
        let text = sheet_text(&partial, None);
        assert!(text.contains("Email *") && text.contains("⚠ OBLIGATORIA, contéstala tú"));
        assert!(text.contains("lista incompleta"), "{text}");
        let unreadable = sheet_text(&FieldReport::Unreadable, Some("page_error: HTTP 403"));
        assert!(unreadable.contains("no pudo leer"), "{unreadable}");
        assert!(unreadable.contains("page_error: HTTP 403"), "{unreadable}");
    }

    #[test]
    fn the_sheet_lives_under_a_file_name_made_from_the_application_key() {
        let path = sheet_path(Path::new("data/handoff"), "ashby:acme/123");
        assert_eq!(path, Path::new("data/handoff").join("ashby_acme_123.md"));
    }

    #[test]
    fn write_sheet_creates_the_folder_and_returns_the_path() {
        let dir = tempfile::tempdir().unwrap();
        let handoff = dir.path().join("data").join("handoff");
        let job = make_job_with(|_| {});
        let path = write_sheet(
            &handoff,
            &job,
            Path::new("cv.pdf"),
            &FieldReport::Unreadable,
            None,
            None,
        )
        .unwrap();
        assert_eq!(path, handoff.join("test_1.md"));
        assert!(
            std::fs::read_to_string(&path)
                .unwrap()
                .contains("Hoja de respuestas")
        );
    }

    fn result(status: SubmitStatus, reason: Option<&str>) -> SubmitResult {
        SubmitResult {
            status,
            reason: reason.map(str::to_string),
            screenshot: None,
        }
    }

    #[test]
    fn the_form_is_scanned_only_while_the_page_still_shows_it() {
        assert!(page_shows_the_form(&result(SubmitStatus::DryRun, None)));
        for reason in [
            "faltan respuestas: Email *",
            "opción no encontrada: Country = Atlantis",
            "no se pudo completar Phone: timeout",
        ] {
            assert!(
                page_shows_the_form(&result(SubmitStatus::NeedsGonza, Some(reason))),
                "{reason}"
            );
        }
        // Walls and error pages show something else: scanning them would list a login box.
        for reason in [
            "captcha",
            "login_required",
            "page_error: HTTP 403",
            "no_form_found",
            "sin campo de CV: el formulario no tiene dónde adjuntar el CV",
        ] {
            assert!(
                !page_shows_the_form(&result(SubmitStatus::NeedsGonza, Some(reason))),
                "{reason}"
            );
        }
        assert!(!page_shows_the_form(&result(SubmitStatus::Submitted, None)));
    }

    #[test]
    fn the_labels_a_filler_reports_as_missing_are_read_back_from_its_reason() {
        assert_eq!(
            missing_labels("faltan respuestas: Email *; Why this role? *"),
            vec!["Email *", "Why this role? *"]
        );
        // The failures that follow the dash are not questions.
        assert_eq!(
            missing_labels(
                "faltan respuestas: Email * — opción no encontrada: Country = Atlantis; no se pudo completar Phone: timeout"
            ),
            vec!["Email *"]
        );
        // An assisted form that was left open says what was pending at the end.
        assert_eq!(
            missing_labels(
                "assist_closed: cerraste la pestaña. Quedaba pendiente: faltan respuestas: Email *"
            ),
            vec!["Email *"]
        );
        assert!(missing_labels("opción no encontrada: Country = Atlantis").is_empty());
        assert!(missing_labels("captcha").is_empty());
    }

    #[test]
    fn a_filler_that_cannot_list_fields_still_tells_which_answers_are_missing() {
        let lacking = result(SubmitStatus::NeedsGonza, Some("faltan respuestas: Email *"));
        assert_eq!(
            report_from_result(&lacking),
            FieldReport::MissingOnly(vec![SheetField::missing("Email *")])
        );
        // A clean dry run: nothing is missing, but the rest of the form is not listed.
        assert_eq!(
            report_from_result(&result(SubmitStatus::DryRun, None)),
            FieldReport::MissingOnly(vec![])
        );
        // A wall or an error page: the form was never seen.
        assert_eq!(
            report_from_result(&result(SubmitStatus::NeedsGonza, Some("captcha"))),
            FieldReport::Unreadable
        );
    }

    #[test]
    fn handoff_reasons_are_recognised_by_their_prefix() {
        assert!(is_handoff_reason("handoff: envía tú. Hoja: x.md"));
        assert!(!is_handoff_reason("faltan respuestas: Email *"));
    }

    #[test]
    fn a_job_is_a_handoff_when_either_of_its_links_is_on_a_handoff_host() {
        let ashby = "https://jobs.ashbyhq.com/acme/1";
        let elsewhere = "https://acme.example.com/careers/1";
        assert!(is_handoff_job(&make_job_with(
            |j| j.apply_url = ashby.into()
        )));
        assert!(is_handoff_job(&make_job_with(|j| {
            j.url = ashby.into();
            j.apply_url = elsewhere.into();
        })));
        assert!(!is_handoff_job(&make_job_with(|j| {
            j.url = elsewhere.into();
            j.apply_url = elsewhere.into();
        })));
    }

    // ---- Browser test: `cargo test --lib handoff:: -- --ignored --test-threads=1` ----

    use crate::paths::Paths;

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "needs chromium"]
    async fn the_browser_inspector_lists_every_question_of_an_ashby_style_form() {
        let dir = tempfile::tempdir().unwrap();
        let cv = dir.path().join("cv.pdf");
        std::fs::write(&cv, b"%PDF-1.4 fake").unwrap();
        let fixture = Paths::new(env!("CARGO_MANIFEST_DIR"))
            .root
            .join("tests")
            .join("fixtures")
            .join("ashby_form.html");
        let url = format!(
            "file:///{}",
            fixture.display().to_string().replace('\\', "/")
        );
        let job = make_job_with(|j| j.apply_url = url);
        let mut inspector = BrowserInspector::launch().await.unwrap();
        inspector.nav = NavPolicy::LOCAL_TESTS; // the fixture is a file on this machine
        let inspection = inspector
            .inspect(&job, &cv, &bank(), &dir.path().join("shots"))
            .await
            .unwrap();
        inspector.close().await.unwrap();

        // The bank knows no phone, location or start date: the dry run stops for them, with the
        // form on screen and the screenshot taken.
        assert_eq!(inspection.result.status, SubmitStatus::NeedsGonza);
        assert!(
            inspection
                .result
                .reason
                .as_deref()
                .unwrap()
                .starts_with("faltan respuestas"),
            "{:?}",
            inspection.result.reason
        );
        assert!(inspection.result.screenshot.as_ref().unwrap().exists());
        let FieldReport::Full(fields) = &inspection.report else {
            panic!("the form was not listed: {:?}", inspection.report);
        };
        let labels: Vec<&str> = fields.iter().map(|f| f.label.as_str()).collect();
        assert_eq!(labels.len(), 15, "{labels:?}");
        let by_label = |label: &str| {
            fields
                .iter()
                .find(|f| f.label.starts_with(label))
                .unwrap_or_else(|| panic!("no {label:?} in {labels:?}"))
        };
        assert_eq!(
            by_label("Name").answer,
            SheetAnswer::Text("Jane Doe".into())
        );
        assert_eq!(by_label("Resume").answer, SheetAnswer::Attach(cv.clone()));
        assert_eq!(by_label("Phone Number").answer, SheetAnswer::Missing);
        assert_eq!(
            by_label("Will you now or in the future require sponsorship").answer,
            SheetAnswer::Choice {
                pick: "No".into(),
                alternatives: vec![]
            }
        );
        assert_eq!(
            by_label("Applicant Arbitration Agreement").answer,
            SheetAnswer::Missing,
            "a box that accepts arbitration is never ticked for Gonza"
        );
        assert_eq!(by_label("LinkedIn").answer, SheetAnswer::Skipped);
        // The sheet reads well end to end.
        let sheet = render_sheet(&job, &cv, &inspection.report, None, None);
        assert!(sheet.contains("### 1. Name (obligatoria)"), "{sheet}");
        assert!(sheet.contains("**Opciones:** Yes / No"), "{sheet}");
    }
}
