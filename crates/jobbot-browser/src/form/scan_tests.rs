//! The field scan (`FIELD_JS`) against offline copies of real Greenhouse and Ashby forms.
//! Browser tests run with `cargo test -p jobbot-browser --lib form::scan_tests -- --ignored --test-threads=1`.

use std::path::Path;

use playwright_rs::{Browser, GotoOptions, Page, Playwright, WaitUntil};

use super::*;
use jobbot_domain::answers::AnswerEntry;
use jobbot_domain::fixtures::make_job_with;

fn field(
    idx: usize,
    tag: &str,
    kind: &str,
    label: &str,
    option: &str,
    required: bool,
) -> FieldInfo {
    FieldInfo {
        idx,
        tag: tag.into(),
        kind: kind.into(),
        label: label.into(),
        required,
        option: option.into(),
        name: "group".into(),
        checked: false,
    }
}

/// What the Greenhouse scan reports for a required checkbox group: every option carries the
/// question as its label, so the planner reports the question once, not once per option.
#[test]
fn a_required_checkbox_group_is_one_missing_question() {
    let question = "How did you hear about this opportunity? (select all that apply) *";
    let fields = [
        field(0, "input", "checkbox", question, "LinkedIn", true),
        field(1, "input", "checkbox", question, "Glassdoor", true),
        field(2, "input", "checkbox", question, "Indeed", true),
    ];
    let plan = plan_fields(&fields, &AnswerBank::default(), Path::new("cv.pdf"));
    assert_eq!(plan.missing, vec![question]);
    assert!(plan.actions.is_empty());
}

/// A group with a single option is labelled "question: option", so a consent phrase in either
/// half still lets the box be ticked, and anything else stays a missing answer.
#[test]
fn a_single_option_group_is_judged_by_its_question_and_its_option() {
    let fields = [field(
        0,
        "input",
        "checkbox",
        "Applicant Privacy Notice: Acknowledge",
        "Acknowledge",
        true,
    )];
    let allowed = AnswerBank {
        consent_phrases: vec!["privacy notice".into()],
        ..AnswerBank::default()
    };
    let plan = plan_fields(&fields, &allowed, Path::new("cv.pdf"));
    assert!(plan.missing.is_empty());
    assert_eq!(plan.actions.len(), 1);
    let plan = plan_fields(&fields, &AnswerBank::default(), Path::new("cv.pdf"));
    assert_eq!(plan.missing, vec!["Applicant Privacy Notice: Acknowledge"]);
}

#[test]
fn the_scan_output_with_the_new_kinds_deserializes() {
    let json = r#"[
        {"idx":0,"tag":"input","type":"combobox","label":"Country*","required":true,"option":"","name":""},
        {"idx":1,"tag":"button","type":"radio","label":"Authorized to work?","required":true,"option":"Yes","name":"auth"},
        {"idx":2,"tag":"input","type":"date","label":"Start date","required":false}
    ]"#;
    let fields: Vec<FieldInfo> = serde_json::from_str(json).unwrap();
    assert_eq!(fields[0].kind, "combobox");
    assert_eq!(
        (fields[1].tag.as_str(), fields[1].kind.as_str()),
        ("button", "radio")
    );
    assert_eq!(fields[2].kind, "date");
    assert_eq!(fields[2].option, "");
}

// ---- Browser tests ----

fn fixture_url(file: &str) -> String {
    let path = crate::test_support::fixtures_dir().join(file);
    format!("file:///{}", path.display().to_string().replace('\\', "/"))
}

/// A headless Chromium page; `stop` closes the browser.
struct Scanner {
    _playwright: Playwright,
    browser: Browser,
    page: Page,
}

impl Scanner {
    async fn start() -> Scanner {
        let playwright = Playwright::launch().await.unwrap();
        let browser = playwright
            .chromium()
            .launch_with_options(crate::launch::launch_options())
            .await
            .unwrap();
        let page = browser.new_page().await.unwrap();
        Scanner {
            _playwright: playwright,
            browser,
            page,
        }
    }

    async fn open(&self, url: &str) {
        let options = GotoOptions::new().wait_until(WaitUntil::Load);
        self.page.goto(url, options).await.unwrap();
    }

    async fn scan(&self, file: &str) -> Vec<FieldInfo> {
        self.open(&fixture_url(file)).await;
        scan_fields(&self.page).await.unwrap()
    }

    async fn stop(self) {
        self.browser.close().await.unwrap();
    }
}

async fn scan_fixture(file: &str) -> Vec<FieldInfo> {
    let scanner = Scanner::start().await;
    let fields = scanner.scan(file).await;
    scanner.stop().await;
    fields
}

/// `idx tag/kind "label" [option="..."] [*] [(selected)]`: one line per control, `*` when
/// required.
fn summary(field: &FieldInfo) -> String {
    let mut line = format!(
        "{} {}/{} {:?}",
        field.idx, field.tag, field.kind, field.label
    );
    if !field.option.is_empty() {
        line.push_str(&format!(" option={:?}", field.option));
    }
    if field.required {
        line.push_str(" *");
    }
    if field.checked {
        line.push_str(" (selected)");
    }
    line
}

fn summaries(fields: &[FieldInfo]) -> Vec<String> {
    fields.iter().map(summary).collect()
}

fn with_label<'a>(fields: &'a [FieldInfo], label: &str) -> Vec<&'a FieldInfo> {
    fields
        .iter()
        .filter(|field| field.label.trim() == label)
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn greenhouse_form_is_scanned_control_by_control() {
    let fields = scan_fixture("greenhouse_form.html").await;
    let expected = [
        r#"0 input/text "First Name*" *"#,
        r#"1 input/text "Preferred First Name""#,
        r#"2 input/text "Email*" *"#,
        r#"3 input/combobox "Country*" *"#,
        r#"4 input/tel "Phone*" *"#,
        r#"5 input/combobox "Location (City)*" *"#,
        r#"6 input/file "Resume/CV*" *"#,
        r#"7 input/file "Cover Letter""#,
        r#"8 input/combobox "School*" *"#,
        r#"9 input/combobox "Degree*" *"#,
        r#"10 input/combobox "Discipline*" *"#,
        r#"11 input/text "LinkedIn Profile""#,
        r#"12 input/checkbox "How did you hear about this opportunity? (select all that apply) *" option="LinkedIn" *"#,
        r#"13 input/checkbox "How did you hear about this opportunity? (select all that apply) *" option="Glassdoor" *"#,
        r#"14 input/checkbox "How did you hear about this opportunity? (select all that apply) *" option="Indeed" *"#,
        r#"15 input/checkbox "Please confirm whether any of the below applies to you. Select all that apply. *" option="Citizen or permanent resident of a restricted country" *"#,
        r#"16 input/checkbox "Please confirm whether any of the below applies to you. Select all that apply. *" option="None of the above" *"#,
        r#"17 input/checkbox "Point of Data Transfer: Acknowledge" option="Acknowledge" *"#,
        r#"18 input/combobox "Will you need sponsorship now or in the future? *" *"#,
        r#"19 input/combobox "Which of the following most influenced your decision to apply? Select all that apply.""#,
        r#"20 textarea/ "Compensation Expectations*" *"#,
    ];
    assert_eq!(summaries(&fields), expected);
}

/// Root cause of the "campo sin etiqueta" reported on every real Greenhouse form: react-select
/// puts an invisible, unlabeled `required` input (`aria-hidden`, `tabindex=-1`) after each
/// drop-down, and the scan counted it as a required control.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn react_select_shadow_inputs_are_not_fields() {
    let fields = scan_fixture("greenhouse_form.html").await;
    let unlabeled: Vec<String> = fields
        .iter()
        .filter(|field| field.label.trim().is_empty())
        .map(summary)
        .collect();
    assert!(unlabeled.is_empty(), "unlabeled controls: {unlabeled:?}");
}

/// The helper is recognised as the invisible `...-requiredInput` sibling of a combobox, not by
/// `aria-hidden` alone: the same page without that attribute used to send every Greenhouse form to
/// Gonza as "campo sin etiqueta". An unlabeled required control that is not such a helper (no
/// combobox beside it) is a real finding and stays in the list.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn react_select_helper_inputs_are_skipped_but_other_unlabeled_controls_are_not() {
    let fields = scan_fixture("greenhouse_helpers.html").await;
    let expected = [
        r#"0 input/combobox "Country*" *"#,
        r#"1 input/combobox "Degree*" *"#,
        r#"2 input/combobox "Discipline*" *"#,
        r#"3 input/text "" *"#,
        r#"4 input/ "" *"#,
    ];
    assert_eq!(summaries(&fields), expected);
    // With every drop-down answered, the only open questions are the two real unlabeled controls.
    let bank = AnswerBank {
        entries: vec![
            AnswerEntry::new(&["=country"], Some("Chile")),
            AnswerEntry::new(&["=degree"], Some("Bachelor's Degree")),
            AnswerEntry::new(&["=discipline"], Some("Computer Science")),
        ],
        consent_phrases: vec![],
    };
    let plan = plan_fields(&fields, &bank, Path::new("cv.pdf"));
    assert_eq!(plan.missing, vec!["campo sin etiqueta"]);
    assert_eq!(plan.actions.len(), 3);
}

/// The visible label of a Greenhouse file input is a hidden "Attach"; the caption is on the
/// group around it. Without it the CV input did not look like a CV input.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn greenhouse_file_inputs_take_the_caption_of_their_group() {
    let fields = scan_fixture("greenhouse_form.html").await;
    let files: Vec<&FieldInfo> = fields.iter().filter(|f| f.kind == "file").collect();
    assert_eq!(files.len(), 2);
    assert_eq!(
        (files[0].label.as_str(), files[0].required),
        ("Resume/CV*", true)
    );
    assert_eq!(
        (files[1].label.as_str(), files[1].required),
        ("Cover Letter", false)
    );
    let plan = plan_fields(&fields, &AnswerBank::default(), Path::new("cv.pdf"));
    assert!(
        plan.actions
            .iter()
            .any(|a| a.value == FillValue::File(PathBuf::from("cv.pdf")) && a.label == "Resume/CV*"),
        "the CV must be uploaded to the Resume/CV input: {:?}",
        plan.actions
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn greenhouse_checkbox_groups_are_one_question_with_their_options() {
    let fields = scan_fixture("greenhouse_form.html").await;
    let group = with_label(
        &fields,
        "How did you hear about this opportunity? (select all that apply) *",
    );
    let options: Vec<&str> = group.iter().map(|f| f.option.as_str()).collect();
    assert_eq!(options, ["LinkedIn", "Glassdoor", "Indeed"]);
    assert!(group.iter().all(|f| f.required && f.kind == "checkbox"));
    // One name identifies the group.
    assert!(group.iter().all(|f| f.name == "question_2[]"));
    // The planner then reports the question once instead of once per option.
    let plan = plan_fields(&fields, &AnswerBank::default(), Path::new("cv.pdf"));
    let hear = plan
        .missing
        .iter()
        .filter(|label| label.starts_with("How did you hear"))
        .count();
    assert_eq!(hear, 1, "missing: {:?}", plan.missing);
    assert!(!plan.missing.iter().any(|label| label == "LinkedIn"));
}

/// Whether each box of a group is ticked, as `true,false,...` in page order.
async fn ticked(scanner: &Scanner, ids: &[&str]) -> String {
    let expression = format!(
        "{}.map((id) => String(document.getElementById(id).checked)).join(',')",
        serde_json::to_string(ids).unwrap()
    );
    scanner.page.evaluate_value(&expression).await.unwrap()
}

const HEAR_IDS: [&str; 3] = ["question_2[]_1", "question_2[]_2", "question_2[]_3"];
const EXPORT_IDS: [&str; 2] = ["question_3[]_1", "question_3[]_2"];

/// "Select all that apply": the bank's answer lists the options to tick, every other box of the
/// group (and of the other groups) stays as it was.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn greenhouse_checkbox_group_is_ticked_from_the_bank() {
    let scanner = Scanner::start().await;
    let fields = scanner.scan("greenhouse_form.html").await;
    let bank = AnswerBank {
        entries: vec![AnswerEntry::new(
            &["how did you hear about this opportunity"],
            Some("LinkedIn; Indeed"),
        )],
        consent_phrases: vec![],
    };
    let plan = plan_fields(&fields, &bank, Path::new("cv.pdf"));
    assert!(
        !plan
            .missing
            .iter()
            .any(|label| label.starts_with("How did you hear")),
        "missing: {:?}",
        plan.missing
    );
    for action in plan
        .actions
        .iter()
        .filter(|action| action.label.starts_with("How did you hear"))
    {
        apply_action(&scanner.page, action).await.unwrap();
    }
    assert_eq!(ticked(&scanner, &HEAR_IDS).await, "true,false,true");
    assert_eq!(ticked(&scanner, &EXPORT_IDS).await, "false,false");
    scanner.stop().await;
}

/// Ashby names every checkbox after its own text, so the group is told by its fieldset: the one
/// option the bank names is ticked, the other is not.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn ashby_checkbox_group_is_ticked_from_the_bank() {
    let scanner = Scanner::start().await;
    let fields = scanner.scan("ashby_form.html").await;
    let bank = AnswerBank {
        entries: vec![AnswerEntry::new(&["how did you hear"], Some("Glassdoor"))],
        consent_phrases: vec![],
    };
    let plan = plan_fields(&fields, &bank, Path::new("cv.pdf"));
    let hear: Vec<&FillAction> = plan
        .actions
        .iter()
        .filter(|action| action.label.starts_with("How did you hear"))
        .collect();
    assert_eq!(hear.len(), 1, "actions: {:?}", plan.actions);
    apply_action(&scanner.page, hear[0]).await.unwrap();
    assert_eq!(
        ticked(
            &scanner,
            &[
                "hear-uuid-labeled-checkbox-0",
                "hear-uuid-labeled-checkbox-1"
            ]
        )
        .await,
        "false,true"
    );
    // The arbitration acknowledgement is a lone checkbox: the allow list never ticks it.
    let allowed = AnswerBank {
        consent_phrases: vec!["acknowledge".into()],
        ..bank
    };
    let plan = plan_fields(&fields, &allowed, Path::new("cv.pdf"));
    assert!(
        plan.missing
            .iter()
            .any(|label| label.starts_with("Applicant Arbitration Agreement")),
        "missing: {:?}",
        plan.missing
    );
    scanner.stop().await;
}

/// A checkbox group nobody answered is outlined on its first option, and the outline goes away
/// when Gonza ticks any option of it (not only the outlined one).
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn an_open_checkbox_group_is_flagged_until_gonza_ticks_an_option() {
    let scanner = Scanner::start().await;
    let dir = tempfile::tempdir().unwrap();
    let cv = dir.path().join("cv.pdf");
    std::fs::write(&cv, b"%PDF-1.4 fake").unwrap();
    let job = make_job_with(|j| j.apply_url = fixture_url("greenhouse_form.html"));
    fill_application(
        &scanner.page,
        &job,
        &cv,
        &AnswerBank::default(),
        SubmitMode::DryRun,
        &dir.path().join("shots"),
        NavPolicy::LOCAL_TESTS,
    )
    .await
    .unwrap();
    let flagged = |id: &'static str| {
        format!(r#"String(document.getElementById("{id}").hasAttribute("data-jb-todo"))"#)
    };
    let first: String = scanner
        .page
        .evaluate_value(&flagged("question_2[]_1"))
        .await
        .unwrap();
    assert_eq!(first, "true");
    // Only the first option carries the mark.
    let second: String = scanner
        .page
        .evaluate_value(&flagged("question_2[]_2"))
        .await
        .unwrap();
    assert_eq!(second, "false");
    scanner
        .page
        .locator(r#"[id="question_2[]_3"]"#)
        .check(None)
        .await
        .unwrap();
    let first: String = scanner
        .page
        .evaluate_value(&flagged("question_2[]_1"))
        .await
        .unwrap();
    assert_eq!(first, "false");
    scanner.stop().await;
}

/// The whole-page symptom: nothing but real questions in the list of missing answers.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn greenhouse_dry_run_lists_each_open_question_once() {
    let scanner = Scanner::start().await;
    let dir = tempfile::tempdir().unwrap();
    let cv = dir.path().join("cv.pdf");
    std::fs::write(&cv, b"%PDF-1.4 fake").unwrap();
    let job = make_job_with(|j| j.apply_url = fixture_url("greenhouse_form.html"));
    let result = fill_application(
        &scanner.page,
        &job,
        &cv,
        &AnswerBank::default(),
        SubmitMode::DryRun,
        &dir.path().join("shots"),
        NavPolicy::LOCAL_TESTS,
    )
    .await
    .unwrap();
    scanner.stop().await;
    assert_eq!(result.status, SubmitStatus::NeedsGonza);
    let reason = result.reason.unwrap();
    let missing = reason.strip_prefix("faltan respuestas: ").unwrap();
    let missing: Vec<&str> = missing.split("; ").collect();
    assert_eq!(
        missing,
        [
            "First Name*",
            "Email*",
            "Country*",
            "Phone*",
            "Location (City)*",
            "School*",
            "Degree*",
            "Discipline*",
            "How did you hear about this opportunity? (select all that apply) *",
            "Please confirm whether any of the below applies to you. Select all that apply. *",
            "Point of Data Transfer: Acknowledge",
            "Will you need sponsorship now or in the future? *",
            "Compensation Expectations*",
        ]
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn ashby_form_is_scanned_control_by_control() {
    let fields = scan_fixture("ashby_form.html").await;
    let expected = [
        r#"0 input/text "Name" *"#,
        r#"1 input/tel "Phone Number" *"#,
        r#"2 input/email "Email" *"#,
        r#"3 input/combobox "Location" *"#,
        r#"4 input/text "LinkedIn""#,
        r#"5 input/text "Current employer" *"#,
        r#"6 input/file "Resume" *"#,
        r#"7 input/date "When can you start a new role?" *"#,
        r#"8 button/radio "Are you authorized to work in the country where the job is located?" option="Yes" *"#,
        r#"9 button/radio "Are you authorized to work in the country where the job is located?" option="No" *"#,
        r#"10 button/radio "Will you now or in the future require sponsorship for employment visa status in this country?" option="Yes" *"#,
        r#"11 button/radio "Will you now or in the future require sponsorship for employment visa status in this country?" option="No" *"#,
        r#"12 button/radio "Would you consider an office-based role?" option="Yes""#,
        r#"13 button/radio "Would you consider an office-based role?" option="No" (selected)"#,
        r#"14 input/radio "How many years of relevant professional experience do you have?" option="1-2 years" *"#,
        r#"15 input/radio "How many years of relevant professional experience do you have?" option="3-5 years" * (selected)"#,
        r#"16 textarea/ "Additional Information""#,
        r#"17 input/checkbox "How did you hear about this opportunity? (select all that apply)" option="LinkedIn""#,
        r#"18 input/checkbox "How did you hear about this opportunity? (select all that apply)" option="Glassdoor""#,
        r#"19 input/checkbox "Applicant Arbitration Agreement Acknowledgement: I acknowledge that I have read the Arbitration Agreement." option="I acknowledge that I have read the Arbitration Agreement." *"#,
    ];
    assert_eq!(summaries(&fields), expected);
}

/// The "Autofill from resume" box has its own file input; filling it would make Ashby parse the
/// CV and rewrite the fields, so it is not part of the application.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn ashby_autofill_upload_is_not_a_field() {
    let fields = scan_fixture("ashby_form.html").await;
    let files: Vec<&str> = fields
        .iter()
        .filter(|f| f.kind == "file")
        .map(|f| f.label.as_str())
        .collect();
    assert_eq!(files, ["Resume"]);
}

/// The Location typeahead and the date picker have no id: the label's `for` points at nothing,
/// and the placeholder ("Start typing...", "Pick date...") was used as their label.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn ashby_typeahead_and_date_picker_take_the_label_of_their_entry() {
    let fields = scan_fixture("ashby_form.html").await;
    let location = with_label(&fields, "Location");
    assert_eq!(location.len(), 1);
    assert_eq!(
        (location[0].kind.as_str(), location[0].required),
        ("combobox", true)
    );
    let start = with_label(&fields, "When can you start a new role?");
    assert_eq!(start.len(), 1);
    assert_eq!((start[0].kind.as_str(), start[0].required), ("date", true));
    assert!(
        !fields
            .iter()
            .any(|f| f.label == "Start typing..." || f.label == "Pick date..."),
        "a placeholder was used as a label: {:?}",
        summaries(&fields)
    );
}

/// The scan now gives the date picker its real question ("When can you start a new role?", not the
/// placeholder), which the shipped bank answers with "Immediately". A date picker takes no such
/// text: it stays a missing answer and nothing is typed into it.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn ashby_date_picker_stays_unanswered_with_the_shipped_bank() {
    let fields = scan_fixture("ashby_form.html").await;
    let bank_path = crate::test_support::workspace_root()
        .join("profile")
        .join("answers.toml");
    let bank = crate::test_support::load_bank(&bank_path).unwrap();
    let question = "When can you start a new role?";
    assert!(
        bank.lookup(question)
            .is_some_and(|entry| !entry.candidates().is_empty()),
        "the shipped bank must answer this question for the test to mean anything"
    );
    let start = with_label(&fields, question);
    assert_eq!((start.len(), start[0].kind.as_str()), (1, "date"));
    let plan = plan_fields(&fields, &bank, Path::new("cv.pdf"));
    assert!(plan.missing.iter().any(|label| label == question));
    assert!(
        !plan.actions.iter().any(|action| action.idx == start[0].idx),
        "text was planned for the date picker: {:?}",
        plan.actions
    );
}

/// Yes/No questions are two toggle buttons and a hidden checkbox: the scan found nothing at all.
/// They are reported as a radio group whose options are the two buttons.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn ashby_yes_no_buttons_are_a_radio_group() {
    let fields = scan_fixture("ashby_form.html").await;
    let question = "Will you now or in the future require sponsorship for employment visa status in this country?";
    let group = with_label(&fields, question);
    let options: Vec<(&str, &str, &str)> = group
        .iter()
        .map(|f| (f.tag.as_str(), f.kind.as_str(), f.option.as_str()))
        .collect();
    assert_eq!(
        options,
        [("button", "radio", "Yes"), ("button", "radio", "No")]
    );
    assert!(group.iter().all(|f| f.required));
    assert_eq!(group[0].name, "sponsor-uuid");
    assert_eq!(group[1].name, "sponsor-uuid");
    // With the bank's answer the planner picks the "No" button.
    let bank = AnswerBank {
        entries: vec![AnswerEntry::new(&["sponsorship"], Some("No"))],
        consent_phrases: vec![],
    };
    let plan = plan_fields(&fields, &bank, Path::new("cv.pdf"));
    let no = group[1].idx;
    assert!(
        plan.actions
            .iter()
            .any(|a| a.idx == no && a.value == FillValue::Check),
        "actions: {:?}",
        plan.actions
    );
}

/// Which option of a group is selected right now: a pressed toggle button or a checked radio.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn ashby_toggle_buttons_and_radios_report_which_option_is_selected() {
    let fields = scan_fixture("ashby_form.html").await;
    let state = |label: &str| -> Vec<(String, bool)> {
        with_label(&fields, label)
            .iter()
            .map(|f| (f.option.clone(), f.checked))
            .collect()
    };
    let yes_no = |yes: bool, no: bool| vec![("Yes".to_string(), yes), ("No".to_string(), no)];
    // Pressed by the fixture; the other Yes/No questions have nothing pressed.
    assert_eq!(
        state("Would you consider an office-based role?"),
        yes_no(false, true)
    );
    assert_eq!(
        state(
            "Will you now or in the future require sponsorship for employment visa status in this country?"
        ),
        yes_no(false, false)
    );
    assert_eq!(
        state("How many years of relevant professional experience do you have?"),
        vec![
            ("1-2 years".to_string(), false),
            ("3-5 years".to_string(), true)
        ]
    );
}

/// `aria-pressed` of the buttons of the Yes/No question at `field_path`, in page order.
async fn pressed(scanner: &Scanner, field_path: &str) -> String {
    let expression = format!(
        r#"Array.from(document.querySelectorAll('[data-field-path="{field_path}"] button')).map((b) => b.getAttribute('aria-pressed')).join(',')"#
    );
    scanner.page.evaluate_value(&expression).await.unwrap()
}

/// A Yes/No answer is a click on one of two buttons: `check()` only works on checkboxes and radio
/// buttons, so the planned answer never reached the page. The click must leave the right button
/// pressed and a second run over the same form must not toggle it off again.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn ashby_yes_no_answer_is_clicked_once_and_stays_selected() {
    let scanner = Scanner::start().await;
    let fields = scanner.scan("ashby_form.html").await;
    let bank = AnswerBank {
        entries: vec![AnswerEntry::new(&["sponsorship"], Some("No"))],
        consent_phrases: vec![],
    };
    let plan = plan_fields(&fields, &bank, Path::new("cv.pdf"));
    let action = plan
        .actions
        .iter()
        .find(|a| a.label.starts_with("Will you now"))
        .expect("the sponsorship question is answered");
    assert_eq!(pressed(&scanner, "sponsor-uuid").await, "false,false");
    apply_action(&scanner.page, action).await.unwrap();
    assert_eq!(pressed(&scanner, "sponsor-uuid").await, "false,true");
    apply_action(&scanner.page, action).await.unwrap();
    assert_eq!(pressed(&scanner, "sponsor-uuid").await, "false,true");
    // The other questions were not touched.
    assert_eq!(pressed(&scanner, "auth-uuid").await, "false,false");
    scanner.stop().await;
}

/// The whole Ashby path in a dry run: the answered Yes/No is pressed, the open one is outlined for
/// Gonza on its first button, and the outline goes away once he presses an answer himself (an
/// outline that stays after he answered would make the screenshot and the assisted browser lie).
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn ashby_dry_run_presses_the_answer_and_flags_the_open_yes_no() {
    let scanner = Scanner::start().await;
    let dir = tempfile::tempdir().unwrap();
    let cv = dir.path().join("cv.pdf");
    std::fs::write(&cv, b"%PDF-1.4 fake").unwrap();
    let job = make_job_with(|j| j.apply_url = fixture_url("ashby_form.html"));
    let bank = AnswerBank {
        entries: vec![AnswerEntry::new(&["sponsorship"], Some("No"))],
        consent_phrases: vec![],
    };
    let result = fill_application(
        &scanner.page,
        &job,
        &cv,
        &bank,
        SubmitMode::DryRun,
        &dir.path().join("shots"),
        NavPolicy::LOCAL_TESTS,
    )
    .await
    .unwrap();
    assert_eq!(result.status, SubmitStatus::NeedsGonza);
    let reason = result.reason.unwrap();
    assert!(
        reason.contains("Are you authorized to work in the country where the job is located?")
            && !reason.contains("Will you now or in the future require sponsorship"),
        "reason was: {reason}"
    );
    assert!(
        !reason.contains("no se pudo completar"),
        "a field failed: {reason}"
    );
    assert_eq!(pressed(&scanner, "sponsor-uuid").await, "false,true");
    let flags = |path: &str| {
        format!(
            r#"String(document.querySelectorAll('[data-field-path="{path}"] [data-jb-todo]').length)"#
        )
    };
    let flagged: String = scanner
        .page
        .evaluate_value(&flags("auth-uuid"))
        .await
        .unwrap();
    assert_eq!(flagged, "1");
    let answered: String = scanner
        .page
        .evaluate_value(&flags("sponsor-uuid"))
        .await
        .unwrap();
    assert_eq!(answered, "0");
    // Gonza presses "Yes" himself.
    scanner
        .page
        .locator(r#"[data-field-path="auth-uuid"] button[data-option="yes"]"#)
        .click(None)
        .await
        .unwrap();
    let flagged: String = scanner
        .page
        .evaluate_value(&flags("auth-uuid"))
        .await
        .unwrap();
    assert_eq!(flagged, "0");
    scanner.stop().await;
}

/// Ashby draws the required star with `::after`, so the label text never has an asterisk and
/// radio buttons and checkboxes carry no `required` attribute.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn ashby_required_star_drawn_by_css_counts() {
    let fields = scan_fixture("ashby_form.html").await;
    // A star that only the stylesheet knows about: no attribute and no class says "required".
    let employer = with_label(&fields, "Current employer");
    assert_eq!(employer.len(), 1);
    assert!(employer[0].required, "the ::after star was not seen");
    let radios = with_label(
        &fields,
        "How many years of relevant professional experience do you have?",
    );
    assert_eq!(radios.len(), 2);
    assert!(
        radios.iter().all(|f| f.required),
        "required star was not seen"
    );
    let optional = with_label(&fields, "Would you consider an office-based role?");
    assert!(optional.iter().all(|f| !f.required));
    let hear = with_label(
        &fields,
        "How did you hear about this opportunity? (select all that apply)",
    );
    assert_eq!(hear.len(), 2);
    assert!(hear.iter().all(|f| !f.required));
    // The names Ashby gives its checkboxes (their own text) do not split the group.
    assert_eq!(hear[0].name, hear[1].name);
    assert!(!hear[0].name.is_empty());
}

/// Shapes the grouping must not get wrong: two groups in one fieldset, two unrelated required
/// checkboxes, a formatting toolbar next to a text area, and the plain native controls.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn grouping_does_not_overreach() {
    let fields = scan_fixture("scan_edge_cases.html").await;
    let expected = [
        r#"0 input/radio "Pick a size" option="Small""#,
        r#"1 input/radio "Pick a size" option="Large""#,
        r#"2 input/radio "Pick a color" option="Red""#,
        r#"3 input/radio "Pick a color" option="Blue""#,
        r#"4 input/checkbox "I accept the terms of use" *"#,
        r#"5 input/checkbox "I accept the Privacy Policy" *"#,
        r#"6 textarea/ "Cover note""#,
        r#"7 input/date "Date of birth""#,
        r#"8 select/ "Level""#,
    ];
    assert_eq!(summaries(&fields), expected);
    assert_eq!(
        (fields[0].name.as_str(), fields[2].name.as_str()),
        ("size", "color")
    );
    // The two consents stay separate decisions: the allow list decides each by its own text.
    let allowed = AnswerBank {
        consent_phrases: vec!["privacy policy".into()],
        ..AnswerBank::default()
    };
    let plan = plan_fields(&fields, &allowed, Path::new("cv.pdf"));
    assert_eq!(plan.missing, vec!["I accept the terms of use"]);
    assert_eq!(plan.actions.len(), 1);
    assert_eq!(plan.actions[0].label, "I accept the Privacy Policy");
}

/// Boards catch bots with fields a person can never see or reach, named like the real ones (`email`,
/// `phone`, `name`). A bot that fills one gets its application dropped without a word, so the scan
/// leaves every such input out: invisible (opacity 0, `visibility: hidden`), pushed off screen,
/// zero or one pixel wide, clipped away by a parent, inside an `aria-hidden` ancestor, or out of
/// the tab order with no label. A small visible input, or one with only a placeholder or a name, is
/// still a question.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn honeypot_inputs_are_not_fields_but_small_visible_ones_are() {
    let fields = scan_fixture("honeypots.html").await;
    let expected = [
        r#"0 input/text "First name *" *"#,
        r#"1 input/email "Email *" *"#,
        r#"2 input/tel "Phone""#,
        r#"3 input/text "ZIP""#,
        r#"4 input/text "Nickname""#,
        r#"5 input/text "company""#,
        r#"6 input/file "Resume/CV *" *"#,
        // A styled checkbox: its native box is invisible but its label is on the page.
        r#"7 input/checkbox "Remember me""#,
        // The hidden file input named "resume" has no label, and its name is not used as one.
        r#"8 input/file """#,
    ];
    assert_eq!(summaries(&fields), expected);
}

/// The whole path: with answers for `email`, `phone` and `name` in the bank, only the real fields
/// get them and the CV goes to the one visible resume input.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn a_form_with_honeypots_is_filled_only_where_a_person_would_type() {
    let scanner = Scanner::start().await;
    let dir = tempfile::tempdir().unwrap();
    let cv = dir.path().join("cv.pdf");
    std::fs::write(&cv, b"%PDF-1.4 fake").unwrap();
    let job = make_job_with(|j| j.apply_url = fixture_url("honeypots.html"));
    let bank = AnswerBank {
        entries: vec![
            AnswerEntry::new(&["first name"], Some("Jane")),
            AnswerEntry::new(&["email"], Some("gonza@example.com")),
            AnswerEntry::new(&["phone"], Some("+56 9 0000 0000")),
            AnswerEntry::new(&["=name"], Some("Jane Doe Garcia")),
            AnswerEntry::new(&["=zip"], Some("8320000")),
            AnswerEntry::new(&["=nickname"], Some("Gonza")),
            AnswerEntry::new(&["=company"], Some("Acme")),
            AnswerEntry::new(&["authorized to work"], Some("Yes")),
            AnswerEntry::new(&["=country"], Some("Chile")),
        ],
        consent_phrases: vec!["privacy policy".into()],
    };
    let result = fill_application(
        &scanner.page,
        &job,
        &cv,
        &bank,
        SubmitMode::DryRun,
        &dir.path().join("shots"),
        NavPolicy::LOCAL_TESTS,
    )
    .await
    .unwrap();
    assert_eq!(result.status, SubmitStatus::DryRun, "{:?}", result.reason);
    let value = |id: &str| format!(r#"String(document.getElementById("{id}").value)"#);
    for (id, want) in [
        ("first_name", "Jane"),
        ("real_email", "gonza@example.com"),
        ("real_phone", "+56 9 0000 0000"),
        ("zip", "8320000"),
        ("nick", "Gonza"),
        ("company_box", "Acme"),
    ] {
        let got: String = scanner.page.evaluate_value(&value(id)).await.unwrap();
        assert_eq!(got, want, "{id}");
    }
    for id in [
        "hp_opacity",
        "hp_visibility",
        "hp_left",
        "hp_top",
        "hp_tiny",
        "hp_zero",
        "hp_clipped",
        "hp_aria",
        "hp_parent_opacity",
        "hp_tabindex",
    ] {
        let got: String = scanner.page.evaluate_value(&value(id)).await.unwrap();
        assert_eq!(got, "", "the honeypot {id} was filled");
    }
    // Checkboxes, radios and lists that nobody can see are left as they were.
    for expression in [
        r#"String(document.getElementById("hp_consent").checked)"#,
        r#"String(document.getElementById("hp_consent2").checked)"#,
        r#"String(document.getElementById("hp_auth_yes").checked)"#,
        r#"String(document.getElementById("hp_auth_no").checked)"#,
        r#"String(document.getElementById("hp_country").value)"#,
    ] {
        let got: String = scanner.page.evaluate_value(expression).await.unwrap();
        assert!(
            got.is_empty() || got == "false",
            "{expression} was touched: {got}"
        );
    }
    let files = |id: &str| format!(r#"String(document.getElementById("{id}").files.length)"#);
    let real: String = scanner.page.evaluate_value(&files("resume")).await.unwrap();
    let hidden: String = scanner
        .page
        .evaluate_value(&files("hp_file"))
        .await
        .unwrap();
    assert_eq!((real.as_str(), hidden.as_str()), ("1", "0"));
    scanner.stop().await;
}

/// Scanning the same page twice gives the same list: the `data-jb` tags are reset each time.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn scanning_twice_gives_the_same_list() {
    let scanner = Scanner::start().await;
    for file in ["greenhouse_form.html", "ashby_form.html"] {
        let first = scanner.scan(file).await;
        let second = scan_fields(&scanner.page).await.unwrap();
        assert_eq!(first, second, "{file}");
    }
    scanner.stop().await;
}

/// Read-only look at real forms: `JOBBOT_SCAN_URLS` holds comma-separated application URLs;
/// each page is loaded (nothing is filled or clicked) and the field scan is printed. When
/// `JOBBOT_SCAN_OUT` names a folder the form's markup (scripts and styles stripped) is saved there
/// as `<n>.html`, for building offline fixtures. One page at a time, a few seconds apart.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium and JOBBOT_SCAN_URLS"]
async fn scan_real_forms() {
    let Ok(urls) = std::env::var("JOBBOT_SCAN_URLS") else {
        eprintln!("scan_real_forms: JOBBOT_SCAN_URLS is not set, nothing to scan");
        return;
    };
    let out = std::env::var("JOBBOT_SCAN_OUT").ok().map(PathBuf::from);
    let scanner = Scanner::start().await;
    scanner.page.set_default_timeout(ACTION_TIMEOUT_MS).await;
    for (n, url) in urls
        .split(',')
        .map(str::trim)
        .filter(|url| !url.is_empty())
        .enumerate()
    {
        let options = GotoOptions::new().wait_until(WaitUntil::DomContentLoaded);
        if let Err(err) = scanner.page.goto(url, options).await {
            eprintln!("[{n}] {url}\n  goto failed: {}", first_line(&err));
            continue;
        }
        let fields = scan_fields(&scanner.page).await.unwrap();
        eprintln!("[{n}] {url}");
        for field in &fields {
            eprintln!("  {} name={:?}", summary(field), field.name);
        }
        // What the planner makes of the scan with the shipped answer bank.
        let bank_path = crate::test_support::workspace_root()
            .join("profile")
            .join("answers.toml");
        if let Ok(bank) = crate::test_support::load_bank(&bank_path) {
            let plan = plan_fields(&fields, &bank, Path::new("cv.pdf"));
            eprintln!(
                "  plan: {} actions, missing: {:?}",
                plan.actions.len(),
                plan.missing
            );
        }
        if let Some(dir) = &out {
            let html: String = scanner
                .page
                .evaluate(
                    r#"() => {
                      const source = document.querySelector('form') || document.body;
                      const root = source.cloneNode(true);
                      // Styles are dropped, so keep what they hid as inline styles.
                      const originals = [source, ...source.querySelectorAll('*')];
                      const clones = [root, ...root.querySelectorAll('*')];
                      originals.forEach((el, i) => {
                        const style = getComputedStyle(el);
                        if (style.display === 'none') clones[i].style.display = 'none';
                        else if (style.visibility === 'hidden') clones[i].style.visibility = 'hidden';
                      });
                      root.querySelectorAll('script, style, svg, noscript').forEach((el) => el.remove());
                      return root.outerHTML;
                    }"#,
                    None::<&()>,
                )
                .await
                .unwrap();
            std::fs::create_dir_all(dir).unwrap();
            std::fs::write(dir.join(format!("{n}.html")), html).unwrap();
        }
        if !url.starts_with("file:") {
            tokio::time::sleep(Duration::from_secs(4)).await;
        }
    }
    scanner.stop().await;
}

// ---- The submit click and what follows it ----

/// A bank that answers the first-name box of the small click fixtures.
fn first_name_bank() -> AnswerBank {
    AnswerBank {
        entries: vec![AnswerEntry::new(&["first name"], Some("Jane"))],
        consent_phrases: vec![],
    }
}

/// Runs `fill_application` on the fixture at `url` in a fresh page, and returns what it answered.
async fn fill_fixture(url: &str, mode: SubmitMode) -> anyhow::Result<SubmitResult> {
    let scanner = Scanner::start().await;
    let dir = tempfile::tempdir().unwrap();
    let cv = dir.path().join("cv.pdf");
    std::fs::write(&cv, b"%PDF-1.4 fake").unwrap();
    let job = make_job_with(|j| j.apply_url = url.to_string());
    let result = fill_application(
        &scanner.page,
        &job,
        &cv,
        &first_name_bank(),
        mode,
        &dir.path().join("shots"),
        NavPolicy::LOCAL_TESTS,
    )
    .await;
    scanner.stop().await;
    result
}

/// The click call can fail after the form went out (the click triggered a navigation, or the page
/// took longer than the deadline). A failed click proves nothing: the application may have been
/// sent, so it is recorded as an unconfirmed click and never as "not sent".
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn a_failed_submit_click_is_an_unconfirmed_submit() {
    let result = fill_fixture(&fixture_url("submit_click.html"), SubmitMode::Live)
        .await
        .unwrap();
    assert_eq!(result.status, SubmitStatus::NeedsGonza);
    let reason = result.reason.unwrap();
    assert!(
        reason.starts_with("unconfirmed_submit: el clic de envío devolvió un error"),
        "{reason}"
    );
}

/// The page confirmed while the click call was still failing: the confirmation wins.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn a_failed_submit_click_followed_by_a_confirmation_is_submitted() {
    let url = format!("{}?confirm", fixture_url("submit_click.html"));
    let result = fill_fixture(&url, SubmitMode::Live).await.unwrap();
    assert_eq!(
        result.status,
        SubmitStatus::Submitted,
        "{:?}",
        result.reason
    );
}

// ---- A page that stops answering ----

/// Like `fill_fixture`, with the whole application limited to `wall`.
async fn fill_fixture_within(
    url: &str,
    mode: SubmitMode,
    wall: Duration,
) -> anyhow::Result<SubmitResult> {
    let scanner = Scanner::start().await;
    let dir = tempfile::tempdir().unwrap();
    let cv = dir.path().join("cv.pdf");
    std::fs::write(&cv, b"%PDF-1.4 fake").unwrap();
    let job = make_job_with(|j| j.apply_url = url.to_string());
    let result = fill_application_within(
        &scanner.page,
        &job,
        &cv,
        &first_name_bank(),
        mode,
        &dir.path().join("shots"),
        NavPolicy::LOCAL_TESTS,
        wall,
    )
    .await;
    scanner.stop().await;
    result
}

/// A page stuck in a busy loop answers nothing (`evaluate` has no deadline of its own): the
/// application ends at its wall with the page given back to Gonza, not waiting for ever.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn a_page_that_freezes_before_the_click_ends_at_the_wall() {
    let url = format!("{}?at=load", fixture_url("hang.html"));
    for mode in [SubmitMode::DryRun, SubmitMode::Live] {
        let started = tokio::time::Instant::now();
        let result = fill_fixture_within(&url, mode, Duration::from_secs(3))
            .await
            .unwrap();
        assert_eq!(result.status, SubmitStatus::NeedsGonza, "{mode:?}");
        assert_eq!(
            result.reason.as_deref(),
            Some("page_error: la página no respondió en 3 s"),
            "{mode:?}"
        );
        assert!(started.elapsed() < Duration::from_secs(60), "{mode:?}");
    }
}

/// Once the click was tried the form may be out: a page that freezes after it is an unconfirmed
/// submit, which seals the attempt.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn a_page_that_freezes_after_the_click_is_an_unconfirmed_submit() {
    let url = format!("{}?at=click", fixture_url("hang.html"));
    let result = fill_fixture_within(&url, SubmitMode::Live, Duration::from_secs(12))
        .await
        .unwrap();
    assert_eq!(result.status, SubmitStatus::NeedsGonza);
    assert_eq!(
        result.reason.as_deref(),
        Some("unconfirmed_submit: la página dejó de responder después del clic")
    );
}

// ---- A failure of the page is not a failure of the browser ----

/// A page that replaces itself over and over destroys the context a script was running in. That
/// is this page's problem: it goes back to Gonza as a result, and only a browser that cannot open
/// a page at all is an `Err` (three of those in a row stop the run).
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn a_page_that_destroys_its_own_context_is_a_result_not_an_error() {
    let result = fill_fixture(&fixture_url("restless.html"), SubmitMode::DryRun).await;
    let result = result.expect("a failure of the page is a result, not an Err");
    assert_eq!(result.status, SubmitStatus::NeedsGonza);
    let reason = result.reason.unwrap();
    assert!(
        reason.starts_with("page_error:") || reason == "no_form_found",
        "{reason}"
    );
}

// ---- Waiting for the confirmation ----

/// A board that shows "Submitting..." in an alert region while the request is on its way is not
/// rejecting the form: the confirmation that follows wins.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn a_progress_alert_does_not_end_the_wait_before_the_confirmation() {
    let result = fill_fixture(&fixture_url("slow_confirm.html"), SubmitMode::Live)
        .await
        .unwrap();
    assert_eq!(
        result.status,
        SubmitStatus::Submitted,
        "{:?}",
        result.reason
    );
}

// ---- An option is clicked only if it is still the one that was judged ----

/// The script that tags the option to click names it by position and by the text that was judged:
/// a list redrawn since the position was read must not tag another option.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn an_option_is_tagged_only_if_it_is_still_the_judged_one() {
    let scanner = Scanner::start().await;
    scanner.open(&fixture_url("swapped_list.html")).await;
    scanner
        .page
        .evaluate_value(
            r#"(() => { const l = document.getElementById('school_list'); l.style.display = 'block';
              l.innerHTML = '<li role="option">Gamma Institute</li><li role="option">Alpha  University</li>'; return 'ok'; })()"#,
        )
        .await
        .unwrap();
    let tag = |index: usize, text: &str| {
        let arg = serde_json::json!({ "controls": "school_list", "index": index, "text": text });
        let page = scanner.page.clone();
        async move {
            let tagged: bool = page.evaluate(TAG_OPTION_JS, Some(&arg)).await.unwrap();
            tagged
        }
    };
    let tagged = || async {
        scanner
            .page
            .evaluate_value(
                r#"Array.from(document.querySelectorAll('[data-jb-opt]')).map((e) => e.textContent).join('|')"#,
            )
            .await
            .unwrap()
    };
    // The judged option moved from position 0 to 1: position 0 now holds another option.
    assert!(!tag(0, "Alpha University").await);
    assert_eq!(tagged().await, "");
    // The same text is found by its position, whatever the spaces in the page's markup.
    assert!(tag(1, "Alpha University").await);
    assert_eq!(tagged().await, "Alpha  University");
    scanner.stop().await;
}

/// The whole path against a list that is swapped under the filler: the box ends with the option
/// that was judged, or the field fails; never with another one.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn a_list_swapped_under_the_filler_never_gets_another_option_clicked() {
    let scanner = Scanner::start().await;
    let dir = tempfile::tempdir().unwrap();
    let cv = dir.path().join("cv.pdf");
    std::fs::write(&cv, b"%PDF-1.4 fake").unwrap();
    let job = make_job_with(|j| j.apply_url = fixture_url("swapped_list.html"));
    let bank = AnswerBank {
        entries: vec![AnswerEntry::new(&["school"], Some("Alpha University"))],
        consent_phrases: vec![],
    };
    let result = fill_application(
        &scanner.page,
        &job,
        &cv,
        &bank,
        SubmitMode::DryRun,
        &dir.path().join("shots"),
        NavPolicy::LOCAL_TESTS,
    )
    .await
    .unwrap();
    let value: String = scanner
        .page
        .evaluate_value("String(document.getElementById('school').value)")
        .await
        .unwrap();
    scanner.stop().await;
    assert!(
        value == "Alpha University" || result.status == SubmitStatus::NeedsGonza,
        "{value:?} {result:?}"
    );
    assert!(
        value == "Alpha University" || value.is_empty(),
        "another option was clicked: {value:?}"
    );
}

// ---- Which button sends the form ----

/// The submit button is looked for in the form that holds the CV, and a button that names another
/// service or is not the send ("Apply with LinkedIn", "Apply filter") is never the candidate, even
/// when it has the submit type and comes first.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn the_submit_button_is_the_one_of_the_form_that_takes_the_cv() {
    let scanner = Scanner::start().await;
    let dir = tempfile::tempdir().unwrap();
    let cv = dir.path().join("cv.pdf");
    std::fs::write(&cv, b"%PDF-1.4 fake").unwrap();
    let job = make_job_with(|j| j.apply_url = fixture_url("submit_buttons.html"));
    let result = fill_application(
        &scanner.page,
        &job,
        &cv,
        &first_name_bank(),
        SubmitMode::Live,
        &dir.path().join("shots"),
        NavPolicy::LOCAL_TESTS,
    )
    .await
    .unwrap();
    let clicked: String = scanner
        .page
        .evaluate_value("window.clicked.join(',')")
        .await
        .unwrap();
    scanner.stop().await;
    assert_eq!(clicked, "send");
    assert_eq!(
        result.status,
        SubmitStatus::Submitted,
        "{:?}",
        result.reason
    );
}

/// With no button left that could be the send, the answer is `no_submit_button`, as before.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn a_form_with_only_other_services_buttons_has_no_submit_button() {
    let scanner = Scanner::start().await;
    let dir = tempfile::tempdir().unwrap();
    let cv = dir.path().join("cv.pdf");
    std::fs::write(&cv, b"%PDF-1.4 fake").unwrap();
    let job = make_job_with(|j| j.apply_url = fixture_url("submit_buttons.html"));
    scanner.open(&job.apply_url).await;
    scanner
        .page
        .evaluate_value("document.getElementById('send').remove(); 'ok'")
        .await
        .unwrap();
    let fields = scan_fields(&scanner.page).await.unwrap();
    let plan = plan_fields(&fields, &first_name_bank(), &cv);
    let tagged = tag_submit(&scanner.page, &plan).await.unwrap();
    scanner.stop().await;
    assert!(!tagged);
}

// ---- Questions that appear when another is answered ----

fn referral_bank(describe: bool) -> AnswerBank {
    let mut entries = vec![
        AnswerEntry::new(&["first name"], Some("Jane")),
        AnswerEntry::new(&["do you have a referral"], Some("Yes")),
        AnswerEntry::new(&["phone"], Some("+56 9 0000 0000")),
    ];
    if describe {
        entries.push(AnswerEntry::new(
            &["=describe your referral"],
            Some("A former colleague"),
        ));
    }
    AnswerBank {
        entries,
        consent_phrases: vec![],
    }
}

/// Runs `fill_application` on `dependent_question.html` and also returns the values the page ended
/// with (`first_name`, `phone`, `who`) and how many files the CV input holds.
async fn fill_dependent(
    bank: &AnswerBank,
    mode: SubmitMode,
) -> (SubmitResult, [String; 3], String) {
    let scanner = Scanner::start().await;
    let dir = tempfile::tempdir().unwrap();
    let cv = dir.path().join("cv.pdf");
    std::fs::write(&cv, b"%PDF-1.4 fake").unwrap();
    let job = make_job_with(|j| j.apply_url = fixture_url("dependent_question.html"));
    let result = fill_application(
        &scanner.page,
        &job,
        &cv,
        bank,
        mode,
        &dir.path().join("shots"),
        NavPolicy::LOCAL_TESTS,
    )
    .await
    .unwrap();
    let mut values = Vec::new();
    for id in ["first_name", "phone", "who"] {
        let expression = format!(
            r#"String((document.getElementById("{id}") || {{ value: "(absent)" }}).value)"#
        );
        values.push(scanner.page.evaluate_value(&expression).await.unwrap());
    }
    let files: String = scanner
        .page
        .evaluate_value(r#"String(document.getElementById("resume").files.length)"#)
        .await
        .unwrap();
    scanner.stop().await;
    let [a, b, c]: [String; 3] = values.try_into().unwrap();
    (result, [a, b, c], files)
}

/// Answering "Yes" opened a required question the first read never saw: the dry run must not come
/// out green, because the real send would be rejected for it.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn a_required_question_opened_by_an_answer_is_reported_by_the_dry_run() {
    let (result, values, files) = fill_dependent(&referral_bank(false), SubmitMode::DryRun).await;
    assert_eq!(
        result.status,
        SubmitStatus::NeedsGonza,
        "{:?}",
        result.reason
    );
    let reason = result.reason.unwrap();
    assert!(
        reason.starts_with("faltan respuestas: ") && reason.contains("Describe your referral"),
        "{reason}"
    );
    // What was answerable was filled, and nothing was filled twice or into the wrong box.
    assert_eq!(values[0], "Jane");
    assert_eq!(values[1], "+56 9 0000 0000");
    assert_eq!(values[2], "");
    assert_eq!(files, "1");
}

/// When the bank does answer the new question it is filled in a second round, and the renumbered
/// page is not filled again.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn a_question_opened_by_an_answer_is_filled_in_a_second_round() {
    let (result, values, files) = fill_dependent(&referral_bank(true), SubmitMode::DryRun).await;
    assert_eq!(result.status, SubmitStatus::DryRun, "{:?}", result.reason);
    assert_eq!(values, ["Jane", "+56 9 0000 0000", "A former colleague"]);
    assert_eq!(files, "1");
}

/// A live run stops before the click, as it does for any required answer it lacks.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn a_live_run_does_not_click_while_an_opened_question_is_unanswered() {
    let (result, _, _) = fill_dependent(&referral_bank(false), SubmitMode::Live).await;
    assert_eq!(result.status, SubmitStatus::NeedsGonza);
    let reason = result.reason.unwrap();
    assert!(reason.starts_with("faltan respuestas: "), "{reason}");
}
