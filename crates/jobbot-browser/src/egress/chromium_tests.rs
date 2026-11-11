//! The proxy with a real Chromium: what a page asks for reaches the proxy and nothing else.
//! `cargo test -p jobbot-browser --lib egress::chromium_tests -- --ignored --test-threads=4`

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use jobbot_domain::answers::AnswerBank;
use jobbot_domain::fixtures::make_job_with;
use jobbot_domain::form::{SubmitMode, SubmitStatus};
use jobbot_domain::navigation::NavPolicy;
use playwright_rs::{Browser, LaunchOptions, Playwright};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

use super::tests::FakeResolver;
use super::*;
use crate::form::fill_application;
use crate::launch::{guarded_launch_options, launch_options};
use crate::test_support::fixtures_dir;

/// The driver and a Chromium started with `options`.
async fn chromium(options: LaunchOptions) -> (Playwright, Browser) {
    let playwright = Playwright::launch().await.unwrap();
    let browser = playwright
        .chromium()
        .launch_with_options(options)
        .await
        .unwrap();
    (playwright, browser)
}

/// A listener on `ip` that counts the connections it is offered and answers none of them.
struct Counter {
    port: u16,
    seen: Arc<AtomicUsize>,
    accept: JoinHandle<()>,
}

impl Counter {
    async fn on(ip: IpAddr) -> Option<Counter> {
        let listener = TcpListener::bind((ip, 0)).await.ok()?;
        let port = listener.local_addr().ok()?.port();
        let seen = Arc::new(AtomicUsize::new(0));
        let count = Arc::clone(&seen);
        let accept = tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                count.fetch_add(1, Ordering::SeqCst);
                drop(stream);
            }
        });
        Some(Counter { port, seen, accept })
    }

    fn seen(&self) -> usize {
        self.seen.load(Ordering::SeqCst)
    }
}

impl Drop for Counter {
    fn drop(&mut self) {
        self.accept.abort();
    }
}

/// Opens the probe page aimed at `v4` (and `v6` when this machine has IPv6 loopback) and gives it
/// time to try everything, its last navigation included.
async fn run_probe(browser: &Browser, v4: &Counter, v6: Option<&Counter>) {
    let mut url = url::Url::from_file_path(fixtures_dir().join("egress_probe.html")).unwrap();
    let mut query = format!("port={}", v4.port);
    if let Some(v6) = v6 {
        query.push_str(&format!("&port6={}", v6.port));
    }
    url.set_query(Some(&query));
    let page = browser.new_page().await.unwrap();
    page.goto(url.as_str(), None).await.unwrap();
    tokio::time::sleep(Duration::from_secs(4)).await;
    let _ = page.close().await;
}

fn hosts(refusals: &[Refusal]) -> Vec<&str> {
    refusals
        .iter()
        .map(|refusal| refusal.host.as_str())
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn a_page_cannot_reach_this_machine_through_the_proxy() {
    let v4 = Counter::on(IpAddr::V4(Ipv4Addr::LOCALHOST)).await.unwrap();
    let v6 = Counter::on(IpAddr::V6(Ipv6Addr::LOCALHOST)).await;

    // Control: without the proxy the probe does reach the listener, so zero below means something.
    let (_driver, plain) = chromium(launch_options()).await;
    run_probe(&plain, &v4, v6.as_ref()).await;
    let _ = plain.close().await;
    let reached = v4.seen() + v6.as_ref().map_or(0, Counter::seen);
    assert!(
        reached > 0,
        "the probe reached nothing even without the proxy"
    );

    let v4 = Counter::on(IpAddr::V4(Ipv4Addr::LOCALHOST)).await.unwrap();
    let v6 = Counter::on(IpAddr::V6(Ipv6Addr::LOCALHOST)).await;
    let egress = Egress::start(Arc::new(SystemResolver)).await.unwrap();
    let (_driver, guarded) = chromium(guarded_launch_options(&egress)).await;
    run_probe(&guarded, &v4, v6.as_ref()).await;
    let _ = guarded.close().await;

    assert_eq!(v4.seen(), 0, "a page reached 127.0.0.1 past the proxy");
    if let Some(v6) = &v6 {
        assert_eq!(v6.seen(), 0, "a page reached [::1] past the proxy");
    }
    let refusals = egress.refusals_since(0);
    let refused = hosts(&refusals);
    let mut expected = vec!["127.0.0.1", "localhost", "169.254.169.254"];
    if v6.is_some() {
        expected.push("[::1]");
    }
    for host in expected {
        assert!(
            refused.contains(&host),
            "the proxy was never asked for {host}: {refusals:?}"
        );
    }
    assert!(
        refusals.iter().all(|refusal| refusal.blocked),
        "{refusals:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn the_resolver_rule_leaves_chromium_no_lookups_of_its_own() {
    let rule = crate::launch::EGRESS_ARGS[0].to_string();
    let (_driver, browser) = chromium(launch_options().args(vec![rule])).await;
    let page = browser.new_page().await.unwrap();
    let error = page
        .goto("https://example.com/", None)
        .await
        .expect_err("a name the browser may not look up does not open");
    let _ = browser.close().await;
    assert!(
        error.to_string().contains("ERR_NAME_NOT_RESOLVED"),
        "{error}"
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn a_rebinding_name_is_refused_and_explained() {
    let egress = Egress::start(Arc::new(FakeResolver::fixed(&[(
        "rebind.example",
        &["192.168.1.1"],
    )])))
    .await
    .unwrap();
    let (_driver, browser) = chromium(guarded_launch_options(&egress)).await;
    let dir = tempfile::tempdir().unwrap();
    let cv = dir.path().join("cv.pdf");
    std::fs::write(&cv, b"%PDF-1.4 fake").unwrap();
    let job = make_job_with(|j| j.apply_url = "https://rebind.example/apply".into());
    let bank = AnswerBank {
        entries: Vec::new(),
        consent_phrases: Vec::new(),
    };
    let page = browser.new_page().await.unwrap();

    let mark = egress.mark();
    let result = fill_application(
        &page,
        &job,
        &cv,
        &bank,
        SubmitMode::DryRun,
        &dir.path().join("shots"),
        NavPolicy::STRICT,
    )
    .await
    .unwrap();
    let raw = result.reason.clone();
    let result = explain_tunnel_failure(result, &egress, mark);
    let _ = browser.close().await;

    assert_eq!(result.status, SubmitStatus::NeedsGonza);
    assert_eq!(
        result.reason.as_deref(),
        Some(
            "unsafe_url: the address check refused rebind.example (the name answered \
             192.168.1.1, a local or private address)"
        ),
        "Chromium said {raw:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium and the internet"]
async fn a_public_site_loads_through_the_proxy() {
    let egress = Egress::start(Arc::new(SystemResolver)).await.unwrap();
    let (_driver, browser) = chromium(guarded_launch_options(&egress)).await;
    let page = browser.new_page().await.unwrap();
    let response = page.goto("https://example.com/", None).await;
    let title = page.title().await;
    let _ = browser.close().await;
    response.expect("example.com opens through the proxy");
    assert_eq!(title.unwrap(), "Example Domain");
    assert!(
        egress.refusals_since(0).is_empty(),
        "{:?}",
        egress.refusals_since(0)
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn a_browser_whose_proxy_stopped_opens_nothing() {
    let egress = Egress::start(Arc::new(SystemResolver)).await.unwrap();
    let (_driver, browser) = chromium(guarded_launch_options(&egress)).await;
    drop(egress);
    tokio::time::sleep(Duration::from_millis(200)).await;
    let page = browser.new_page().await.unwrap();
    let error = page
        .goto("https://example.com/", None)
        .await
        .expect_err("no proxy, no way out");
    let _ = browser.close().await;
    assert!(
        error.to_string().contains("ERR_PROXY_CONNECTION_FAILED"),
        "{error}"
    );
}

/// Opens the WebRTC probe aimed at a UDP socket on 127.0.0.1 and returns what the page said and
/// how many datagrams the socket got while ICE gathered its candidates.
async fn webrtc_datagrams(browser: &Browser) -> (String, usize) {
    let socket = tokio::net::UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let port = socket.local_addr().unwrap().port();
    let seen = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&seen);
    let listen = tokio::spawn(async move {
        let mut datagram = [0u8; 1500];
        while socket.recv_from(&mut datagram).await.is_ok() {
            count.fetch_add(1, Ordering::SeqCst);
        }
    });
    let mut url = url::Url::from_file_path(fixtures_dir().join("webrtc_probe.html")).unwrap();
    url.set_query(Some(&format!("port={port}")));
    let page = browser.new_page().await.unwrap();
    page.goto(url.as_str(), None).await.unwrap();
    let said: String = page.evaluate("window.probe", None::<&()>).await.unwrap();
    tokio::time::sleep(Duration::from_secs(3)).await;
    let _ = page.close().await;
    listen.abort();
    (said, seen.load(Ordering::SeqCst))
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn webrtc_sends_no_udp_from_a_guarded_browser() {
    // Control: without the switches the STUN request does arrive.
    let (_driver, plain) = chromium(launch_options()).await;
    let (said, datagrams) = webrtc_datagrams(&plain).await;
    let _ = plain.close().await;
    assert_eq!(said, "offered");
    assert!(
        datagrams > 0,
        "WebRTC sent nothing even without the switches"
    );

    let egress = Egress::start(Arc::new(SystemResolver)).await.unwrap();
    let (_driver, guarded) = chromium(guarded_launch_options(&egress)).await;
    let (said, datagrams) = webrtc_datagrams(&guarded).await;
    let _ = guarded.close().await;
    assert_eq!(said, "offered", "the page did run its WebRTC");
    assert_eq!(datagrams, 0, "WebRTC sent UDP past the proxy");
}
