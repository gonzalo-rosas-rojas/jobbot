//! How jobbot starts Chromium. Every launch takes its options from here, so the sandbox, the
//! deadline and the way out (`egress`) are set in one place.

use playwright_rs::protocol::ProxySettings;
use playwright_rs::{BrowserContextOptions, LaunchOptions};

use crate::egress::Egress;

/// What a browser that opens strangers' pages adds to its launch, next to the proxy: Chromium makes
/// no name lookup of its own (a name can only reach the proxy, at 127.0.0.1), WebRTC sends no UDP
/// past the proxy, and QUIC (UDP, which an HTTP proxy cannot carry) is off.
pub(crate) const EGRESS_ARGS: [&str; 3] = [
    "--host-resolver-rules=MAP * ^NOTFOUND , EXCLUDE 127.0.0.1",
    "--force-webrtc-ip-handling-policy=disable_non_proxied_udp",
    "--disable-quic",
];

/// Every connection through `egress`. The bypass list only removes Chromium's implicit one
/// (`<-loopback>`), so not even this machine's addresses skip the proxy. Named here rather than
/// left to Playwright, which adds it itself unless `PLAYWRIGHT_DISABLE_FORCED_CHROMIUM_PROXIED_LOOPBACK`
/// is set in the environment the driver inherits.
fn through(egress: &Egress) -> ProxySettings {
    ProxySettings::new(egress.server()).bypass("<-loopback>")
}

fn egress_args() -> Vec<String> {
    EGRESS_ARGS.map(String::from).to_vec()
}

/// The most a Chromium launch may take. Playwright's own default is three minutes; a browser that
/// has not started in 30 s is stuck, and the run should say so.
pub(crate) const LAUNCH_TIMEOUT_MS: f64 = 30_000.0;

/// Options for a plain browser launch (the CV printer, which opens no web page, and the Chromium
/// tests' own sessions). The sandbox is asked for: without it Playwright starts Chromium with
/// `--no-sandbox`.
pub(crate) fn launch_options() -> LaunchOptions {
    LaunchOptions::default()
        .chromium_sandbox(true)
        .timeout(LAUNCH_TIMEOUT_MS)
}

/// The plain launch for a browser that opens strangers' pages (the form inspector): every
/// connection goes through `egress`.
pub(crate) fn guarded_launch_options(egress: &Egress) -> LaunchOptions {
    launch_options().proxy(through(egress)).args(egress_args())
}

/// The filler's persistent profile, headless unless `headed`, every connection through `egress`,
/// downloads refused.
pub(crate) fn persistent_options(headed: bool, egress: &Egress) -> BrowserContextOptions {
    BrowserContextOptions::builder()
        .headless(!headed)
        .chromium_sandbox(true)
        .timeout(LAUNCH_TIMEOUT_MS)
        // A page that starts a download (a form is not a place to get a file) saves nothing here.
        .accept_downloads(false)
        .proxy(through(egress))
        .args(egress_args())
        .build()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::egress::SystemResolver;
    use playwright_rs::protocol::AcceptDownloads;

    #[test]
    fn a_plain_launch_asks_for_the_sandbox_and_a_deadline_and_no_proxy() {
        let plain = launch_options();
        assert_eq!(plain.chromium_sandbox, Some(true));
        assert_eq!(plain.timeout, Some(LAUNCH_TIMEOUT_MS));
        assert!(plain.proxy.is_none(), "the CV printer opens no web page");
    }

    #[tokio::test]
    async fn every_guarded_launch_goes_through_the_proxy() {
        let egress = Egress::start(Arc::new(SystemResolver)).await.unwrap();
        let switches = Some(EGRESS_ARGS.map(String::from).to_vec());

        let plain = guarded_launch_options(&egress);
        let proxy = plain.proxy.as_ref().expect("a proxy");
        assert_eq!(proxy.server, egress.server());
        assert_eq!(
            proxy.bypass.as_deref(),
            Some("<-loopback>"),
            "only the implicit bypass is named, and it is removed: nothing skips the proxy"
        );
        assert_eq!(plain.args, switches);
        assert_eq!(plain.chromium_sandbox, Some(true));
        assert_eq!(plain.timeout, Some(LAUNCH_TIMEOUT_MS));

        for headed in [false, true] {
            let persistent = persistent_options(headed, &egress);
            let proxy = persistent.proxy.as_ref().expect("a proxy");
            assert_eq!(proxy.server, egress.server());
            assert_eq!(proxy.bypass.as_deref(), Some("<-loopback>"));
            assert_eq!(persistent.args, switches);
            assert_eq!(persistent.chromium_sandbox, Some(true));
            assert_eq!(persistent.timeout, Some(LAUNCH_TIMEOUT_MS));
            assert_eq!(persistent.headless, Some(!headed));
            assert_eq!(
                persistent.accept_downloads,
                Some(AcceptDownloads::Deny),
                "a page the filler opens never saves a file on this machine"
            );
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
        let egress = Egress::start(Arc::new(SystemResolver)).await.unwrap();
        // The filler's browser names its profile folder; the plain one carries a switch Chromium
        // ignores, so both can be found among the running processes.
        let profile = dir.path().join("profile");
        let context = playwright
            .chromium()
            .launch_persistent_context_with_options(
                profile.to_string_lossy().into_owned(),
                persistent_options(false, &egress),
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

    /// What Chromium is really started with when it opens strangers' pages: the proxy, nothing
    /// that skips it but what Playwright removes (`<-loopback>` takes away the implicit bypass of
    /// this machine's addresses), no lookups of its own, and WebRTC kept to the proxy.
    #[cfg(windows)]
    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "needs chromium"]
    async fn a_guarded_chromium_carries_the_egress_switches() {
        let dir = tempfile::tempdir().unwrap();
        let playwright = playwright_rs::Playwright::launch().await.unwrap();
        let for_filler = Egress::start(Arc::new(SystemResolver)).await.unwrap();
        let for_inspector = Egress::start(Arc::new(SystemResolver)).await.unwrap();
        let context = playwright
            .chromium()
            .launch_persistent_context_with_options(
                dir.path().join("profile").to_string_lossy().into_owned(),
                persistent_options(false, &for_filler),
            )
            .await
            .unwrap();
        let browser = playwright
            .chromium()
            .launch_with_options(guarded_launch_options(&for_inspector))
            .await
            .unwrap();

        // Each browser is found by its proxy's port, which only its own command line carries.
        let port = |egress: &Egress| egress.server().trim_start_matches("http://").to_string();
        let filler = chromium_command_lines(&port(&for_filler));
        let inspector = chromium_command_lines(&port(&for_inspector));
        let _ = context.close().await;
        let _ = browser.close().await;
        for (which, lines) in [("persistent", &filler), ("plain", &inspector)] {
            assert!(!lines.is_empty(), "no {which} Chromium process found");
            for switch in ["--proxy-server=", "--proxy-bypass-list=<-loopback>"]
                .into_iter()
                .chain(EGRESS_ARGS)
            {
                assert!(
                    lines.iter().any(|line| line.contains(switch)),
                    "{which} Chromium lacks {switch}: {lines:?}"
                );
            }
        }
    }
}
