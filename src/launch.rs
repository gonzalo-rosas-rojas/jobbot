//! How jobbot starts Chromium. Every launch takes its options from here, so the sandbox and the
//! deadline are set in one place.

use playwright_rs::{BrowserContextOptions, LaunchOptions};

/// The most a Chromium launch may take. Playwright's own default is three minutes; a browser that
/// has not started in 30 s is stuck, and the run should say so.
pub(crate) const LAUNCH_TIMEOUT_MS: f64 = 30_000.0;

/// Options for a plain browser launch (the form inspector, the CV printer). The sandbox is asked
/// for: without it Playwright starts Chromium with `--no-sandbox`.
pub(crate) fn launch_options() -> LaunchOptions {
    LaunchOptions::default()
        .chromium_sandbox(true)
        .timeout(LAUNCH_TIMEOUT_MS)
}

/// The same for the filler's persistent profile, headless unless `headed`.
pub(crate) fn persistent_options(headed: bool) -> BrowserContextOptions {
    BrowserContextOptions::builder()
        .headless(!headed)
        .chromium_sandbox(true)
        .timeout(LAUNCH_TIMEOUT_MS)
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_launch_asks_for_the_sandbox_and_a_deadline() {
        let plain = launch_options();
        assert_eq!(plain.chromium_sandbox, Some(true));
        assert_eq!(plain.timeout, Some(LAUNCH_TIMEOUT_MS));
        for headed in [false, true] {
            let persistent = persistent_options(headed);
            assert_eq!(persistent.chromium_sandbox, Some(true));
            assert_eq!(persistent.timeout, Some(LAUNCH_TIMEOUT_MS));
            assert_eq!(persistent.headless, Some(!headed));
        }
    }

    /// The command lines of the running Chromium processes that contain `marker`.
    #[cfg(windows)]
    fn chromium_command_lines(marker: &str) -> Vec<String> {
        let script = format!(
            "Get-CimInstance Win32_Process | Where-Object {{ $_.Name -like 'chrome*' -and $_.CommandLine -like '*{marker}*' }} | ForEach-Object {{ $_.CommandLine }}"
        );
        let out = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .output()
            .expect("powershell runs");
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(str::to_string)
            .collect()
    }

    /// Playwright passes `--no-sandbox` to Chromium unless the sandbox is asked for: the renderer
    /// that parses strangers' pages would then run with every right the user has.
    #[cfg(windows)]
    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "needs chromium"]
    async fn chromium_runs_with_its_sandbox_on() {
        let dir = tempfile::tempdir().unwrap();
        let name = dir
            .path()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let playwright = playwright_rs::Playwright::launch().await.unwrap();
        // The filler's browser names its profile folder; the plain one carries a switch Chromium
        // ignores, so both can be found among the running processes.
        let profile = dir.path().join("profile");
        let context = playwright
            .chromium()
            .launch_persistent_context_with_options(
                profile.to_string_lossy().into_owned(),
                persistent_options(false),
            )
            .await
            .unwrap();
        let marker = format!("--jobbot-test-marker={name}");
        let browser = playwright
            .chromium()
            .launch_with_options(launch_options().args(vec![marker.clone()]))
            .await
            .unwrap();

        let filler = chromium_command_lines(&profile.to_string_lossy());
        let plain = chromium_command_lines(&marker);
        let _ = context.close().await;
        let _ = browser.close().await;
        for (which, lines) in [("persistent", &filler), ("plain", &plain)] {
            assert!(!lines.is_empty(), "no {which} Chromium process found");
            assert!(
                lines.iter().all(|line| !line.contains("--no-sandbox")),
                "{which} Chromium runs without its sandbox: {lines:?}"
            );
        }
    }
}
