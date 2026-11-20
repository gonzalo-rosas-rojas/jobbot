//! The two batch steps of a run: prepare CVs for scored jobs, then submit prepared applications.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::path::Path;

use anyhow::Context;
use chrono::{DateTime, Utc};
use jobbot_domain::answers::{AnswerBank, australian_salary_ask};
use jobbot_domain::form::{SubmitMode, SubmitStatus};
use jobbot_domain::gate::{REASON_LISTING_ONLY, REASON_MANUAL_ONLY, REASON_STALE, stale_note};
use jobbot_domain::handoff::{
    FormInspection, FormTally, HANDOFF_REASON_PREFIX, is_handoff_job, missing_labels,
    report_from_result,
};
use jobbot_domain::models::Job;
use jobbot_domain::rules::{Criteria, Exclusions};
use jobbot_domain::state::State;
use jobbot_domain::text::safe_key;

use crate::apply::attachment::{
    CopyFailure, attachment_for, keep_before_replacing, refresh_attachment_copies,
};
use crate::apply::cv::{
    CvError, Renderer, assert_no_new_skills, ensure_ready, relocating_to, tailor,
};
use crate::apply::gate::check_gate;
use crate::apply::handoff::{FormInspector, write_sheet};
use crate::apply::results::{AttemptMode, ResultLine, append_result, results_path};
use crate::discovery::{region_ok, region_ok_with};
use crate::ports::store::{ApplicationRow, INTENT_SUBMIT, INTERRUPTED_PREFIX, Store};
use crate::ports::{FillMode, FormFiller};
use crate::ports::{OpenChecker, OpenStatus};
use tracing::field::Empty;

/// Screenshots of the forms, under the data directory.
const SHOTS_DIR: &str = "screenshots";
/// Answer sheets of the forms Gonza sends himself, under the data directory.
const HANDOFF_DIR: &str = "handoff";
/// The note of an application withdrawn because its source no longer lists the posting.
const CLOSED_NOTE: &str = "posting_closed: el aviso ya no está abierto en la fuente";

/// The most applications one company may have in flight at once. Mass applying is weak: of the
/// postings of one company only the best-scored ones go forward. `prepare` and `submit` leave the
/// rest as they are (they are looked at again on every run, so a place that frees up is taken).
pub const MAX_PER_COMPANY: usize = 2;

/// The states that count against a company's cap: everything on its way to an answer.
const IN_FLIGHT: [State; 7] = [
    State::Prepared,
    State::NeedsGonza,
    State::Submitted,
    State::Replied,
    State::TechnicalTest,
    State::Interview,
    State::Offer,
];

/// Of those, the ones that are already past a real submission: they always count and are never
/// skipped.
const SENT: [State; 5] = [
    State::Submitted,
    State::Replied,
    State::TechnicalTest,
    State::Interview,
    State::Offer,
];

/// The identity of a company for the cap: its name, lowercased and trimmed.
pub fn company_key(name: &str) -> String {
    name.trim().to_lowercase()
}

/// An application the cap leaves out of a run.
#[derive(Debug, Clone, PartialEq)]
pub struct CapSkip {
    pub job_key: String,
    pub company: String,
    pub score: f64,
}

impl CapSkip {
    /// The line that says so, in Spanish.
    pub fn line(&self) -> String {
        format!(
            "tope por empresa: {} ya tiene el máximo de {MAX_PER_COMPANY} postulaciones en curso; \
             {} (puntaje {:.1}) queda como está",
            self.company, self.job_key, self.score
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CapStage {
    /// `prepare`: what is already in flight holds its place and the `scored` ones compete for
    /// the rest.
    Prepare,
    /// `submit`: what is already sent holds its place and the `prepared` ones compete for the rest
    /// together with the ones waiting for Gonza (which are not sent by this run, but are the
    /// company's next choice and keep a place by their score).
    Submit,
}

/// Whether a row is an application on its way to an answer. A posting below the pay floor that
/// discovery set aside for Gonza to decide on (`needs_gonza`, `below_floor_exception`) is only a
/// question: it holds no place at its company and does not compete for one.
fn is_application(row: &ApplicationRow) -> bool {
    !(row.state == State::NeedsGonza
        && row
            .needs_reason
            .as_deref()
            .is_some_and(|reason| reason.starts_with("below_floor_exception")))
}

/// Which applications of the competing state a company's cap leaves out: the best-scored ones
/// (ties by key) fill the places the held ones leave free, and the rest are skipped. A blank
/// company name is nobody's company and is never capped.
fn cap_skips(tracker: &dyn Store, stage: CapStage) -> anyhow::Result<Vec<CapSkip>> {
    let (holding, competing, skippable): (&[State], &[State], State) = match stage {
        CapStage::Prepare => (&IN_FLIGHT, &[State::Scored], State::Scored),
        CapStage::Submit => (
            &SENT,
            &[State::Prepared, State::NeedsGonza],
            State::Prepared,
        ),
    };
    let mut held: BTreeMap<String, usize> = BTreeMap::new();
    for state in holding {
        for row in tracker.list_by_state(*state)? {
            if is_application(&row) {
                *held.entry(company_key(&row.company)).or_default() += 1;
            }
        }
    }
    let mut pools: BTreeMap<String, Vec<ApplicationRow>> = BTreeMap::new();
    for state in competing {
        for row in tracker.list_by_state(*state)? {
            let company = company_key(&row.company);
            if !company.is_empty() && is_application(&row) {
                pools.entry(company).or_default().push(row);
            }
        }
    }
    let mut skips = Vec::new();
    for (company, mut pool) in pools {
        pool.sort_by(|a, b| {
            b.score
                .total_cmp(&a.score)
                .then_with(|| a.job_key.cmp(&b.job_key))
        });
        let room = MAX_PER_COMPANY.saturating_sub(held.get(&company).copied().unwrap_or(0));
        skips.extend(
            pool.into_iter()
                .skip(room)
                .filter(|row| row.state == skippable)
                .map(|row| CapSkip {
                    job_key: row.job_key,
                    company: row.company,
                    score: row.score,
                }),
        );
    }
    skips.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then_with(|| a.job_key.cmp(&b.job_key))
    });
    Ok(skips)
}

/// What a `prepare` run did.
#[derive(Debug, Clone, PartialEq)]
pub struct PrepareReport {
    /// The keys moved to `prepared`, best score first.
    pub prepared: Vec<String>,
    /// The `scored` applications the per-company cap left alone (`MAX_PER_COMPANY`).
    pub skipped: Vec<CapSkip>,
}

/// `prepare_report` without the report of what the cap skipped: the keys that were moved.
pub async fn prepare_all(
    tracker: &dyn Store,
    cv_master: &Path,
    out_dir: &Path,
    renderer: &dyn Renderer,
    now: DateTime<Utc>,
) -> Result<Vec<String>, CvError> {
    Ok(prepare_report(tracker, cv_master, out_dir, renderer, now)
        .await?
        .prepared)
}

/// Tailors the CV master for every `scored` application, renders it to
/// `<out_dir>/<safe job key>.pdf` and moves the application to `prepared`, except the ones the
/// per-company cap leaves out: a company keeps at most `MAX_PER_COMPANY` applications in flight
/// (prepared, waiting for Gonza, sent or answered), the best-scored ones win, and the others stay
/// `scored` and are listed in the report. A CV master that still has `⟦` placeholders fails with
/// `CvError::NotReady` before anything is read from the tracker or written to disk.
pub async fn prepare_report(
    tracker: &dyn Store,
    cv_master: &Path,
    out_dir: &Path,
    renderer: &dyn Renderer,
    now: DateTime<Utc>,
) -> Result<PrepareReport, CvError> {
    prepare_report_relocating(
        tracker,
        cv_master,
        out_dir,
        renderer,
        Relocation::default(),
        now,
    )
    .await
}

/// Where the CV says Gonza would move for a job (`extra_open_relocation`), and which jobs those are:
/// the ones open only through `extra_open_places`. The default moves nowhere.
#[derive(Debug, Clone, Copy, Default)]
pub struct Relocation<'a> {
    pub places: &'a [String],
    pub to: Option<&'a str>,
}

impl<'a> Relocation<'a> {
    pub fn from_criteria(criteria: &'a Criteria) -> Self {
        Relocation {
            places: &criteria.extra_open_places,
            to: criteria
                .extra_open_relocation
                .as_deref()
                .map(str::trim)
                .filter(|place| !place.is_empty()),
        }
    }

    /// The master to tailor for `job`: relocated (`cv::relocating_to`) for a job in the place he
    /// would move to, the master itself for any other.
    pub fn master_for<'m>(&self, master: &'m str, job: &Job) -> Cow<'m, str> {
        match self.to {
            Some(place) if open_only_through(job, self.places) => {
                Cow::Owned(relocating_to(master, place))
            }
            _ => Cow::Borrowed(master),
        }
    }
}

/// True when `job` passes the region filter only because `places` (`extra_open_places`) name its
/// place: for Gonza, a job in Australia.
fn open_only_through(job: &Job, places: &[String]) -> bool {
    !places.is_empty() && !region_ok(job) && region_ok_with(job, places)
}

/// The CV master's text, once it has no placeholder left.
async fn read_master(cv_master: &Path) -> Result<String, CvError> {
    let master = tokio::fs::read_to_string(cv_master)
        .await
        .with_context(|| format!("reading {}", cv_master.display()))?;
    ensure_ready(&master)?;
    Ok(master)
}

/// The posting of a tracked application. A missing one is a broken tracker, not a skipped job.
fn job_or_missing(tracker: &dyn Store, key: &str) -> anyhow::Result<Job> {
    tracker
        .get_job(key)?
        .with_context(|| format!("job {key} is missing from the tracker"))
}

/// The master tailored for `job` (relocated first when the job is in the place Gonza would move
/// to), checked to add no skill the master lacks.
fn tailored_variant(
    master: &str,
    job: &Job,
    relocation: Relocation<'_>,
) -> Result<String, CvError> {
    let base = relocation.master_for(master, job);
    let variant = tailor(&base, job);
    assert_no_new_skills(&base, &variant)?;
    Ok(variant)
}

/// `prepare_report` for a run that knows where Gonza would move (`Relocation::from_criteria`): the
/// CV of a job in that place is tailored from the relocated master.
#[tracing::instrument(name = "prepare", skip_all, fields(prepared = Empty, skipped = Empty))]
pub async fn prepare_report_relocating(
    tracker: &dyn Store,
    cv_master: &Path,
    out_dir: &Path,
    renderer: &dyn Renderer,
    relocation: Relocation<'_>,
    now: DateTime<Utc>,
) -> Result<PrepareReport, CvError> {
    let master = read_master(cv_master).await?;
    tokio::fs::create_dir_all(out_dir)
        .await
        .with_context(|| format!("creating {}", out_dir.display()))?;
    let skipped = cap_skips(tracker, CapStage::Prepare)?;
    let mut prepared = Vec::new();
    for row in tracker.list_by_state(State::Scored)? {
        if skipped.iter().any(|skip| skip.job_key == row.job_key) {
            continue;
        }
        let job = job_or_missing(tracker, &row.job_key)?;
        let variant = tailored_variant(&master, &job, relocation)?;
        let pdf_path = out_dir.join(format!("{}.pdf", safe_key(&row.job_key)));
        let pdf = renderer.render(&variant, &pdf_path).await?;
        tracker.set_cv_path(&row.job_key, &pdf)?;
        tracker
            .transition(&row.job_key, State::Prepared, None, now)
            .map_err(anyhow::Error::from)?;
        prepared.push(row.job_key);
    }
    crate::record_counts(&[("prepared", prepared.len()), ("skipped", skipped.len())]);
    Ok(PrepareReport { prepared, skipped })
}

/// What `refresh_cvs` did.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RefreshReport {
    /// The keys whose CV was rendered again.
    pub refreshed: Vec<String>,
    /// Readable copies (`adjuntos/`) that still hold the replaced CV.
    pub copy_failures: Vec<CopyFailure>,
}

/// Folder, next to the CVs, that keeps every CV `refresh_cvs` replaced.
pub const REPLACED_CV_DIR: &str = "replaced";

/// Renders again, in place, the CV of every application that is `prepared` or waiting for Gonza
/// and has one, from the master as it is now (after an edit of `cv/cv-master.md`), tailored as
/// `prepare` would. No state changes. A sent application is never touched: its PDF is the record of
/// what went out. Gonza may have sent a waiting one by hand without confirming it, so the PDF being
/// replaced is first copied to `<cv_dir>/replaced/<name>.<UTC time>.pdf`. The readable copies
/// in `adjuntos/<job key>/` (the files the handoff sheets name) are overwritten with the new PDF
/// too (each one kept first in `replaced/`), and a copy that could not be replaced is returned in
/// `copy_failures`. `cv_dir` is the folder of the tailored CVs, `<data_dir>/cv`. Returns the keys
/// whose CV was rendered again, best score first within each state. When a later row fails, the
/// copy failures found so far are logged before the error is returned.
#[tracing::instrument(skip_all, fields(refreshed = Empty, copy_failures = Empty))]
pub async fn refresh_cvs(
    tracker: &dyn Store,
    cv_master: &Path,
    cv_dir: &Path,
    renderer: &dyn Renderer,
    relocation: Relocation<'_>,
    now: DateTime<Utc>,
) -> Result<RefreshReport, CvError> {
    let master = read_master(cv_master).await?;
    let stamp = now.format("%Y%m%dT%H%M%SZ").to_string();
    let places = RefreshPlaces {
        cv_dir,
        replaced_dir: &cv_dir.join(REPLACED_CV_DIR),
        stamp: &stamp,
    };
    let mut report = RefreshReport::default();
    let result = refresh_rows(tracker, &master, renderer, relocation, places, &mut report).await;
    if let Err(err) = result {
        // Rendering stopped half way: the copies already found stale must not be forgotten.
        for failure in &report.copy_failures {
            tracing::warn!(
                key = %failure.job_key,
                file = %failure.file.display(),
                reason = %failure.reason,
                "a readable CV copy could not be refreshed; the refresh then failed"
            );
        }
        return Err(with_copy_failures(err, &report.copy_failures));
    }
    crate::record_counts(&[
        ("refreshed", report.refreshed.len()),
        ("copy_failures", report.copy_failures.len()),
    ]);
    Ok(report)
}

/// Where a refresh reads and keeps files, and the time mark of the copies it keeps.
#[derive(Clone, Copy)]
struct RefreshPlaces<'a> {
    /// The folder of the tailored CVs, `<data_dir>/cv`.
    cv_dir: &'a Path,
    /// The folder that keeps what the refresh replaced.
    replaced_dir: &'a Path,
    /// The UTC time mark in the names of the kept copies.
    stamp: &'a str,
}

/// `err` with, when copies were found stale before it, a line naming their job keys (only keys,
/// no paths) and telling Gonza not to attach them. Without such copies, `err` as it is.
fn with_copy_failures(err: CvError, failures: &[CopyFailure]) -> CvError {
    if failures.is_empty() {
        return err;
    }
    let keys = failures
        .iter()
        .map(|failure| failure.job_key.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    CvError::Other(anyhow::anyhow!(
        "{err}. Antes de fallar, estas copias legibles de CV no se pudieron actualizar y todavía \
         tienen el CV anterior, no las adjuntes: {keys}"
    ))
}

/// The rows of `refresh_cvs`, filling `report` as it goes so that a failure leaves what was done.
async fn refresh_rows(
    tracker: &dyn Store,
    master: &str,
    renderer: &dyn Renderer,
    relocation: Relocation<'_>,
    places: RefreshPlaces<'_>,
    report: &mut RefreshReport,
) -> Result<(), CvError> {
    let RefreshPlaces {
        cv_dir,
        replaced_dir,
        stamp,
    } = places;
    for state in [State::Prepared, State::NeedsGonza] {
        for row in tracker.list_by_state(state)? {
            let Some(cv_path) = row.cv_path.as_deref() else {
                continue;
            };
            let job = job_or_missing(tracker, &row.job_key)?;
            let variant = tailored_variant(master, &job, relocation)?;
            let pdf_path = Path::new(cv_path);
            if pdf_path.exists() {
                let stem = pdf_path
                    .file_stem()
                    .map(|stem| stem.to_string_lossy().into_owned())
                    .unwrap_or_else(|| safe_key(&row.job_key));
                keep_before_replacing(pdf_path, replaced_dir, &format!("{stem}.{stamp}.pdf"))
                    .await
                    .with_context(|| format!("keeping {}", pdf_path.display()))?;
            }
            renderer.render(&variant, pdf_path).await?;
            // The copy a handoff sheet names is a file of its own: it carries the new CV too,
            // and one that could not be replaced is reported, so nobody attaches it believing
            // it was refreshed.
            report.copy_failures.extend(
                refresh_attachment_copies(cv_dir, pdf_path, &row.job_key, replaced_dir, stamp)
                    .await,
            );
            report.refreshed.push(row.job_key);
        }
    }
    Ok(())
}

/// What `submit_all` did with one application.
#[derive(Debug, Clone, PartialEq)]
pub struct SubmitOutcome {
    pub job_key: String,
    /// `dry_run`, `submitted`, `needs_gonza` or `blocked` (the gate refused: see `reason`).
    pub status: String,
    pub reason: Option<String>,
}

fn outcome(job_key: &str, status: &str, reason: Option<&str>) -> SubmitOutcome {
    SubmitOutcome {
        job_key: job_key.to_string(),
        status: status.to_string(),
        // The reasons carry text from web pages and are printed raw.
        reason: reason.map(jobbot_domain::outbound::plain_line),
    }
}

/// Walks the `prepared` applications by score (highest first), asks the gate about each one and
/// fills the form through `filler`.
///
/// * A STOP file returns immediately. `stop_file` and `daily_cap` from the gate end the run.
/// * `manual_only_platform` and `listing_only` move the application to `needs_gonza`; an
///   `excluded:*` company and a `stale_posting` move it to `withdrawn`; `company_cooldown` and
///   `warmup_hold` leave it as it is. All of them are reported with status `blocked`.
/// * The mode is decided per application: `Assist` stays `Assist` (Gonza's click is the review,
///   so a submission also approves the application); `DryRun` stays `DryRun`; `Live` becomes
///   `DryRun` for `review_required` applications (the first approved submissions are still
///   pending) and stays `Live` otherwise.
/// * Each application gets one dry run (a later run skips the ones that already have a
///   screenshot) and a run makes at most `criteria.review_dry_runs_per_run` new ones, so a
///   review session has a bounded number of screenshots.
/// * An application to a handoff host (`handoff::is_handoff`, Ashby) never gets `Assist` or
///   `Live`, whatever `mode` says: the bot dry-runs the form, writes an answer sheet to
///   `<data_dir>/handoff/` and moves the application to `needs_gonza` with a reason that starts
///   with `handoff:` and names the sheet. Gonza sends the form from his own browser and records it
///   with `jobbot confirm`. It counts as one of the run's new dry runs, and it is handed off
///   without waiting for an approval (his own click is the review).
/// * A company keeps at most `MAX_PER_COMPANY` applications in flight (sent or answered ones
///   always count, then the best scores): a `prepared` application over its company's cap is
///   reported as `blocked` with a reason that starts with `tope por empresa`, before the gate is
///   asked, and stays `prepared`.
/// * A `prepared` application without a CV path goes to `needs_gonza` and the run continues.
/// * `needs_gonza` replies from the filler (including `unconfirmed_submit: ...`) are recorded with
///   their reason and never retried by the program. A dry run that found required questions the
///   bank cannot answer (`faltan respuestas: ...`) comes back as one of these, with the screenshot
///   of the form filled as far as it could be; `Assist` is the way to finish such a form, since
///   Gonza answers the rest in the browser. Only `unconfirmed_submit` counts as a click: an
///   `assist_timeout` or `assist_closed` (the form he skipped) does not lock the company.
/// * Before it clicks (`Live` or `Assist`) a run writes an intent row; a run that finds one still
///   open moves that application to `needs_gonza` (`interrupted_submit`) and never clicks it again.
///
/// An `Err` from the form side (filler or inspector) on one application moves that application to
/// `needs_gonza` with `page_error: ...` and the run goes on; three in a row stop the run with a
/// `stopped` outcome and leave the remaining applications untouched (the browser is gone).
/// `data_dir` is the project's `data/` folder: screenshots go to `screenshots/` and answer sheets
/// to `handoff/` under it.
#[allow(clippy::too_many_arguments)] // the shared contract fixes this signature
pub async fn submit_all(
    tracker: &dyn Store,
    criteria: &Criteria,
    exclusions: &Exclusions,
    bank: &AnswerBank,
    filler: &dyn FormFiller,
    mode: SubmitMode,
    data_dir: &Path,
    stop_file: &Path,
    now: DateTime<Utc>,
) -> anyhow::Result<Vec<SubmitOutcome>> {
    submit_all_while(
        tracker,
        criteria,
        exclusions,
        bank,
        filler,
        None,
        None,
        mode,
        data_dir,
        stop_file,
        now,
        &|| true,
    )
    .await
}

/// `submit_all` with a guard asked before each application: when `keep_going` returns false the
/// run stops there and the remaining applications stay as they are. The CLI passes a check of the
/// execution window, because a long run that starts in the evening must not keep clicking once
/// Gonza's working hours begin. `inspector` opens the handoff forms and lists every question they
/// ask for the answer sheet; without one, a handoff form is dry-run through `filler` and the sheet
/// lists only the required questions the bank could not answer. `open_checker` asks the posting's
/// source whether it is still open once the gate has let the application through: a posting the
/// source says is gone is withdrawn (`posting_closed`) before its form is opened and without using
/// up a dry-run slot, while an unclear answer never stops an application. Everything else is
/// documented on `submit_all`.
#[allow(clippy::too_many_arguments)] // the shared contract fixes the base signature
#[tracing::instrument(name = "submit", skip_all, fields(applications = Empty))]
pub async fn submit_all_while(
    tracker: &dyn Store,
    criteria: &Criteria,
    exclusions: &Exclusions,
    bank: &AnswerBank,
    filler: &dyn FormFiller,
    inspector: Option<&dyn FormInspector>,
    open_checker: Option<&dyn OpenChecker>,
    mode: SubmitMode,
    data_dir: &Path,
    stop_file: &Path,
    now: DateTime<Utc>,
    keep_going: &dyn Fn() -> bool,
) -> anyhow::Result<Vec<SubmitOutcome>> {
    let shots_dir = data_dir.join(SHOTS_DIR);
    let mut outcomes = Vec::new();
    // A run that died between the intent and the end of a click left a row open: that application
    // may have been sent, so it goes to Gonza and the program never clicks it again.
    for key in tracker.recover_open_intents(now)? {
        outcomes.push(outcome(&key, "needs_gonza", Some(INTERRUPTED_PREFIX)));
    }
    if stop_file.exists() {
        return Ok(outcomes);
    }
    let capped = cap_skips(tracker, CapStage::Submit)?;
    let mut new_dry_runs: u32 = 0;
    let mut errors = FormErrorStreak::default();
    for row in tracker.list_by_state(State::Prepared)? {
        if !keep_going() {
            break; // the execution window closed: what is left waits for the next run
        }
        if let Some(skip) = capped.iter().find(|skip| skip.job_key == row.job_key) {
            // Over its company's cap: untouched, and looked at again on the next run.
            outcomes.push(outcome(&row.job_key, "blocked", Some(&skip.line())));
            continue;
        }
        let job = job_or_missing(tracker, &row.job_key)?;
        let gated = check_gate(&job, &row, tracker, criteria, exclusions, stop_file, now)?;
        let gate_reason = gated.decision.reason.as_deref();
        match gate_reason {
            Some("stop_file") => break,
            // The cap stops the forms, not the bookkeeping: a later posting that is only a
            // listing, or is excluded or stale, is still moved (the gate checks those first).
            Some("daily_cap") => continue,
            Some(reason @ (REASON_MANUAL_ONLY | REASON_LISTING_ONLY)) => {
                let note = format!("{reason}: postular a mano desde el enlace del aviso");
                tracker.transition(&row.job_key, State::NeedsGonza, Some(&note), now)?;
                outcomes.push(outcome(&row.job_key, "blocked", gate_reason));
                continue;
            }
            Some(why) if why.starts_with("excluded:") => {
                tracker.transition(&row.job_key, State::Withdrawn, Some(why), now)?;
                outcomes.push(outcome(&row.job_key, "blocked", gate_reason));
                continue;
            }
            Some(REASON_STALE) => {
                let note = stale_note(criteria);
                tracker.transition(&row.job_key, State::Withdrawn, Some(&note), now)?;
                outcomes.push(outcome(&row.job_key, "blocked", gate_reason));
                continue;
            }
            Some("review_required") | None => {}
            Some(_) => {
                // company_cooldown, warmup_hold: nothing to change, try again on a later run.
                outcomes.push(outcome(&row.job_key, "blocked", gate_reason));
                continue;
            }
        }
        // The gate let it through: only now is it worth a request to the posting's source. Only a
        // clear "gone" stops the application; Open and Unknown both go on.
        if let Some(checker) = open_checker
            && checker.check(&job).await == OpenStatus::Closed
        {
            tracker.transition(&row.job_key, State::Withdrawn, Some(CLOSED_NOTE), now)?;
            outcomes.push(outcome(&row.job_key, "blocked", Some("posting_closed")));
            continue;
        }
        // Ashby flags the bot's Chromium as spam even when a human clicks: those forms are only
        // ever dry-run here and sent by Gonza from his own browser.
        let handoff = is_handoff_job(&job);
        // Only the gate's approval sends: an application still in review (the gate blocked it with
        // `review_required` and gave no approval) is dry-run instead.
        let fill = match (mode, gated.approval) {
            _ if handoff => FillMode::DryRun,
            (SubmitMode::Assist, _) => FillMode::Assist,
            (SubmitMode::Live, Some(approval)) => FillMode::Live(approval),
            (SubmitMode::DryRun, _) | (SubmitMode::Live, None) => FillMode::DryRun,
        };
        let effective = fill.submit_mode();
        if effective == SubmitMode::DryRun {
            if !handoff && row.screenshot_path.is_some() {
                continue; // already dry-run once: waiting for Gonza's approval or for a live run
            }
            if new_dry_runs >= criteria.review_dry_runs_per_run {
                continue; // enough new screenshots for one review session; the rest wait
            }
            new_dry_runs += 1;
        }
        let Some(cv_pdf) = row.cv_path.as_deref() else {
            // `resolve` on an application that never had a PDF lands here: hand it back to Gonza
            // instead of aborting the whole run.
            let note = "sin CV: la postulación llegó a prepared sin PDF; `jobbot resolve` la devuelve a scored";
            tracker.transition(&row.job_key, State::NeedsGonza, Some(note), now)?;
            outcomes.push(outcome(&row.job_key, "needs_gonza", Some(note)));
            continue;
        };
        // A job in Australia (open only because `extra_open_places` says so) gets the Australian
        // work-rights answers: the bank's contractor answers are false for a role that needs him there.
        let job_bank = bank_for_job(bank, &job, criteria);
        let bank = job_bank.as_ref();
        // The form (and the answer sheet) get a copy named the way Gonza and a recruiter read it.
        let cv_pdf = attachment_for(Path::new(cv_pdf), &job, bank, data_dir);
        if handoff {
            let handed = hand_off(
                tracker, &job, &cv_pdf, bank, filler, inspector, data_dir, now,
            )
            .await;
            match handed {
                Ok(handed) => {
                    errors.form_worked();
                    outcomes.push(handed);
                }
                Err(err) => {
                    page_error(tracker, &row.job_key, &err, now, &mut outcomes)?;
                    errors.form_failed();
                    if errors.must_stop() {
                        outcomes.push(stopped_outcome());
                        break;
                    }
                }
            }
            continue;
        }
        // Written, and durable, BEFORE the click: if the process dies from here on, the next run
        // finds the row open and hands the application to Gonza instead of sending it again.
        let intent = if effective == SubmitMode::DryRun {
            None
        } else {
            match tracker.begin_intent(INTENT_SUBMIT, &row.job_key, now)? {
                Some(id) => Some(id),
                None => {
                    outcomes.push(outcome(&row.job_key, "blocked", Some("submit_in_progress")));
                    continue;
                }
            }
        };
        let result = match filler.submit(&job, &cv_pdf, bank, fill, &shots_dir).await {
            Ok(result) => {
                errors.form_worked();
                result
            }
            Err(err) => {
                // This form broke (a redirect destroyed the page, a script threw). It goes to
                // Gonza and the run moves on; only a streak means the browser itself is gone.
                page_error(tracker, &row.job_key, &err, now, &mut outcomes)?;
                close_intent(tracker, intent, "error", now)?;
                errors.form_failed();
                if errors.must_stop() {
                    outcomes.push(stopped_outcome());
                    break;
                }
                continue;
            }
        };
        // The state change comes first and the screenshot path last: once a form may have been
        // sent, what matters is that the application leaves `prepared`; a failure while storing
        // the path must not leave a sent form waiting to be sent again.
        let status = match result.status {
            SubmitStatus::DryRun => "dry_run",
            SubmitStatus::Submitted => {
                tracker.transition(&row.job_key, State::Submitted, None, now)?;
                if effective == SubmitMode::Assist {
                    tracker.approve(&row.job_key)?; // Gonza reviewed it in the browser himself
                }
                "submitted"
            }
            // While an assisted form waited, Gonza may have sent it from his own browser (Lever's
            // hCaptcha rejects the bot's Chromium) and run `jobbot confirm`: closing the tab
            // afterwards changes nothing, and it must not abort the run.
            SubmitStatus::NeedsGonza
                if tracker
                    .get_application(&row.job_key)?
                    .is_some_and(|current| current.state != State::Prepared) =>
            {
                "submitted"
            }
            SubmitStatus::NeedsGonza => {
                let reason = result.reason.as_deref();
                if reason.is_some_and(|text| text.starts_with("unconfirmed_submit")) {
                    // The program clicked and the page did not confirm: the form may well have
                    // gone through, so the daily cap and the company cooldown count it.
                    tracker.record_attempt(&row.job_key, now)?;
                }
                tracker.transition(&row.job_key, State::NeedsGonza, reason, now)?;
                "needs_gonza"
            }
        };
        close_intent(tracker, intent, status, now)?;
        // The state is already saved; a screenshot that cannot be stored is a warning, not a
        // reason to end the run with the rest of the applications unprocessed.
        if let Some(screenshot) = &result.screenshot
            && let Err(err) = tracker.set_screenshot(&row.job_key, screenshot)
        {
            tracing::warn!(
                key = %row.job_key,
                error = %format!("{err:#}"),
                "could not store the screenshot of the submit; the run goes on"
            );
        }
        // The filler does not count the questions it answered, only name the ones it lacked.
        let tally = FormTally {
            filled: None,
            left: result
                .reason
                .as_deref()
                .map(missing_labels)
                .unwrap_or_default(),
        };
        // The state in the database is already right. A log line that cannot be written (disk
        // full, file locked) must not abort the run and lose the report of what was just sent.
        if let Err(err) = log_attempt(
            tracker,
            data_dir,
            &job,
            attempt_mode(effective),
            tally,
            result.screenshot.as_deref(),
            None,
            now,
        ) {
            tracing::warn!(
                key = %row.job_key,
                error = %format!("{err:#}"),
                "could not append the attempt to results.jsonl; the run goes on"
            );
        }
        outcomes.push(outcome(&row.job_key, status, result.reason.as_deref()));
    }
    crate::record_counts(&[("applications", outcomes.len())]);
    Ok(outcomes)
}

/// Errors from the form side in a row after which the run stops: one page that breaks is that
/// page's problem, three in a row mean the browser or the driver is gone.
const MAX_ERRORS_IN_A_ROW: u32 = 3;

/// The form errors in a row of one run: a form that works resets it, and
/// `MAX_ERRORS_IN_A_ROW` failures in a row mean the run must stop.
#[derive(Default)]
struct FormErrorStreak(u32);

impl FormErrorStreak {
    fn form_worked(&mut self) {
        self.0 = 0;
    }

    fn form_failed(&mut self) {
        self.0 += 1;
    }

    /// Whether the failures in a row have reached the limit.
    fn must_stop(&self) -> bool {
        self.0 >= MAX_ERRORS_IN_A_ROW
    }
}

/// Moves an application whose form raised an error to `needs_gonza` with `page_error: <first line>`
/// (Playwright errors carry a long call log after it) and reports it.
fn page_error(
    tracker: &dyn Store,
    key: &str,
    err: &anyhow::Error,
    now: DateTime<Utc>,
    outcomes: &mut Vec<SubmitOutcome>,
) -> anyhow::Result<()> {
    let first_line = err.to_string();
    let first_line = first_line.lines().next().unwrap_or_default().trim();
    let reason = format!("page_error: {first_line}");
    tracker.transition(key, State::NeedsGonza, Some(&reason), now)?;
    outcomes.push(outcome(key, "needs_gonza", Some(&reason)));
    Ok(())
}

/// Closes the intent opened before a click, with how the click ended.
fn close_intent(
    tracker: &dyn Store,
    intent: Option<i64>,
    how: &str,
    now: DateTime<Utc>,
) -> anyhow::Result<()> {
    if let Some(id) = intent {
        tracker.finish_intent(id, how, now)?;
    }
    Ok(())
}

fn stopped_outcome() -> SubmitOutcome {
    outcome(
        "(corrida)",
        "stopped",
        Some(&format!(
            "{MAX_ERRORS_IN_A_ROW} errores seguidos: se detiene la corrida; lo que queda espera a la próxima"
        )),
    )
}

fn attempt_mode(mode: SubmitMode) -> AttemptMode {
    match mode {
        SubmitMode::DryRun => AttemptMode::DryRun,
        SubmitMode::Assist => AttemptMode::Assist,
        SubmitMode::Live => AttemptMode::Live,
    }
}

/// Appends one line to `data/results.jsonl` for an attempt that has just been recorded in the
/// tracker (the line carries the state the application ended up in).
#[allow(clippy::too_many_arguments)] // one line of the log, field by field
fn log_attempt(
    tracker: &dyn Store,
    data_dir: &Path,
    job: &Job,
    mode: AttemptMode,
    tally: FormTally,
    screenshot: Option<&Path>,
    handoff_sheet: Option<&Path>,
    now: DateTime<Utc>,
) -> anyhow::Result<()> {
    let state = tracker
        .get_application(&job.key())?
        .with_context(|| format!("application {} is missing from the tracker", job.key()))?
        .state;
    let line = ResultLine::new(job, mode, state, tally, screenshot, handoff_sheet, now);
    append_result(&results_path(data_dir), &line)
}

/// The handoff of one application: a dry run of the form (through `inspector` when there is one,
/// so the sheet lists every question), the answer sheet, and the move to `needs_gonza` with a
/// `handoff:` reason that names the sheet. Never approves, never records a click and never moves
/// the application to `submitted`: Gonza's `jobbot confirm` does that after he sends the form.
#[allow(clippy::too_many_arguments)]
async fn hand_off(
    tracker: &dyn Store,
    job: &Job,
    cv_pdf: &Path,
    bank: &AnswerBank,
    filler: &dyn FormFiller,
    inspector: Option<&dyn FormInspector>,
    data_dir: &Path,
    now: DateTime<Utc>,
) -> anyhow::Result<SubmitOutcome> {
    let key = job.key();
    let shots_dir = data_dir.join(SHOTS_DIR);
    let FormInspection { result, report } = match inspector {
        Some(inspector) => inspector.inspect(job, cv_pdf, bank, &shots_dir).await?,
        None => {
            let result = filler
                .submit(job, cv_pdf, bank, FillMode::DryRun, &shots_dir)
                .await?;
            let report = report_from_result(&result);
            FormInspection { result, report }
        }
    };
    let seen = result.reason.as_deref();
    let sheet = write_sheet(
        &data_dir.join(HANDOFF_DIR),
        job,
        cv_pdf,
        &report,
        result.screenshot.as_deref(),
        seen,
    )?;
    let mut note = format!(
        "{HANDOFF_REASON_PREFIX} envía tú este formulario desde tu navegador normal \
         (el sitio marca como spam el Chromium del bot). Hoja de respuestas: {}",
        sheet.display()
    );
    if let Some(seen) = seen {
        note.push_str(&format!(" · el bot vio: {seen}"));
    }
    // The screenshot is stored before the move: a failure then is only a warning and a line in
    // the note the owner reads, never a page error after the application is already with him
    // (needs_gonza -> needs_gonza is illegal and would end the run).
    if let Some(screenshot) = &result.screenshot
        && let Err(err) = tracker.set_screenshot(&key, screenshot)
    {
        tracing::warn!(
            key = %key,
            error = %format!("{err:#}"),
            "could not store the handoff screenshot; the run goes on"
        );
        note.push_str(" · no se guardó la captura de pantalla");
    }
    tracker.transition(&key, State::NeedsGonza, Some(&note), now)?;
    // The application is already with Gonza and the sheet is written. A log line that cannot be
    // written must not turn into a page error: that would try needs_gonza -> needs_gonza and
    // abort the whole run, as in the main path (which only warns).
    if let Err(err) = log_attempt(
        tracker,
        data_dir,
        job,
        AttemptMode::Handoff,
        report.tally(),
        result.screenshot.as_deref(),
        Some(&sheet),
        now,
    ) {
        tracing::warn!(
            key = %key,
            error = %format!("{err:#}"),
            "could not append the handoff to results.jsonl; the run goes on"
        );
    }
    Ok(outcome(&key, "needs_gonza", Some(&note)))
}

/// The bank to answer `job`'s form with. A job that is open only because `extra_open_places` names
/// its place (Australia) gets the bank with the Australian work-rights answers: "no sponsorship
/// needed" is the contractor's answer from Chile and is false for a role that needs him on site, so
/// the facts replace it (no work rights, sponsorship needed), and pay is asked in AUD a year (the
/// bottom of the posted range, or the ask for the level of its title), never with the bank's USD
/// monthly figure. Any other job gets the bank as it is.
pub fn bank_for_job<'a>(
    bank: &'a AnswerBank,
    job: &Job,
    criteria: &Criteria,
) -> Cow<'a, AnswerBank> {
    if open_only_through(job, &criteria.extra_open_places) {
        Cow::Owned(
            bank.with_australian_work_rights_answers()
                .with_australian_on_site_answers()
                .with_australian_salary(australian_salary_ask(&job.title, &job.description)),
        )
    } else {
        Cow::Borrowed(bank)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cap_is_one_constant_and_the_company_key_ignores_case_and_edges() {
        assert_eq!(MAX_PER_COMPANY, 2);
        assert_eq!(company_key("  Acme AI "), "acme ai");
        assert_eq!(company_key("ACME AI"), company_key("acme ai"));
    }
}
