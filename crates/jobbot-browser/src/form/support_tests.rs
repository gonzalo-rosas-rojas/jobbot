//! What the tests of `form` and its children share: control and answer-bank builders, and the
//! headless browser `Session` of the tests that need Chromium.

use std::path::PathBuf;

use super::*;
use jobbot_domain::answers::AnswerEntry;
use jobbot_domain::fixtures::make_job_with;
use playwright_rs::Browser;

pub(super) fn field(idx: usize, tag: &str, kind: &str, label: &str, required: bool) -> FieldInfo {
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
pub(super) fn radio(
    idx: usize,
    name: &str,
    label: &str,
    option: &str,
    required: bool,
) -> FieldInfo {
    FieldInfo {
        option: option.into(),
        name: name.into(),
        ..field(idx, "input", "radio", label, required)
    }
}

/// One checkbox of the group `name`: `label` is the question, `option` its own text.
pub(super) fn checkbox(
    idx: usize,
    name: &str,
    label: &str,
    option: &str,
    required: bool,
) -> FieldInfo {
    FieldInfo {
        option: option.into(),
        name: name.into(),
        ..field(idx, "input", "checkbox", label, required)
    }
}

/// A group of checkboxes that share the question `label` and a group name made from `first`,
/// one per option text, numbered from `first`.
pub(super) fn checkbox_group(
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

/// The bank with holes on purpose: no answer for the email, a blank one for the phone.
pub(super) fn bank_with_gaps() -> AnswerBank {
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

/// An answer bank with `entries` and no consent phrases.
pub(super) fn bank_with(entries: Vec<AnswerEntry>) -> AnswerBank {
    AnswerBank {
        entries,
        consent_phrases: vec![],
    }
}

/// The `data-jb` numbers the plan acts on, in plan order.
pub(super) fn action_idxs(plan: &FillPlan) -> Vec<usize> {
    plan.actions.iter().map(|action| action.idx).collect()
}

pub(super) fn pay_and_start_bank() -> AnswerBank {
    AnswerBank {
        entries: vec![
            AnswerEntry::new(&["start"], Some("Immediately")),
            AnswerEntry::new(&["salary"], Some("USD 6,000 per month")),
            AnswerEntry::new(&["annual"], Some("72000")),
        ],
        consent_phrases: vec![],
    }
}

pub(super) fn race_bank() -> AnswerBank {
    AnswerBank {
        entries: vec![
            AnswerEntry::new(&["=race"], Some("White")).with_alternatives(&["Caucasian"]),
        ],
        consent_phrases: vec![],
    }
}

pub(super) fn texts(options: &[&str]) -> Vec<String> {
    options.iter().map(|option| option.to_string()).collect()
}

pub(super) fn fixture_url(variant: &str) -> String {
    let path = crate::test_support::fixtures_dir().join("ats_form.html");
    format!(
        "file:///{}?variant={variant}",
        path.display().to_string().replace('\\', "/")
    )
}

pub(super) fn full_bank() -> AnswerBank {
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
pub(super) struct Session {
    _playwright: Playwright,
    browser: Browser,
    pub(super) page: Page,
    pub(super) dir: tempfile::TempDir,
}

impl Session {
    pub(super) async fn start() -> Session {
        let playwright = Playwright::launch().await.unwrap();
        let browser = crate::launch::start(&playwright, crate::launch::launch_options())
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

    pub(super) fn cv(&self) -> PathBuf {
        let path = self.dir.path().join("cv.pdf");
        std::fs::write(&path, b"%PDF-1.4 fake").unwrap();
        path
    }

    pub(super) fn shots(&self) -> PathBuf {
        self.dir.path().join("shots")
    }

    pub(super) async fn fill_url(
        &self,
        url: &str,
        bank: &AnswerBank,
        mode: SubmitMode,
    ) -> SubmitResult {
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

    pub(super) async fn fill(
        &self,
        variant: &str,
        bank: &AnswerBank,
        mode: SubmitMode,
    ) -> SubmitResult {
        self.fill_url(&fixture_url(variant), bank, mode).await
    }

    pub(super) async fn eval(&self, expression: &str) -> String {
        self.page.evaluate_value(expression).await.unwrap()
    }

    pub(super) async fn value(&self, selector: &str) -> String {
        self.page.locator(selector).input_value(None).await.unwrap()
    }

    pub(super) async fn stop(self) {
        self.browser.close().await.unwrap();
    }
}

/// Which controls carry the "left for Gonza" flag, by element id.
pub(super) async fn flagged_ids(session: &Session) -> String {
    session
        .eval(
            "Array.from(document.querySelectorAll('[data-jb-todo]')).map((el) => el.id).join(',')",
        )
        .await
}
