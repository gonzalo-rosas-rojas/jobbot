//! Browser tests of the pages the filler must never fill: redirects to platforms it does not
//! use, private addresses and forms with nowhere to attach the CV.
//!
//! Run with `cargo test -p jobbot-browser --lib form:: -- --ignored --test-threads=1`

use super::support_tests::{Session, full_bank};
use super::*;
use jobbot_domain::fixtures::make_job_with;
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn a_page_that_sends_the_browser_to_a_manual_only_platform_is_never_filled() {
    use playwright_rs::FulfillOptions;

    let playwright = Playwright::launch().await.unwrap();
    let browser = crate::launch::start(&playwright, crate::launch::launch_options())
        .await
        .unwrap();
    let page = browser.new_page().await.unwrap();

    // Nothing in this test may reach the internet. The catch-all route is registered first (so
    // it has the lowest priority): it lets the local server through and aborts everything else.
    page.route("**/*", |route| async move {
        if route.request().url().starts_with("http://127.0.0.1") {
            route.continue_(None).await
        } else {
            route.abort(None).await
        }
    })
    .await
    .unwrap();
    // LinkedIn is a stub served by the test.
    page.route("https://www.linkedin.com/**", |route| async move {
        let form = r#"<form><label for="fn">First name *</label><input id="fn" name="fn" required>
                <label for="cv">Resume/CV *</label><input id="cv" type="file" required>
                <button type="submit">Submit application</button></form>"#;
        route
            .fulfill(
                FulfillOptions::builder()
                    .status(200)
                    .content_type("text/html")
                    .body_string(form)
                    .build(),
            )
            .await
    })
    .await
    .unwrap();

    // The first address looks harmless (a job board's own link) and sends the browser on to
    // LinkedIn, like RemoteOK's `/l/<id>` redirect links do.
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            "<script>location.href = 'https://www.linkedin.com/jobs/view/1'</script>",
            "text/html",
        ))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let cv = dir.path().join("cv.pdf");
    std::fs::write(&cv, b"%PDF-1.4 fake").unwrap();
    let job = make_job_with(|j| j.apply_url = format!("{}/l/1", server.uri()));
    let result = fill_application(
        &page,
        &job,
        &cv,
        &full_bank(),
        SubmitMode::DryRun,
        &dir.path().join("shots"),
        NavPolicy::LOCAL_TESTS,
    )
    .await
    .unwrap();
    assert_eq!(
        result.status,
        SubmitStatus::NeedsGonza,
        "{:?}",
        result.reason
    );
    assert_eq!(result.reason.as_deref(), Some("manual_only_platform"));
    let typed = page.locator("#fn").input_value(None).await.unwrap();
    assert_eq!(typed, "", "nothing is typed into the page it landed on");
    browser.close().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn a_link_to_a_private_address_is_never_opened() {
    let session = Session::start().await;
    let job = make_job_with(|j| j.apply_url = "https://169.254.169.254/latest/meta-data/".into());
    let result = fill_application(
        &session.page,
        &job,
        &session.cv(),
        &full_bank(),
        SubmitMode::Live,
        &session.shots(),
        NavPolicy::STRICT,
    )
    .await
    .unwrap();
    assert_eq!(
        result.status,
        SubmitStatus::NeedsGonza,
        "{:?}",
        result.reason
    );
    assert!(result.reason.as_deref().unwrap().starts_with("unsafe_url:"));
    assert_eq!(session.page.url(), "about:blank", "the browser never moved");
    session.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn a_redirect_to_a_private_address_is_never_filled() {
    use playwright_rs::FulfillOptions;

    let session = Session::start().await;
    // Nothing here may reach the network: the catch-all (registered first, lowest priority)
    // lets the local server through and aborts everything else.
    session
        .page
        .route("**/*", |route| async move {
            if route.request().url().starts_with("http://127.0.0.1") {
                route.continue_(None).await
            } else {
                route.abort(None).await
            }
        })
        .await
        .unwrap();
    // The private address answers with a form that would be filled if the guard did not exist.
    session
            .page
            .route("https://169.254.169.254/**", |route| async move {
                let form = r#"<form><label for="fn">First name *</label><input id="fn" name="fn" required>
                    <label for="cv">Resume/CV *</label><input id="cv" type="file" required>
                    <button type="submit">Submit application</button></form>"#;
                route
                    .fulfill(
                        FulfillOptions::builder()
                            .status(200)
                            .content_type("text/html")
                            .body_string(form)
                            .build(),
                    )
                    .await
            })
            .await
            .unwrap();
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            "<script>location.href = 'https://169.254.169.254/latest/meta-data/'</script>",
            "text/html",
        ))
        .mount(&server)
        .await;
    let job = make_job_with(|j| j.apply_url = format!("{}/l/1", server.uri()));
    let result = fill_application(
        &session.page,
        &job,
        &session.cv(),
        &full_bank(),
        SubmitMode::DryRun,
        &session.shots(),
        NavPolicy::LOCAL_TESTS,
    )
    .await
    .unwrap();
    assert_eq!(
        result.status,
        SubmitStatus::NeedsGonza,
        "{:?}",
        result.reason
    );
    assert!(
        result.reason.as_deref().unwrap().starts_with("unsafe_url:"),
        "{:?}",
        result.reason
    );
    let typed = session.page.locator("#fn").input_value(None).await.unwrap();
    assert_eq!(typed, "", "nothing is typed into the page it landed on");
    session.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn a_page_that_sends_the_browser_to_ashby_is_never_assisted_or_sent() {
    use playwright_rs::FulfillOptions;

    let playwright = Playwright::launch().await.unwrap();
    let browser = crate::launch::start(&playwright, crate::launch::launch_options())
        .await
        .unwrap();
    let page = browser.new_page().await.unwrap();
    page.route("**/*", |route| async move {
        if route.request().url().starts_with("http://127.0.0.1") {
            route.continue_(None).await
        } else {
            route.abort(None).await
        }
    })
    .await
    .unwrap();
    // Ashby is a stub served by the test: nothing here reaches the internet.
    page.route("https://jobs.ashbyhq.com/**", |route| async move {
        let form = r#"<form><label for="fn">First name *</label><input id="fn" name="fn" required>
                <label for="cv">Resume/CV *</label><input id="cv" type="file" required>
                <button type="submit">Submit application</button></form>"#;
        route
            .fulfill(
                FulfillOptions::builder()
                    .status(200)
                    .content_type("text/html")
                    .body_string(form)
                    .build(),
            )
            .await
    })
    .await
    .unwrap();
    // A harmless-looking board link that sends the browser on to an Ashby form.
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            "<script>location.href = 'https://jobs.ashbyhq.com/acme/0b1c2d3e/application'</script>",
            "text/html",
        ))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let cv = dir.path().join("cv.pdf");
    std::fs::write(&cv, b"%PDF-1.4 fake").unwrap();
    let job = make_job_with(|j| j.apply_url = format!("{}/l/1", server.uri()));
    for mode in [SubmitMode::Live, SubmitMode::Assist] {
        let result = fill_application(
            &page,
            &job,
            &cv,
            &full_bank(),
            mode,
            &dir.path().join("shots"),
            NavPolicy::LOCAL_TESTS,
        )
        .await
        .unwrap();
        assert_eq!(
            result.status,
            SubmitStatus::NeedsGonza,
            "{mode:?}: {:?}",
            result.reason
        );
        assert_eq!(result.reason.as_deref(), Some("handoff_host"), "{mode:?}");
        let typed = page.locator("#fn").input_value(None).await.unwrap();
        assert_eq!(typed, "", "{mode:?}: nothing is typed into the Ashby form");
    }
    browser.close().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs chromium"]
async fn a_form_with_nowhere_to_attach_the_cv_is_neither_filled_nor_clicked() {
    let session = Session::start().await;
    for mode in [SubmitMode::DryRun, SubmitMode::Live] {
        let result = session.fill("no_cv_field", &full_bank(), mode).await;
        assert_eq!(
            result.status,
            SubmitStatus::NeedsGonza,
            "{mode:?}: {:?}",
            result.reason
        );
        assert!(
            result
                .reason
                .as_deref()
                .unwrap_or("")
                .starts_with("sin campo de CV"),
            "{mode:?}: {:?}",
            result.reason
        );
        assert_eq!(
            session.eval("String(Boolean(window.submitted))").await,
            "false",
            "{mode:?}"
        );
        assert_eq!(
            session.value("#first_name").await,
            "",
            "{mode:?}: nothing was typed"
        );
    }
    session.stop().await;
}
