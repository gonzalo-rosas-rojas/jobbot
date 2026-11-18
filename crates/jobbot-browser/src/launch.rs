//! How jobbot starts Chromium. Every launch goes through `start` or `start_persistent` and takes
//! its options from here, so the sandbox, the deadline and the way out (`egress`) are set in one
//! place, and so is the refusal to start where the library would take the sandbox away.

use std::path::Path;

use anyhow::Context;
use playwright_rs::protocol::ProxySettings;
use playwright_rs::{Browser, BrowserContext, BrowserContextOptions, LaunchOptions, Playwright};

use crate::egress::Egress;

/// The variables that make playwright-rs weaken Chromium on its own. Version 0.20.1, on Windows
/// (`protocol/browser_type.rs`, "Windows CI stability flags"), adds `--no-sandbox`,
/// `--disable-web-security` and `--disable-features=IsolateOrigins,site-per-process` to every
/// launch when either is in the environment, whatever the options ask for. The renderer that
/// parses strangers' pages would then run with every right the user has, and a page could read
/// another site's answers.
const WEAKENING_VARIABLES: [&str; 2] = ["CI", "GITHUB_ACTIONS"];

fn in_the_environment(name: &str) -> bool {
    std::env::var_os(name).is_some()
}

/// Fails where a Chromium started now would lose its sandbox to `WEAKENING_VARIABLES`.
fn refuse_when_set(is_set: impl Fn(&str) -> bool) -> anyhow::Result<()> {
    if !cfg!(windows) {
        return Ok(());
    }
    match WEAKENING_VARIABLES.into_iter().find(|name| is_set(name)) {
        Some(name) => anyhow::bail!(
            "{name} is set, and with it playwright-rs starts Chromium with --no-sandbox and \
             --disable-web-security on Windows whatever jobbot asks for: jobbot does not start \
             Chromium like that. Remove {name} from the environment of this run."
        ),
        None => Ok(()),
    }
}

/// Runs `launch` unless the refusal stands: a refused launch is never begun.
async fn unless_weakened<T, Launching>(
    is_set: impl Fn(&str) -> bool,
    launch: impl FnOnce() -> Launching,
) -> anyhow::Result<T>
where
    Launching: Future<Output = playwright_rs::Result<T>>,
{
    refuse_when_set(is_set)?;
    launch().await.context(
        "Chromium did not start (if it is not installed yet, playwright_rs::install_browsers installs it)",
    )
}

/// Starts a Chromium with `options`, or refuses where the library would take its sandbox away.
/// Public for the browser tests outside this crate, so that theirs starts the same way.
pub async fn start(playwright: &Playwright, options: LaunchOptions) -> anyhow::Result<Browser> {
    unless_weakened(in_the_environment, || {
        playwright.chromium().launch_with_options(options)
    })
    .await
}

/// Starts a Chromium that keeps its profile in `profile_dir`, or refuses like `start`.
pub(crate) async fn start_persistent(
    playwright: &Playwright,
    profile_dir: &Path,
    options: BrowserContextOptions,
) -> anyhow::Result<BrowserContext> {
    unless_weakened(in_the_environment, || {
        playwright
            .chromium()
            .launch_persistent_context_with_options(
                profile_dir.to_string_lossy().into_owned(),
                options,
            )
    })
    .await
}

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

    /// The two names playwright-rs 0.20.1 reads (`std::env::var("CI")` and
    /// `std::env::var("GITHUB_ACTIONS")` in its `protocol/browser_type.rs`), as literals.
    #[cfg(windows)]
    #[tokio::test]
    async fn with_ci_or_github_actions_set_the_launch_is_refused_by_name_and_never_begun() {
        for name in ["CI", "GITHUB_ACTIONS"] {
            let begun = std::cell::Cell::new(false);
            let refusal = unless_weakened(
                |asked| asked == name,
                || async {
                    begun.set(true);
                    Ok(())
                },
            )
            .await
            .unwrap_err()
            .to_string();
            assert!(!begun.get(), "the launch began with {name} set");
            assert!(refusal.starts_with(&format!("{name} is set")), "{refusal}");
            assert!(refusal.contains("--no-sandbox"), "{refusal}");
        }
    }

    #[tokio::test]
    async fn with_neither_set_the_launch_runs_and_its_own_failure_keeps_its_cause() {
        let started = unless_weakened(|_| false, || async { Ok(7) }).await;
        assert_eq!(started.unwrap(), 7);

        let failed = unless_weakened(
            |_| false,
            || async { Err::<(), _>(playwright_rs::Error::Timeout("no browser".to_string())) },
        )
        .await
        .unwrap_err();
        let chain = format!("{failed:#}");
        assert!(chain.starts_with("Chromium did not start"), "{chain}");
        assert!(chain.contains("no browser"), "{chain}");
    }

    /// The refusal is in `start` and `start_persistent`; a launch written anywhere else in this
    /// crate would skip it.
    #[test]
    fn chromium_is_launched_only_from_this_file() {
        fn rust_files(dir: &Path, found: &mut Vec<std::path::PathBuf>) {
            for entry in std::fs::read_dir(dir).expect("a source folder") {
                let path = entry.expect("an entry").path();
                if path.is_dir() {
                    rust_files(&path, found);
                } else if path.extension().is_some_and(|ext| ext == "rs") {
                    found.push(path);
                }
            }
        }
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = Vec::new();
        rust_files(&src, &mut files);
        assert!(files.len() > 1, "no sources found under {}", src.display());
        // Every launch starts from the library's `chromium()`; the two names are there for a
        // launch written without it (`BrowserType::launch_with_options(..)`).
        let calls = [
            "chromium()",
            "launch_with_options(",
            "launch_persistent_context",
        ];
        let this_file = src.join("launch.rs");
        let elsewhere: Vec<String> = files
            .iter()
            .filter(|path| **path != this_file)
            .filter(|path| {
                let text = std::fs::read_to_string(path).expect("a readable source");
                calls.iter().any(|call| text.contains(call))
            })
            .map(|path| path.display().to_string())
            .collect();
        assert!(
            elsewhere.is_empty(),
            "Chromium launched outside launch.rs: {elsewhere:?}"
        );
    }

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
        let switches = Some(egress_args());

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
        let context = start_persistent(&playwright, &profile, persistent_options(false, &egress))
            .await
            .unwrap();
        let marker = format!("--jobbot-test-marker={name}");
        let browser = start(&playwright, launch_options().args(vec![marker.clone()]))
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
        let context = start_persistent(
            &playwright,
            &dir.path().join("profile"),
            persistent_options(false, &for_filler),
        )
        .await
        .unwrap();
        let browser = start(&playwright, guarded_launch_options(&for_inspector))
            .await
            .unwrap();

        // Each browser is found by its proxy's port, which only its own command line carries.
        let proxy_address =
            |egress: &Egress| egress.server().trim_start_matches("http://").to_string();
        let filler = chromium_command_lines(&proxy_address(&for_filler));
        let inspector = chromium_command_lines(&proxy_address(&for_inspector));
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
