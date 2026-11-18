//! Browser tests of the drop-down lists the filler picks from: custom lists, native selects,
//! typeaheads and lists that redraw.
//!
//! Run with `cargo test -p jobbot-browser --lib form:: -- --ignored --test-threads=1`

use super::apply::apply_action;
use super::support_tests::{Session, flagged_ids, full_bank};
use super::*;
use jobbot_domain::answers::AnswerEntry;

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
    let path = crate::test_support::fixtures_dir().join("ats_dropdowns.html");
    let url = format!("file:///{}", path.display().to_string().replace('\\', "/"));
    session.page.goto(&url, None).await.unwrap();
    let bank = AnswerBank {
        entries: vec![
            AnswerEntry::new(&["work location in the united states"], Some("No")),
            AnswerEntry::new(&["=country"], Some("Chile")),
            AnswerEntry::new(&["=location city"], Some("Santiago, Chile")),
            AnswerEntry::new(&["=school"], Some("Example Professional Institute"))
                .with_alternatives(&["Example PI", "Example", "Other"]),
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

/// The drop-down fixture with the set of questions `set` (see tests/fixtures/ats_dropdowns.html).
fn dropdowns_url(set: &str) -> String {
    let path = crate::test_support::fixtures_dir().join("ats_dropdowns.html");
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
async fn a_list_redrawn_between_choosing_and_clicking_is_read_again() {
    let session = Session::start().await;
    session
        .page
        .goto(&dropdowns_url("redraw"), None)
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
    // The option chosen is replaced by an identical one just before the click lands: the
    // filler looks it up again instead of waiting for a node that is gone.
    let started = std::time::Instant::now();
    apply_action(&session.page, grad[0]).await.unwrap();
    assert_eq!(dropdown_value(&session, "grad").await, "Slow University");
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "took {:?}",
        started.elapsed()
    );
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
