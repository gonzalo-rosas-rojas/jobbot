//! Is the posting still open? The bot asks the source right before it applies, because a posting
//! can be filled between the discovery run and the evening submission window.
//!
//! The answer is `Closed` only when the source says so in no uncertain terms: a 404 for the one
//! posting (Greenhouse, Lever) or a successful, non-empty Ashby listing that no longer has the
//! posting's id. Anything else that is not a plain "yes" (an error status, a timeout, a network
//! failure, a body that is not what it should be, a source without a check) is `Unknown`, and an
//! unknown answer must never stop an application.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use reqwest::{Client, StatusCode, Url};
use serde_json::Value;

use crate::models::Job;

/// Upper bound for one open check. The check sits in front of a submission: a slow board must not
/// hold the whole run, and a check that did not answer in time says nothing.
pub const CHECK_TIMEOUT: Duration = Duration::from_secs(10);

const GREENHOUSE_BASE_URL: &str = "https://boards-api.greenhouse.io";
const LEVER_BASE_URL: &str = "https://api.lever.co";
const ASHBY_BASE_URL: &str = "https://api.ashbyhq.com";

/// What the source says about a posting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenStatus {
    /// The source still lists the posting.
    Open,
    /// The source says the posting is gone.
    Closed,
    /// No clear answer. Treat it as open.
    Unknown,
}

/// Asks the posting's own source whether it is still open (see the module docs for the rules).
pub async fn check_open(client: &Client, job: &Job) -> OpenStatus {
    check_open_with(client, job, None, CHECK_TIMEOUT).await
}

/// `check_open` against another server: `base_url` replaces the Greenhouse, Lever and Ashby API
/// hosts alike (their paths never collide). Tests point it at a local mock.
pub async fn check_open_at(client: &Client, job: &Job, base_url: &str) -> OpenStatus {
    check_open_with(client, job, Some(base_url), CHECK_TIMEOUT).await
}

async fn check_open_with(
    client: &Client,
    job: &Job,
    base_url: Option<&str>,
    timeout: Duration,
) -> OpenStatus {
    let base = |default: &'static str| base_url.unwrap_or(default).trim_end_matches('/');
    match job.source.as_str() {
        "greenhouse" => {
            let Some(board) = greenhouse_board(job) else {
                return OpenStatus::Unknown;
            };
            let url = format!(
                "{}/v1/boards/{board}/jobs/{}",
                base(GREENHOUSE_BASE_URL),
                job.external_id
            );
            single_posting(client, &url, timeout).await
        }
        "lever" => {
            let Some(company) = slug_in_links(job, "jobs.lever.co") else {
                return OpenStatus::Unknown;
            };
            let url = format!(
                "{}/v0/postings/{company}/{}",
                base(LEVER_BASE_URL),
                job.external_id
            );
            single_posting(client, &url, timeout).await
        }
        "ashby" => {
            let Some(org) = slug_in_links(job, "jobs.ashbyhq.com") else {
                return OpenStatus::Unknown;
            };
            let url = format!("{}/posting-api/job-board/{org}", base(ASHBY_BASE_URL));
            ashby_listing(client, &url, &job.external_id, timeout).await
        }
        // The other sources have no per-posting check: their feeds are listings of everything
        // currently open, which discovery already reads.
        _ => OpenStatus::Unknown,
    }
}

/// GET `url` and keep only what a check needs: the answer's status and body, or `None` when there
/// was no answer (network error, timeout) or the body could not be read.
async fn get(client: &Client, url: &str, timeout: Duration) -> Option<(StatusCode, String)> {
    let url = Url::parse(url).ok()?;
    let response = client.get(url).timeout(timeout).send().await.ok()?;
    let status = response.status();
    let body = response.text().await.ok()?;
    Some((status, body))
}

/// A source with one endpoint per posting (Greenhouse, Lever): a success means it is listed and the
/// API's own "not found" answer means it is gone. Every other answer, or none, proves nothing: a
/// proxy, a CDN or a captive portal can answer 404 for the whole host, and withdrawing is terminal.
async fn single_posting(client: &Client, url: &str, timeout: Duration) -> OpenStatus {
    match get(client, url, timeout).await {
        Some((status, _)) if status.is_success() => OpenStatus::Open,
        Some((StatusCode::NOT_FOUND, body)) if is_not_found_body(&body) => OpenStatus::Closed,
        _ => OpenStatus::Unknown,
    }
}

/// The JSON both APIs send for a posting that is gone: Greenhouse `{"status":404,"error":"Job not
/// found"}`, Lever `{"ok":false,"error":"Document not found"}` (both seen live on 2026-10-04).
fn is_not_found_body(body: &str) -> bool {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|json| json["error"].as_str().map(str::to_lowercase))
        .is_some_and(|error| error.contains("not found"))
}

/// Ashby has no per-posting endpoint, so the board listing is the evidence: the posting is gone
/// when a successful listing that does list postings no longer has its id. A listing that is
/// empty, lacks the `jobs` array, or whose entries carry no ids is not trusted: an outage or a
/// changed format would otherwise close every posting of the board.
async fn ashby_listing(
    client: &Client,
    url: &str,
    external_id: &str,
    timeout: Duration,
) -> OpenStatus {
    let ids = ashby_ids(client, url, timeout).await;
    ashby_verdict(ids.as_deref(), external_id)
}

/// The posting ids of an Ashby board listing, or `None` when the listing cannot be trusted (no
/// answer, an error status, a body that is not the expected shape, no ids at all).
async fn ashby_ids(client: &Client, url: &str, timeout: Duration) -> Option<Vec<String>> {
    let (status, body) = get(client, url, timeout).await?;
    if !status.is_success() {
        return None;
    }
    let listing = serde_json::from_str::<Value>(&body).ok()?;
    let ids: Vec<String> = listing["jobs"]
        .as_array()?
        .iter()
        .filter_map(|posting| posting["id"].as_str().map(str::to_string))
        .collect();
    (!ids.is_empty()).then_some(ids)
}

fn ashby_verdict(ids: Option<&[String]>, external_id: &str) -> OpenStatus {
    match ids {
        None => OpenStatus::Unknown,
        Some(ids) if ids.iter().any(|id| id.eq_ignore_ascii_case(external_id)) => OpenStatus::Open,
        Some(_) => OpenStatus::Closed,
    }
}

/// Anything that can say whether a posting is still open: the network in production, a fake in
/// the submit tests.
#[async_trait::async_trait]
pub trait OpenChecker: Send + Sync {
    async fn check(&self, job: &Job) -> OpenStatus;
}

/// The real checker. It keeps each Ashby board's listing for as long as it lives (one run): a
/// listing weighs about a megabyte and many queued postings share a board. A listing that failed
/// is remembered too, so a board that is down is asked about once.
pub struct NetworkOpenChecker {
    client: Client,
    base_url: Option<String>,
    ashby_boards: Mutex<HashMap<String, Option<Vec<String>>>>,
}

impl NetworkOpenChecker {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            base_url: None,
            ashby_boards: Mutex::new(HashMap::new()),
        }
    }

    /// Against another server, like `check_open_at`: tests point it at a local mock.
    pub fn with_base_url(client: Client, base_url: &str) -> Self {
        Self {
            base_url: Some(base_url.to_string()),
            ..Self::new(client)
        }
    }
}

#[async_trait::async_trait]
impl OpenChecker for NetworkOpenChecker {
    async fn check(&self, job: &Job) -> OpenStatus {
        if job.source != "ashby" {
            return check_open_with(&self.client, job, self.base_url.as_deref(), CHECK_TIMEOUT)
                .await;
        }
        let Some(org) = slug_in_links(job, "jobs.ashbyhq.com") else {
            return OpenStatus::Unknown;
        };
        let known = self.ashby_boards.lock().unwrap().get(&org).cloned();
        let ids = match known {
            Some(ids) => ids,
            None => {
                let base = self.base_url.as_deref().unwrap_or(ASHBY_BASE_URL);
                let url = format!("{}/posting-api/job-board/{org}", base.trim_end_matches('/'));
                let ids = ashby_ids(&self.client, &url, CHECK_TIMEOUT).await;
                self.ashby_boards.lock().unwrap().insert(org, ids.clone());
                ids
            }
        };
        ashby_verdict(ids.as_deref(), &job.external_id)
    }
}

/// True for the text a board slug or a posting id is made of. Anything else (a slash, `%`, a
/// space, `..`) could change the path of the request, so it never goes into one.
fn is_slug(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 100
        && text.chars().any(|c| c != '.')
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// The host (lowercase) and the non-empty path segments of a link.
fn host_and_segments(link: &str) -> Option<(String, Vec<String>, Url)> {
    let url = Url::parse(link).ok()?;
    let host = url.host_str()?.to_ascii_lowercase();
    let segments = url
        .path_segments()?
        .filter(|segment| !segment.is_empty())
        .map(str::to_string)
        .collect();
    Some((host, segments, url))
}

/// The company slug of a Lever posting or the org slug of an Ashby one, read from the posting's
/// own links (`https://{host}/{slug}/{id}[/apply]`): the adapters store the slug as `company`,
/// but a link is the better witness because it must also carry the posting's id, so a slug and an
/// id that do not belong together are never asked about.
fn slug_in_links(job: &Job, host: &str) -> Option<String> {
    if !is_slug(&job.external_id) {
        return None;
    }
    [&job.url, &job.apply_url].into_iter().find_map(|link| {
        let (link_host, segments, _) = host_and_segments(link)?;
        match segments.as_slice() {
            [slug, id, ..] if link_host == host && *id == job.external_id && is_slug(slug) => {
                Some(slug.clone())
            }
            _ => None,
        }
    })
}

/// The board slug of a Greenhouse posting. The adapter's `company` is the display name ("Gusto,
/// Inc."), so the slug comes from a link: the hosted page `https://{host}/{board}/jobs/{id}`, or
/// the embed form `https://{host}/embed/job_app?for={board}&token={id}` that a posting shown on
/// the company's own site applies through. The link must carry the posting's own id.
fn greenhouse_board(job: &Job) -> Option<String> {
    if !job.external_id.chars().all(|c| c.is_ascii_digit()) || job.external_id.is_empty() {
        return None;
    }
    [&job.apply_url, &job.url].into_iter().find_map(|link| {
        let (host, segments, url) = host_and_segments(link)?;
        if host != "greenhouse.io" && !host.ends_with(".greenhouse.io") {
            return None;
        }
        let (board, id) = match segments.as_slice() {
            [embed, ..] if embed == "embed" => {
                let param = |key: &str| {
                    url.query_pairs()
                        .find(|(name, _)| name == key)
                        .map(|(_, value)| value.into_owned())
                };
                (param("for")?, param("token")?)
            }
            [board, jobs, id, ..] if jobs == "jobs" => (board.clone(), id.clone()),
            _ => return None,
        };
        (id == job.external_id && is_slug(&board)).then_some(board)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// A Greenhouse posting on its hosted board page, as the adapter stores it.
    fn greenhouse_job(board: &str, id: &str) -> Job {
        let url = format!("https://job-boards.greenhouse.io/{board}/jobs/{id}");
        Job {
            source: "greenhouse".into(),
            external_id: id.into(),
            company: "Acme Corp".into(),
            title: "AI Engineer".into(),
            url: url.clone(),
            apply_url: url,
            ..Job::default()
        }
    }

    /// A Lever posting; the adapter stores the slug as the company.
    fn lever_job(company: &str, id: &str) -> Job {
        let url = format!("https://jobs.lever.co/{company}/{id}");
        Job {
            source: "lever".into(),
            external_id: id.into(),
            company: company.into(),
            title: "AI Engineer".into(),
            apply_url: format!("{url}/apply"),
            url,
            ..Job::default()
        }
    }

    /// An Ashby posting; the adapter stores the org slug as the company.
    fn ashby_job(org: &str, id: &str) -> Job {
        let url = format!("https://jobs.ashbyhq.com/{org}/{id}");
        Job {
            source: "ashby".into(),
            external_id: id.into(),
            company: org.into(),
            title: "AI Engineer".into(),
            apply_url: format!("{url}/application"),
            url,
            ..Job::default()
        }
    }

    async fn mount(server: &MockServer, route: &str, response: ResponseTemplate) {
        Mock::given(method("GET"))
            .and(path(route))
            .respond_with(response)
            .mount(server)
            .await;
    }

    async fn check(server: &MockServer, job: &Job) -> OpenStatus {
        check_open_at(&Client::new(), job, &server.uri()).await
    }

    async fn requests_made(server: &MockServer) -> usize {
        server.received_requests().await.unwrap().len()
    }

    fn greenhouse_posting() -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({
            "id": 4461, "title": "AI Engineer", "absolute_url": "https://job-boards.greenhouse.io/acme/jobs/4461"
        }))
    }

    #[tokio::test]
    async fn greenhouse_200_is_open() {
        let server = MockServer::start().await;
        mount(&server, "/v1/boards/acme/jobs/4461", greenhouse_posting()).await;

        assert_eq!(
            check(&server, &greenhouse_job("acme", "4461")).await,
            OpenStatus::Open
        );
    }

    #[tokio::test]
    async fn greenhouse_404_is_closed() {
        let server = MockServer::start().await;
        // The body the live API sends for a posting that is gone (2026-10-04).
        mount(
            &server,
            "/v1/boards/acme/jobs/4461",
            ResponseTemplate::new(404)
                .set_body_json(json!({"status": 404, "error": "Job not found"})),
        )
        .await;

        assert_eq!(
            check(&server, &greenhouse_job("acme", "4461")).await,
            OpenStatus::Closed
        );
    }

    #[tokio::test]
    async fn a_greenhouse_form_embedded_in_the_company_site_is_checked_on_its_own_board() {
        // Databricks-style posting: `url` is the company site, the board is only in the embed link.
        let server = MockServer::start().await;
        mount(
            &server,
            "/v1/boards/databricks/jobs/8441894002",
            ResponseTemplate::new(404)
                .set_body_json(json!({"status": 404, "error": "Job not found"})),
        )
        .await;
        let job = Job {
            source: "greenhouse".into(),
            external_id: "8441894002".into(),
            company: "Databricks".into(),
            url: "https://databricks.com/company/careers/open-positions/job?gh_jid=8441894002"
                .into(),
            apply_url:
                "https://job-boards.greenhouse.io/embed/job_app?for=databricks&token=8441894002"
                    .into(),
            ..Job::default()
        };

        assert_eq!(check(&server, &job).await, OpenStatus::Closed);
    }

    #[tokio::test]
    async fn lever_200_is_open_and_404_is_closed() {
        let server = MockServer::start().await;
        let open = "5eca795c-48dd-496a-be23-2181068a5450";
        let gone = "00000000-0000-0000-0000-000000000000";
        mount(
            &server,
            &format!("/v0/postings/metabase/{open}"),
            ResponseTemplate::new(200).set_body_json(json!({"id": open, "text": "Engineer"})),
        )
        .await;
        // The body the live API sends for a posting that is gone (2026-10-04).
        mount(
            &server,
            &format!("/v0/postings/metabase/{gone}"),
            ResponseTemplate::new(404)
                .set_body_json(json!({"ok": false, "error": "Document not found"})),
        )
        .await;

        assert_eq!(
            check(&server, &lever_job("metabase", open)).await,
            OpenStatus::Open
        );
        assert_eq!(
            check(&server, &lever_job("metabase", gone)).await,
            OpenStatus::Closed
        );
    }

    fn ashby_listing(ids: &[&str]) -> ResponseTemplate {
        let jobs: Vec<Value> = ids
            .iter()
            .map(|id| json!({"id": id, "title": "Engineer", "isListed": true}))
            .collect();
        ResponseTemplate::new(200).set_body_json(json!({"apiVersion": "1", "jobs": jobs}))
    }

    #[tokio::test]
    async fn ashby_posting_present_in_the_listing_is_open() {
        let server = MockServer::start().await;
        mount(
            &server,
            "/posting-api/job-board/baseten",
            ashby_listing(&["a1", "a2"]),
        )
        .await;

        assert_eq!(
            check(&server, &ashby_job("baseten", "a2")).await,
            OpenStatus::Open
        );
    }

    #[tokio::test]
    async fn ashby_posting_missing_from_a_non_empty_listing_is_closed() {
        let server = MockServer::start().await;
        mount(
            &server,
            "/posting-api/job-board/baseten",
            ashby_listing(&["a1", "a2"]),
        )
        .await;

        assert_eq!(
            check(&server, &ashby_job("baseten", "a3")).await,
            OpenStatus::Closed
        );
    }

    #[tokio::test]
    async fn an_empty_or_unusable_ashby_listing_proves_nothing() {
        let bodies = [
            (
                "empty listing",
                ResponseTemplate::new(200).set_body_json(json!({"jobs": []})),
            ),
            (
                "no jobs array",
                ResponseTemplate::new(200).set_body_json(json!({"error": "x"})),
            ),
            (
                "jobs is not an array",
                ResponseTemplate::new(200).set_body_json(json!({"jobs": "nope"})),
            ),
            (
                "entries without ids",
                ResponseTemplate::new(200)
                    .set_body_json(json!({"jobs": [{"title": "x"}, {"title": "y"}]})),
            ),
            (
                "not JSON",
                ResponseTemplate::new(200).set_body_string("<html>blocked</html>"),
            ),
            ("404", ResponseTemplate::new(404)),
            ("500", ResponseTemplate::new(500)),
            ("429", ResponseTemplate::new(429)),
        ];
        for (what, response) in bodies {
            let server = MockServer::start().await;
            mount(&server, "/posting-api/job-board/baseten", response).await;

            assert_eq!(
                check(&server, &ashby_job("baseten", "a1")).await,
                OpenStatus::Unknown,
                "{what}"
            );
        }
    }

    #[tokio::test]
    async fn a_404_that_is_not_the_apis_own_answer_closes_nothing() {
        // A proxy, a CDN or a captive portal can answer 404 for the whole API host; withdrawing is
        // terminal, so only the API's documented "not found" JSON closes a posting.
        let bodies = [
            (
                "html page",
                ResponseTemplate::new(404).set_body_string("<html>404 Not Found</html>"),
            ),
            ("empty body", ResponseTemplate::new(404)),
            (
                "other error",
                ResponseTemplate::new(404).set_body_json(json!({"error": "Bad gateway"})),
            ),
            (
                "no error key",
                ResponseTemplate::new(404).set_body_json(json!({"status": 404})),
            ),
        ];
        for (what, response) in bodies {
            let server = MockServer::start().await;
            mount(&server, "/v1/boards/acme/jobs/4461", response.clone()).await;
            mount(&server, "/v0/postings/acme/abc", response).await;

            assert_eq!(
                check(&server, &greenhouse_job("acme", "4461")).await,
                OpenStatus::Unknown,
                "greenhouse {what}"
            );
            assert_eq!(
                check(&server, &lever_job("acme", "abc")).await,
                OpenStatus::Unknown,
                "lever {what}"
            );
        }
    }

    #[tokio::test]
    async fn only_a_clear_404_closes_a_single_posting() {
        for status in [400, 401, 403, 429, 500, 502, 503] {
            let server = MockServer::start().await;
            mount(
                &server,
                "/v1/boards/acme/jobs/4461",
                ResponseTemplate::new(status),
            )
            .await;
            mount(
                &server,
                "/v0/postings/acme/abc",
                ResponseTemplate::new(status),
            )
            .await;

            assert_eq!(
                check(&server, &greenhouse_job("acme", "4461")).await,
                OpenStatus::Unknown,
                "greenhouse {status}"
            );
            assert_eq!(
                check(&server, &lever_job("acme", "abc")).await,
                OpenStatus::Unknown,
                "lever {status}"
            );
        }
    }

    #[tokio::test]
    async fn a_slow_source_is_unknown_not_closed() {
        let server = MockServer::start().await;
        mount(
            &server,
            "/v1/boards/acme/jobs/4461",
            greenhouse_posting().set_delay(Duration::from_millis(600)),
        )
        .await;
        mount(
            &server,
            "/posting-api/job-board/acme",
            ashby_listing(&["a1"]).set_delay(Duration::from_millis(600)),
        )
        .await;
        let short = Duration::from_millis(100);

        let greenhouse = check_open_with(
            &Client::new(),
            &greenhouse_job("acme", "4461"),
            Some(&server.uri()),
            short,
        )
        .await;
        let ashby = check_open_with(
            &Client::new(),
            &ashby_job("acme", "a2"),
            Some(&server.uri()),
            short,
        )
        .await;

        assert_eq!(greenhouse, OpenStatus::Unknown);
        assert_eq!(ashby, OpenStatus::Unknown);
    }

    #[test]
    fn the_production_timeout_is_ten_seconds() {
        assert_eq!(CHECK_TIMEOUT, Duration::from_secs(10));
    }

    #[tokio::test]
    async fn an_unreachable_source_is_unknown() {
        // A listener that accepts one connection and closes it without answering. The port stays
        // owned by this test, so a parallel test's MockServer can never be assigned it.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let uri = format!("http://{}", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            let _ = listener.incoming().take(1).count();
        });

        let status = check_open_at(&Client::new(), &greenhouse_job("acme", "4461"), &uri).await;

        assert_eq!(status, OpenStatus::Unknown);
    }

    #[tokio::test]
    async fn a_source_without_a_check_is_unknown_and_sends_no_request() {
        let server = MockServer::start().await;
        mount(&server, "/", ResponseTemplate::new(404)).await;
        for source in [
            "remoteok",
            "remotive",
            "workable",
            "recruitee",
            "hackernews",
            "test",
            "",
        ] {
            let job = Job {
                source: source.into(),
                external_id: "1".into(),
                url: "https://example.com/jobs/1".into(),
                apply_url: "https://example.com/jobs/1/apply".into(),
                ..Job::default()
            };

            assert_eq!(
                check(&server, &job).await,
                OpenStatus::Unknown,
                "{source:?}"
            );
        }

        assert_eq!(requests_made(&server).await, 0);
    }

    #[tokio::test]
    async fn a_posting_whose_links_do_not_name_its_board_is_unknown_and_sends_no_request() {
        // A 404 from a guessed board would close a posting that may well be open, so the board
        // comes from the posting's own links or the check does not run.
        let server = MockServer::start().await;
        let cases = [
            // The company is a display name, not a board slug: never used as one.
            Job {
                source: "greenhouse".into(),
                external_id: "4461".into(),
                company: "Acme Corp".into(),
                url: "https://acme.example/careers?gh_jid=4461".into(),
                apply_url: "https://acme.example/careers/apply?gh_jid=4461".into(),
                ..Job::default()
            },
            // A link on another host that merely looks like the board path.
            Job {
                url: "https://evil.example/acme/jobs/4461".into(),
                apply_url: "https://evil.example/acme/jobs/4461".into(),
                ..greenhouse_job("acme", "4461")
            },
            // The link points at another posting than the one stored.
            Job {
                external_id: "9999".into(),
                ..greenhouse_job("acme", "4461")
            },
            // Not a URL at all.
            Job {
                url: "not a url".into(),
                apply_url: String::new(),
                ..greenhouse_job("acme", "4461")
            },
            // The id would climb out of the path.
            Job {
                external_id: "../x".into(),
                url: "https://jobs.lever.co/acme/..%2Fx".into(),
                apply_url: String::new(),
                ..lever_job("acme", "x")
            },
            // A Lever posting on the EU host, which the adapter never reads.
            Job {
                url: "https://jobs.eu.lever.co/acme/abc".into(),
                apply_url: "https://jobs.eu.lever.co/acme/abc/apply".into(),
                ..lever_job("acme", "abc")
            },
            // An Ashby posting whose link names another id.
            Job {
                external_id: "other".into(),
                ..ashby_job("acme", "a1")
            },
        ];
        for job in &cases {
            assert_eq!(
                check(&server, job).await,
                OpenStatus::Unknown,
                "{:?}",
                (&job.source, &job.url, &job.apply_url)
            );
        }

        assert_eq!(requests_made(&server).await, 0);
    }

    #[tokio::test]
    async fn the_board_is_read_from_the_boards_hosted_page_too() {
        // Hosted pages exist on boards.greenhouse.io, job-boards.greenhouse.io and the EU host.
        let server = MockServer::start().await;
        mount(&server, "/v1/boards/acme/jobs/4461", greenhouse_posting()).await;
        for host in [
            "boards.greenhouse.io",
            "job-boards.greenhouse.io",
            "job-boards.eu.greenhouse.io",
        ] {
            let url = format!("https://{host}/acme/jobs/4461");
            let job = Job {
                url: url.clone(),
                apply_url: url,
                ..greenhouse_job("other", "4461")
            };

            assert_eq!(check(&server, &job).await, OpenStatus::Open, "{host}");
        }
    }

    #[tokio::test]
    async fn the_network_checker_reads_an_ashby_board_once_for_all_its_postings() {
        let server = MockServer::start().await;
        mount(
            &server,
            "/posting-api/job-board/baseten",
            ashby_listing(&["a1", "a2"]),
        )
        .await;
        let checker = NetworkOpenChecker::with_base_url(Client::new(), &server.uri());

        assert_eq!(
            checker.check(&ashby_job("baseten", "a1")).await,
            OpenStatus::Open
        );
        assert_eq!(
            checker.check(&ashby_job("baseten", "a3")).await,
            OpenStatus::Closed
        );
        assert_eq!(
            checker.check(&ashby_job("baseten", "a2")).await,
            OpenStatus::Open
        );

        assert_eq!(requests_made(&server).await, 1, "one listing per board");
    }

    #[tokio::test]
    async fn a_failed_ashby_listing_is_not_asked_again_and_proves_nothing() {
        let server = MockServer::start().await;
        mount(
            &server,
            "/posting-api/job-board/baseten",
            ResponseTemplate::new(500),
        )
        .await;
        let checker = NetworkOpenChecker::with_base_url(Client::new(), &server.uri());

        for id in ["a1", "a2"] {
            assert_eq!(
                checker.check(&ashby_job("baseten", id)).await,
                OpenStatus::Unknown
            );
        }

        assert_eq!(requests_made(&server).await, 1);
    }

    #[tokio::test]
    async fn the_network_checker_answers_like_check_open_for_the_other_sources() {
        let server = MockServer::start().await;
        mount(
            &server,
            "/v1/boards/acme/jobs/4461",
            ResponseTemplate::new(404)
                .set_body_json(json!({"status": 404, "error": "Job not found"})),
        )
        .await;
        let checker = NetworkOpenChecker::with_base_url(Client::new(), &server.uri());

        assert_eq!(
            checker.check(&greenhouse_job("acme", "4461")).await,
            OpenStatus::Closed
        );
        let other = Job {
            source: "remotive".into(),
            ..greenhouse_job("acme", "4461")
        };
        assert_eq!(checker.check(&other).await, OpenStatus::Unknown);
    }
}
