//! The form inspector: a dry run of a handoff form in its own headless Chromium that also lists
//! every question the form asked, for the answer sheet Gonza sends the form from.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use async_trait::async_trait;
use jobbot_app::ports::FormInspector;
use jobbot_domain::answers::AnswerBank;
use jobbot_domain::form::{FieldInfo, SubmitMode};
use jobbot_domain::handoff::{FieldReport, FormInspection, page_shows_the_form, sheet_fields};
use jobbot_domain::models::Job;
use jobbot_domain::navigation::NavPolicy;
use playwright_rs::{Browser, Page, Playwright};

use crate::egress::{Egress, SystemResolver, explain_tunnel_failure};
use crate::form::{APPLICATION_WALL, fill_application_within, needs_gonza};

/// The same scan the filler runs on the page (`FieldInfo` is its output).
const FIELD_JS: &str = include_str!("form/field_scan.js");

/// Dry-runs the form open in `page` and scans it again: the page keeps showing the filled form
/// after a dry run, so the scan sees every question the form asked. The dry run and the scan share
/// one `wall`, like any other application: a page that stops answering ends the inspection as
/// `page_error: la página no respondió en <wall> s`.
async fn inspect_page(
    page: &Page,
    job: &Job,
    cv_pdf: &Path,
    bank: &AnswerBank,
    shots_dir: &Path,
    nav: NavPolicy,
    wall: Duration,
) -> anyhow::Result<FormInspection> {
    let started = tokio::time::Instant::now();
    let result = fill_application_within(
        page,
        job,
        cv_pdf,
        bank,
        SubmitMode::DryRun,
        shots_dir,
        nav,
        wall,
    )
    .await?;
    let mut report = FieldReport::Unreadable;
    if page_shows_the_form(&result) {
        let left = wall.saturating_sub(started.elapsed());
        let Ok(scanned) = tokio::time::timeout(left, page.evaluate(FIELD_JS, None::<&()>)).await
        else {
            let _ = page.close().await;
            return Ok(FormInspection {
                result: needs_gonza(
                    format!("page_error: la página no respondió en {} s", wall.as_secs()),
                    None,
                ),
                report,
            });
        };
        let scanned: Vec<FieldInfo> = scanned.context("scanning the form fields")?;
        if !scanned.is_empty() {
            report = FieldReport::Full(sheet_fields(&scanned, bank, cv_pdf));
        }
    }
    Ok(FormInspection { result, report })
}

/// A headless Chromium of its own, with no profile: it only looks at the form and never sends it.
pub struct BrowserInspector {
    // Field order matters: the browser is dropped before the driver that owns it, and the proxy
    // the browser reaches the web through goes last.
    browser: Browser,
    _playwright: Playwright,
    egress: Egress,
    /// Which addresses the browser may open; strict outside the Chromium tests.
    nav: NavPolicy,
}

impl BrowserInspector {
    /// Starts the browser's proxy, the driver and Chromium. Without the proxy the browser does
    /// not start.
    pub async fn launch() -> anyhow::Result<BrowserInspector> {
        let egress = Egress::start(Arc::new(SystemResolver))
            .await
            .context("starting the browser's proxy")?;
        let playwright = Playwright::launch()
            .await
            .context("starting the Playwright driver")?;
        let browser = playwright
            .chromium()
            .launch_with_options(crate::launch::guarded_launch_options(&egress))
            .await
            .context("launching Chromium")?;
        Ok(BrowserInspector {
            browser,
            _playwright: playwright,
            egress,
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
        let mark = self.egress.mark();
        let inspection = inspect_page(
            &page,
            job,
            cv_pdf,
            bank,
            shots_dir,
            self.nav,
            APPLICATION_WALL,
        )
        .await;
        let _ = page.close().await;
        inspection.map(|inspection| FormInspection {
            result: explain_tunnel_failure(inspection.result, &self.egress, mark),
            ..inspection
        })
    }
}

#[cfg(test)]
mod tests {
    use jobbot_domain::answers::AnswerEntry;
    use jobbot_domain::fixtures::make_job_with;
    use jobbot_domain::form::SubmitStatus;
    use jobbot_domain::handoff::SheetAnswer;

    use super::*;

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

    // ---- Browser test: `cargo test -p jobbot-browser --lib handoff:: -- --ignored --test-threads=1` ----

    /// The inspector gives a page that freezes the same wall as the filler: the inspection ends as
    /// a page error instead of waiting for ever for the scan.
    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "needs chromium"]
    async fn the_inspector_stops_waiting_for_a_page_that_freezes() {
        let dir = tempfile::tempdir().unwrap();
        let cv = dir.path().join("cv.pdf");
        std::fs::write(&cv, b"%PDF-1.4 fake").unwrap();
        let fixture = crate::test_support::fixtures_dir().join("hang.html");
        let url = format!(
            "file:///{}?at=late",
            fixture.display().to_string().replace('\\', "/")
        );
        let job = make_job_with(|j| j.apply_url = url);
        let playwright = Playwright::launch().await.unwrap();
        let browser = playwright
            .chromium()
            .launch_with_options(crate::launch::launch_options())
            .await
            .unwrap();
        let page = browser.new_page().await.unwrap();
        let inspection = inspect_page(
            &page,
            &job,
            &cv,
            &bank(),
            &dir.path().join("shots"),
            NavPolicy::LOCAL_TESTS,
            Duration::from_secs(8),
        )
        .await
        .unwrap();
        browser.close().await.unwrap();
        assert_eq!(inspection.result.status, SubmitStatus::NeedsGonza);
        assert_eq!(
            inspection.result.reason.as_deref(),
            Some("page_error: la página no respondió en 8 s")
        );
        assert!(matches!(inspection.report, FieldReport::Unreadable));
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "needs chromium"]
    async fn the_browser_inspector_lists_every_question_of_an_ashby_style_form() {
        let dir = tempfile::tempdir().unwrap();
        let cv = dir.path().join("cv.pdf");
        std::fs::write(&cv, b"%PDF-1.4 fake").unwrap();
        let fixture = crate::test_support::fixtures_dir().join("ashby_form.html");
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
        // What the sheet is written from: the first question and the options of a Yes/No pair.
        assert_eq!(
            (fields[0].label.as_str(), fields[0].required),
            ("Name", true)
        );
        assert_eq!(
            by_label("Will you now or in the future require sponsorship").options,
            ["Yes", "No"]
        );
    }
}
