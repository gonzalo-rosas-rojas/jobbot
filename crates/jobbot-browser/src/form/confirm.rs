//! The submit button, and the wait for the page to confirm that the form was sent.

use std::time::Duration;

use playwright_rs::Page;

use super::walls::wall_after_click;
use super::{
    FillPlan, FillValue, POLL_MS, PageSnapshot, SubmitMode, newly_confirmed, newly_rejected,
};

/// How long the page gets to react after the submit click.
pub(super) const CONFIRM_WAIT_MS: u64 = 8_000;

pub(super) const SUBMIT_SELECTOR: &str = r#"[data-jb-submit="1"]"#;

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

/// Tags the submit button with `data-jb-submit="1"`; returns false when there is none. The button
/// is looked for in the form that holds the CV input (`arg.cv`, its `data-jb` number: the form
/// Gonza is applying with), else in the form of the first tagged field, else in the page. A button
/// is a candidate when it is visible, its text says submit, apply, enviar or postular, and it does
/// not name another service or another action: "Apply with LinkedIn", "Submit feedback",
/// "Don't apply", a search or a filter, sign up or log in, save, cancel. Among the candidates the
/// one of type submit is preferred.
const TAG_SUBMIT_JS: &str = r#"(arg) => {
  const re = /submit|apply|enviar|postular/i;
  const other = /linkedin|indeed|google|facebook|seek|xing|feedback|subscribe|newsletter|search|filter|sign up|log in|sign in|cancel|don't|do not|save/i;
  document.querySelectorAll('[data-jb-submit]').forEach((el) => el.removeAttribute('data-jb-submit'));
  const cv = arg && arg.cv !== null && arg.cv !== undefined
    ? document.querySelector('[data-jb="' + arg.cv + '"]') : null;
  const anchor = cv || document.querySelector('[data-jb]');
  const scope = (anchor && anchor.closest('form')) || document;
  const text = (b) => (b.innerText || b.value || b.getAttribute('aria-label') || '').replace(/’/g, "'");
  const candidates = Array.from(scope.querySelectorAll('button, input[type="submit"], input[type="button"], [role="button"]'))
    .filter((b) => b.offsetParent !== null && re.test(text(b)) && !other.test(text(b)));
  const hit = candidates.find((b) => (b.getAttribute('type') || '').toLowerCase() === 'submit') || candidates[0];
  if (!hit) return false;
  hit.setAttribute('data-jb-submit', '1');
  return true;
}"#;

/// Tags the submit button (see `TAG_SUBMIT_JS`) in the form that takes the CV of `plan`; false when
/// there is none.
pub(super) async fn tag_submit(page: &Page, plan: &FillPlan) -> anyhow::Result<bool> {
    let cv = plan
        .actions
        .iter()
        .find(|action| matches!(action.value, FillValue::File(_)))
        .map(|action| action.idx);
    let arg = serde_json::json!({ "cv": cv });
    Ok(page.evaluate(TAG_SUBMIT_JS, Some(&arg)).await?)
}

pub(super) async fn snapshot(page: &Page) -> PageSnapshot {
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
pub(super) enum Confirmation {
    Confirmed,
    Rejected(String),
    Blocked(&'static str),
    Silent,
    /// The tab was closed while waiting (in Assist mode, Gonza skipping the form).
    Closed,
}

/// One look at the page while waiting: true when it confirms. In `Live` mode the first validation
/// message that shows up is remembered in `rejection` and the wait goes on: many boards show
/// "Submitting..." in an alert region while the request is on its way, and the confirmation that
/// follows must win over it.
fn judge(
    before: &PageSnapshot,
    after: &PageSnapshot,
    mode: SubmitMode,
    rejection: &mut Option<String>,
) -> bool {
    if newly_confirmed(before, after) {
        return true;
    }
    if mode == SubmitMode::Live && rejection.is_none() {
        *rejection = newly_rejected(before, after);
    }
    false
}

/// What the wait ends with when the page never confirmed: the first validation message it showed, or
/// silence.
fn unconfirmed(rejection: Option<String>) -> Confirmation {
    rejection.map_or(Confirmation::Silent, Confirmation::Rejected)
}

/// Polls the page for up to `wait_ms`: a new thank-you address or phrase confirms; a wall ends the
/// wait early in `Live` mode. A validation message does not end it: it is the answer only if the
/// time runs out without a confirmation. `Silent` means nothing happened in time. In `Assist` mode
/// only the confirmation counts, because Gonza may be fixing fields or solving a captcha in the
/// meantime.
pub(super) async fn await_confirmation(
    page: &Page,
    before: &PageSnapshot,
    mode: SubmitMode,
    wait_ms: u64,
) -> Confirmation {
    let deadline = tokio::time::Instant::now() + Duration::from_millis(wait_ms);
    let mut rejection = None;
    loop {
        if page.is_closed() {
            return Confirmation::Closed;
        }
        let after = snapshot(page).await;
        if judge(before, &after, mode, &mut rejection) {
            return Confirmation::Confirmed;
        }
        if mode == SubmitMode::Live
            && let Some(reason) = wall_after_click(page).await
        {
            return Confirmation::Blocked(reason);
        }
        if tokio::time::Instant::now() >= deadline {
            return unconfirmed(rejection);
        }
        tokio::time::sleep(Duration::from_millis(POLL_MS)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(url: &str, text: &str) -> PageSnapshot {
        PageSnapshot {
            url: url.into(),
            text: text.into(),
            errors: String::new(),
        }
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
    fn a_message_does_not_end_the_wait_and_a_later_confirmation_wins() {
        let before = snap("https://x.test/apply", "form");
        let mut sending = before.clone();
        sending.errors = "Submitting your application...".into();
        let thanks = snap("https://x.test/apply", "Thank you for applying");
        let mut rejection = None;
        assert!(!judge(&before, &sending, SubmitMode::Live, &mut rejection));
        assert_eq!(rejection.as_deref(), Some("Submitting your application..."));
        // The first message is the one kept, and the confirmation still wins.
        let mut later = before.clone();
        later.errors = "Phone number is invalid.".into();
        assert!(!judge(&before, &later, SubmitMode::Live, &mut rejection));
        assert_eq!(rejection.as_deref(), Some("Submitting your application..."));
        assert!(judge(&before, &thanks, SubmitMode::Live, &mut rejection));
    }

    #[test]
    fn without_a_confirmation_the_first_message_is_the_answer() {
        assert_eq!(
            unconfirmed(Some("Phone number is invalid.".into())),
            Confirmation::Rejected("Phone number is invalid.".into())
        );
        assert_eq!(unconfirmed(None), Confirmation::Silent);
    }

    #[test]
    fn only_a_live_run_judges_messages() {
        let before = snap("https://x.test/apply", "form");
        let mut after = before.clone();
        after.errors = "Phone number is invalid.".into();
        for mode in [SubmitMode::Assist, SubmitMode::DryRun] {
            let mut rejection = None;
            assert!(!judge(&before, &after, mode, &mut rejection));
            assert_eq!(rejection, None, "{mode:?}");
        }
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
}
