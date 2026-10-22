//! The panel page in a real Chromium, against a real server and real SQLite files in a temp
//! folder. Run with `cargo test --test panel_browser -- --ignored --test-threads=1`.
//!
//! The page is "El Turno Continuado": one sheet that shows one moment at a time, five stops (`hoy`,
//! `procesos`, `cola`, `aprender`, `sistema`) and a prologue, reached by the number keys 1-5 and 0, by the
//! tabs of the HUD or by the address (`#cola`, `#procesos/<job key>`). On a desktop window the sheet rides a
//! camera over a sky; under 760 px wide the same sheet is a flat page with six tabs. A drawer (`aside#detail`)
//! shows one application. Every moment draws from the one snapshot the server serves, and every verb that
//! changes something goes to the server first (the row dims at once, `data-pending`) and the page redraws from
//! the snapshot that follows.
//!
//! To keep a test about a state and never about an animation, a page is opened in Spanish (`lang=es`, what the
//! Rust-served page does anyway), with trips that do not travel (`motion=calm`), with the sky pinned to nine in
//! the morning (`clock=9`, which is also the hour the moments write) and with `debug=1`, which gives the tests
//! `window.__app` (`view`, `engine`, `lenis`, `stage`, `model()`, `mode()`...). Keys are typed with the real
//! keyboard: the page ignores a synthetic `keydown`, as it should. A test about the motion opens the page
//! without `motion=calm` on purpose.
//!
//! Every test ends in `Browse::stop`, which fails on a console error, an uncaught exception or a violation of
//! the page's Content-Security-Policy, so each of them is also a test that the page breaks no rule of its policy.
//! `PANEL_SHOTS=<folder>` makes the tests that call `Browse::shot` leave a picture there.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::TimeDelta;
use jobbot::panel::Panel;
use jobbot::panel::server::PanelServer;
use jobbot::paths::Paths;
use jobbot::prep::PrepStore;
use jobbot::testing::{NOW, make_job_with};
use jobbot::tracker::{ApplicationRow, EmailKind, NewEmail, NewTask, State, Tracker};
use playwright_rs::protocol::{KeyboardOptions, Viewport};
use playwright_rs::{Browser, LaunchOptions, Page, Playwright};

const HOSTILE: &str = "<img src=x onerror=\"window.pwned=1\">Evil";

/// The flags most tests open the page with (see the header of this file).
const CALM: &str = "&lang=es&motion=calm&debug=1&clock=9";

/// A permanent button is armed by the first press and fires on a second one at least 350 ms later (the
/// page's own minimum, so that a double click can never confirm); this is a little over that.
const ARM_PAUSE: Duration = Duration::from_millis(420);

/// The words that mean a template or a value leaked into the text of the page.
const STRAYS: [&str; 4] = ["undefined", "NaN", "[object", "null"];

/// A 1x1 PNG, so the proposals have a real dry-run capture to show.
const TINY_PNG: [u8; 70] = [
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4,
    0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0xda, 0x63, 0x64, 0x60, 0xf8, 0x5f,
    0x0f, 0x00, 0x02, 0x87, 0x01, 0x80, 0xeb, 0x47, 0xba, 0x92, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45,
    0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

struct Env {
    _dir: tempfile::TempDir,
    paths: Paths,
    server: PanelServer,
}

impl Env {
    /// `seed`: three applications. test:1 needs Gonza and has a hostile title; test:2 is prepared
    /// with a dry-run screenshot waiting for approval; test:3 was submitted. Plus four tasks (one
    /// overdue with hostile text, one with a deadline, two added by hand: one with a link and one
    /// whose stored address is not http) and two emails of test:3 (a recruiter's, hostile, and an
    /// acknowledgement).
    fn start(seed: bool) -> Env {
        Env::start_with(seed, false)
    }

    /// The seed plus the shipped `criteria.toml` and `schedule.toml` in the profile folder, so that the
    /// threshold, the trust meter and the busy hours exist for the moments to draw.
    fn start_full() -> Env {
        Env::start_with(true, true)
    }

    fn start_with(seed: bool, profile: bool) -> Env {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::new(dir.path());
        if profile {
            std::fs::create_dir_all(&paths.profile_dir).unwrap();
            let shipped = jobbot::testing::real_paths().profile_dir;
            for file in ["criteria.toml", "schedule.toml"] {
                std::fs::copy(shipped.join(file), paths.profile_dir.join(file)).unwrap();
            }
        }
        let panel = Panel::open(paths.clone(), Box::new(|| *NOW)).unwrap();
        if seed {
            let t = &panel.tracker;
            let add = |id: &str, title: &str, states: &[State]| {
                let job = make_job_with(|j| {
                    j.external_id = id.into();
                    j.company = format!("Co {id}");
                    j.title = title.into();
                });
                t.add_job(&job, 80.0, &BTreeMap::new(), *NOW).unwrap();
                t.create_application(&job.key(), *NOW).unwrap();
                for state in states {
                    t.transition(&job.key(), *state, Some("falta email"), *NOW)
                        .unwrap();
                }
                job.key()
            };
            add("1", HOSTILE, &[State::NeedsGonza]);
            let two = add("2", "Prepared role", &[State::Prepared]);
            add("3", "Sent role", &[State::Prepared, State::Submitted]);
            let shots = paths.data_dir.join("screenshots");
            std::fs::create_dir_all(&shots).unwrap();
            let shot = shots.join("test_2.png");
            std::fs::write(&shot, TINY_PNG).unwrap();
            t.set_screenshot(&two, &shot).unwrap();

            // Two tasks (one overdue with hostile text, one due in a day and a half), and a recent
            // answer with hostile text next to an acknowledgement that must stay out of the list.
            let task = |text: &str, due: TimeDelta, key: Option<&str>| NewTask {
                job_key: key.map(str::to_string),
                kind: "action_required".into(),
                text: text.into(),
                due_at: Some(*NOW + due),
                source_message_id: None,
                link: None,
            };
            t.add_task(
                &task(
                    &format!("Responder {HOSTILE}"),
                    -TimeDelta::hours(2),
                    Some("test:1"),
                ),
                *NOW,
            )
            .unwrap();
            t.add_task(
                &task(
                    "Entregar prueba técnica de Acme",
                    TimeDelta::hours(30),
                    None,
                ),
                *NOW,
            )
            .unwrap();
            // Two tasks added by hand: one with a normal link, one whose stored address is not
            // http(s) (the CLI never stores that; the page must not trust it anyway).
            let manual = |text: &str, link: &str| NewTask {
                job_key: None,
                kind: "manual".into(),
                text: text.into(),
                due_at: None,
                source_message_id: None,
                link: Some(link.into()),
            };
            t.add_manual_task(
                &manual("Revisar la oferta de Acme", "https://example.com/oferta"),
                *NOW,
            )
            .unwrap();
            t.add_manual_task(&manual("Enlace raro", "javascript:window.pwned=1"), *NOW)
                .unwrap();
            let email = |id: &str, kind: EmailKind, subject: &str, from: &str| NewEmail {
                message_id: id.into(),
                thread_id: format!("th-{id}"),
                from_addr: from.into(),
                subject: subject.into(),
                received_at: *NOW - TimeDelta::hours(1),
                job_key: Some("test:3".into()),
                kind,
                summary: "Quiere hablar de sueldo".into(),
            };
            t.add_email(
                &email(
                    "m1",
                    EmailKind::Recruiter,
                    HOSTILE,
                    "<svg onload=window.pwned=1>",
                ),
                None,
                *NOW,
            )
            .unwrap();
            t.add_email(
                &email(
                    "m2",
                    EmailKind::Ack,
                    "Recibimos tu postulación",
                    "no-reply@acme.example",
                ),
                None,
                *NOW,
            )
            .unwrap();
        }
        let server = PanelServer::start(panel, 0).unwrap();
        Env {
            _dir: dir,
            paths,
            server,
        }
    }

    fn tracker(&self) -> Tracker {
        Tracker::open(&self.paths.data_dir.join("jobbot.db")).unwrap()
    }

    /// One application as SQLite has it right now.
    fn app(&self, key: &str) -> ApplicationRow {
        self.tracker().get_application(key).unwrap().unwrap()
    }

    /// The address to open: the token and the usual flags.
    fn url(&self) -> String {
        format!("{}{CALM}", self.server.url())
    }

    /// The address with flags of the test's own, and none of the usual ones.
    fn url_with(&self, flags: &str) -> String {
        format!("{}{flags}", self.server.url())
    }

    /// One more prepared application with a dry-run capture, so the gate has another proposal.
    fn add_proposal(&self, id: &str, title: &str, score: f64) -> String {
        let t = self.tracker();
        let job = make_job_with(|j| {
            j.external_id = id.into();
            j.company = format!("Co {id}");
            j.title = title.into();
        });
        t.add_job(&job, score, &BTreeMap::new(), *NOW).unwrap();
        t.create_application(&job.key(), *NOW).unwrap();
        t.transition(&job.key(), State::Prepared, None, *NOW)
            .unwrap();
        let shot = self
            .paths
            .data_dir
            .join("screenshots")
            .join(format!("test_{id}.png"));
        std::fs::write(&shot, TINY_PNG).unwrap();
        t.set_screenshot(&job.key(), &shot).unwrap();
        job.key()
    }
}

struct Browse {
    _playwright: Playwright,
    browser: Browser,
    page: Page,
    /// What the page complained about: console errors and uncaught exceptions. A CSP violation is a
    /// console error in Chromium, so every test is also a test that the page breaks no rule of its policy.
    problems: Arc<Mutex<Vec<String>>>,
    /// Every address the page asked for (`METHOD url`) and what each answer said (`status url`).
    requests: Arc<Mutex<Vec<String>>>,
    answers: Arc<Mutex<Vec<String>>>,
}

impl Browse {
    async fn start() -> Browse {
        Browse::start_with_args(Vec::new()).await
    }

    /// A browser with extra command-line switches (`--host-resolver-rules` gives it another name for this PC).
    async fn start_with_args(mut args: Vec<String>) -> Browse {
        // The page has a sound engine; a test browser must never play it through the real speakers.
        args.push("--mute-audio".to_string());
        let playwright = Playwright::launch().await.unwrap();
        let browser = playwright
            .chromium()
            .launch_with_options(LaunchOptions::new().args(args))
            .await
            .unwrap();
        let page = browser.new_page().await.unwrap();
        let problems = Arc::new(Mutex::new(Vec::new()));
        let seen = problems.clone();
        page.on_console(move |message| {
            let seen = seen.clone();
            async move {
                if message.type_() == "error" {
                    seen.lock().unwrap().push(message.text().to_string());
                }
                Ok(())
            }
        })
        .await
        .unwrap();
        let seen = problems.clone();
        page.on_pageerror(move |error| {
            let seen = seen.clone();
            async move {
                seen.lock().unwrap().push(format!("uncaught: {error}"));
                Ok(())
            }
        })
        .await
        .unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let seen = requests.clone();
        page.on_request(move |request| {
            let seen = seen.clone();
            async move {
                seen.lock()
                    .unwrap()
                    .push(format!("{} {}", request.method(), request.url()));
                Ok(())
            }
        })
        .await
        .unwrap();
        let answers = Arc::new(Mutex::new(Vec::new()));
        let seen = answers.clone();
        page.on_response(move |response| {
            let seen = seen.clone();
            async move {
                seen.lock()
                    .unwrap()
                    .push(format!("{} {}", response.status(), response.url()));
                Ok(())
            }
        })
        .await
        .unwrap();
        // The browser's own report of a policy violation, in case a console line is ever missing.
        page.add_init_script(
            "window.__csp = []; document.addEventListener('securitypolicyviolation', (e) => window.__csp.push(e.violatedDirective + ' ' + e.blockedURI), true);",
        )
        .await
        .unwrap();
        Browse {
            _playwright: playwright,
            browser,
            page,
            problems,
            requests,
            answers,
        }
    }

    /// The window of a phone (the flat page) or any other size; before the page is opened.
    async fn resize(&self, width: u32, height: u32) {
        self.page
            .set_viewport_size(Viewport { width, height })
            .await
            .unwrap();
    }

    async fn open(&self, url: &str) {
        self.page.goto(url, None).await.unwrap();
    }

    /// Opens the page of `env` and waits for the first snapshot to be drawn.
    async fn visit(&self, env: &Env) {
        self.open(&env.url()).await;
        self.ready().await;
    }

    /// The frame is there and the first snapshot has been accepted and drawn in the moment on the sheet.
    async fn ready(&self) {
        self.wait("window.__app && window.__app.model() !== null && document.querySelector('#app .sheet-host section') !== null")
            .await;
    }

    /// Polls `js` until it is truthy. Not `Page::wait_for_function`: that evaluates its predicate
    /// with `eval` inside the page, which the page's own policy (no `unsafe-eval`) refuses on
    /// purpose. `evaluate_value` goes through the debugging protocol, so the real policy stays on.
    async fn wait(&self, js: &str) {
        let probe = format!("String(!!({js}))");
        for _ in 0..150 {
            if self
                .page
                .evaluate_value(&probe)
                .await
                .is_ok_and(|v| v == "true")
            {
                return;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        panic!(
            "timed out waiting for {js}\npage text:\n{}",
            self.text().await
        );
    }

    async fn eval(&self, js: &str) -> String {
        self.page.evaluate_value(js).await.unwrap()
    }

    /// How many elements match, as a number.
    async fn count(&self, selector: &str) -> usize {
        self.eval(&format!(
            "String(document.querySelectorAll({selector:?}).length)"
        ))
        .await
        .parse()
        .unwrap()
    }

    /// The text of the first match (`textContent`: it works for SVG text too, which has no `innerText`).
    async fn text_of(&self, selector: &str) -> String {
        self.eval(&format!(
            "String(document.querySelector({selector:?}).textContent)"
        ))
        .await
    }

    /// An attribute of the first match (the word "null" when it has none or there is no such element).
    async fn attr(&self, selector: &str, name: &str) -> String {
        self.eval(&format!(
            "String((document.querySelector({selector:?}) || document.body).getAttribute({name:?}))"
        ))
        .await
    }

    /// Clicks the first match, as a person would (it must be visible, enabled and not covered).
    async fn click(&self, selector: &str) {
        self.page
            .locator(selector)
            .first()
            .click(None)
            .await
            .unwrap();
    }

    /// A permanent action asks twice: the first press only arms the button, a deliberate second one does it.
    async fn confirm_click(&self, selector: &str) {
        self.click(selector).await;
        tokio::time::sleep(ARM_PAUSE).await;
        self.click(selector).await;
    }

    /// A key as the page sees it from the keyboard: it goes to whatever has the focus.
    async fn key(&self, name: &str) {
        self.page
            .keyboard()
            .press(name, None::<KeyboardOptions>)
            .await
            .unwrap();
    }

    /// Goes to a stop by its number key and waits for its moment to be on the sheet.
    async fn go(&self, stop: &str) {
        let key = match stop {
            "hoy" => "1",
            "procesos" => "2",
            "cola" => "3",
            "aprender" => "4",
            "sistema" => "5",
            other => panic!("no stop called {other}"),
        };
        self.key(key).await;
        self.at(stop).await;
    }

    /// Waits for the page to be at `stop` with its moment mounted in the sheet.
    async fn at(&self, stop: &str) {
        self.wait(&format!(
            "document.body.dataset.stop === '{stop}' && document.querySelector('#app[data-moment=\"{stop}\"] .sheet-host section') !== null && document.querySelector('#stops [role=tab][aria-selected=true]').dataset.stop === '{stop}'"
        ))
        .await;
    }

    /// Moves the keyboard's cursor down the rows of the stop until it stands on `row` (`data-row`).
    async fn cursor_to(&self, row: &str) {
        for _ in 0..12 {
            if self
                .eval(&format!(
                    "String(!!document.querySelector('[data-cursor=\"true\"][data-row=\"{row}\"]'))"
                ))
                .await
                == "true"
            {
                return;
            }
            self.key("j").await;
        }
        panic!("the cursor never reached {row}");
    }

    /// Records, before an action, whether any row was ever dimmed while the server answered
    /// (`data-pending`). The page marks the row in the same instant as the press, so a mutation
    /// observer sees it however fast the server is.
    async fn watch_pending(&self) {
        self.eval(
            "(() => { window.__pending = false; new MutationObserver((records) => { for (const r of records) { if (r.target.getAttribute && r.target.getAttribute('data-pending') === 'true') window.__pending = true; } }).observe(document.getElementById('app'), { subtree: true, attributes: true, attributeFilter: ['data-pending'] }); return 'ok'; })()",
        )
        .await;
    }

    async fn saw_pending(&self) -> bool {
        self.eval("String(window.__pending === true)").await == "true"
    }

    async fn text(&self) -> String {
        self.page.locator("body").inner_text().await.unwrap()
    }

    /// A picture of the page in the folder `PANEL_SHOTS` names, when it is set.
    async fn shot(&self, name: &str) {
        if let Ok(dir) = std::env::var("PANEL_SHOTS") {
            let path = std::path::Path::new(&dir).join(format!("{name}.png"));
            self.page.screenshot_to_file(&path, None).await.unwrap();
        }
    }

    /// What the page complained about so far. A failed request is the server saying no (a refused mark, a
    /// wrong token) and the page handling it, so Chromium's line about the status is not a problem.
    fn problems(&self) -> Vec<String> {
        self.problems
            .lock()
            .unwrap()
            .iter()
            .filter(|line| !line.starts_with("Failed to load resource: the server responded with"))
            .cloned()
            .collect()
    }

    /// Closes the browser and fails if the page broke a rule of its policy or threw.
    async fn stop(self) {
        let violations = self
            .page
            .evaluate_value("JSON.stringify(window.__csp || [])")
            .await
            .unwrap_or_default();
        let problems = self.problems();
        self.browser.close().await.unwrap();
        assert!(problems.is_empty(), "the page complained: {problems:#?}");
        assert!(
            violations.is_empty() || violations == "[]",
            "policy violations: {violations}"
        );
    }
}

/// The text of the sheet reads like words: nothing in it is a leaked value.
fn assert_no_strays(what: &str, text: &str) {
    for stray in STRAYS {
        assert!(
            !text.contains(stray),
            "{what} prints a stray {stray:?}:\n{text}"
        );
    }
}

// ---------------------------------------------------------------------------------------------
// The frame: the first paint, the token, the network, the stops, the keys, the themes, the flat page
// ---------------------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn the_frame_is_up_before_the_data_and_the_token_leaves_the_address() {
    let env = Env::start(true);
    let b = Browse::start().await;
    b.open(&env.url()).await;
    // The HUD, the five stops and the sheet are painted without waiting for the snapshot.
    b.wait("document.querySelectorAll('#stops [role=tab]').length === 5")
        .await;
    b.ready().await;
    assert_eq!(b.eval("document.title").await, "Panel");
    assert_eq!(b.text_of(".hud-brand").await.trim().to_lowercase(), "panel");
    assert_eq!(
        b.eval("location.search").await,
        "",
        "the token and the flags are out of the address bar"
    );
    // At nine in the morning the page opens on HOY.
    assert_eq!(b.eval("document.body.dataset.stop").await, "hoy");
    assert_eq!(b.attr("#tab-hoy", "aria-selected").await, "true");
    assert_eq!(b.attr("#app", "data-moment").await, "hoy");
    // The debug handle the page exposes to its tests.
    for part in ["view", "engine", "lenis", "stage", "drawer", "session"] {
        assert_eq!(
            b.eval(&format!("String(window.__app.{part} != null)"))
                .await,
            "true",
            "window.__app.{part}"
        );
    }
    assert_eq!(b.eval("window.__app.mode()").await, "stage");
    // The server accepted the token the page carries in a header, and the first moment says what waits.
    assert!(b.text_of("#hoy-title").await.contains("Te tocan"));
    b.shot("frame-hoy").await;
    b.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn a_wrong_token_leaves_the_frame_standing_and_says_so_in_words() {
    let env = Env::start(true);
    let b = Browse::start().await;
    b.open(&format!(
        "http://127.0.0.1:{}/?t=wrong{CALM}",
        env.server.addr().port()
    ))
    .await;
    b.wait("document.querySelectorAll('#stops [role=tab]').length === 5")
        .await;
    b.wait("(() => { const b = document.querySelector('.sheet-banner'); return !!b && !b.hidden && b.textContent.trim().length > 10; })()")
        .await;
    let banner = b.text_of(".sheet-banner").await;
    assert_no_strays("the banner", &banner);
    assert_eq!(
        b.eval("String(window.__app.model())").await,
        "null",
        "no data came in"
    );
    // Nothing of the applications leaked into a page that was refused.
    let body = b.text().await;
    assert!(!body.contains("Co 2") && !body.contains("Evil"), "{body}");
    assert_eq!(b.count("#stops [role=tab]").await, 5, "the stops stay");
    b.shot("frame-wrong-token").await;
    b.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn nothing_leaves_this_pc_and_the_fonts_come_from_the_launch_prefix() {
    let env = Env::start_full();
    let b = Browse::start().await;
    b.visit(&env).await;
    // Every moment, the drawer and a reload: whatever the page can ask for, it asks for.
    for stop in ["procesos", "cola", "aprender", "sistema", "hoy"] {
        b.go(stop).await;
    }
    b.go("procesos").await;
    b.click("button.cell[data-card=\"test:2\"]").await;
    b.wait("document.getElementById('detail').classList.contains('open')")
        .await;
    b.key("Escape").await;
    b.wait("document.fonts.status === 'loaded'").await;
    let origin = format!("http://127.0.0.1:{}", env.server.addr().port());
    let requests = b.requests.lock().unwrap().clone();
    assert!(requests.len() > 8, "{requests:#?}");
    for request in &requests {
        let url = request.split_once(' ').unwrap().1;
        assert!(
            url.starts_with(&format!("{origin}/"))
                || url.starts_with(&format!("blob:{origin}/"))
                || url.starts_with("data:"),
            "a request left this PC: {request}"
        );
    }
    // The page asked the API with the token in a header, never in an address.
    assert!(
        requests
            .iter()
            .any(|r| r == &format!("GET {origin}/api/snapshot")),
        "{requests:#?}"
    );
    assert!(
        requests.iter().all(|r| !r.contains("t=")
            || r.starts_with(&format!("GET {origin}/?t=")) && !r.contains("/api/")),
        "the token travels as a header: {requests:#?}"
    );
    // The five fonts of the page arrive from the binary, under this launch's own path.
    let launch = env.server.launch_id();
    let answers = b.answers.lock().unwrap().clone();
    for font in [
        "barlow-condensed-700",
        "geist-mono-400",
        "instrument-serif-400italic",
        "inter-400",
        "inter-600",
    ] {
        let url = format!("{origin}/a/{launch}/{font}.woff2");
        assert!(
            answers.iter().any(|a| a == &format!("200 {url}")),
            "{font} did not load: {answers:#?}"
        );
    }
    assert_eq!(
        b.eval("String(Array.from(document.fonts).filter((f) => f.status === 'error').length)")
            .await,
        "0"
    );
    b.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn the_api_wants_the_token_even_from_the_page_itself() {
    let env = Env::start(true);
    let b = Browse::start().await;
    b.visit(&env).await;
    let token = env.server.token().to_string();
    let status = |js: String| {
        let b = &b;
        async move { b.eval(&js).await }
    };
    // A same-origin request with no token is refused, for a read and for a write.
    assert_eq!(
        status("(async () => String((await fetch('/api/snapshot')).status))()".into()).await,
        "401"
    );
    assert_eq!(
        status("(async () => String((await fetch('/api/stop', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: '{}' })).status))()".into()).await,
        "401"
    );
    assert!(
        !env.paths.stop_file.exists(),
        "a refused STOP must not stop anything"
    );
    // A wrong one too.
    assert_eq!(
        status("(async () => String((await fetch('/api/snapshot', { headers: { 'X-Panel-Token': 'nope' } })).status))()".into()).await,
        "401"
    );
    // The right one, as the page sends it, is served; and a GET with the token in the address is not a way in.
    assert_eq!(
        status(format!("(async () => String((await fetch('/api/snapshot', {{ headers: {{ 'X-Panel-Token': '{token}' }} }})).status))()")).await,
        "200"
    );
    assert_eq!(
        status(format!(
            "(async () => String((await fetch('/api/snapshot?t={token}')).status))()"
        ))
        .await,
        "401"
    );
    b.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn another_host_name_gets_no_page_and_the_page_cannot_call_another_origin() {
    let env = Env::start(true);
    // `evil.example` is made to point at this PC, the way a DNS-rebinding page would reach the panel.
    let b = Browse::start_with_args(vec![
        "--host-resolver-rules=MAP evil.example 127.0.0.1".to_string(),
    ])
    .await;
    let port = env.server.addr().port();
    // The Host guard refuses before it looks at anything else, token included.
    let response = b
        .page
        .goto(
            &format!("http://evil.example:{port}/?t={}{CALM}", env.server.token()),
            None,
        )
        .await
        .unwrap()
        .expect("a response");
    assert_eq!(response.status(), 403);
    assert_eq!(b.count("#stops").await, 0, "no page came with it");
    assert!(!b.text().await.contains("Co 2"));

    // And the page the server did give cannot be made to call another origin (`connect-src 'self'`):
    // the request is not even sent.
    b.visit(&env).await;
    let url = format!("http://evil.example:{port}/api/stop");
    let outcome = b
        .eval(&format!(
            "(async () => {{ try {{ await fetch('{url}', {{ method: 'POST', mode: 'no-cors', body: '{{}}' }}); return 'sent'; }} catch (e) {{ return 'blocked'; }} }})()"
        ))
        .await;
    assert_eq!(outcome, "blocked");
    assert!(
        b.eval("JSON.stringify(window.__csp)")
            .await
            .contains("connect-src"),
        "the browser reports the policy that stopped it"
    );
    assert!(
        b.requests
            .lock()
            .unwrap()
            .iter()
            .all(|r| !r.contains("/api/stop")),
        "nothing was sent"
    );
    assert!(!env.paths.stop_file.exists(), "nothing was stopped");
    // That one violation and its console line were the point of the test: the rest of the page is clean.
    b.eval("(() => { window.__csp = []; return 'ok'; })()")
        .await;
    b.problems.lock().unwrap().clear();
    b.go("sistema").await;
    assert_eq!(b.attr(".sistema", "data-stopped").await, "false");
    b.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn the_number_keys_the_tabs_the_arrows_and_back_move_around_the_page() {
    let env = Env::start(true);
    let b = Browse::start().await;
    b.visit(&env).await;
    for (stop, key) in [
        ("procesos", "2"),
        ("cola", "3"),
        ("aprender", "4"),
        ("sistema", "5"),
        ("hoy", "1"),
    ] {
        b.key(key).await;
        b.at(stop).await;
        assert_eq!(
            b.eval("location.hash").await,
            format!("#{stop}"),
            "the address follows the stop"
        );
    }
    // 0 is the prologue: it is not a tab, so none of the five is lit.
    b.key("0").await;
    b.wait("document.body.dataset.stop === 'prologo'").await;
    assert_eq!(b.count("#stops [role=tab][aria-selected=true]").await, 0);
    // A click on a tab goes there; the arrows walk the tabs and select as they go.
    b.click("#tab-cola").await;
    b.at("cola").await;
    b.key("ArrowRight").await;
    b.at("aprender").await;
    b.key("ArrowLeft").await;
    b.key("ArrowLeft").await;
    b.at("procesos").await;
    b.key("End").await;
    b.at("sistema").await;
    b.key("Home").await;
    b.at("hoy").await;
    // The camera and the tabs agree about where the page is.
    assert_eq!(
        b.eval("String(window.__app.view.room)").await,
        "hoy",
        "the shared view says the same"
    );
    // Back walks the stops the way the history recorded them.
    b.go("procesos").await;
    b.go("cola").await;
    b.eval("(() => { history.back(); return 'ok'; })()").await;
    b.at("procesos").await;
    b.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn the_theme_and_the_language_follow_the_key_and_the_address_and_are_remembered() {
    let env = Env::start(true);
    let b = Browse::start().await;
    // The page the Rust server serves speaks Spanish and is Tinta (night glass) with nothing saved.
    b.open(&env.url_with("&debug=1&motion=calm&clock=9")).await;
    b.ready().await;
    assert_eq!(b.eval("document.documentElement.lang").await, "es");
    assert_eq!(b.attr("html", "data-theme").await, "null");
    assert_eq!(b.text_of("#tab-hoy .stop-name").await, "Hoy");
    // `t` is a switch between Tinta and Papel; the choice is kept on this machine.
    b.key("t").await;
    b.wait("document.documentElement.getAttribute('data-theme') === 'light'")
        .await;
    b.wait("document.querySelector('#toast .toast-text') && document.querySelector('#toast .toast-text').textContent.length > 0")
        .await;
    b.open(&env.url_with("&debug=1&motion=calm&clock=9")).await;
    b.ready().await;
    assert_eq!(
        b.attr("html", "data-theme").await,
        "light",
        "Papel is kept for the next visit"
    );
    b.key("t").await;
    b.wait("document.documentElement.getAttribute('data-theme') === null")
        .await;
    // `l` changes the words of the whole page at once and is kept as well.
    b.key("l").await;
    b.wait("document.documentElement.lang === 'en'").await;
    assert!(b.text_of("#hoy-title").await.starts_with("You have "));
    assert_ne!(b.text_of("#tab-hoy .stop-name").await, "Hoy");
    b.open(&env.url_with("&debug=1&motion=calm&clock=9")).await;
    b.ready().await;
    assert_eq!(
        b.eval("document.documentElement.lang").await,
        "en",
        "the language is kept"
    );
    // An address says it too (and wins), for a screenshot or a test.
    b.open(&env.url_with("&debug=1&motion=calm&clock=9&lang=es&theme=light"))
        .await;
    b.ready().await;
    assert_eq!(b.eval("document.documentElement.lang").await, "es");
    assert_eq!(b.attr("html", "data-theme").await, "light");
    b.shot("frame-papel").await;
    b.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn the_shortcut_sheet_opens_with_a_question_mark_and_owns_the_keyboard_until_escape() {
    let env = Env::start(true);
    let b = Browse::start().await;
    b.visit(&env).await;
    b.key("?").await;
    b.wait("document.querySelector('.help[role=dialog]') !== null")
        .await;
    let sheet = b.eval("document.querySelector('.help').innerText").await;
    assert!(
        sheet.to_lowercase().contains("esc") && sheet.contains("1 – 5"),
        "the sheet lists the keys of the page: {sheet}"
    );
    // The page behind it is out of reach, so a number key does nothing while it is open.
    assert_eq!(
        b.eval("String(document.getElementById('doc').inert)").await,
        "true"
    );
    b.key("3").await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(b.eval("document.body.dataset.stop").await, "hoy");
    b.key("Escape").await;
    b.wait("document.querySelector('.help[role=dialog]') === null")
        .await;
    assert_eq!(
        b.eval("String(document.getElementById('doc').inert)").await,
        "false"
    );
    b.go("cola").await;
    b.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn a_phone_gets_the_flat_page_with_six_tabs_and_the_same_data() {
    let env = Env::start(true);
    let b = Browse::start().await;
    b.resize(390, 844).await;
    b.visit(&env).await;
    assert_eq!(b.eval("window.__app.mode()").await, "flat");
    assert_eq!(
        b.count("#stops [role=tab]").await,
        6,
        "the prologue is a tab on the flat page"
    );
    assert_eq!(b.eval("document.body.dataset.stop").await, "hoy");
    assert_eq!(
        b.eval("String(document.documentElement.scrollWidth <= window.innerWidth)")
            .await,
        "true",
        "no sideways scrolling on a phone"
    );
    assert!(b.text_of("#hoy-title").await.contains("Te tocan"));
    // The tabs work, and the moment on the sheet is the real one.
    b.click("#tab-cola").await;
    b.at("cola").await;
    assert_eq!(b.text_of(".prop .role-title").await, "Prepared role");
    b.click("#tab-sistema").await;
    b.at("sistema").await;
    // Widening the window brings the stage back without a reload.
    b.resize(1280, 720).await;
    b.wait("window.__app.mode() === 'stage'").await;
    b.shot("frame-flat-then-stage").await;
    b.stop().await;
}

// ---------------------------------------------------------------------------------------------
// HOY: what waits for Gonza
// ---------------------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn hoy_says_what_waits_and_hostile_text_stays_text() {
    let env = Env::start(true);
    let b = Browse::start().await;
    b.visit(&env).await;
    b.wait("document.querySelector('[data-moment=\"hoy\"] .next') !== null")
        .await;
    // Six things are his: the four tasks, the proposal to approve and the form that needs him.
    // The headline speaks with one voice: one run of text, the spelled-out number inside it.
    assert_eq!(b.text_of("#hoy-title").await, "Te tocan seis.");
    assert_eq!(b.text_of(".ask").await, "1 espera tu aprobación");
    for (fig, n) in [
        ("read", "3"),
        ("above", "3"),
        ("complete", "2"),
        ("awaiting", "1"),
    ] {
        assert_eq!(
            b.text_of(&format!("[data-fig=\"{fig}\"] .fig-n")).await,
            n,
            "figure {fig}"
        );
    }
    // The most urgent one leads (the overdue task, with how late it is); the five others follow in the list.
    assert_eq!(b.attr(".next", "data-task").await, "1");
    assert_eq!(b.attr(".next", "data-tone").await, "late");
    assert!(b.text_of(".next .meta").await.contains("Venció hace 2 h"));
    assert_eq!(b.text_of(".after").await, "después de esta: 5");
    assert_eq!(
        b.eval("Array.from(document.querySelectorAll('.rows > li')).map((li) => li.dataset.row).join(',')")
            .await,
        "task:2,card:test:2,card:test:1,task:3,task:4"
    );
    // What the data says is drawn as the letters it is made of.
    let room = b
        .eval("document.querySelector('[data-moment=\"hoy\"]').innerText")
        .await;
    assert!(room.contains(HOSTILE), "shown as text: {room}");
    // (the small mono lines are drawn in capitals, so the words are compared without case)
    let room_lower = room.to_lowercase();
    assert!(
        room_lower.contains("entregar prueba técnica de acme")
            && room_lower.contains("vence en 30 h"),
        "{room}"
    );
    assert_eq!(b.eval("String(typeof window.pwned)").await, "undefined");
    assert_eq!(b.count("img[src=\"x\"]").await, 0);
    assert_eq!(
        b.count("[data-moment=\"hoy\"] img").await,
        0,
        "nothing in HOY is a picture"
    );
    // The link of a task added by hand is offered only for an http(s) address; the other is dropped.
    assert_eq!(
        b.attr("[data-task=\"3\"]", "data-actions").await,
        "done,link"
    );
    assert_eq!(b.attr("[data-task=\"4\"]", "data-actions").await, "done");
    assert_eq!(b.count("a[href^=\"javascript\"]").await, 0);
    // The stops bar counts what waits in the other rooms, and says it in words too.
    assert_eq!(b.text_of("#tab-procesos .stop-count").await, "1");
    assert_eq!(b.text_of("#tab-cola .stop-count").await, "1");
    assert!(
        b.attr("#tab-cola", "aria-label")
            .await
            .contains("1 por aprobar")
    );
    assert!(b.text_of(".foot-line").await.contains("STOP apagado"));
    b.shot("hoy-seeded").await;
    b.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn a_task_is_closed_only_by_a_second_press_and_the_row_dims_while_the_server_answers() {
    let env = Env::start(true);
    let b = Browse::start().await;
    b.visit(&env).await;
    let button = "[data-task=\"4\"] [data-action=\"done\"]";
    b.wait(&format!("document.querySelector('{button}') !== null"))
        .await;
    let open = |env: &Env| {
        env.tracker()
            .open_tasks()
            .unwrap()
            .iter()
            .any(|task| task.text == "Enlace raro")
    };
    b.watch_pending().await;
    b.click(button).await;
    assert_eq!(
        b.attr("[data-task=\"4\"]", "data-armed").await,
        "done",
        "the first press only arms the row"
    );
    assert!(
        b.text_of(button).await.contains("Seguro"),
        "the button says so"
    );
    assert!(open(&env), "one press on 'Hecha' changes nothing");
    tokio::time::sleep(ARM_PAUSE).await;
    b.click(button).await;
    b.wait("document.querySelector('[data-task=\"4\"]') === null")
        .await;
    assert!(!open(&env), "the second press closed the task");
    assert!(b.saw_pending().await, "the row dimmed at once");
    // What waits for him went from six to five, in the headline and in the list.
    b.wait("document.querySelector('#hoy-title').textContent === 'Te tocan cinco.'")
        .await;
    assert_eq!(b.count(".rows > li").await, 4);
    b.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn a_double_click_only_arms_and_an_armed_row_gives_itself_back() {
    let env = Env::start(true);
    let b = Browse::start().await;
    b.visit(&env).await;
    let button = "[data-card=\"test:2\"] [data-action=\"approve\"]";
    b.wait(&format!("document.querySelector('{button}') !== null"))
        .await;
    // Two clicks in the same instant, the way an accidental double click arrives.
    b.eval(&format!(
        "(() => {{ const b = document.querySelector('{button}'); b.click(); b.click(); return 'ok'; }})()"
    ))
    .await;
    assert_eq!(
        b.attr("[data-card=\"test:2\"]", "data-armed").await,
        "approve"
    );
    assert!(
        b.text_of(&format!("{button} .act-label"))
            .await
            .contains("Seguro")
    );
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(
        !env.app("test:2").approved,
        "a double click must not approve"
    );
    // Left alone it disarms itself and goes back to its own word.
    b.wait("document.querySelector('[data-card=\"test:2\"]').getAttribute('data-armed') === null")
        .await;
    assert_eq!(b.text_of(&format!("{button} .act-label")).await, "Aprobar");
    b.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn the_most_urgent_task_leads_and_closing_it_brings_the_next_one() {
    let env = Env::start(true);
    let b = Browse::start().await;
    b.visit(&env).await;
    b.wait("document.querySelector('.next[data-task=\"1\"]') !== null")
        .await;
    b.confirm_click(".next [data-action=\"done\"]").await;
    b.wait("document.querySelector('.next') !== null && document.querySelector('.next').dataset.task === '2'")
        .await;
    assert!(
        env.tracker()
            .open_tasks()
            .unwrap()
            .iter()
            .all(|task| !task.text.starts_with("Responder")),
        "the overdue task is closed"
    );
    assert_eq!(b.text_of("#hoy-title").await, "Te tocan cinco.");
    assert!(b.text_of(".next .meta").await.contains("Vence en 30 h"));
    b.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn approving_from_hoy_lands_in_sqlite_and_the_row_leaves() {
    let env = Env::start(true);
    let b = Browse::start().await;
    b.visit(&env).await;
    let row = "[data-card=\"test:2\"]";
    let button = format!("{row} [data-action=\"approve\"]");
    b.wait(&format!("document.querySelector('{button}') !== null"))
        .await;
    b.watch_pending().await;
    // The first press only arms the button; a second, deliberate one does it.
    b.click(&button).await;
    assert!(!env.app("test:2").approved);
    tokio::time::sleep(ARM_PAUSE).await;
    b.click(&button).await;
    b.wait(&format!("document.querySelector('{row}') === null"))
        .await;
    assert!(env.app("test:2").approved, "it is in SQLite");
    assert!(b.saw_pending().await, "the row dimmed at once");
    // Nothing waits for the approval any more: the headline, the figure and the stop all say so.
    b.wait("document.querySelector('[data-fig=\"awaiting\"] .fig-n').textContent === '0'")
        .await;
    assert_eq!(b.text_of("#hoy-title").await, "Te tocan cinco.");
    b.wait("document.querySelector('#tab-cola .stop-count').hidden")
        .await;
    b.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn a_card_is_put_aside_from_the_keyboard_and_comes_back_from_the_fold() {
    let env = Env::start(true);
    let b = Browse::start().await;
    b.visit(&env).await;
    b.wait("document.querySelector('[data-card=\"test:2\"]') !== null")
        .await;
    b.cursor_to("card:test:2").await;
    b.key("s").await;
    // "Until when" is a little menu beside the row; its keys are its own.
    b.wait("document.querySelector('.menu[role=\"menu\"]') !== null")
        .await;
    assert_eq!(b.count(".menu .menu-item").await, 4);
    b.watch_pending().await;
    b.key("2").await;
    b.wait("document.querySelector('[data-card=\"test:2\"]') === null")
        .await;
    let until = env
        .app("test:2")
        .snoozed_until
        .expect("it is put aside in SQLite");
    assert!(
        until > *NOW + TimeDelta::days(2) && until <= *NOW + TimeDelta::days(4),
        "{until}"
    );
    assert!(b.saw_pending().await, "the row dimmed at once");
    assert!(
        !env.app("test:2").approved,
        "putting it aside approves nothing"
    );
    // It waits in the fold under the list, and one press brings it back.
    b.wait("!document.querySelector('details.snoozed').hidden")
        .await;
    assert_eq!(b.text_of(".snoozed-title").await.trim(), "Pospuestas · 1");
    b.click(".snoozed-title").await;
    b.click("[data-unsnooze=\"test:2\"]").await;
    b.wait("document.querySelector('[data-card=\"test:2\"]') !== null")
        .await;
    assert!(env.app("test:2").snoozed_until.is_none());
    b.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn a_form_that_needs_him_is_confirmed_or_returned_to_the_queue_from_the_keyboard() {
    let env = Env::start(true);
    let b = Browse::start().await;
    b.visit(&env).await;
    b.wait("document.querySelector('[data-card=\"test:1\"]') !== null")
        .await;
    // `u` takes it back to the queue: it is not permanent (the engine can do it again), so one press does it.
    b.cursor_to("card:test:1").await;
    b.key("u").await;
    b.wait("document.querySelector('[data-card=\"test:1\"]') === null")
        .await;
    assert_eq!(
        env.app("test:1").state,
        State::Scored,
        "without a CV it goes back to the scored ones"
    );

    // Another form that needs him: `c` says it was sent, and that cannot be undone, so it takes two presses.
    let t = env.tracker();
    let job = make_job_with(|j| {
        j.external_id = "5".into();
        j.company = "Co 5".into();
        j.title = "Needs a hand".into();
    });
    t.add_job(&job, 75.0, &BTreeMap::new(), *NOW).unwrap();
    t.create_application(&job.key(), *NOW).unwrap();
    t.transition(&job.key(), State::NeedsGonza, Some("falta email"), *NOW)
        .unwrap();
    drop(t);
    b.open(&env.url()).await;
    b.ready().await;
    b.wait("document.querySelector('[data-card=\"test:5\"]') !== null")
        .await;
    b.cursor_to("card:test:5").await;
    b.key("c").await;
    assert_eq!(
        b.attr("[data-card=\"test:5\"]", "data-armed").await,
        "confirm"
    );
    assert_eq!(
        env.app("test:5").state,
        State::NeedsGonza,
        "one press on 'ya la envié' changes nothing"
    );
    tokio::time::sleep(ARM_PAUSE).await;
    b.key("c").await;
    b.wait("document.querySelector('[data-card=\"test:5\"]') === null")
        .await;
    assert_eq!(env.app("test:5").state, State::Submitted);
    b.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn with_nothing_to_do_hoy_is_calm_and_the_other_moments_say_so_in_words() {
    let env = Env::start(false);
    let b = Browse::start().await;
    b.visit(&env).await;
    b.wait("document.querySelector('#hoy-title') !== null")
        .await;
    assert_eq!(b.count(".rows > li").await, 0);
    assert_eq!(b.count(".next").await, 0);
    assert_no_strays("HOY", &b.text().await);
    b.shot("hoy-empty").await;
    for stop in ["procesos", "cola", "aprender", "sistema", "hoy"] {
        b.go(stop).await;
        // Give the moment its own frame to patch before reading it.
        tokio::time::sleep(Duration::from_millis(200)).await;
        let text = b
            .eval(&format!(
                "document.querySelector('#app[data-moment=\"{stop}\"]').innerText"
            ))
            .await;
        assert!(text.trim().len() > 40, "{stop} says something: {text}");
        assert_no_strays(stop, &text);
    }
    // The stops bar has nothing to count in the rooms that wait for him: no number is drawn there.
    // (The review bank is the one thing that is always due, so APRENDER does show its own.)
    assert_eq!(
        b.eval("String(Array.from(document.querySelectorAll('#stops .stop-count')).filter((c) => !c.hidden && c.textContent.trim() !== '').map((c) => c.closest('[role=tab]').dataset.stop).join(','))")
            .await,
        "aprender",
        "only the review bank is counted"
    );
    b.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn no_moment_prints_a_stray_null_or_object_with_real_data() {
    let env = Env::start_full();
    let b = Browse::start().await;
    b.visit(&env).await;
    for stop in ["hoy", "procesos", "cola", "aprender", "sistema"] {
        b.go(stop).await;
        tokio::time::sleep(Duration::from_millis(200)).await;
        let text = b
            .eval(&format!(
                "document.querySelector('#app[data-moment=\"{stop}\"]').innerText"
            ))
            .await;
        // The word "null" can be a company's own; this fixture has none, so a hit is a leak.
        assert_no_strays(stop, &text);
        // A date that could not be read must not reach the screen either.
        assert!(!text.contains("Invalid Date"), "{stop}: {text}");
    }
    // The drawer too, for the application with the hostile name.
    b.go("procesos").await;
    b.click("button.cell[data-card=\"test:1\"]").await;
    b.wait("document.getElementById('detail').classList.contains('open')")
        .await;
    assert_no_strays(
        "the drawer",
        &b.eval("document.getElementById('detail').innerText").await,
    );
    b.stop().await;
}

// ---------------------------------------------------------------------------------------------
// PROCESOS: where each application is
// ---------------------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn procesos_says_where_each_application_is_and_whose_turn_it_is() {
    let env = Env::start(true);
    let b = Browse::start().await;
    b.visit(&env).await;
    b.go("procesos").await;
    b.wait("document.querySelectorAll('button.cell').length === 3")
        .await;
    // Two lanes: what waits to be sent (both are his turn) and what was sent (the company's turn).
    assert_eq!(b.text_of("[data-lane=\"presend\"] .lane-n").await, "2");
    assert_eq!(
        b.text_of("[data-lane=\"presend\"] .lane-you").await,
        "2 te tocan"
    );
    assert_eq!(b.text_of("[data-lane=\"submitted\"] .lane-n").await, "1");
    assert!(b.text_of(".big .mega").await.trim() == "1");
    for (key, turn, state) in [
        ("test:1", "you", "needs_gonza"),
        ("test:2", "you", "prepared"),
        ("test:3", "wait", "submitted"),
    ] {
        let cell = format!("button.cell[data-card=\"{key}\"]");
        assert_eq!(b.attr(&cell, "data-turn").await, turn, "{key}");
        assert_eq!(b.attr(&cell, "data-state").await, state, "{key}");
    }
    // Five layers per application, and the application with the hostile title keeps it as text.
    assert_eq!(b.count("button.cell[data-card=\"test:1\"] .stk i").await, 5);
    assert_eq!(b.eval("String(typeof window.pwned)").await, "undefined");
    assert_eq!(b.count("[data-moment=\"procesos\"] img").await, 0);
    assert!(
        b.attr("button.cell[data-card=\"test:1\"]", "aria-label")
            .await
            .contains(HOSTILE)
    );

    // The filter: only what is his turn.
    b.click("[data-filter=\"turn\"]").await;
    assert_eq!(
        b.attr("[data-filter=\"turn\"]", "aria-pressed").await,
        "true"
    );
    b.wait("Array.from(document.querySelectorAll('button.cell')).filter((c) => c.getClientRects().length > 0).length === 2")
        .await;
    // The same from the keyboard (`f` and then the number of the filter), back to everything.
    b.key("f").await;
    b.key("1").await;
    b.wait(
        "document.querySelector('[data-filter=\"all\"]').getAttribute('aria-pressed') === 'true'",
    )
    .await;
    b.wait("Array.from(document.querySelectorAll('button.cell')).filter((c) => c.getClientRects().length > 0).length === 3")
        .await;
    // The search box takes `/`; a word narrows the list and Esc gives it back.
    b.key("/").await;
    b.wait("document.activeElement && document.activeElement.classList.contains('find-input')")
        .await;
    b.page.keyboard().type_text("Co 3", None).await.unwrap();
    b.wait("Array.from(document.querySelectorAll('button.cell')).filter((c) => c.getClientRects().length > 0).length === 1")
        .await;
    b.key("Escape").await;
    b.shot("procesos-seeded").await;
    b.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn the_drawer_opens_on_an_application_steps_through_them_and_escape_closes_it() {
    let env = Env::start(true);
    let b = Browse::start().await;
    b.visit(&env).await;
    b.go("procesos").await;
    b.wait("document.querySelector('button.cell[data-card=\"test:2\"]') !== null")
        .await;
    b.click("button.cell[data-card=\"test:2\"]").await;
    b.wait("document.getElementById('detail').classList.contains('open') && document.getElementById('detail-title') !== null")
        .await;
    assert_eq!(b.text_of("#detail-title").await, "Co 2");
    assert_eq!(b.attr("#detail .detail-body", "data-job").await, "test:2");
    assert!(b.text_of("#detail .pd-sub").await.contains("Remote"));
    assert_eq!(
        b.eval("location.hash").await,
        "#procesos/test%3A2",
        "the address says which application is open"
    );
    assert_eq!(b.text_of("#detail .seq .num").await, "2 de 3");
    // Five layers: the application waits at the gate, and says it is his step.
    assert_eq!(b.count("#detail .plates .plate").await, 5);
    assert_eq!(
        b.attr("#detail .plate[data-you=\"true\"]", "data-layer")
            .await,
        "compuerta"
    );
    b.shot("drawer-test2").await;
    // The next one: its own title, its own address.
    b.click("#seq-next").await;
    b.wait("document.querySelector('#detail .detail-body').dataset.job === 'test:3'")
        .await;
    assert_eq!(b.text_of("#detail .seq .num").await, "3 de 3");
    assert_eq!(b.eval("location.hash").await, "#procesos/test%3A3");
    // Esc closes it and the address goes back to the stop.
    b.key("Escape").await;
    b.wait("!document.getElementById('detail').classList.contains('open')")
        .await;
    assert_eq!(b.eval("location.hash").await, "#procesos");
    // An address with an application opens its drawer in its stop (a link, or a reload).
    b.open(&format!("{}#cola/test%3A2", env.url())).await;
    b.wait("document.getElementById('detail').classList.contains('open') && document.getElementById('detail-title') !== null")
        .await;
    assert_eq!(b.text_of("#detail-title").await, "Co 2 — Prepared role");
    assert_eq!(b.eval("document.body.dataset.stop").await, "cola");
    b.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn what_an_email_says_stays_text_in_the_drawer() {
    let env = Env::start(true);
    let b = Browse::start().await;
    b.visit(&env).await;
    b.go("procesos").await;
    b.click("button.cell[data-card=\"test:3\"]").await;
    b.wait("document.getElementById('detail').classList.contains('open') && document.querySelector('#detail .pd-mails') !== null")
        .await;
    b.wait(
        "document.querySelector('#detail .pd-mails').innerText.includes('Quiere hablar de sueldo')",
    )
    .await;
    let mails = b
        .eval("document.querySelector('#detail .pd-mails').innerText")
        .await;
    // The recruiter's message is there with its hostile subject as letters; the acknowledgement is not a message to answer.
    assert!(mails.contains(HOSTILE), "{mails}");
    assert!(!mails.contains("Recibimos tu postulación"), "{mails}");
    assert_eq!(b.eval("String(typeof window.pwned)").await, "undefined");
    assert_eq!(b.count("#detail img, #detail svg[onload]").await, 0);
    b.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn withdrawing_from_the_drawer_asks_which_reason_and_lands_in_sqlite() {
    let env = Env::start(true);
    let b = Browse::start().await;
    b.visit(&env).await;
    b.go("procesos").await;
    b.click("button.cell[data-card=\"test:3\"]").await;
    b.wait("document.querySelector('#detail [data-act=\"withdraw\"]') !== null")
        .await;
    b.click("#detail [data-act=\"withdraw\"]").await;
    // The question is the second step: nothing changes until a reason is chosen.
    b.wait("document.querySelector('.menu[role=\"menu\"]') !== null")
        .await;
    assert_eq!(env.app("test:3").state, State::Submitted);
    assert_eq!(b.count(".menu .menu-item").await, 3);
    b.key("2").await;
    b.wait("document.querySelector('.menu') === null").await;
    b.wait("document.querySelector('button.cell[data-card=\"test:3\"]') === null || document.querySelector('button.cell[data-card=\"test:3\"]').dataset.state === 'withdrawn'")
        .await;
    let app = env.app("test:3");
    assert_eq!(app.state, State::Withdrawn);
    assert!(
        app.note.as_deref().is_some_and(|n| !n.is_empty()),
        "the reason is written down on it: {:?}",
        app.note
    );
    b.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn an_answer_is_registered_from_procesos_with_the_keyboard() {
    let env = Env::start(true);
    let b = Browse::start().await;
    b.visit(&env).await;
    b.go("procesos").await;
    b.wait("document.querySelector('button.cell[data-card=\"test:3\"]') !== null")
        .await;
    b.cursor_to("card:test:3").await;
    b.key("r").await;
    b.wait("document.querySelector('.menu[role=\"menu\"]') !== null")
        .await;
    let first = b.text_of(".menu .menu-item .menu-label").await;
    assert!(!first.is_empty());
    b.key("1").await;
    b.wait("document.querySelector('.menu') === null").await;
    b.wait(
        "document.querySelector('button.cell[data-card=\"test:3\"]').dataset.state !== 'submitted'",
    )
    .await;
    let state = env.app("test:3").state;
    assert_eq!(
        state,
        State::Replied,
        "the first answer of the menu ({first}) is what SQLite has"
    );
    b.stop().await;
}

// ---------------------------------------------------------------------------------------------
// COLA: the gate, the path, the lote, the trust
// ---------------------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn the_gate_shows_what_decides_a_proposal_and_approving_lands_in_sqlite() {
    let env = Env::start_full();
    let b = Browse::start().await;
    b.visit(&env).await;
    b.go("cola").await;
    b.wait("document.querySelector('.cola[data-state=\"ready\"] .prop[data-card=\"test:2\"]') !== null")
        .await;
    // What he needs to decide is on the proposal: who, the score, where it stands, the dry-run capture.
    assert_eq!(b.text_of(".prop .role-title").await, "Prepared role");
    assert!(b.text_of(".prop .facts").await.contains("Co 2"));
    assert_eq!(b.text_of(".prop .score").await, "80");
    assert_eq!(
        b.attr(".prop .path [aria-current=\"step\"]", "data-layer")
            .await,
        "compuerta"
    );
    assert_eq!(b.text_of("#cola-title .n").await, "1");
    b.wait("(() => { const i = document.querySelector('.plane img.shot'); return !!i && i.naturalWidth > 0; })()")
        .await;
    assert_eq!(b.text_of("#tab-cola .stop-count").await, "1");
    // The engine has not run yet, and the page says what an approval means then.
    assert!(b.text_of(".warn").await.contains("no envía nada"));
    // Nothing is approved yet and the trust meter is empty: 0 of the first 5.
    assert!(b.text_of(".trust-a").await.contains("0 de 5"));
    assert!(b.text_of(".trust-b").await.contains("1 de 10"));
    b.shot("cola-gate").await;

    let button = ".prop [data-action=\"approve\"]";
    b.watch_pending().await;
    // The first press only arms the button.
    b.click(button).await;
    assert_eq!(b.attr(button, "data-armed").await, "true");
    assert!(b.text_of(button).await.contains("Seguro"));
    assert!(!env.app("test:2").approved);
    tokio::time::sleep(ARM_PAUSE).await;
    b.click(button).await;
    b.wait("document.querySelector('.cola').dataset.state === 'empty'")
        .await;
    assert!(env.app("test:2").approved, "it is in SQLite");
    assert!(b.saw_pending().await, "the proposal dimmed at once");
    assert!(
        b.text().await.to_lowercase().contains("nada por aprobar"),
        "the gate says it is clear"
    );
    // The stop has nothing to count. The trust meter counts SENDS he approved, and nothing has gone
    // out: an approval is a promise for the next run, not yet a send.
    b.wait("document.querySelector('#tab-cola .stop-count').hidden")
        .await;
    assert!(b.text_of(".trust-a").await.contains("0 de 5"));
    b.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn trust_counts_the_sends_you_approved_and_the_cap_of_the_day() {
    let env = Env::start_full();
    let b = Browse::start().await;
    b.visit(&env).await;
    b.go("cola").await;
    b.wait("document.querySelector('.trust') !== null").await;
    // Nothing approved and sent yet: five empty boxes, and the one send of the day against the lower cap.
    assert_eq!(b.count(".trust-a .sq i").await, 5);
    assert_eq!(b.count(".trust-a .sq i[data-on=\"true\"]").await, 0);
    assert!(b.text_of(".trust-a").await.contains("0 de 5"));
    assert!(b.text_of(".trust-b").await.contains("1 de 10"));
    // test:3 was approved before it went out: that is the first of the five.
    env.tracker().approve("test:3").unwrap();
    b.open(&env.url()).await;
    b.ready().await;
    b.go("cola").await;
    b.wait("document.querySelector('.trust-a .sq i[data-on=\"true\"]') !== null")
        .await;
    assert_eq!(b.count(".trust-a .sq i[data-on=\"true\"]").await, 1);
    assert!(b.text_of(".trust-a").await.contains("1 de 5"));
    b.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn a_double_click_on_the_gate_does_not_approve() {
    let env = Env::start(true);
    let b = Browse::start().await;
    b.visit(&env).await;
    b.go("cola").await;
    b.wait("document.querySelector('.prop [data-action=\"approve\"]') !== null")
        .await;
    b.eval("(() => { const b = document.querySelector('.prop [data-action=\"approve\"]'); b.click(); b.click(); return 'ok'; })()")
        .await;
    assert!(
        b.text_of(".prop [data-action=\"approve\"]")
            .await
            .contains("Seguro")
    );
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(
        !env.app("test:2").approved,
        "a double click must not approve"
    );
    b.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn the_gate_steps_through_the_proposals_and_skipping_sends_nothing() {
    let env = Env::start(true);
    env.add_proposal("4", "Second prepared role", 70.0);
    let b = Browse::start().await;
    b.visit(&env).await;
    b.go("cola").await;
    b.wait("document.querySelector('.prop') !== null && document.querySelector('.step-text').textContent === '1 de 2'")
        .await;
    // The best score first; j and k walk the proposals and stop at both ends.
    assert_eq!(b.attr(".prop", "data-card").await, "test:2");
    b.key("j").await;
    b.wait("document.querySelector('.prop').dataset.card === 'test:4'")
        .await;
    assert_eq!(b.text_of(".step-text").await, "2 de 2");
    b.key("k").await;
    b.wait("document.querySelector('.prop').dataset.card === 'test:2'")
        .await;
    // Skipping asks until when and sends nothing: it only puts the proposal aside.
    b.key("s").await;
    b.wait("document.querySelector('.menu[role=\"menu\"]') !== null")
        .await;
    b.key("1").await;
    b.wait("document.querySelector('.prop').dataset.card === 'test:4'")
        .await;
    assert!(env.app("test:2").snoozed_until.is_some());
    assert!(
        !env.app("test:2").approved && !env.app("test:4").approved,
        "stepping and skipping never approve anything"
    );
    assert_eq!(b.text_of(".step-text").await, "1 de 1");
    // The way back is offered at once: "Deshacer" brings the skipped one back (the page keeps
    // showing the one it was on, which is now the second by score).
    b.wait("!document.querySelector('#toast .toast-undo').hidden")
        .await;
    b.click("#toast .toast-undo").await;
    b.wait("document.querySelector('.step-text').textContent === '2 de 2'")
        .await;
    assert!(env.app("test:2").snoozed_until.is_none());
    assert_eq!(b.attr(".prop", "data-card").await, "test:4");
    b.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn the_lote_approves_what_reaches_the_threshold_in_two_presses() {
    let env = Env::start(true);
    env.add_proposal("5", "High score one", 90.0);
    env.add_proposal("6", "High score two", 95.0);
    let b = Browse::start().await;
    b.visit(&env).await;
    b.go("cola").await;
    b.wait("document.querySelector('.step-text').textContent === '1 de 3'")
        .await;
    // Two of the three reach the lote's threshold (85); the one with 80 never goes in it.
    assert_eq!(
        b.attr(".batch-btn", "data-none").await,
        "null",
        "the lote is offered"
    );
    b.key("Shift+A").await;
    b.wait("!document.querySelector('.confirm').hidden").await;
    let said = b.text_of(".confirm").await;
    assert!(
        said.contains("Co 5") && said.contains("Co 6") && !said.contains("Co 2"),
        "the confirmation names exactly what it will approve: {said}"
    );
    assert!(
        !env.app("test:5").approved && !env.app("test:6").approved,
        "drawing the confirmation sends nothing"
    );
    // Esc drops it; the second time it is confirmed, and one request approves both.
    b.key("Escape").await;
    b.wait("document.querySelector('.confirm').hidden").await;
    b.key("Shift+A").await;
    b.wait("!document.querySelector('.confirm').hidden").await;
    // A press inside 350 ms of the first is not a second press (a double click confirms nothing).
    tokio::time::sleep(ARM_PAUSE).await;
    b.click("[data-batch=\"confirm\"]").await;
    b.wait("document.querySelector('.q-rows') !== null && document.querySelector('.prop').dataset.card === 'test:2' && document.querySelector('.step-text').textContent === '1 de 1'")
        .await;
    assert!(env.app("test:5").approved && env.app("test:6").approved);
    assert!(
        !env.app("test:2").approved,
        "below the threshold: untouched"
    );
    assert_eq!(b.attr(".prop", "data-card").await, "test:2");
    b.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn cola_draws_the_path_of_the_week_and_the_queue_below_the_proposal() {
    let env = Env::start_full();
    env.add_proposal("4", "Second prepared role", 70.0);
    let b = Browse::start().await;
    b.visit(&env).await;
    b.go("cola").await;
    b.wait("document.querySelector('.funnel .fig') !== null")
        .await;
    // From the notice to the send, with the counts of the last seven days (the seed stored three notices).
    assert_eq!(b.text_of("[data-fig=\"read\"] .fig-n").await, "4");
    assert_eq!(b.text_of("[data-fig=\"sent\"] .fig-n").await, "1");
    assert_eq!(b.count(".funnel .fig").await, 5);
    // The other proposal waits in the queue, and nothing in it is a picture.
    b.wait(
        "document.querySelector('.q-rows') !== null && !document.querySelector('.q-rows').hidden",
    )
    .await;
    assert_eq!(b.count(".q-rows .q-row").await, 1);
    assert_eq!(b.eval("String(typeof window.pwned)").await, "undefined");
    b.stop().await;
}

// ---------------------------------------------------------------------------------------------
// APRENDER and SISTEMA
// ---------------------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn the_drill_grades_a_card_and_sm2_schedules_it() {
    let env = Env::start(false);
    let b = Browse::start().await;
    b.visit(&env).await;
    b.go("aprender").await;
    b.wait("document.querySelector('[data-ap=\"start\"]') !== null")
        .await;
    b.click("[data-ap=\"start\"]").await;
    b.wait("(() => { const q = document.getElementById('ap-question'); return !!q && q.textContent.length > 0; })()")
        .await;
    assert_eq!(
        b.eval("String(document.getElementById('ap-answer') === null)")
            .await,
        "true",
        "the answer waits for the reveal"
    );
    b.click("[data-ap=\"reveal\"]").await;
    b.wait("document.getElementById('ap-answer') !== null")
        .await;
    b.click("[data-grade=\"5\"]").await;
    b.wait("document.querySelector('#toast .toast-text').textContent.includes('nota 5')")
        .await;
    let store = PrepStore::open(&env.paths.data_dir.join("prep.db")).unwrap();
    let questions = store.all_questions().unwrap();
    assert_eq!(store.stats(&questions, *NOW).unwrap().seen, 1);
    b.shot("aprender-graded").await;
    b.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn the_drill_works_from_the_keyboard_and_its_digits_belong_to_it() {
    let env = Env::start(false);
    let b = Browse::start().await;
    b.visit(&env).await;
    b.go("aprender").await;
    b.wait("document.querySelector('[data-ap=\"start\"]') !== null")
        .await;
    b.click("[data-ap=\"start\"]").await;
    b.wait("document.getElementById('ap-question') !== null")
        .await;
    // Enter shows the answer, and from then on the digits are grades, not stops.
    b.key("Enter").await;
    b.wait("document.getElementById('ap-answer') !== null")
        .await;
    b.key("4").await;
    b.wait("document.querySelector('#toast .toast-text').textContent.includes('nota 4')")
        .await;
    assert_eq!(
        b.eval("document.body.dataset.stop").await,
        "aprender",
        "a grade is not a trip"
    );
    let store = PrepStore::open(&env.paths.data_dir.join("prep.db")).unwrap();
    let questions = store.all_questions().unwrap();
    assert_eq!(store.stats(&questions, *NOW).unwrap().seen, 1);
    b.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn stop_and_resume_work_from_sistema_and_the_whole_page_knows() {
    let env = Env::start(false);
    let b = Browse::start().await;
    b.visit(&env).await;
    b.go("sistema").await;
    let button = "[data-row=\"stop\"] [data-action=\"engine\"]";
    b.wait(&format!("document.querySelector('{button}') !== null"))
        .await;
    assert_eq!(b.attr(".sistema", "data-stopped").await, "false");
    assert_eq!(b.text_of("[data-row=\"stop\"] .stop-text").await, "apagado");
    // Stopping the engine is one press (it must never be slower than the thing it stops); the
    // STOP file is on disk when the page says so.
    b.watch_pending().await;
    b.click(button).await;
    b.wait("document.querySelector('.sistema').dataset.stopped === 'true'")
        .await;
    assert!(env.paths.stop_file.exists());
    assert!(b.saw_pending().await, "the row dimmed at once");
    assert_ne!(b.text_of("[data-row=\"stop\"] .stop-text").await, "apagado");
    // The other moments say so too: HOY's footer, and COLA's engine switch.
    b.go("hoy").await;
    b.wait("document.querySelector('.foot-line').textContent.includes('STOP activo')")
        .await;
    b.go("cola").await;
    b.wait("document.querySelector('.stop-state').dataset.on === 'true'")
        .await;
    // And the way back, from where the switch is: resuming asks twice.
    b.go("sistema").await;
    b.click(button).await;
    assert!(
        env.paths.stop_file.exists(),
        "one press on RESUME only arms it"
    );
    tokio::time::sleep(ARM_PAUSE).await;
    b.click(button).await;
    b.wait("document.querySelector('.sistema').dataset.stopped === 'false'")
        .await;
    assert!(!env.paths.stop_file.exists());
    b.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn the_machine_room_reads_its_figures_from_real_data_and_says_what_is_missing() {
    let env = Env::start_full();
    let b = Browse::start().await;
    b.visit(&env).await;
    b.go("sistema").await;
    b.wait("document.querySelector('[data-fig=\"sends\"] .fig-n') !== null && document.querySelector('[data-fig=\"sends\"] .fig-n').textContent !== ''")
        .await;
    assert_eq!(b.attr(".sistema", "data-engine").await, "never");
    // The cap of the day: one send in 24 hours (test:3) against the cap of the review phase.
    assert_eq!(b.text_of("[data-fig=\"sends\"] .fig-n").await, "1 de 10");
    // The read-only limits come from the profile's own files.
    assert_eq!(b.text_of("[data-cell=\"score\"] .cell-v").await, "60");
    assert_eq!(b.text_of("[data-cell=\"batch\"] .cell-v").await, "85");
    assert!(
        b.text_of("[data-cell=\"sends\"] .cell-s")
            .await
            .contains("0 de 5")
    );
    // This profile has no claude.toml and no check was ever made: the account says so and what to do.
    assert!(
        b.text_of("[data-row=\"spend\"] .spend-text")
            .await
            .contains("sin tope")
    );
    assert!(
        b.text_of("[data-row=\"account\"] .row-s")
            .await
            .contains("jobbot claude-check")
    );
    // The sources: one board with its three applications, one sent and no answer.
    assert_eq!(
        b.text_of("tr[data-src=\"src:test\"] [data-label=\"postulaciones\"]")
            .await,
        "3"
    );
    assert_eq!(
        b.text_of("tr[data-src=\"src:test\"] [data-label=\"enviadas\"]")
            .await,
        "1"
    );
    // The night's errors name the application that needs him, as letters, and open its drawer.
    assert_eq!(b.count(".errors-list .err").await, 1);
    assert!(b.text_of(".err .err-co").await.contains(HOSTILE));
    assert_eq!(b.eval("String(typeof window.pwned)").await, "undefined");
    b.click(".err [data-open]").await;
    b.wait("document.getElementById('detail').classList.contains('open') && document.getElementById('detail-title') !== null")
        .await;
    assert_eq!(
        b.text_of("#detail-title").await,
        format!("Co 1 — {HOSTILE}")
    );
    b.shot("sistema-seeded").await;
    b.stop().await;
}

// ---------------------------------------------------------------------------------------------
// The common drawer: the page of one application that HOY, COLA and SISTEMA open (PROCESOS has
// its own, tested above). It holds the capture, the verbs of the card and the answer to register.
// ---------------------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn the_open_drawer_keeps_its_screenshot_and_note_when_the_page_redraws() {
    let env = Env::start(true);
    let b = Browse::start().await;
    b.visit(&env).await;
    b.go("cola").await;
    b.wait("document.querySelector('.prop [data-action=\"open\"]') !== null")
        .await;
    b.click(".prop [data-action=\"open\"]").await;
    b.wait("document.getElementById('detail').classList.contains('open') && document.querySelector('#detail #detail-title') !== null")
        .await;
    assert_eq!(b.text_of("#detail-title").await, "Co 2 — Prepared role");
    let picture = "(() => { const i = document.querySelector('#detail img'); return !!i && i.naturalWidth > 0; })()";
    b.wait(picture).await;
    b.eval("(() => { const n = document.getElementById('mark-note'); n.value = 'revisar lunes'; n.dispatchEvent(new Event('input')); return 'ok'; })()")
        .await;
    // A new picture of the data redraws the sheet behind the drawer, never the drawer itself.
    b.eval("(() => { window.__done = false; window.__app.session.refresh({ force: true }).then(() => { window.__done = true; }); return 'ok'; })()")
        .await;
    b.wait("window.__done === true").await;
    assert_eq!(b.eval(&format!("String({picture})")).await, "true");
    assert_eq!(
        b.eval("document.getElementById('mark-note').value").await,
        "revisar lunes"
    );
    // A new language does draw the drawer again, in the new words: the picture is not asked for twice and
    // the half-typed note is still in the box. (The key goes to the page, not to the field.)
    b.eval("(() => { document.activeElement.blur(); return 'ok'; })()")
        .await;
    b.key("l").await;
    b.wait("document.documentElement.lang === 'en' && document.querySelector('#detail').innerText.toLowerCase().includes('journey')")
        .await;
    assert_eq!(b.eval(&format!("String({picture})")).await, "true");
    assert_eq!(
        b.eval("document.getElementById('mark-note').value").await,
        "revisar lunes"
    );
    b.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn a_refused_mark_keeps_the_typed_note() {
    let env = Env::start(true);
    let b = Browse::start().await;
    b.open(&format!("{}#hoy/test%3A3", env.url())).await;
    b.wait("document.getElementById('detail').classList.contains('open') && document.querySelector('#detail [data-mark=\"replied\"]') !== null")
        .await;
    b.eval("(() => { const n = document.getElementById('mark-note'); n.value = 'martes 19:00'; n.dispatchEvent(new Event('input')); return 'ok'; })()")
        .await;
    // Behind the page's back the application already moved to replied, so "replied" is now illegal.
    env.tracker()
        .transition("test:3", State::Replied, None, *NOW)
        .unwrap();
    b.click("#detail [data-mark=\"replied\"]").await;
    b.wait("document.getElementById('toast').textContent.includes('No se pudo marcar')")
        .await;
    // The drawer is read again and now offers the moves from replied; the note is still in the box, and
    // the refusal is written under the buttons in the server's own words.
    b.wait("document.querySelector('#detail [data-mark=\"interview\"]') !== null")
        .await;
    assert_eq!(
        b.eval("document.getElementById('mark-note').value").await,
        "martes 19:00",
        "a refusal must not throw away what was typed"
    );
    b.wait("(() => { const r = document.getElementById('mark-refusal'); return !!r && !r.hidden && r.textContent.includes('No se pudo marcar'); })()")
        .await;
    b.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn the_drawer_records_an_answer_with_its_note() {
    let env = Env::start(true);
    let b = Browse::start().await;
    b.open(&format!("{}#hoy/test%3A3", env.url())).await;
    b.wait("document.getElementById('detail').classList.contains('open') && document.querySelector('#detail [data-mark=\"replied\"]') !== null")
        .await;
    b.eval("(() => { const n = document.getElementById('mark-note'); n.value = 'martes 19:00'; n.dispatchEvent(new Event('input')); return 'ok'; })()")
        .await;
    b.click("#detail [data-mark=\"replied\"]").await;
    b.wait("document.querySelector('#detail [data-mark=\"replied\"]') === null")
        .await;
    let app = env.app("test:3");
    assert_eq!(app.state, State::Replied);
    assert_eq!(app.note.as_deref(), Some("martes 19:00"));
    // PROCESOS knows it too once the drawer is closed.
    b.key("Escape").await;
    b.wait("!document.getElementById('detail').classList.contains('open')")
        .await;
    b.go("procesos").await;
    b.wait(
        "document.querySelector('button.cell[data-card=\"test:3\"]').dataset.state === 'replied'",
    )
    .await;
    b.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn approving_from_the_drawer_takes_two_presses_and_says_what_it_did() {
    let env = Env::start(true);
    let b = Browse::start().await;
    b.visit(&env).await;
    b.go("cola").await;
    b.click(".prop [data-action=\"open\"]").await;
    let button = "#detail .d-actions [data-action=\"approve\"]";
    b.wait(&format!("document.querySelector('{button}') !== null"))
        .await;
    b.click(button).await;
    assert!(!env.app("test:2").approved, "the first press only arms it");
    assert!(b.text_of(button).await.contains("Seguro"));
    tokio::time::sleep(ARM_PAUSE).await;
    b.click(button).await;
    b.wait(&format!("document.querySelector('{button}') === null || document.querySelector('{button}').textContent.includes('Aprobada')"))
        .await;
    assert!(env.app("test:2").approved);
    b.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn escape_closes_the_drawer_and_the_number_keys_do_not_travel_while_it_is_open() {
    let env = Env::start(true);
    let b = Browse::start().await;
    b.visit(&env).await;
    b.go("cola").await;
    b.click(".prop [data-action=\"open\"]").await;
    b.wait("document.getElementById('detail').classList.contains('open') && document.querySelector('#detail #detail-title') !== null")
        .await;
    // The drawer is not a modal dialog (the sheet stays where it is), but it is the one place the keys
    // are in: the number keys do not travel while it is open, and a closed drawer can take no focus.
    assert_eq!(
        b.eval("String(document.getElementById('detail').inert)")
            .await,
        "false"
    );
    b.key("2").await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        b.eval("document.body.dataset.stop").await,
        "cola",
        "a number key does not travel with the drawer open"
    );
    b.key("Escape").await;
    b.wait("!document.getElementById('detail').classList.contains('open')")
        .await;
    assert_eq!(
        b.eval("String(document.getElementById('detail').inert)")
            .await,
        "true"
    );
    b.go("procesos").await;
    b.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn the_drawer_answers_first_and_the_journey_sits_where_the_application_stopped() {
    let env = Env::start_full();
    let b = Browse::start().await;
    // Prepared with a capture: four layers done, and the fifth is Gonza's, with the approval right there.
    b.open(&format!("{}#hoy/test%3A2", env.url())).await;
    b.wait("document.getElementById('detail').classList.contains('open') && document.querySelector('#journey') !== null")
        .await;
    assert_eq!(b.count("#journey li.j-done").await, 4);
    assert_eq!(
        b.attr("#journey li.j-you", "data-layer").await,
        "compuerta",
        "the layer that stopped is the fifth and it is his"
    );
    // The answer comes first: whose turn it is and what comes now, and the one button under it.
    let answer = b.text_of("#d-now").await;
    assert!(
        answer.contains("Te toca") && answer.contains("Esperando tu aprobación"),
        "{answer}"
    );
    assert_eq!(b.attr("#d-now", "data-tone").await, "you");
    assert_eq!(
        b.count("#detail [data-action=\"approve\"]").await,
        1,
        "the button is drawn once"
    );
    assert_eq!(
        b.eval("String(document.querySelector('#d-now').compareDocumentPosition(document.querySelector('#journey')) & Node.DOCUMENT_POSITION_FOLLOWING)")
            .await,
        "4",
        "the answer is above the journey"
    );
    b.key("Escape").await;
    b.wait("!document.getElementById('detail').classList.contains('open')")
        .await;
    // One that went out has all five behind it, and the line goes on to the stops of the process.
    b.open(&format!("{}#hoy/test%3A3", env.url())).await;
    b.wait("document.getElementById('detail').classList.contains('open') && document.querySelector('#journey .j-after') !== null")
        .await;
    assert_eq!(b.count("#journey li.j-done").await, 5);
    let waiting = b.text_of("#d-now").await;
    assert!(
        waiting.contains("Esperando la respuesta de la empresa"),
        "{waiting}"
    );
    b.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn no_drawer_prints_a_stray_null_or_object_whatever_the_application_has_or_lacks() {
    let env = Env::start(true);
    let b = Browse::start().await;
    // test:3 has a note, no reason, no buttons, no dossier and no gaps; test:1 is stuck with a reason; test:2
    // waits for approval: every optional part of the drawer is absent in one of them.
    for key in ["test%3A3", "test%3A1", "test%3A2"] {
        b.open(&format!("{}#hoy/{key}", env.url())).await;
        b.wait("document.getElementById('detail').classList.contains('open') && document.querySelector('#detail .d-sec, #detail section') !== null")
            .await;
        let drawer = b.eval("document.getElementById('detail').innerText").await;
        assert_no_strays(&format!("the drawer of {key}"), &drawer);
        assert_eq!(b.eval("String(typeof window.pwned)").await, "undefined");
    }
    b.stop().await;
}

// ---------------------------------------------------------------------------------------------
// Motion: the tests above open the page calm on purpose; this one does not
// ---------------------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn with_full_motion_every_trip_arrives_and_a_request_for_calm_is_kept() {
    let env = Env::start(true);
    let b = Browse::start().await;
    b.open(&env.url_with("&lang=es&debug=1&clock=9")).await;
    b.ready().await;
    assert_eq!(b.attr("html", "data-motion").await, "full");
    // The sky is painted with no data at all behind it, and the dial is drawn.
    assert_eq!(
        b.eval("String((() => { const sky = document.getElementById('sky'); const blank = document.createElement('canvas'); blank.width = sky.width; blank.height = sky.height; return sky.width > 0 && sky.toDataURL() !== blank.toDataURL(); })())")
            .await,
        "true"
    );
    assert!(b.count("#dial *").await > 0, "the dial is drawn");
    // The camera travels between the stops and arrives at each (with the moment mounted), forwards and back.
    for stop in ["procesos", "cola", "aprender", "sistema", "cola", "hoy"] {
        b.go(stop).await;
    }
    b.shot("motion-full").await;
    // The system's request for no motion is kept: the page says so about itself.
    b.page
        .emulate_media(Some(
            playwright_rs::protocol::EmulateMediaOptions::builder()
                .reduced_motion(playwright_rs::protocol::ReducedMotion::Reduce)
                .build(),
        ))
        .await
        .unwrap();
    b.wait("document.documentElement.dataset.motion === 'calm'")
        .await;
    b.go("procesos").await;
    b.stop().await;
}
