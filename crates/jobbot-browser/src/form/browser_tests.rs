//! The form filler against Chromium and the offline form fixtures: whole applications in dry,
//! live and assist mode.
//!
//! Browser tests: `cargo test -p jobbot-browser --lib form:: -- --ignored --test-threads=1`

use super::support_tests::{Session, fixture_url, flagged_ids, full_bank};
use super::*;
use jobbot_domain::answers::AnswerEntry;
use jobbot_domain::fixtures::make_job_with;
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, ResponseTemplate};

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
            FillMode::DryRun,
            &dir.path().join("shots"),
        )
        .await
        .unwrap();
    assert_eq!(result.status, SubmitStatus::DryRun);
    assert!(result.screenshot.unwrap().exists());
    filler.close().await.unwrap();
}

/// The tabs a page opens (popups, ads) are closed with the application: they are not left
/// behind to use up the browser for the rest of the run.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn tabs_a_page_opens_do_not_outlive_its_application() {
    let dir = tempfile::tempdir().unwrap();
    let cv = dir.path().join("cv.pdf");
    std::fs::write(&cv, b"%PDF-1.4 fake").unwrap();
    let mut filler = BrowserFiller::launch(&dir.path().join("profile"), false)
        .await
        .unwrap();
    filler.nav = NavPolicy::LOCAL_TESTS;
    let open_tabs = |filler: &BrowserFiller| {
        filler
            .context
            .pages()
            .iter()
            .filter(|page| !page.is_closed())
            .count()
    };
    let before = open_tabs(&filler);
    let path = crate::test_support::fixtures_dir().join("popups.html");
    let url = format!("file:///{}", path.display().to_string().replace('\\', "/"));
    let job = make_job_with(|j| j.apply_url = url);
    let result = filler
        .submit(
            &job,
            &cv,
            &full_bank(),
            FillMode::DryRun,
            &dir.path().join("shots"),
        )
        .await
        .unwrap();
    let after = open_tabs(&filler);
    filler.close().await.unwrap();
    assert_eq!(result.status, SubmitStatus::DryRun, "{:?}", result.reason);
    assert_eq!(after, before, "tabs left open by the page");
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
            .ends_with("thank-you.html")
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
    let shots = crate::test_support::workspace_root()
        .join("target")
        .join("real-shots");
    let filler = BrowserFiller::launch(&dir.path().join("profile"), true)
        .await
        .unwrap();
    for url in urls.split(',').map(str::trim).filter(|url| !url.is_empty()) {
        let job = make_job_with(|j| j.apply_url = url.to_string());
        let result = filler
            .submit(&job, &cv, &full_bank(), FillMode::DryRun, &shots)
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
