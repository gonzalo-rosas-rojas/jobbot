//! Application form filling: the Playwright code that drives the page and implements the
//! application's `FormFiller` port. The pure planning core is `jobbot_domain::form`, re-exported
//! here so the tests and the driver read it under one name.
//!
//! This file holds one application from start to end (`fill_application`, `fill_steps`,
//! `BrowserFiller`) and the one door through which the browser is sent to an address: `open`,
//! with the checks on the link before it and on the landing page after it. A test reads the
//! sources to see that no other `goto` exists. The steps live in the children:
//!
//! - `rescan`: reading the form's controls, and following them when the form changes.
//! - `apply` and `dropdown`: carrying out the plan on the controls, and choosing in a list.
//! - `walls`: what stops a form while it is being filled (captcha, login, block text).
//! - `leftover`: what is left for Gonza and how he is told.
//! - `confirm`: the submit button and the wait for the page to confirm.
//! - `shots` and `retry`: screenshots, and the one retry of a call a busy browser failed.

use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::Context;
use playwright_rs::server::channel_owner::ChannelOwner;
use playwright_rs::{BrowserContext, GotoOptions, Page, Playwright, Response, WaitUntil};

use jobbot_domain::answers::AnswerBank;
use jobbot_domain::gate::{
    REASON_LISTING_ONLY, REASON_MANUAL_ONLY, is_listing_only, is_manual_only,
};
use jobbot_domain::handoff::is_handoff;
use jobbot_domain::models::Job;
use jobbot_domain::navigation::{self, NavPolicy, NavigableUrl};
use jobbot_domain::outbound::plain_line;
use jobbot_domain::text::safe_key;

use jobbot_app::ports::{FillMode, FormFiller};
pub use jobbot_domain::form::*;

use self::apply::apply_all;
use self::confirm::{
    CONFIRM_WAIT_MS, Confirmation, SUBMIT_SELECTOR, await_confirmation, snapshot, tag_submit,
};
pub(crate) use self::leftover::needs_gonza;
use self::leftover::{
    assist_banner, assist_closed_reason, assist_timeout_reason, click_error_reason, controls_left,
    first_line, flag_for_gonza, flag_groups, leftover_reason, stop_for_gonza,
};
use self::rescan::{carried_over, merged, new_controls, renumbered, scan_fields, scan_once};
use self::shots::take_shot;
use self::walls::detect_blocker;
use crate::egress::{Egress, SystemResolver, explain_tunnel_failure};

mod apply;
mod confirm;
mod dropdown;
mod leftover;
mod rescan;
mod retry;
mod shots;
mod walls;

/// Default timeout for every page action. A missing field fails in 10 s, not in Playwright's 30 s.
const ACTION_TIMEOUT_MS: f64 = 10_000.0;
/// The deadline of the first load of the application page.
const FIRST_LOAD_TIMEOUT_MS: u64 = 30_000;

/// How long Gonza has to review and click in Assist mode.
const ASSIST_TIMEOUT_MS: u64 = 600_000;
/// The most one application may take in a dry run or a live run, from the first page action to the
/// last. Playwright gives `evaluate` no deadline: a page stuck in a busy loop would hold the
/// application, and the whole night's run behind it, for ever. Assist mode has no wall: Gonza has
/// `ASSIST_TIMEOUT_MS` there by design.
pub(crate) const APPLICATION_WALL: Duration = Duration::from_secs(180);
/// How long closing a page that stopped answering may take before it is left to its owner.
const CLOSE_STUCK_PAGE_MS: u64 = 10_000;
const POLL_MS: u64 = 500;

/// How the first load of the application page is asked for: until `DOMContentLoaded`, as a hosted
/// board draws its form from script afterwards, and with a deadline of its own. Without one the
/// load would take the page's default of `ACTION_TIMEOUT_MS`, and a slow board would fail with
/// `page_error: Timeout 10000ms` before its form even arrived.
fn first_load_options() -> GotoOptions {
    GotoOptions::new()
        .wait_until(WaitUntil::DomContentLoaded)
        .timeout(Duration::from_millis(FIRST_LOAD_TIMEOUT_MS))
}

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
        Some(REASON_MANUAL_ONLY)
    } else if is_listing_only(url) {
        Some(REASON_LISTING_ONLY)
    } else if mode != SubmitMode::DryRun && is_handoff(url) {
        Some("handoff_host")
    } else {
        None
    }
}

/// The address the browser may open for this posting, or the hand-back when it must not open it.
/// Asked before the browser moves; the reason starts with `unsafe_url:`.
fn checked_link(job: &Job, nav: NavPolicy) -> Result<NavigableUrl, SubmitResult> {
    NavigableUrl::parse(&job.apply_url, nav)
        .map_err(|refusal| needs_gonza(format!("unsafe_url: {refusal}"), None))
}

/// The one place shipped code sends the browser to a page, and only to an address that passed the
/// rules. A test of this crate fails when any other `goto` appears outside the tests.
async fn open(
    page: &Page,
    url: &NavigableUrl,
    options: GotoOptions,
) -> playwright_rs::Result<Option<Response>> {
    page.goto(url.as_str(), options).await
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
/// After the first round of fills the page is read once more: a question the answers opened is
/// planned and filled in a second round, and a required one the bank cannot answer is a missing
/// answer in every mode.
///
/// Problems with one page (captcha, login, a missing answer, an HTTP error, a call the page broke,
/// a click the page did not confirm) come back as `NeedsGonza`; `Err` is left for what makes the
/// whole run unsafe, such as the shots folder that cannot be made. (A browser that is gone shows
/// as `new_page` failing, before this is called.)
///
/// A dry run or a live run is limited to `APPLICATION_WALL`: a page that stops answering ends as
/// `NeedsGonza` with `page_error: la página no respondió en 180 s` before the click was tried, and
/// with `unconfirmed_submit: la página dejó de responder después del clic` after it (the form may
/// be out), and the page is closed.
pub(crate) async fn fill_application(
    page: &Page,
    job: &Job,
    cv_pdf: &Path,
    bank: &AnswerBank,
    mode: SubmitMode,
    shots_dir: &Path,
    nav: NavPolicy,
) -> anyhow::Result<SubmitResult> {
    fill_application_within(
        page,
        job,
        cv_pdf,
        bank,
        mode,
        shots_dir,
        nav,
        APPLICATION_WALL,
    )
    .await
}

/// `fill_application` with the wall as a parameter (the tests use a short one).
#[allow(clippy::too_many_arguments)] // `fill_application`'s arguments plus the wall
pub(crate) async fn fill_application_within(
    page: &Page,
    job: &Job,
    cv_pdf: &Path,
    bank: &AnswerBank,
    mode: SubmitMode,
    shots_dir: &Path,
    nav: NavPolicy,
    wall: Duration,
) -> anyhow::Result<SubmitResult> {
    tokio::fs::create_dir_all(shots_dir)
        .await
        .with_context(|| format!("creating {}", shots_dir.display()))?;
    // Set just before the submit click is called: from then on the form may be out.
    let clicked = AtomicBool::new(false);
    let steps = async {
        match fill_steps(page, job, cv_pdf, bank, mode, shots_dir, nav, &clicked).await {
            Err(err) => {
                // This page broke (a script destroyed the context a call ran in, a call timed
                // out): it goes back to Gonza with what the page looked like, and the run goes on.
                // Whether the browser itself is gone is told by `new_page` failing, not by this.
                let shot = shots_dir.join(format!("{}.png", safe_key(&job.key())));
                let shot = take_shot(page, &shot).await.ok().map(|()| shot);
                Ok(needs_gonza(
                    page_failure_reason(&err, clicked.load(Ordering::SeqCst)),
                    shot,
                ))
            }
            ok => ok,
        }
    };
    let result = if mode == SubmitMode::Assist {
        steps.await
    } else {
        match tokio::time::timeout(wall, steps).await {
            Ok(result) => result,
            Err(_) => {
                // The page is closed so that nothing it still runs goes on; one that does not even
                // answer a close is left to the owner of the page.
                close_within_limit(page).await;
                Ok(needs_gonza(
                    stuck_reason(clicked.load(Ordering::SeqCst), wall),
                    None,
                ))
            }
        }
    };
    // Every reason leaves through one cleaner: the page's own words are in many of them.
    result.map(|mut result| {
        result.reason = result.reason.map(|reason| clean_reason(&reason));
        result
    })
}

/// The most characters of a reason that is stored.
const REASON_MAX_CHARS: usize = 500;

/// A reason as it is stored: on one line, without control or invisible characters (a page can put
/// both in a label or a message), and cut to `REASON_MAX_CHARS` characters, an ellipsis last when
/// it was cut. The cut falls at the end, so the prefix the pipeline reads (`unconfirmed_submit`,
/// `page_error`, `faltan respuestas`...) always survives.
fn clean_reason(reason: &str) -> String {
    let line = plain_line(reason);
    if line.chars().count() <= REASON_MAX_CHARS {
        return line;
    }
    let cut: String = line.chars().take(REASON_MAX_CHARS - 1).collect();
    format!("{}…", cut.trim_end())
}

/// The reason for a page call that failed: `err` as one line, `page_error` before the submit click
/// was tried, `unconfirmed_submit` after it (nothing after the click is meant to fail, but if it
/// did the form may be out).
fn page_failure_reason(err: &anyhow::Error, clicked: bool) -> String {
    let prefix = if clicked {
        "unconfirmed_submit"
    } else {
        "page_error"
    };
    format!("{prefix}: {}", first_line(&format!("{err:#}")))
}

/// Why an application ended at its wall: `clicked` says whether the submit click was tried.
fn stuck_reason(clicked: bool, wall: Duration) -> String {
    if clicked {
        "unconfirmed_submit: la página dejó de responder después del clic".to_string()
    } else {
        format!("page_error: la página no respondió en {} s", wall.as_secs())
    }
}

/// The steps of `fill_application_within`.
#[allow(clippy::too_many_arguments)] // `fill_application`'s arguments plus the click flag
async fn fill_steps(
    page: &Page,
    job: &Job,
    cv_pdf: &Path,
    bank: &AnswerBank,
    mode: SubmitMode,
    shots_dir: &Path,
    nav: NavPolicy,
    clicked: &AtomicBool,
) -> anyhow::Result<SubmitResult> {
    if let Some(reason) = wall_for(&job.apply_url, mode)
        && reason != REASON_LISTING_ONLY
    {
        // Before any navigation. A listing-only link may still be followed: it can lead to a form.
        return Ok(needs_gonza(reason, None));
    }
    let target = match checked_link(job, nav) {
        Ok(target) => target,
        // Before any navigation: a link the browser must not follow goes back to Gonza.
        Err(refused) => return Ok(refused),
    };
    let shot = shots_dir.join(format!("{}.png", safe_key(&job.key())));
    page.set_default_timeout(ACTION_TIMEOUT_MS).await;

    let response = match open(page, &target, first_load_options()).await {
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
        return stop_for_gonza(
            page,
            shot,
            format!("page_error: HTTP {}", response.status()),
        )
        .await;
    }
    if let Some(reason) = landing_refusal(page, nav) {
        return stop_for_gonza(page, shot, reason).await;
    }
    if let Some(reason) = landing_wall(page, mode) {
        return stop_for_gonza(page, shot, reason).await;
    }
    if let Some(reason) = detect_blocker(page).await? {
        return stop_for_gonza(page, shot, reason).await;
    }

    let fields = scan_fields(page).await?;
    if let Some(reason) = landing_refusal(page, nav) {
        return stop_for_gonza(page, shot, reason).await;
    }
    if let Some(reason) = landing_wall(page, mode) {
        return stop_for_gonza(page, shot, reason).await;
    }
    if fields.is_empty() {
        return stop_for_gonza(page, shot, "no_form_found").await;
    }
    let plan = plan_fields(&fields, bank, cv_pdf);
    // Live is the only mode that clicks on its own, and it never goes on with a required answer
    // missing: it stops before typing a word into a form it will not send.
    if mode == SubmitMode::Live
        && let Some(reason) = leftover_reason(&plan.missing, &[])
    {
        return stop_for_gonza(page, shot, reason).await;
    }
    // A form with nowhere to attach the CV is not an application Gonza wants sent, or approved
    // from a dry run that looked fine: Live would click a button on a page where nothing was
    // filled (a header search box, a newsletter field). In Assist mode Gonza sees the page himself.
    let takes_the_cv = plan
        .actions
        .iter()
        .any(|action| matches!(action.value, FillValue::File(_)));
    if mode != SubmitMode::Assist && !takes_the_cv {
        return stop_for_gonza(
            page,
            shot,
            "sin campo de CV: el formulario no tiene dónde adjuntar el CV",
        )
        .await;
    }
    // Fill everything the plan can answer. One field that fails (an option the form does not
    // have, a control that refuses the value) must not hide the others, so every action is tried
    // and the failures are reported together. Required questions with no answer stay untouched.
    // An OPTIONAL field whose list has no option for the answer is not a failure: it is skipped and
    // stays as the page had it, like an optional radio group with no matching option.
    let mut failures = Vec::new();
    let mut failed = Vec::new();
    apply_all(page, &plan.actions, &fields, &mut failures, &mut failed).await;
    // Answering can open new questions ("Yes" to a referral opens "Describe it", required). The page
    // is read again, once: the controls that were not there before are planned and filled in a
    // second round, and a required one the bank cannot answer is reported like any other. The
    // read gives every control a new number, so the first round's plan and failures are carried
    // over to them; nothing already filled is touched again.
    let reread = scan_once(page).await?;
    if reread.is_empty() {
        return stop_for_gonza(
            page,
            shot,
            "page_error: el formulario desapareció al releerlo después de llenarlo",
        )
        .await;
    }
    let carried = carried_over(&fields, &reread);
    let appeared = new_controls(&reread, &carried);
    let mut plan = renumbered(plan, &carried);
    let mut failed: Vec<usize> = failed
        .iter()
        .filter_map(|idx| carried.get(idx).copied())
        .collect();
    let fields = reread;
    if !appeared.is_empty() {
        let extra = plan_fields(&appeared, bank, cv_pdf);
        apply_all(page, &extra.actions, &fields, &mut failures, &mut failed).await;
        plan = merged(plan, extra);
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
    // Set when the click call itself failed: it proves nothing about whether the form went out.
    let mut click_error: Option<String> = None;
    let outcome = if mode == SubmitMode::Assist {
        eprintln!("{}", assist_banner(job, leftover.as_deref()));
        await_confirmation(page, &before, mode, ASSIST_TIMEOUT_MS).await
    } else {
        // A widget injected while the form was being filled still stops a real submission.
        if let Some(reason) = detect_blocker(page).await? {
            return Ok(needs_gonza(reason, Some(shot)));
        }
        let has_button = tag_submit(page, &plan).await?;
        if !has_button {
            return Ok(needs_gonza("no_submit_button", Some(shot)));
        }
        // The button was found and is visible, so a click that comes back with an error (the click
        // started a navigation and the page went away, or it took longer than its deadline) does
        // not prove the form stayed unsent. It is waited on like any other click, and if the page
        // does not confirm it is an unconfirmed submit: the next run must not send it again.
        clicked.store(true, Ordering::SeqCst);
        if let Err(err) = page.locator(SUBMIT_SELECTOR).click(None).await {
            click_error = Some(click_error_reason(&err));
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
        Confirmation::Silent | Confirmation::Closed => needs_gonza(
            click_error.unwrap_or_else(|| "unconfirmed_submit: sin confirmación visible".into()),
            screenshot,
        ),
    })
}

/// Closes `page`, waiting at most `CLOSE_STUCK_PAGE_MS` for it: a tab that stopped answering must
/// not hold the next application.
pub(crate) async fn close_within_limit(page: &Page) {
    let _ = tokio::time::timeout(Duration::from_millis(CLOSE_STUCK_PAGE_MS), page.close()).await;
}

/// What tells one tab from another in the browser's list of pages.
fn page_id(page: &Page) -> String {
    page.guid().to_string()
}

/// Fills application forms in a persistent Chromium profile (cookies and logins survive runs).
pub struct BrowserFiller {
    // Field order matters: the context is dropped before the driver that owns it, and the proxy
    // the browser reaches the web through goes last.
    context: BrowserContext,
    _playwright: Playwright,
    egress: Egress,
    /// Which addresses the browser may open; strict outside the Chromium tests.
    nav: NavPolicy,
}

impl BrowserFiller {
    /// Starts the browser's proxy, the driver and Chromium with `profile_dir` as the user data
    /// directory. `headed` shows the browser window (the CLI's `--headed`). Without the proxy the
    /// browser does not start.
    pub async fn launch(profile_dir: &Path, headed: bool) -> anyhow::Result<BrowserFiller> {
        tokio::fs::create_dir_all(profile_dir)
            .await
            .with_context(|| format!("creating {}", profile_dir.display()))?;
        let egress = Egress::start(Arc::new(SystemResolver))
            .await
            .context("starting the browser's proxy")?;
        let playwright = Playwright::launch()
            .await
            .context("starting the Playwright driver")?;
        let context = crate::launch::start_persistent(
            &playwright,
            profile_dir,
            crate::launch::persistent_options(headed, &egress),
        )
        .await
        .context("launching Chromium with the persistent profile")?;
        Ok(BrowserFiller {
            context,
            _playwright: playwright,
            egress,
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
        mode: FillMode,
        shots_dir: &Path,
    ) -> anyhow::Result<SubmitResult> {
        // The tabs that exist now are not this application's: whatever else is open when it ends
        // (a popup the page opened, an ad) is.
        let known: HashSet<String> = self.context.pages().iter().map(page_id).collect();
        let page = self.context.new_page().await?;
        let mode = mode.submit_mode();
        let mark = self.egress.mark();
        let result = fill_application(&page, job, cv_pdf, bank, mode, shots_dir, self.nav).await;
        for opened in self.context.pages() {
            if !known.contains(&page_id(&opened)) {
                close_within_limit(&opened).await;
            }
        }
        close_within_limit(&page).await;
        result.map(|result| explain_tunnel_failure(result, &self.egress, mark))
    }
}

#[cfg(test)]
mod scan_tests;

#[cfg(test)]
mod browser_tests;

#[cfg(test)]
mod dropdown_tests;

#[cfg(test)]
mod navigation_tests;

#[cfg(test)]
mod plan_tests;

#[cfg(test)]
mod plan_no_guess_tests;

#[cfg(test)]
mod support_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use jobbot_domain::fixtures::make_job_with;
    use std::path::PathBuf;

    #[test]
    fn the_first_load_has_its_own_longer_deadline_than_a_field_action() {
        let options = first_load_options();
        assert_eq!(options.timeout, Some(Duration::from_secs(30)));
        assert_eq!(options.wait_until, Some(WaitUntil::DomContentLoaded));
        assert!(options.timeout.unwrap() > Duration::from_millis(ACTION_TIMEOUT_MS as u64));
    }

    #[test]
    fn a_failed_page_call_is_named_by_its_first_line_and_by_whether_the_click_was_tried() {
        let err = anyhow::anyhow!(
            "Execution context was destroyed
  at call log"
        )
        .context("scanning the form fields");
        assert_eq!(
            page_failure_reason(&err, false),
            "page_error: scanning the form fields: Execution context was destroyed"
        );
        assert_eq!(
            page_failure_reason(&err, true),
            "unconfirmed_submit: scanning the form fields: Execution context was destroyed"
        );
    }

    #[test]
    fn a_reason_is_one_clean_line_of_at_most_500_characters() {
        assert_eq!(
            clean_reason("faltan respuestas: \u{202e}Phone\u{200b} *\n\x1b[31mred\x07;\tEmail"),
            "faltan respuestas: Phone * [31mred ; Email"
        );
        let long = format!("page_error: {}", "ñ".repeat(2_000));
        let cleaned = clean_reason(&long);
        assert_eq!(cleaned.chars().count(), REASON_MAX_CHARS);
        assert!(cleaned.ends_with('…'));
        // Exactly at the limit nothing is cut.
        let exact = format!("page_error: {}", "ñ".repeat(REASON_MAX_CHARS - 12));
        assert_eq!(clean_reason(&exact), exact);
    }

    #[test]
    fn the_prefix_of_a_reason_survives_a_megabyte_of_page_text() {
        let message = "x".repeat(1_000_000);
        for prefix in ["unconfirmed_submit", "page_error", "faltan respuestas"] {
            let cleaned = clean_reason(&format!("{prefix}: {message}"));
            assert!(cleaned.starts_with(&format!("{prefix}: x")), "{prefix}");
            assert_eq!(cleaned.chars().count(), REASON_MAX_CHARS, "{prefix}");
        }
    }

    #[test]
    fn an_application_stuck_at_its_wall_says_whether_the_click_was_tried() {
        assert_eq!(APPLICATION_WALL, Duration::from_secs(180));
        assert_eq!(
            stuck_reason(false, APPLICATION_WALL),
            "page_error: la página no respondió en 180 s"
        );
        assert_eq!(
            stuck_reason(true, APPLICATION_WALL),
            "unconfirmed_submit: la página dejó de responder después del clic"
        );
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
            let result = checked_link(&job, NavPolicy::STRICT)
                .err()
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
        assert!(checked_link(&fine, NavPolicy::STRICT).is_ok());
    }

    /// The `.rs` files under `dir`, at any depth.
    fn rust_files(dir: &Path) -> Vec<PathBuf> {
        let mut files = Vec::new();
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                files.extend(rust_files(&path));
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                files.push(path);
            }
        }
        files
    }

    #[test]
    fn shipped_code_sends_the_browser_only_to_an_address_that_passed_the_rules() {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut gotos = Vec::new();
        for path in rust_files(&src) {
            let test_file = path
                .file_stem()
                .is_some_and(|stem| stem.to_string_lossy().ends_with("_tests"));
            if test_file {
                continue;
            }
            let text = std::fs::read_to_string(&path)
                .unwrap()
                .replace("\r\n", "\n");
            // Each file's test modules sit at its end, after its first `#[cfg(test)]`.
            let shipped = text.split("\n#[cfg(test)]\n").next().unwrap();
            gotos.extend(
                shipped
                    .lines()
                    .filter(|line| line.contains(".goto("))
                    .map(|line| line.trim().to_string()),
            );
        }
        assert_eq!(
            gotos,
            ["page.goto(url.as_str(), options).await"],
            "the one goto is `open`'s, which takes a NavigableUrl"
        );
        let form = std::fs::read_to_string(src.join("form.rs")).unwrap();
        let open = &form[form.find("async fn open(").expect("form.rs has `open`")..];
        assert!(
            open[..open.find(')').unwrap()].contains("url: &NavigableUrl"),
            "`open` takes a NavigableUrl"
        );
    }
}
