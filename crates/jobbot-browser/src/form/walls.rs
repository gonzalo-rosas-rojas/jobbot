//! What stops a form while it is being filled: a captcha, a password field, or page text that
//! announces a verification code, a login or a bot block.

use anyhow::Context;
use playwright_rs::Page;

use super::blocker_in_text;
use super::retry::retry_once;

/// A captcha wall: a visible reCAPTCHA or hCaptcha frame or widget. The invisible-reCAPTCHA badge
/// that Greenhouse shows on every form (`size=invisible` in the frame address) is not a wall; the
/// challenge frame that may pop up after a click has no `size=invisible`, so it still is.
const CAPTCHA_WALL_SELECTOR: &str = r#"iframe[src*="recaptcha"]:not([src*="size=invisible"]):visible, iframe[src*="hcaptcha"]:not([src*="size=invisible"]):visible, .g-recaptcha:not([data-size="invisible"]):visible, .h-captcha:not([data-size="invisible"]):visible"#;
const PASSWORD_SELECTOR: &str = r#"input[type="password"]:visible"#;

/// Looks for a wall that must not be bypassed: a visible captcha widget or challenge, a visible
/// password field or a page whose text announces a verification code, a login or a bot block.
pub(super) async fn detect_blocker(page: &Page) -> anyhow::Result<Option<&'static str>> {
    if let Some(wall) = widget_wall(page).await? {
        return Ok(Some(wall));
    }
    wall_in_page_text(|| async { page.locator("body").inner_text().await }).await
}

/// The wall a page shows as a control: a visible captcha widget or challenge, or a password field.
async fn widget_wall(page: &Page) -> anyhow::Result<Option<&'static str>> {
    if page.locator(CAPTCHA_WALL_SELECTOR).count().await? > 0 {
        return Ok(Some("captcha"));
    }
    if page.locator(PASSWORD_SELECTOR).count().await? > 0 {
        return Ok(Some("login_required"));
    }
    Ok(None)
}

/// `detect_blocker` for the wait after the click, where the page may be navigating: one read and
/// no retry, and whatever cannot be read counts as no wall yet (the wait asks again until its
/// deadline). Before the click the same failure is an error.
pub(super) async fn wall_after_click(page: &Page) -> Option<&'static str> {
    if let Some(wall) = widget_wall(page).await.ok()? {
        return Some(wall);
    }
    let text = page.locator("body").inner_text().await.unwrap_or_default();
    blocker_in_text(&text)
}

/// How long the page text waits before its second read.
const TEXT_RETRY_MS: u64 = 500;

/// Reads the page text (twice at most) and looks for a wall in it. A text that cannot be read is an
/// error, never "no wall": the check that guards the submit must not pass on what it could not see.
async fn wall_in_page_text<E, F, Fut>(read: F) -> anyhow::Result<Option<&'static str>>
where
    E: std::error::Error + Send + Sync + 'static,
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<String, E>>,
{
    let text = retry_once(TEXT_RETRY_MS, read)
        .await
        .context("reading the page text to look for a wall")?;
    Ok(blocker_in_text(&text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocker_text_detects_codes_login_and_bot_walls() {
        assert_eq!(
            blocker_in_text("We sent a Verification Code to your email"),
            Some("captcha")
        );
        assert_eq!(
            blocker_in_text("Please confirm: I\u{2019}m not a robot"),
            Some("captcha")
        );
        assert_eq!(
            blocker_in_text("Sign in to apply for this job"),
            Some("login_required")
        );
        assert_eq!(
            blocker_in_text("Access denied. Unusual traffic detected."),
            Some("blocked_page")
        );
    }

    #[test]
    fn blocker_text_ignores_ordinary_application_pages() {
        let text = "Apply for Senior AI Engineer. First name. Last name. Email. Resume. Submit application.";
        assert_eq!(blocker_in_text(text), None);
        assert_eq!(blocker_in_text(""), None);
    }

    fn unreadable_text() -> std::io::Error {
        std::io::Error::other("the page is gone")
    }

    #[tokio::test]
    async fn a_page_text_that_cannot_be_read_is_an_error_not_a_page_without_a_wall() {
        let reads = std::sync::atomic::AtomicUsize::new(0);
        let result = wall_in_page_text(|| {
            reads.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            async { Err::<String, _>(unreadable_text()) }
        })
        .await;
        let message = format!("{:#}", result.unwrap_err());
        assert!(
            message.contains("reading the page text to look for a wall")
                && message.contains("the page is gone"),
            "{message}"
        );
        assert_eq!(reads.into_inner(), 2, "one retry, no third read");
    }

    #[tokio::test]
    async fn a_page_text_read_that_works_on_the_second_try_is_judged() {
        let reads = std::sync::atomic::AtomicUsize::new(0);
        let result = wall_in_page_text(|| {
            let first = reads.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0;
            async move {
                if first {
                    Err(unreadable_text())
                } else {
                    Ok("Sign in to apply for this job".to_string())
                }
            }
        })
        .await;
        assert_eq!(result.unwrap(), Some("login_required"));
    }

    #[tokio::test]
    async fn a_readable_page_text_without_a_wall_gives_none() {
        let result = wall_in_page_text(|| async {
            Ok::<_, std::io::Error>("Apply. First name. Email. Resume.".to_string())
        })
        .await;
        assert_eq!(result.unwrap(), None);
    }
}
