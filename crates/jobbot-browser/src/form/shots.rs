//! Screenshots of the page.

use std::path::Path;

use playwright_rs::{Page, ScreenshotOptions};

use super::retry::retry_once;

/// How long a failed screenshot waits before its second try.
const SHOT_RETRY_MS: u64 = 500;

/// A full-page screenshot, tried a second time after a pause: a busy Chromium sometimes answers
/// "Unable to capture screenshot", and the shot taken before the click must not end the fill.
pub(super) async fn take_shot(page: &Page, path: &Path) -> anyhow::Result<()> {
    retry_once(SHOT_RETRY_MS, || async {
        page.screenshot_to_file(path, ScreenshotOptions::builder().full_page(true).build())
            .await
    })
    .await?;
    Ok(())
}
